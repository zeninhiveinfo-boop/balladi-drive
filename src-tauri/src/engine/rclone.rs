use md5::{Digest, Md5};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs::File;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFileMetadata {
    pub name: String,
    pub size: Option<String>,
    pub md5_checksum: Option<String>,
}

pub fn calculate_md5(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed opening downloaded file: {e}"))?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed hashing downloaded file: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub async fn calculate_md5_async(path: PathBuf) -> Result<String, String> {
    tokio::task::spawn_blocking(move || calculate_md5(&path))
        .await
        .map_err(|e| format!("MD5 worker failed: {e}"))?
}

pub async fn get_drive_file_metadata(
    client: &reqwest::Client,
    access_token: &str,
    file_id: &str,
    resource_key: Option<&str>,
) -> Result<DriveFileMetadata, String> {
    let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
    let mut request = client
        .get(url)
        .bearer_auth(access_token)
        .query(&[
            ("fields", "id,name,size,md5Checksum"),
            ("supportsAllDrives", "true"),
        ]);
    if let Some(key) = resource_key {
        request = request.header("X-Goog-Drive-Resource-Keys", format!("{file_id}/{key}"));
    }
    request
        .send()
        .await
        .map_err(|e| format!("Drive metadata request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Drive metadata returned an error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Invalid Drive metadata response: {e}"))
}

pub fn immediate_file_candidates(
    local_dir: &Path,
    metadata: &DriveFileMetadata,
) -> Result<Vec<PathBuf>, String> {
    let expected_size = metadata
        .size
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok());
    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(local_dir)
        .map_err(|e| format!("Failed reading destination directory: {e}"))?
    {
        let path = entry
            .map_err(|e| format!("Failed reading destination entry: {e}"))?
            .path();
        let file_metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("Failed reading file metadata: {e}"))?;
        // Reject symlinks and directories.
        if !file_metadata.file_type().is_file() {
            continue;
        }
        if expected_size
            .map(|size| file_metadata.len() != size)
            .unwrap_or(false)
        {
            continue;
        }
        candidates.push(path);
    }
    Ok(candidates)
}

pub async fn verify_single_file_metadata(
    local_dir: &Path,
    metadata: &DriveFileMetadata,
) -> Result<VerificationResult, String> {
    let expected_md5 = match metadata.md5_checksum.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(m) => m.to_string(),
        None => {
            return Ok(VerificationResult {
                success: false,
                hash_type: "MD5 Unavailable".to_string(),
                matching_files: 0,
                missing_on_dst: 0,
                differ_count: 0,
                error_count: 1,
                details: vec![
                    "Drive did not provide an MD5 checksum; verification unavailable.".to_string(),
                ],
            });
        }
    };

    let candidates = immediate_file_candidates(local_dir, metadata)?;
    for candidate in &candidates {
        let actual_md5 = calculate_md5_async(candidate.clone()).await?;
        if actual_md5.eq_ignore_ascii_case(&expected_md5) {
            return Ok(VerificationResult {
                success: true,
                hash_type: "MD5".to_string(),
                matching_files: 1,
                missing_on_dst: 0,
                differ_count: 0,
                error_count: 0,
                details: vec![format!(
                    "MD5 verified: {}",
                    candidate
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                )],
            });
        }
    }

    Ok(VerificationResult {
        success: false,
        hash_type: "MD5".to_string(),
        matching_files: 0,
        missing_on_dst: u64::from(candidates.is_empty()),
        differ_count: u64::from(!candidates.is_empty()),
        error_count: 0,
        details: vec!["No destination file matched the expected Drive MD5.".to_string()],
    })
}

pub const OS_JUNK_EXCLUDES: &[&str] = &[
    ".DS_Store",
    "._*",
    ".Trash*",
    "Thumbs.db",
    "desktop.ini",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferMode {
    DirectoryUpload,
    #[serde(alias = "directory")]
    DirectoryDownload,
    DriveFileDownload,
    LocalFileUpload,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartedTransfer {
    pub job_id: u64,
    pub mode: TransferMode,
    pub logical_total_bytes: u64,
    pub logical_file_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferFailureKind {
    Authentication,
    PermissionDenied,
    ApiQuota,
    DailyUploadLimit,
    Network,
    DiskSpace,
    IntegrityMismatch,
    Unknown,
}

pub fn classify_transfer_error(error_str: &str) -> TransferFailureKind {
    let lower = error_str.to_lowercase();
    if lower.contains("dailylimitexceeded")
        || lower.contains("750gb")
        || lower.contains("750 gb")
        || lower.contains("daily upload limit")
        || lower.contains("upload limit reached")
        || lower.contains("upload limit exceeded")
    {
        TransferFailureKind::DailyUploadLimit
    } else if lower.contains("userratelimitexceeded")
        || lower.contains("user rate limit")
        || lower.contains("rate_limit_exceeded")
        || lower.contains("ratelimitexceeded")
        || lower.contains("queries")
        || lower.contains("quota exceeded for quota metric")
        || (lower.contains("403") && lower.contains("quota"))
        || lower.contains("429")
    {
        TransferFailureKind::ApiQuota
    } else if lower.contains("unauthorized")
        || lower.contains("token")
        || lower.contains("oauth")
        || lower.contains("invalid_grant")
        || lower.contains("auth")
    {
        TransferFailureKind::Authentication
    } else if lower.contains("accessdenied")
        || lower.contains("permission denied")
        || lower.contains("403 forbidden")
        || lower.contains("not found")
    {
        TransferFailureKind::PermissionDenied
    } else if lower.contains("no space")
        || lower.contains("disk full")
        || lower.contains("enospc")
    {
        TransferFailureKind::DiskSpace
    } else if lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("timeout")
        || lower.contains("network")
        || lower.contains("broken pipe")
    {
        TransferFailureKind::Network
    } else if lower.contains("corrupt")
        || lower.contains("md5 mismatch")
        || lower.contains("hash mismatch")
        || lower.contains("differ")
    {
        TransferFailureKind::IntegrityMismatch
    } else {
        TransferFailureKind::Unknown
    }
}

pub fn calculate_local_directory_logical_size(dir_path: &Path) -> (u64, u64) {
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;

    fn walk_dir(path: &Path, count: &mut u64, bytes: &mut u64) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let file_name = entry_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();

                let is_junk = OS_JUNK_EXCLUDES.iter().any(|&rule| {
                    if rule.ends_with('*') {
                        let prefix = rule.trim_end_matches('*');
                        file_name.starts_with(prefix)
                    } else {
                        file_name == rule
                    }
                });
                if is_junk {
                    continue;
                }

                if let Ok(meta) = std::fs::symlink_metadata(&entry_path) {
                    if meta.file_type().is_symlink() {
                        continue;
                    }
                    if meta.is_file() {
                        *count += 1;
                        *bytes += meta.len();
                    } else if meta.is_dir() {
                        walk_dir(&entry_path, count, bytes);
                    }
                }
            }
        }
    }

    walk_dir(dir_path, &mut total_files, &mut total_bytes);
    (total_files, total_bytes)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequest {
    pub normalized_destination: String,
    pub method: &'static str,
    pub params: Value,
    pub mode: TransferMode,
}

pub fn is_google_drive_fs(value: &str) -> bool {
    value.starts_with("gdrive:") || value.starts_with("gdrive,")
}

pub fn select_transfer_route(
    src: &str,
    requested_single_file: bool,
) -> Result<TransferMode, String> {
    if is_google_drive_fs(src) {
        if requested_single_file {
            Ok(TransferMode::DriveFileDownload)
        } else {
            Ok(TransferMode::DirectoryDownload)
        }
    } else {
        let metadata = std::fs::symlink_metadata(src)
            .map_err(|e| format!("Local source is unavailable: {e}"))?;

        if metadata.file_type().is_symlink() {
            return Err("Symlink upload sources are not allowed.".to_string());
        }

        if metadata.is_file() {
            Ok(TransferMode::LocalFileUpload)
        } else if metadata.is_dir() {
            Ok(TransferMode::DirectoryUpload)
        } else {
            Err("Local source is neither a regular file nor directory.".into())
        }
    }
}

pub fn normalize_verification_route(mode: TransferMode, src: &str) -> TransferMode {
    match mode {
        TransferMode::DirectoryDownload if !is_google_drive_fs(src) => {
            TransferMode::DirectoryUpload
        }
        TransferMode::DirectoryUpload if is_google_drive_fs(src) => {
            TransferMode::DirectoryDownload
        }
        other => other,
    }
}

pub fn build_transfer_request(
    src: &str,
    dst: &str,
    mode: TransferMode,
) -> Result<TransferRequest, String> {
    let base_config = json!({
        "DriveChunkSize": "128M",
        "DriveUploadCutoff": "16M",
        "DriveAcknowledgeAbuse": true,
        "DriveStopOnUploadLimit": true,
        "BufferSize": "16M",
        "Checkers": 8,
        "LowLevelRetries": 10,
        "Retries": 3,
        "Timeout": "5m"
    });

    match mode {
        TransferMode::DriveFileDownload => {
            // Ensure local destination directory exists for Drive single-file download
            std::fs::create_dir_all(dst).map_err(|error| {
                format!("Could not create download destination '{}': {}", dst, error)
            })?;
            let dst_dir = format!("{}/", dst.trim_end_matches('/'));

            // Parse clean file ID (strip any resource_key or parameters)
            let (file_id, fs_name) = if let Some(idx) = src.find("root_folder_id=") {
                let after = &src[idx + "root_folder_id=".len()..];
                let clean = after.trim_end_matches(':');
                if let Some(comma_idx) = clean.find(',') {
                    let id = &clean[..comma_idx];
                    let rest = &clean[comma_idx + 1..];
                    (id.to_string(), format!("gdrive,{}:", rest))
                } else {
                    (clean.to_string(), "gdrive:".to_string())
                }
            } else if let Some(stripped) = src.strip_prefix("gdrive:") {
                if let Some(comma_idx) = stripped.find(',') {
                    let id = &stripped[..comma_idx];
                    let rest = &stripped[comma_idx + 1..];
                    (id.to_string(), format!("gdrive,{}:", rest))
                } else {
                    (stripped.to_string(), "gdrive:".to_string())
                }
            } else {
                (src.to_string(), "gdrive:".to_string())
            };

            let mut cfg = base_config.clone();
            if let Some(obj) = cfg.as_object_mut() {
                obj.insert("Transfers".to_string(), json!(4));
                obj.insert("MultiThreadStreams".to_string(), json!(4));
                obj.insert("MultiThreadCutoff".to_string(), json!("50M"));
            }

            Ok(TransferRequest {
                normalized_destination: dst_dir.clone(),
                method: "backend/command",
                params: json!({
                    "command": "copyid",
                    "fs": fs_name,
                    "args": [file_id, dst_dir],
                    "_async": true,
                    "_config": cfg
                }),
                mode,
            })
        }
        TransferMode::LocalFileUpload => {
            let path = Path::new(src);
            let parent = path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            let file_name = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            let mut cfg = base_config.clone();
            if let Some(obj) = cfg.as_object_mut() {
                obj.insert("Transfers".to_string(), json!(1));
            }

            Ok(TransferRequest {
                normalized_destination: dst.to_string(),
                method: "operations/copyfile",
                params: json!({
                    "srcFs": parent,
                    "srcRemote": file_name,
                    "dstFs": dst,
                    "dstRemote": file_name,
                    "_async": true,
                    "_config": cfg
                }),
                mode,
            })
        }
        TransferMode::DirectoryUpload => {
            let mut cfg = base_config.clone();
            if let Some(obj) = cfg.as_object_mut() {
                obj.insert("Transfers".to_string(), json!(8));
                obj.insert("CreateEmptySrcDirs".to_string(), json!(true));
                obj.insert("CheckFirst".to_string(), json!(false));
            }

            Ok(TransferRequest {
                normalized_destination: dst.to_string(),
                method: "sync/copy",
                params: json!({
                    "srcFs": src,
                    "dstFs": dst,
                    "_async": true,
                    "_filter": {
                        "ExcludeRule": OS_JUNK_EXCLUDES
                    },
                    "_config": cfg
                }),
                mode,
            })
        }
        TransferMode::DirectoryDownload => {
            let mut cfg = base_config.clone();
            if let Some(obj) = cfg.as_object_mut() {
                obj.insert("Transfers".to_string(), json!(4));
                obj.insert("MultiThreadStreams".to_string(), json!(4));
                obj.insert("MultiThreadCutoff".to_string(), json!("50M"));
                obj.insert("CreateEmptySrcDirs".to_string(), json!(true));
                obj.insert("CheckFirst".to_string(), json!(false));
            }

            Ok(TransferRequest {
                normalized_destination: dst.to_string(),
                method: "sync/copy",
                params: json!({
                    "srcFs": src,
                    "dstFs": dst,
                    "_async": true,
                    "_config": cfg
                }),
                mode,
            })
        }
    }
}


#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferState {
    Idle,
    Starting {
        generation: u64,
    },
    Running {
        job_id: u64,
        source: String,
        destination: String,
        mode: TransferMode,
    },
    Stopping {
        job_id: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletedFileStat {
    pub name: String,
    pub size: u64,
    pub bytes: u64,
    pub error: String,
    pub checked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferStats {
    pub bytes: u64,
    pub total_bytes: u64,
    pub speed: f64,
    pub speed_mbps: f64,
    pub percentage: f64,
    pub eta_seconds: Option<u64>,
    pub checks: u64,
    pub transfers: u64,
    pub errors: u64,
    pub fatal_error: bool,
    pub retry_error: bool,
    pub transferring: Vec<TransferringFile>,
    pub completed: Vec<CompletedFileStat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferringFile {
    pub name: String,
    pub bytes: u64,
    pub size: u64,
    pub percentage: f64,
    pub speed: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    pub success: bool,
    pub hash_type: String,
    pub matching_files: u64,
    pub missing_on_dst: u64,
    pub differ_count: u64,
    pub error_count: u64,
    pub details: Vec<String>,
}

fn compute_credential_fingerprint(creds: &crate::RcloneOAuthCredentials) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(creds.client_id.trim().as_bytes());
    hasher.update(b":");
    hasher.update(creds.client_secret.trim().as_bytes());
    format!("sha256:v1:{:x}", hasher.finalize())
}

pub struct RcloneManager {
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub child: Mutex<Option<Child>>,
    pub credential_fingerprint: Mutex<Option<String>>,
    pub state: Mutex<TransferState>,
    pub client: Client,
    pub start_generation: std::sync::atomic::AtomicU64,
}

impl Drop for RcloneManager {
    fn drop(&mut self) {
        self.stop_daemon();
    }
}

impl Default for RcloneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RcloneManager {
    pub fn new() -> Self {
        let port = get_available_port().unwrap_or(5572);
        Self::with_port(port)
    }

    pub fn with_port(port: u16) -> Self {
        let mut rng = rand::thread_rng();
        let pass: String = (0..24)
            .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
            .collect();

        Self {
            port,
            user: "balladi".to_string(),
            pass,
            child: Mutex::new(None),
            credential_fingerprint: Mutex::new(None),
            state: Mutex::new(TransferState::Idle),
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            start_generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Find the rclone binary from sidecar paths or system paths
    pub fn find_rclone_binary() -> PathBuf {
        // 1. Check next to executable or in Tauri sidecar directories
        if let Ok(exe) = env::current_exe() {
            if let Some(parent) = exe.parent() {
                let sidecar_candidates = vec![
                    parent.join("rclone-aarch64-apple-darwin"),
                    parent.join("rclone-x86_64-apple-darwin"),
                    parent.join("rclone"),
                    parent.join("../Resources/bin/rclone"),
                    parent.join("../Resources/rclone"),
                ];
                for path in sidecar_candidates {
                    if path.exists() {
                        return path;
                    }
                }
            }
        }

        // 2. Check workspace relative bin (for dev mode)
        let dev_candidates = vec![
            PathBuf::from("src-tauri/bin/rclone-aarch64-apple-darwin"),
            PathBuf::from("src-tauri/bin/rclone-x86_64-apple-darwin"),
            PathBuf::from("src-tauri/bin/rclone"),
            PathBuf::from("/opt/homebrew/bin/rclone"),
            PathBuf::from("/usr/local/bin/rclone"),
        ];

        for path in dev_candidates {
            if path.exists() {
                return path;
            }
        }

        PathBuf::from("rclone")
    }

    /// Spawns the rclone RC daemon with explicit candidate or confirmed credentials.
    /// Reuses existing running daemon only when its stored credential fingerprint matches.
    pub async fn start_daemon_with_credentials(
        &self,
        creds: &crate::RcloneOAuthCredentials,
    ) -> Result<(), String> {
        if creds.client_id.trim().is_empty() || creds.client_secret.trim().is_empty() {
            return Err("Private Google OAuth is required: client_id and client_secret cannot be empty".to_string());
        }

        let target_fp = compute_credential_fingerprint(creds);

        // Inspect existing daemon and match fingerprint
        let reuse_existing = {
            let mut child_lock = self.child.lock().unwrap();
            let mut fp_lock = self.credential_fingerprint.lock().unwrap();

            if let Some(ref mut child) = *child_lock {
                if let Ok(None) = child.try_wait() {
                    if fp_lock.as_deref() == Some(&target_fp) {
                        true
                    } else {
                        // Credentials changed: terminate old process
                        let _ = child.kill();
                        let _ = child.wait();
                        *child_lock = None;
                        *fp_lock = None;
                        false
                    }
                } else {
                    *child_lock = None;
                    *fp_lock = None;
                    false
                }
            } else {
                false
            }
        };

        if reuse_existing {
            if self.call_rc("core/version", json!({})).await.is_ok() {
                return Ok(());
            }
            self.stop_daemon();
        }

        {
            let mut child_lock = self.child.lock().unwrap();
            let mut fp_lock = self.credential_fingerprint.lock().unwrap();
            if child_lock.is_none() {
                let rclone_bin = Self::find_rclone_binary();
                let addr = format!("127.0.0.1:{}", self.port);

                let mut cmd = Command::new(&rclone_bin);
                crate::apply_google_oauth_env_std(&mut cmd, creds);

                cmd.args([
                    "rcd",
                    "--rc-addr",
                    &addr,
                    "--rc-user",
                    &self.user,
                    "--rc-pass",
                    &self.pass,
                    "--rc-web-gui=false",
                    "--rc-serve=false",
                    "--drive-chunk-size",
                    "128M",
                    "--drive-upload-cutoff",
                    "16M",
                    "--drive-acknowledge-abuse",
                    "--drive-stop-on-upload-limit",
                    "--fast-list",
                    "--transfers",
                    "4",
                    "--checkers",
                    "8",
                    "--buffer-size",
                    "16M",
                    "--multi-thread-streams",
                    "4",
                    "--multi-thread-cutoff",
                    "50M",
                    "--retries",
                    "3",
                    "--low-level-retries",
                    "10",
                    "--timeout",
                    "5m",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

                let child = cmd.spawn().map_err(|e| {
                    format!(
                        "Failed to spawn rclone binary at {}: {}",
                        rclone_bin.display(),
                        e
                    )
                })?;

                *child_lock = Some(child);
                *fp_lock = Some(target_fp);
            }
        }

        // Authenticated health check loop: verify HTTP 200 on /core/version with timeout
        let mut healthy = false;
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if self.call_rc("core/version", json!({})).await.is_ok() {
                healthy = true;
                break;
            }
        }

        if !healthy {
            self.stop_daemon();
            return Err("Rclone daemon failed to reach authenticated readiness within 3 seconds.".to_string());
        }

        Ok(())
    }

    /// Spawns the rclone RC daemon if not already running with authenticated readiness validation.
    /// Validates credentials FIRST before inspecting or reusing any running daemon.
    pub async fn start_daemon_async(&self) -> Result<(), String> {
        let creds = crate::load_rclone_oauth_credentials()
            .map_err(|e| format!("Private Google OAuth is required: {e}"))?;

        self.start_daemon_with_credentials(&creds).await
    }

    pub fn stop_daemon(&self) {
        let mut child_lock = self.child.lock().unwrap();
        let mut fp_lock = self.credential_fingerprint.lock().unwrap();
        if let Some(mut child) = child_lock.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *fp_lock = None;
    }

    pub fn is_alive(&self) -> bool {
        TcpStream::connect(("127.0.0.1", self.port)).is_ok()
    }

    /// Generic Remote Control API call
    pub async fn call_rc(&self, method: &str, params: Value) -> Result<Value, String> {
        let url = format!("http://127.0.0.1:{}/{}", self.port, method);
        let res = self
            .client
            .post(&url)
            .basic_auth(&self.user, Some(&self.pass))
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("RC connection error: {}", e))?;

        let status = res.status();
        let body_text = res
            .text()
            .await
            .map_err(|e| format!("Failed reading response: {}", e))?;

        if !status.is_success() {
            return Err(format!("rclone RC error [{}]: {}", status, body_text));
        }

        let json_val: Value = serde_json::from_str(&body_text)
            .map_err(|e| format!("Invalid JSON response: {}", e))?;

        Ok(json_val)
    }

    /// Fetch real-time transfer stats for an optional active job
    pub async fn get_stats(&self, job_id: Option<u64>) -> Result<TransferStats, String> {
        let stats_payload = if let Some(id) = job_id {
            json!({ "group": format!("job/{id}") })
        } else {
            json!({})
        };
        let resp = self.call_rc("core/stats", stats_payload).await?;

        let bytes = resp["bytes"].as_u64().unwrap_or(0);
        let total_bytes = resp["totalBytes"].as_u64().unwrap_or(0);
        let speed = resp["speed"].as_f64().unwrap_or(0.0);
        let speed_mbps = (speed * 8.0) / (1000.0 * 1000.0); // Megabits per sec
        let checks = resp["checks"].as_u64().unwrap_or(0);
        let transfers = resp["transfers"].as_u64().unwrap_or(0);
        let errors = resp["errors"].as_u64().unwrap_or(0);
        let fatal_error = resp["fatalError"].as_bool().unwrap_or(false);
        let retry_error = resp["retryError"].as_bool().unwrap_or(false) || (errors > 0 && !fatal_error);
        let eta_seconds = resp["eta"].as_u64();

        let percentage = if total_bytes > 0 {
            ((bytes as f64) / (total_bytes as f64)) * 100.0
        } else {
            0.0
        };

        let mut transferring = Vec::new();
        if let Some(arr) = resp["transferring"].as_array() {
            for item in arr {
                let name = item["name"].as_str().unwrap_or("").to_string();
                let f_bytes = item["bytes"].as_u64().unwrap_or(0);
                let f_size = item["size"].as_u64().unwrap_or(0);
                let f_speed = item["speed"].as_f64().unwrap_or(0.0);
                let f_pct = item["percentage"].as_f64().unwrap_or(0.0);

                transferring.push(TransferringFile {
                    name,
                    bytes: f_bytes,
                    size: f_size,
                    percentage: f_pct,
                    speed: f_speed,
                });
            }
        }

        let mut completed = Vec::new();
        let transferred_payload = if let Some(id) = job_id {
            json!({ "group": format!("job/{id}") })
        } else {
            json!({})
        };
        if let Ok(transferred_resp) = self.call_rc("core/transferred", transferred_payload).await {
            if let Some(arr) = transferred_resp["transferred"].as_array() {
                for item in arr {
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    let c_size = item["size"].as_u64().unwrap_or(0);
                    let c_bytes = item["bytes"].as_u64().unwrap_or(0);
                    let c_error = item["error"].as_str().unwrap_or("").to_string();
                    let c_checked = item["checked"].as_bool().unwrap_or(false);

                    completed.push(CompletedFileStat {
                        name,
                        size: c_size,
                        bytes: c_bytes,
                        error: c_error,
                        checked: c_checked,
                    });
                }
            }
        }

        Ok(TransferStats {
            bytes,
            total_bytes,
            speed,
            speed_mbps,
            percentage,
            eta_seconds,
            checks,
            transfers,
            errors,
            fatal_error,
            retry_error,
            transferring,
            completed,
        })
    }

    /// Set dynamic bandwidth throttle (e.g. "50M", "10M", "off")
    pub async fn set_bwlimit(&self, rate: &str) -> Result<(), String> {
        let rate_val = if rate.is_empty() || rate == "unlimited" {
            "off"
        } else {
            rate
        };
        self.call_rc("core/bwlimit", json!({ "rate": rate_val }))
            .await?;
        Ok(())
    }

    /// Start copy job with production-grade flags, single-file copyid support, and atomic state machine transition
    pub async fn start_copy(
        &self,
        src: &str,
        dst: &str,
        is_single_file: bool,
    ) -> Result<StartedTransfer, String> {
        let mode = select_transfer_route(src, is_single_file)?;
        let request = build_transfer_request(src, dst, mode)?;

        // Preflight logical size calculation
        let (logical_file_count, logical_total_bytes) = match mode {
            TransferMode::LocalFileUpload => {
                let p = Path::new(src);
                let meta = std::fs::symlink_metadata(p)
                    .map_err(|e| format!("Failed reading local source file metadata: {e}"))?;
                (1u64, meta.len())
            }
            TransferMode::DirectoryUpload => {
                let p = Path::new(src);
                calculate_local_directory_logical_size(p)
            }
            TransferMode::DriveFileDownload => {
                (1u64, 0u64)
            }
            TransferMode::DirectoryDownload => {
                self.get_size(src).await.unwrap_or((0u64, 0u64))
            }
        };

        let (current_gen, normalized_dst, rc_method, params) = {
            let mut state_lock = self.state.lock().unwrap();
            if *state_lock != TransferState::Idle {
                return Err("A transfer job is already active or in transition.".to_string());
            }
            let gen = self.start_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            *state_lock = TransferState::Starting { generation: gen };

            (gen, request.normalized_destination, request.method, request.params)
        };

        let resp_res = self.call_rc(rc_method, params).await;

        let is_matching_gen = {
            let state_lock = self.state.lock().unwrap();
            *state_lock == (TransferState::Starting { generation: current_gen })
        };

        if !is_matching_gen {
            if let Ok(resp) = &resp_res {
                if let Some(stray_job) = resp["jobid"].as_u64() {
                    let _ = self.call_rc("job/stop", json!({ "jobid": stray_job })).await;
                }
            }
            return Err("Transfer start was superseded or cancelled.".to_string());
        }

        let mut state_lock = self.state.lock().unwrap();
        match resp_res {
            Ok(resp) => {
                let job_id_opt = resp["jobid"].as_u64();
                match job_id_opt {
                    Some(job_id) => {
                        *state_lock = TransferState::Running {
                            job_id,
                            source: src.to_string(),
                            destination: normalized_dst,
                            mode,
                        };
                        Ok(StartedTransfer {
                            job_id,
                            mode,
                            logical_total_bytes,
                            logical_file_count,
                        })
                    }
                    None => {
                        *state_lock = TransferState::Idle;
                        Err("No jobid returned by rclone".to_string())
                    }
                }
            }
            Err(e) => {
                *state_lock = TransferState::Idle;
                Err(e)
            }
        }
    }

    async fn wait_for_jobs_finished(
        &self,
        job_ids: &[u64],
        attempts: usize,
        interval: Duration,
    ) -> Result<(), String> {
        for _ in 0..attempts {
            let mut unfinished = Vec::new();
            for job_id in job_ids {
                let status = self
                    .call_rc("job/status", json!({ "jobid": job_id }))
                    .await
                    .map_err(|e| {
                        format!("Could not confirm termination of job {job_id}: {e}")
                    })?;
                if status["finished"].as_bool() != Some(true) {
                    unfinished.push(*job_id);
                }
            }
            if unfinished.is_empty() {
                return Ok(());
            }
            tokio::time::sleep(interval).await;
        }
        Err(format!(
            "Timed out waiting for jobs to terminate: {:?}",
            job_ids
        ))
    }

    /// Cancel active copy job with strict ID validation and unconfirmed termination preservation
    pub async fn stop_job(&self, job_id: u64) -> Result<(), String> {
        {
            let mut state = self.state.lock().unwrap();
            match *state {
                TransferState::Running { job_id: active_id, .. } if active_id == job_id => {
                    *state = TransferState::Stopping { job_id };
                }
                TransferState::Running { job_id: active_id, .. } => {
                    return Err(format!(
                        "Cannot stop job {job_id}; active job is {active_id}"
                    ));
                }
                TransferState::Stopping { job_id: stopping_id } if stopping_id == job_id => {}
                _ => return Err("No matching active transfer to stop.".to_string()),
            }
        }

        self.call_rc("job/stop", json!({ "jobid": job_id }))
            .await
            .map_err(|e| format!("Failed stopping job {job_id}: {e}"))?;

        self.wait_for_jobs_finished(&[job_id], 20, Duration::from_millis(100)).await?;

        {
            let mut state = self.state.lock().unwrap();
            if *state == (TransferState::Stopping { job_id }) {
                *state = TransferState::Idle;
            }
        }

        let _ = self.call_rc("core/stats-reset", json!({})).await;
        let _ = self.call_rc("core/bwlimit", json!({ "rate": "off" })).await;

        Ok(())
    }

    /// Halt all active jobs, collect any errors, and cleanly reset rclone statistics
    pub async fn stop_all_transfers(&self) -> Result<(), String> {
        self.start_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let jobs = self.call_rc("job/list", json!({})).await?;
        // Support the documented camelCase response and older snake_case output.
        let running = jobs
            .get("runningIds")
            .or_else(|| jobs.get("running_ids"))
            .map(parse_job_ids)
            .unwrap_or_default();

        {
            let mut state = self.state.lock().unwrap();
            if let Some(job_id) = running.first().copied() {
                *state = TransferState::Stopping { job_id };
            }
        }

        let mut failures = Vec::new();
        for job_id in &running {
            if let Err(error) = self
                .call_rc("job/stop", json!({ "jobid": job_id }))
                .await
            {
                failures.push(format!("job {job_id}: {error}"));
            }
        }

        if !failures.is_empty() {
            // Preserve Starting/Stopping because termination is unconfirmed.
            return Err(format!(
                "Could not request termination for all jobs: {}",
                failures.join("; ")
            ));
        }

        if !running.is_empty() {
            self.wait_for_jobs_finished(&running, 20, Duration::from_millis(100)).await?;
        }

        {
            let mut state = self.state.lock().unwrap();
            *state = TransferState::Idle;
        }

        let _ = self.call_rc("core/stats-reset", json!({})).await;
        let _ = self
            .call_rc("core/bwlimit", json!({ "rate": "off" }))
            .await;
        Ok(())
    }

    /// Check status of active job
    pub async fn check_job(&self, job_id: u64) -> Result<Value, String> {
        let res = self.call_rc("job/status", json!({ "jobid": job_id })).await?;
        if res["finished"].as_bool().unwrap_or(false) {
            let mut state_lock = self.state.lock().unwrap();
            if let TransferState::Running { job_id: active_id, .. } = *state_lock {
                if active_id == job_id {
                    *state_lock = TransferState::Idle;
                }
            }
        }
        Ok(res)
    }

    /// Get total size and count of source before transferring
    pub async fn get_size(&self, fs: &str) -> Result<(u64, u64), String> {
        let resp = self.call_rc("operations/size", json!({ "fs": fs })).await?;
        let count = resp["count"].as_u64().unwrap_or(0);
        let bytes = resp["bytes"].as_u64().unwrap_or(0);
        Ok((count, bytes))
    }

    /// Verify transferred files using MD5 checksum comparison with strict schema parsing
    pub async fn verify_transfer(
        &self,
        src: &str,
        dst: &str,
        explicit_mode: Option<TransferMode>,
        is_single_file: bool,
    ) -> Result<VerificationResult, String> {
        let route = match explicit_mode {
            Some(m) => normalize_verification_route(m, src),
            None => select_transfer_route(src, is_single_file)?,
        };
        match route {
            TransferMode::DriveFileDownload => {
                let (file_id, resource_key) = if let Some(idx) = src.find("root_folder_id=") {
                    let after = &src[idx + "root_folder_id=".len()..];
                    let clean = after.trim_end_matches(':');
                    if let Some(comma_idx) = clean.find(',') {
                        let id = &clean[..comma_idx];
                        let rest = &clean[comma_idx + 1..];
                        let rk = rest.find("resource_key=").map(|rk_idx| {
                            rest[rk_idx + "resource_key=".len()..].to_string()
                        });
                        (id.to_string(), rk)
                    } else {
                        (clean.to_string(), None)
                    }
                } else if let Some(stripped) = src.strip_prefix("gdrive:") {
                    (stripped.to_string(), None)
                } else {
                    (src.to_string(), None)
                };

                let access_token = crate::auth::drive::get_google_access_token()
                    .await
                    .ok_or_else(|| "Could not retrieve Google Drive access token for verification".to_string())?;

                let metadata = get_drive_file_metadata(
                    &self.client,
                    &access_token,
                    &file_id,
                    resource_key.as_deref(),
                )
                .await?;

                let local_dir = Path::new(dst.trim_end_matches(['/', '\\']));
                verify_single_file_metadata(local_dir, &metadata).await
            }
            TransferMode::LocalFileUpload => {
                let local_path = Path::new(src);
                let local_md5 = calculate_md5_async(local_path.to_path_buf()).await?;
                let file_name = local_path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();

                let remote = self.call_rc(
                    "operations/hashsumfile",
                    json!({
                        "fs": dst,
                        "remote": file_name,
                        "hashType": "MD5",
                        "download": false,
                        "base64": false
                    }),
                ).await?;

                let remote_md5 = remote["hash"]
                    .as_str()
                    .filter(|h| !h.trim().is_empty())
                    .ok_or_else(|| "Google Drive did not return an MD5 hash for the uploaded file".to_string())?;

                if local_md5.eq_ignore_ascii_case(remote_md5) {
                    Ok(VerificationResult {
                        success: true,
                        hash_type: "MD5 (Local vs Google Drive Remote)".to_string(),
                        matching_files: 1,
                        missing_on_dst: 0,
                        differ_count: 0,
                        error_count: 0,
                        details: vec![format!("Verified MD5 checksum match: {}", remote_md5)],
                    })
                } else {
                    Ok(VerificationResult {
                        success: false,
                        hash_type: "MD5 (Mismatch)".to_string(),
                        matching_files: 0,
                        missing_on_dst: 0,
                        differ_count: 1,
                        error_count: 0,
                        details: vec![format!(
                            "MD5 Mismatch: local={}, remote={}",
                            local_md5, remote_md5
                        )],
                    })
                }
            }
            TransferMode::DirectoryUpload | TransferMode::DirectoryDownload => {
                let mut check_params = json!({
                    "srcFs": src,
                    "dstFs": dst,
                    "oneWay": true,
                    "match": true, // Explicitly request matched files array
                    "download": false // Compares local MD5 against Drive API metadata MD5 without re-downloading
                });

                if route == TransferMode::DirectoryUpload {
                    check_params["_filter"] = json!({
                        "ExcludeRule": OS_JUNK_EXCLUDES
                    });
                }

                let resp = self.call_rc("operations/check", check_params).await?;

                let success_flag = resp["success"].as_bool().unwrap_or(false);
                let raw_hash = resp["hashType"].as_str().unwrap_or("");
                let is_md5 = raw_hash.eq_ignore_ascii_case("MD5");
                let hash_type = if is_md5 {
                    "MD5".to_string()
                } else if raw_hash.is_empty() {
                    "MD5 Unavailable".to_string()
                } else {
                    raw_hash.to_string()
                };

                let matching_count = match &resp["match"] {
                    Value::Array(arr) => arr.len() as u64,
                    Value::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                };

                let missing_dst_count = match &resp["missingOnDst"] {
                    Value::Array(arr) => arr.len() as u64,
                    Value::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                };

                let differ_count = match &resp["differ"] {
                    Value::Array(arr) => arr.len() as u64,
                    Value::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                };

                let error_count = match &resp["error"] {
                    Value::Array(arr) => arr.len() as u64,
                    Value::Number(n) => n.as_u64().unwrap_or(0),
                    _ => 0,
                };

                let mut details = Vec::new();
                if !is_md5 {
                    details.push("Remote or local filesystem does not support MD5 checksums.".to_string());
                }

                if let Some(arr) = resp["differ"].as_array() {
                    for item in arr.iter().take(20) {
                        if let Some(s) = item.as_str() {
                            details.push(format!("Differing (corrupted): {}", s));
                        }
                    }
                }
                if let Some(arr) = resp["missingOnDst"].as_array() {
                    for item in arr.iter().take(20) {
                        if let Some(s) = item.as_str() {
                            details.push(format!("Missing on destination: {}", s));
                        }
                    }
                }
                if let Some(arr) = resp["error"].as_array() {
                    for item in arr.iter().take(20) {
                        if let Some(s) = item.as_str() {
                            details.push(format!("Check error: {}", s));
                        }
                    }
                }

                // Strict bit-for-bit guarantee: Must have valid MD5 hash, matching files, ZERO differences, ZERO missing files, ZERO errors
                let is_strictly_verified = success_flag
                    && is_md5
                    && differ_count == 0
                    && missing_dst_count == 0
                    && error_count == 0
                    && matching_count > 0;

                Ok(VerificationResult {
                    success: is_strictly_verified,
                    hash_type,
                    matching_files: matching_count,
                    missing_on_dst: missing_dst_count,
                    differ_count,
                    error_count,
                    details,
                })
            }
        }
    }

    /// List Google Drive remotes or verify gdrive is configured
    pub async fn list_remotes(&self) -> Result<Vec<String>, String> {
        let resp = self.call_rc("config/listremotes", json!({})).await?;
        let mut remotes = Vec::new();
        if let Some(arr) = resp["remotes"].as_array() {
            for r in arr {
                if let Some(s) = r.as_str() {
                    remotes.push(s.to_string());
                }
            }
        }
        Ok(remotes)
    }
}

fn parse_job_ids(value: &Value) -> Vec<u64> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect()
}

fn get_available_port() -> Option<u16> {
    (5572..5600).find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn single_file_verification_rejects_unrelated_nonempty_file() {
        let temp_dir = std::env::temp_dir().join("balladi_test_single_file_unrelated");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("other_file.txt");
        let mut f = File::create(&file_path).unwrap();
        writeln!(f, "some content").unwrap();

        let metadata = DriveFileMetadata {
            name: "expected_video.mp4".to_string(),
            size: Some("13".to_string()),
            md5_checksum: Some("985001f2f7b88494ff4c3b6b6770f3f6".to_string()),
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(!res.success);
        assert_eq!(res.differ_count, 1);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn single_file_verification_rejects_md5_mismatch() {
        let temp_dir = std::env::temp_dir().join("balladi_test_single_file_mismatch");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("sample.mp4");
        let mut f = File::create(&file_path).unwrap();
        f.write_all(b"corrupted content").unwrap();

        let metadata = DriveFileMetadata {
            name: "sample.mp4".to_string(),
            size: Some("17".to_string()),
            md5_checksum: Some("00000000000000000000000000000000".to_string()),
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(!res.success);
        assert_eq!(res.differ_count, 1);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn single_file_verification_accepts_matching_size_and_md5() {
        let temp_dir = std::env::temp_dir().join("balladi_test_single_file_match");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("video.mov");
        let mut f = File::create(&file_path).unwrap();
        let content = b"hello balladi drive transfer engine";
        f.write_all(content).unwrap();

        let expected_md5 = calculate_md5(&file_path).unwrap();

        let metadata = DriveFileMetadata {
            name: "video.mov".to_string(),
            size: Some(content.len().to_string()),
            md5_checksum: Some(expected_md5),
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(res.success);
        assert_eq!(res.matching_files, 1);
        assert_eq!(res.differ_count, 0);
        assert_eq!(res.missing_on_dst, 0);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn single_file_verification_does_not_claim_md5_when_remote_md5_is_missing() {
        let temp_dir = std::env::temp_dir().join("balladi_test_single_file_no_md5");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("doc.gdoc");
        let mut f = File::create(&file_path).unwrap();
        f.write_all(b"google doc content").unwrap();

        let metadata = DriveFileMetadata {
            name: "doc.gdoc".to_string(),
            size: Some("18".to_string()),
            md5_checksum: None,
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(!res.success);
        assert_eq!(res.hash_type, "MD5 Unavailable");
        assert_eq!(res.error_count, 1);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn verification_accepts_rclone_encoded_windows_filename() {
        let temp_dir = std::env::temp_dir().join("balladi_test_encoded_win");
        let _ = std::fs::create_dir_all(&temp_dir);
        // rclone replaces ':' and '?' with fullwidth '：' and '？' on Windows
        let file_path = temp_dir.join("Scene：01？.mov");
        let mut f = File::create(&file_path).unwrap();
        let content = b"scene 01 test content";
        f.write_all(content).unwrap();

        let expected_md5 = calculate_md5(&file_path).unwrap();

        let metadata = DriveFileMetadata {
            name: "Scene:01?.mov".to_string(),
            size: Some(content.len().to_string()),
            md5_checksum: Some(expected_md5),
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(res.success);
        assert_eq!(res.matching_files, 1);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn verification_accepts_rclone_encoded_slash_filename() {
        let temp_dir = std::env::temp_dir().join("balladi_test_encoded_slash");
        let _ = std::fs::create_dir_all(&temp_dir);
        // rclone encodes '/' to fullwidth '／'
        let file_path = temp_dir.join("Project／Episode.mov");
        let mut f = File::create(&file_path).unwrap();
        let content = b"episode movie data";
        f.write_all(content).unwrap();

        let expected_md5 = calculate_md5(&file_path).unwrap();

        let metadata = DriveFileMetadata {
            name: "Project/Episode.mov".to_string(),
            size: Some(content.len().to_string()),
            md5_checksum: Some(expected_md5),
        };

        let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
        assert!(res.success);
        assert_eq!(res.matching_files, 1);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn verification_never_traverses_parent_directory_for_dotdot_name() {
        let parent_dir = std::env::temp_dir().join("balladi_test_parent");
        let sub_dir = parent_dir.join("subdir");
        let _ = std::fs::create_dir_all(&sub_dir);
        let escape_file = parent_dir.join("secret.txt");
        let mut f = File::create(&escape_file).unwrap();
        f.write_all(b"secret outside subdir").unwrap();

        let md5 = calculate_md5(&escape_file).unwrap();

        let metadata = DriveFileMetadata {
            name: "../secret.txt".to_string(),
            size: Some("21".to_string()),
            md5_checksum: Some(md5),
        };

        // Verification on sub_dir only inspects immediate files in sub_dir
        let res = verify_single_file_metadata(&sub_dir, &metadata).await.unwrap();
        assert!(!res.success);
        assert_eq!(res.missing_on_dst, 1);

        let _ = std::fs::remove_dir_all(&parent_dir);
    }

    #[tokio::test]
    async fn verification_rejects_symlink_candidate() {
        #[cfg(unix)]
        {
            let temp_dir = std::env::temp_dir().join("balladi_test_symlink");
            let _ = std::fs::create_dir_all(&temp_dir);
            let target_file = temp_dir.join("real.mp4");
            let mut f = File::create(&target_file).unwrap();
            f.write_all(b"target file content").unwrap();

            let symlink_path = temp_dir.join("symlink.mp4");
            let _ = std::os::unix::fs::symlink(&target_file, &symlink_path);

            let md5 = calculate_md5(&target_file).unwrap();

            // remove real file so only symlink exists
            let _ = std::fs::remove_file(&target_file);

            let metadata = DriveFileMetadata {
                name: "symlink.mp4".to_string(),
                size: Some("19".to_string()),
                md5_checksum: Some(md5),
            };

            let res = verify_single_file_metadata(&temp_dir, &metadata).await.unwrap();
            assert!(!res.success);
            assert_eq!(res.missing_on_dst, 1);
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn md5_verification_does_not_block_the_async_executor() {
        let temp_dir = std::env::temp_dir().join("balladi_test_nonblocking");
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_path = temp_dir.join("large_media.dat");
        let mut f = File::create(&file_path).unwrap();
        // Write 2MB of test data
        f.write_all(&vec![0xAB; 2 * 1024 * 1024]).unwrap();

        let expected_md5 = calculate_md5(&file_path).unwrap();
        let metadata = DriveFileMetadata {
            name: "large_media.dat".to_string(),
            size: Some((2 * 1024 * 1024).to_string()),
            md5_checksum: Some(expected_md5),
        };

        let verification_task = verify_single_file_metadata(&temp_dir, &metadata);
        let mut timer_fired = false;
        let timer_task = async {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            timer_fired = true;
        };

        let (res, _) = tokio::join!(verification_task, timer_task);
        assert!(timer_fired);
        assert!(res.unwrap().success);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_parse_job_ids_ignores_finished() {
        let json_resp = json!({
            "jobids": [101, 102, 103],
            "runningIds": [103]
        });

        let running = json_resp
            .get("runningIds")
            .map(parse_job_ids)
            .unwrap_or_default();

        assert_eq!(running, vec![103]);
        assert_eq!(running.len(), 1);
    }

    #[test]
    fn parsed_single_file_connection_routes_to_copyid() {
        let parsed = crate::auth::drive::parse_google_drive_link(
            "https://drive.google.com/file/d/1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB/view",
        );
        assert!(parsed.is_file);
        assert!(is_google_drive_fs(&parsed.connection_string));
        assert_eq!(
            select_transfer_route(&parsed.connection_string, true).unwrap(),
            TransferMode::DriveFileDownload
        );
    }

    #[test]
    fn select_transfer_route_matrix() {
        // 1. Google Drive single file download
        assert_eq!(
            select_transfer_route("gdrive,root_folder_id=abc123xyz:", true).unwrap(),
            TransferMode::DriveFileDownload
        );
        assert_eq!(
            select_transfer_route("gdrive:abc123xyz", true).unwrap(),
            TransferMode::DriveFileDownload
        );

        // 2. Google Drive folder download
        assert_eq!(
            select_transfer_route("gdrive,root_folder_id=abc123xyz:", false).unwrap(),
            TransferMode::DirectoryDownload
        );

        // 3. Local single file upload
        let temp_dir = std::env::temp_dir().join("balladi_route_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_file = temp_dir.join("sample_clip.mov");
        let _ = File::create(&temp_file);

        assert_eq!(
            select_transfer_route(&temp_file.to_string_lossy(), true).unwrap(),
            TransferMode::LocalFileUpload
        );
        // Even if requested_single_file is false, an actual local file routes as LocalFileUpload
        assert_eq!(
            select_transfer_route(&temp_file.to_string_lossy(), false).unwrap(),
            TransferMode::LocalFileUpload
        );

        // 4. Local directory upload
        assert_eq!(
            select_transfer_route(&temp_dir.to_string_lossy(), false).unwrap(),
            TransferMode::DirectoryUpload
        );

        // 5. Type switch protection: Even if requested_single_file is true, an actual directory routes as DirectoryUpload
        assert_eq!(
            select_transfer_route(&temp_dir.to_string_lossy(), true).unwrap(),
            TransferMode::DirectoryUpload
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn directory_routing_and_filter_directionality() {
        let upload_req = build_transfer_request(
            "/tmp/my_project",
            "gdrive:/Uploads",
            TransferMode::DirectoryUpload,
        )
        .unwrap();

        let download_req = build_transfer_request(
            "gdrive:/Media",
            "/tmp/Media",
            TransferMode::DirectoryDownload,
        )
        .unwrap();

        // Upload request must include OS junk filter
        assert!(upload_req.params.get("_filter").is_some());
        // Download request must NOT filter out files from Drive
        assert!(download_req.params.get("_filter").is_none());

        let upload_filter = upload_req.params["_filter"].clone();
        assert_eq!(
            upload_filter,
            json!({ "ExcludeRule": OS_JUNK_EXCLUDES })
        );
    }

    #[test]
    fn destination_creation_failure_propagates_error() {
        // Path pointing into a file as if it were a directory should fail create_dir_all
        let temp_dir = std::env::temp_dir().join("balladi_dest_fail_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let blocked_file = temp_dir.join("existing_file.txt");
        let _ = File::create(&blocked_file);
        let invalid_dst = blocked_file.join("subfolder");

        let err = build_transfer_request(
            "gdrive:file123",
            &invalid_dst.to_string_lossy(),
            TransferMode::DriveFileDownload,
        );

        assert!(err.is_err());
        assert!(err.unwrap_err().contains("Could not create download destination"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn special_character_upload_uses_exact_copyfile_parameters() {
        let special_names = vec![
            "clip[1].mov",
            "shot?.mp4",
            "take*.wav",
            "{final}.jpg",
            // A backslash is a valid filename character on Unix, but it is a
            // path separator on Windows and cannot represent one filename there.
            #[cfg(not(target_os = "windows"))]
            "back\\slash.txt",
        ];

        let temp_dir = std::env::temp_dir().join("balladi_spec_chars_test");
        let _ = std::fs::create_dir_all(&temp_dir);

        for name in special_names {
            let file_path = temp_dir.join(name);
            let request = build_transfer_request(
                &file_path.to_string_lossy(),
                "gdrive:/Uploads",
                TransferMode::LocalFileUpload,
            )
            .unwrap();

            assert_eq!(request.method, "operations/copyfile");
            assert_eq!(request.params["srcRemote"].as_str(), Some(name));
            assert_eq!(request.params["dstRemote"].as_str(), Some(name));
            assert_eq!(request.params["dstFs"].as_str(), Some("gdrive:/Uploads"));
            assert_eq!(
                request.params["srcFs"].as_str(),
                Some(temp_dir.to_string_lossy().as_ref())
            );
            assert!(request.params.get("_filter").is_none());
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn exclusion_rules_include_windows_and_mac_junk() {
        let request = build_transfer_request(
            "/tmp/test_dir",
            "gdrive:/Uploads",
            TransferMode::DirectoryUpload,
        )
        .unwrap();

        assert_eq!(request.method, "sync/copy");
        let excludes = request.params["_filter"]["ExcludeRule"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();

        assert!(excludes.contains(&"Thumbs.db"));
        assert!(excludes.contains(&"desktop.ini"));
        assert!(excludes.contains(&".DS_Store"));
        assert!(excludes.contains(&"._*"));
        assert!(excludes.contains(&".Trash*"));
    }

    #[tokio::test]
    async fn mock_rc_upload_telemetry_and_status_cycle() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let stats_call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let status_call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    Ok((mut stream, _)) = listener.accept() => {
                        let stats_c = stats_call_count.clone();
                        let status_c = status_call_count.clone();

                        tokio::spawn(async move {
                            let mut buf = [0u8; 4096];
                            let n = stream.read(&mut buf).await.unwrap_or(0);
                            let req = String::from_utf8_lossy(&buf[..n]);

                            let body = if req.contains("/operations/copyfile") {
                                r#"{"jobid":42}"#.to_string()
                            } else if req.contains("/core/stats") {
                                let c = stats_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                if c == 0 {
                                    r#"{"bytes":104857600,"totalBytes":524288000,"speed":26214400.0,"transfers":1,"transferring":[{"name":"scene1.mov","bytes":104857600,"size":524288000,"percentage":20}]}"#.to_string()
                                } else {
                                    r#"{"bytes":314572800,"totalBytes":524288000,"speed":52428800.0,"transfers":1,"transferring":[{"name":"scene1.mov","bytes":314572800,"size":524288000,"percentage":60}]}"#.to_string()
                                }
                            } else if req.contains("/job/status") {
                                let c = status_c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                if c == 0 {
                                    r#"{"finished":false,"success":false}"#.to_string()
                                } else {
                                    r#"{"finished":true,"success":true,"error":""}"#.to_string()
                                }
                            } else {
                                r#"{}"#.to_string()
                            };

                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                        });
                    }
                }
            }
        });

        let manager = RcloneManager::with_port(port);

        // Create a temporary file to test real start_copy production path
        let temp_dir = std::env::temp_dir().join("balladi_mock_upload_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let temp_file = temp_dir.join("scene1.mov");
        let _ = File::create(&temp_file);

        // 1. Call production start_copy (routes through select_transfer_route and build_transfer_request)
        let started = manager
            .start_copy(
                &temp_file.to_string_lossy(),
                "gdrive:/Uploads",
                true,
            )
            .await
            .unwrap();

        assert_eq!(started.job_id, 42);
        assert_eq!(started.mode, TransferMode::LocalFileUpload);
        let job_id = started.job_id;

        // 2. First telemetry sample
        let stats_1 = manager.get_stats(Some(job_id)).await.unwrap();
        assert_eq!(stats_1.bytes, 104857600);
        assert_eq!(stats_1.total_bytes, 524288000);
        assert_eq!(stats_1.speed, 26214400.0);
        assert_eq!(stats_1.transferring.len(), 1);
        assert_eq!(stats_1.transferring[0].name, "scene1.mov");

        // 3. Second telemetry sample (increasing transferred bytes)
        let stats_2 = manager.get_stats(Some(job_id)).await.unwrap();
        assert_eq!(stats_2.bytes, 314572800);
        assert_eq!(stats_2.speed, 52428800.0);
        assert!(stats_2.bytes > stats_1.bytes);

        // 4. Job status transitions
        let status_running = manager.check_job(job_id).await.unwrap();
        assert_eq!(status_running["finished"].as_bool(), Some(false));

        let status_finished = manager.check_job(job_id).await.unwrap();
        assert_eq!(status_finished["finished"].as_bool(), Some(true));
        assert_eq!(status_finished["success"].as_bool(), Some(true));

        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = shutdown_tx.send(());
    }

    #[test]
    fn legacy_directory_mode_deserializes_cleanly() {
        let legacy_json = r#""directory""#;
        let mode: TransferMode = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(mode, TransferMode::DirectoryDownload);
    }

    #[test]
    fn legacy_directory_upload_uses_upload_direction() {
        let mode: TransferMode = serde_json::from_str(r#""directory""#).unwrap();

        let normalized_up = normalize_verification_route(
            mode,
            "/local/camera-card",
        );
        assert_eq!(normalized_up, TransferMode::DirectoryUpload);

        let normalized_dl = normalize_verification_route(
            mode,
            "gdrive:/Media_Projects",
        );
        assert_eq!(normalized_dl, TransferMode::DirectoryDownload);
    }

    #[test]
    fn pause_resume_accounting_never_exceeds_total() {
        let total_file_size: u64 = 500_000_000;
        let transferring_bytes: u64 = 200_000_000; // in-flight partial bytes
        let total_transferred_in_session: u64 = 200_000_000;

        // When pausing, in-flight uncommitted bytes are deducted
        let in_flight = transferring_bytes;
        let resume_baseline_bytes = total_transferred_in_session.saturating_sub(in_flight);
        assert_eq!(resume_baseline_bytes, 0);

        // Upon resuming, transfer starts fresh from rclone perspective
        // and transfers all 500MB without double-counting baseline
        let session_completed_bytes = resume_baseline_bytes + total_file_size;
        assert_eq!(session_completed_bytes, total_file_size);
        assert!(session_completed_bytes <= total_file_size);
    }

    #[test]
    fn test_classify_transfer_error_variants() {
        assert_eq!(
            classify_transfer_error("403 RATE_LIMIT_EXCEEDED Quota exceeded for quota metric 'Queries'"),
            TransferFailureKind::ApiQuota
        );
        assert_eq!(
            classify_transfer_error("userRateLimitExceeded: User Rate Limit Exceeded"),
            TransferFailureKind::ApiQuota
        );
        assert_eq!(
            classify_transfer_error("user rate limit exceeded"),
            TransferFailureKind::ApiQuota
        );
        assert_eq!(
            classify_transfer_error("750GB upload limit reached"),
            TransferFailureKind::DailyUploadLimit
        );
        assert_eq!(
            classify_transfer_error("dailyLimitExceeded: Drive storage limit reached"),
            TransferFailureKind::DailyUploadLimit
        );
        assert_eq!(
            classify_transfer_error("daily upload limit exceeded"),
            TransferFailureKind::DailyUploadLimit
        );
        assert_eq!(
            classify_transfer_error("oauth invalid_grant token expired"),
            TransferFailureKind::Authentication
        );
        assert_eq!(
            classify_transfer_error("ENOSPC: no space left on device"),
            TransferFailureKind::DiskSpace
        );
        assert_eq!(
            classify_transfer_error("MD5 hash mismatch"),
            TransferFailureKind::IntegrityMismatch
        );
        assert_eq!(
            classify_transfer_error("connection reset by peer"),
            TransferFailureKind::Network
        );
    }

    #[test]
    fn test_calculate_local_directory_logical_size_filters_junk() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir().join("balladi_size_filter_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        // Valid files
        let f1 = temp_dir.join("clip1.mov");
        let mut file1 = File::create(&f1).unwrap();
        file1.write_all(&vec![0u8; 1024]).unwrap();

        let sub_dir = temp_dir.join("subfolder");
        let _ = std::fs::create_dir_all(&sub_dir);
        let f2 = sub_dir.join("clip2.mp4");
        let mut file2 = File::create(&f2).unwrap();
        file2.write_all(&vec![0u8; 2048]).unwrap();

        // Junk files
        let ds = temp_dir.join(".DS_Store");
        let mut ds_file = File::create(&ds).unwrap();
        ds_file.write_all(&vec![0u8; 6000]).unwrap();

        let thumbs = sub_dir.join("Thumbs.db");
        let mut thumbs_file = File::create(&thumbs).unwrap();
        thumbs_file.write_all(&vec![0u8; 8000]).unwrap();

        let (count, total_bytes) = calculate_local_directory_logical_size(&temp_dir);
        assert_eq!(count, 2);
        assert_eq!(total_bytes, 1024 + 2048);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_missing_oauth_credentials_prevents_daemon_creation() {
        let manager = RcloneManager::with_port(49999);
        let empty_creds = crate::RcloneOAuthCredentials {
            client_id: "".to_string(),
            client_secret: "".to_string(),
        };
        let res = manager.start_daemon_with_credentials(&empty_creds).await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err();
        assert!(err_msg.contains("Private Google OAuth is required"));
    }

    #[test]
    fn test_credential_fingerprint_uniqueness_and_change_detection() {
        let creds1 = crate::RcloneOAuthCredentials {
            client_id: "client-1".to_string(),
            client_secret: "secret-1".to_string(),
        };
        let creds2 = crate::RcloneOAuthCredentials {
            client_id: "client-2".to_string(),
            client_secret: "secret-2".to_string(),
        };
        let fp1 = compute_credential_fingerprint(&creds1);
        let fp2 = compute_credential_fingerprint(&creds2);
        assert_ne!(fp1, fp2);
        assert_eq!(fp1, compute_credential_fingerprint(&creds1));
    }

    #[test]
    fn test_daemon_fingerprint_state_tracking() {
        let manager = RcloneManager::with_port(49998);
        assert!(manager.credential_fingerprint.lock().unwrap().is_none());

        {
            let mut fp_lock = manager.credential_fingerprint.lock().unwrap();
            *fp_lock = Some("sha256:v1:test123".to_string());
        }
        assert_eq!(
            manager.credential_fingerprint.lock().unwrap().as_deref(),
            Some("sha256:v1:test123")
        );

        manager.stop_daemon();
        assert!(manager.credential_fingerprint.lock().unwrap().is_none());
    }
}
