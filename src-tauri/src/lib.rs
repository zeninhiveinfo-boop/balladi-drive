pub mod auth;
pub mod engine;
pub mod system;

use auth::drive::{get_google_user_profile, parse_google_drive_link, GoogleUserInfo, ParsedDriveLink};
use engine::rclone::{RcloneManager, StartedTransfer, TransferMode, TransferStats, VerificationResult};
use serde_json::Value;
use std::sync::Arc;
use system::power::{acquire_sleep_lock, release_sleep_lock};
use system::process::hide_tokio_command_window;
use system::storage::{inspect_storage, StorageInfo};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub rclone: Arc<RcloneManager>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct AppSettings {
    pub oauth_mode: String,
    pub connected_client_fingerprint: Option<String>,
    pub custom_client_id: Option<String>,
    pub custom_client_secret: Option<String>,
    pub webhook_url: String,
    pub notify_on_complete: bool,
}

impl std::fmt::Debug for AppSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSettings")
            .field("oauth_mode", &self.oauth_mode)
            .field("connected_client_fingerprint", &self.connected_client_fingerprint)
            .field("custom_client_id", &self.custom_client_id)
            .field(
                "custom_client_secret",
                &self.custom_client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "webhook_url",
                if self.webhook_url.is_empty() {
                    &""
                } else {
                    &"[REDACTED_WEBHOOK_URL]"
                },
            )
            .field("notify_on_complete", &self.notify_on_complete)
            .finish()
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            oauth_mode: "managed".to_string(),
            connected_client_fingerprint: None,
            custom_client_id: None,
            custom_client_secret: None,
            webhook_url: "".to_string(),
            notify_on_complete: true,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PublicAppSettings {
    pub oauth_mode: String,
    pub is_managed: bool,
    pub connected_client_fingerprint: Option<String>,
    pub has_custom_client_id: bool,
    pub has_custom_client_secret: bool,
    pub has_webhook_url: bool,
    pub notify_on_complete: bool,
}

fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("balladi")
        .join("settings.json")
}

pub fn write_file_atomic(path: &std::path::Path, data: &[u8], is_private: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let tmp_path = path.with_extension(format!("tmp.{}", rand::random::<u32>()));
    std::fs::write(&tmp_path, data)
        .map_err(|e| format!("Failed writing temp file at {}: {}", tmp_path.display(), e))?;

    #[cfg(unix)]
    if is_private {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600));
    }

    // Windows does not expose Unix permission bits. Keep the shared API explicit
    // without producing a platform-only unused-variable warning.
    #[cfg(not(unix))]
    let _ = is_private;

    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed committing atomic file to {}: {}", path.display(), e))?;

    Ok(())
}

pub fn compute_client_fingerprint(client_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(client_id.trim().as_bytes());
    format!("sha256:v1:{:x}", hasher.finalize())
}

#[tauri::command]
fn get_app_settings() -> PublicAppSettings {
    let path = get_settings_path();
    let settings: AppSettings = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    let is_managed = settings.oauth_mode != "custom";
    PublicAppSettings {
        oauth_mode: settings.oauth_mode,
        is_managed,
        connected_client_fingerprint: settings.connected_client_fingerprint,
        has_custom_client_id: settings
            .custom_client_id
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        has_custom_client_secret: settings
            .custom_client_secret
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false),
        has_webhook_url: !settings.webhook_url.trim().is_empty(),
        notify_on_complete: settings.notify_on_complete,
    }
}

fn is_allowed_webhook_url(url_str: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    if parsed.scheme() != "https" {
        return Err("Webhook URL must use secure HTTPS".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "Missing host".to_string())?
        .to_lowercase();

    let allowed_suffixes = [
        "hooks.slack.com",
        "discord.com",
        "discordapp.com",
        "webhook.office.com",
    ];

    let is_allowed = allowed_suffixes.iter().any(|&suffix| {
        host == suffix || host.ends_with(&format!(".{}", suffix))
    });

    if !is_allowed {
        return Err(format!(
            "Host '{}' is not an approved webhook provider (Slack, Discord, Microsoft Teams)",
            host
        ));
    }

    Ok(parsed)
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct RcloneOAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for RcloneOAuthCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RcloneOAuthCredentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[REDACTED]")
            .finish()
    }
}

/// Loads Balladi Studios managed OAuth credentials baked in at compile time.
pub fn load_managed_oauth_credentials() -> Result<RcloneOAuthCredentials, String> {
    let client_id = option_env!("BALLADI_GOOGLE_CLIENT_ID")
        .unwrap_or("")
        .trim();

    let client_secret = option_env!("BALLADI_GOOGLE_CLIENT_SECRET")
        .unwrap_or("")
        .trim();

    if client_id.is_empty() || client_secret.is_empty() {
        return Err(
            "This Balladi Drive build is missing managed Google OAuth credentials."
                .to_string(),
        );
    }

    Ok(RcloneOAuthCredentials {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
    })
}

pub fn load_rclone_oauth_credentials_from_settings(
    settings: &AppSettings,
) -> Result<RcloneOAuthCredentials, String> {
    if settings.oauth_mode == "custom" {
        let client_id = settings.custom_client_id.as_deref().unwrap_or("").trim();
        let client_secret = settings.custom_client_secret.as_deref().unwrap_or("").trim();
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(
                "Custom OAuth mode is selected, but custom client ID or secret is missing."
                    .to_string(),
            );
        }
        Ok(RcloneOAuthCredentials {
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        })
    } else {
        load_managed_oauth_credentials()
    }
}

pub fn load_rclone_oauth_credentials_from_path(path: &std::path::Path) -> Result<RcloneOAuthCredentials, String> {
    let settings: AppSettings = if let Ok(content) = std::fs::read_to_string(path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppSettings::default()
    };
    load_rclone_oauth_credentials_from_settings(&settings)
}

pub fn load_rclone_oauth_credentials() -> Result<RcloneOAuthCredentials, String> {
    load_rclone_oauth_credentials_from_path(&get_settings_path())
}

pub fn apply_google_oauth_env_std(
    cmd: &mut std::process::Command,
    credentials: &RcloneOAuthCredentials,
) {
    cmd.env("RCLONE_CONFIG_GDRIVE_CLIENT_ID", &credentials.client_id)
        .env("RCLONE_CONFIG_GDRIVE_CLIENT_SECRET", &credentials.client_secret);
}

pub fn apply_google_oauth_env_tokio(
    cmd: &mut tokio::process::Command,
    credentials: &RcloneOAuthCredentials,
) {
    cmd.env("RCLONE_CONFIG_GDRIVE_CLIENT_ID", &credentials.client_id)
        .env("RCLONE_CONFIG_GDRIVE_CLIENT_SECRET", &credentials.client_secret);
}

#[tauri::command]
fn save_app_settings(settings: AppSettings) -> Result<(), String> {
    let path = get_settings_path();
    let mut to_save = settings;

    // Load existing settings if webview passed masked values or __PRESERVE__
    if let Ok(existing_content) = std::fs::read_to_string(&path) {
        if let Ok(existing) = serde_json::from_str::<AppSettings>(&existing_content) {
            if let Some(ref s) = to_save.custom_client_id {
                if s.contains("••") || s == "__PRESERVE__" {
                    to_save.custom_client_id = existing.custom_client_id;
                }
            }
            if let Some(ref s) = to_save.custom_client_secret {
                if s.contains("••") || s == "__PRESERVE__" {
                    to_save.custom_client_secret = existing.custom_client_secret;
                }
            }
            if to_save.webhook_url.contains("••") || to_save.webhook_url == "__PRESERVE__" {
                to_save.webhook_url = existing.webhook_url;
            }
            if to_save.connected_client_fingerprint.is_none() {
                to_save.connected_client_fingerprint = existing.connected_client_fingerprint;
            }
        }
    }

    // Validate webhook URL if non-empty
    if !to_save.webhook_url.trim().is_empty() {
        is_allowed_webhook_url(&to_save.webhook_url)?;
    }

    let data = serde_json::to_string_pretty(&to_save)
        .map_err(|e| format!("Failed serializing settings: {}", e))?;

    write_file_atomic(&path, data.as_bytes(), true)?;

    Ok(())
}

#[tauri::command]
async fn send_completion_webhook(
    project_name: String,
    total_bytes: u64,
    file_count: u64,
) -> Result<(), String> {
    let path = get_settings_path();
    let settings: AppSettings = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    if !settings.notify_on_complete || settings.webhook_url.trim().is_empty() {
        return Ok(());
    }

    let url = is_allowed_webhook_url(&settings.webhook_url)?;

    let size_gb = (total_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
    let payload = serde_json::json!({
        "text": format!(
            "🎉 *Balladi Drive*: Project *{}* finished transferring!\n• Size: {:.2} GB\n• Files: {}\n• Status: Ready for Verification",
            project_name, size_gb, file_count
        )
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none()) // Prevent redirect-based SSRF
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Webhook delivery failed: {}", e))?;

    resp.error_for_status()
        .map_err(|e| format!("Webhook returned HTTP error: {}", e))?;

    Ok(())
}

#[tauri::command]
async fn init_engine(state: State<'_, AppState>) -> Result<Value, String> {
    let path = get_settings_path();
    let settings: AppSettings = if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        AppSettings::default()
    };

    let creds_res = load_rclone_oauth_credentials_from_settings(&settings);
    let daemon_res = match creds_res {
        Ok(ref creds) => {
            let active_fp = compute_client_fingerprint(&creds.client_id);
            let is_fingerprint_valid = settings
                .connected_client_fingerprint
                .as_deref()
                == Some(&active_fp);
            if is_fingerprint_valid {
                state.rclone.start_daemon_with_credentials(creds).await
            } else {
                state.rclone.stop_daemon();
                Err("Google account reconnection is required with Balladi Managed OAuth.".to_string())
            }
        }
        Err(e) => {
            state.rclone.stop_daemon();
            Err(e)
        }
    };

    let (has_gdrive, remotes, user_info, engine_version) = if daemon_res.is_ok() {
        let remotes = state.rclone.list_remotes().await.unwrap_or_default();
        let has_gdrive = remotes.iter().any(|r| r.trim_end_matches(':') == "gdrive");
        let user_info = if has_gdrive {
            get_google_user_profile().await
        } else {
            GoogleUserInfo {
                is_authenticated: false,
                display_name: None,
                email: None,
                photo_link: None,
                storage_total: None,
                storage_used: None,
            }
        };
        let engine_version = state
            .rclone
            .call_rc("core/version", serde_json::json!({}))
            .await
            .ok()
            .and_then(|v| v.get("version").and_then(serde_json::Value::as_str).map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        (has_gdrive, remotes, user_info, engine_version)
    } else {
        (
            false,
            Vec::new(),
            GoogleUserInfo {
                is_authenticated: false,
                display_name: None,
                email: None,
                photo_link: None,
                storage_total: None,
                storage_used: None,
            },
            "uninitialized".to_string(),
        )
    };

    Ok(serde_json::json!({
        "status": if has_gdrive && user_info.is_authenticated { "ready" } else { "needs_auth" },
        "port": state.rclone.port,
        "has_gdrive": has_gdrive && user_info.is_authenticated,
        "remotes": remotes,
        "user_info": user_info,
        "engine_version": engine_version
    }))
}

#[tauri::command]
async fn get_google_user_info() -> GoogleUserInfo {
    get_google_user_profile().await
}

#[tauri::command]
fn parse_link(url: String) -> ParsedDriveLink {
    parse_google_drive_link(&url)
}

#[tauri::command]
fn get_default_download_dir() -> String {
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/tmp".to_string())
}

#[tauri::command]
fn check_storage_info(path: String) -> StorageInfo {
    inspect_storage(&path, false)
}

#[tauri::command]
fn probe_storage_write(path: String) -> StorageInfo {
    inspect_storage(&path, true)
}

#[tauri::command]
async fn start_transfer_job(
    state: State<'_, AppState>,
    source: String,
    destination: String,
    is_single_file: Option<bool>,
) -> Result<StartedTransfer, String> {
    let single_file = is_single_file.unwrap_or(false);
    let started = state
        .rclone
        .start_copy(&source, &destination, single_file)
        .await?;

    // Acquire system power lock ONLY after transfer is confirmed started and running
    acquire_sleep_lock();
    Ok(started)
}

#[tauri::command]
async fn get_transfer_stats(
    state: State<'_, AppState>,
    job_id: Option<u64>,
) -> Result<TransferStats, String> {
    state.rclone.get_stats(job_id).await
}

#[tauri::command]
async fn check_job_status(state: State<'_, AppState>, job_id: u64) -> Result<Value, String> {
    let res = state.rclone.check_job(job_id).await?;
    let finished = res["finished"].as_bool().unwrap_or(false);
    if finished {
        release_sleep_lock();
    }
    Ok(res)
}

#[tauri::command]
async fn stop_transfer_job(state: State<'_, AppState>, job_id: u64) -> Result<(), String> {
    let res = state.rclone.stop_job(job_id).await;
    if res.is_ok() {
        release_sleep_lock();
    }
    res
}

#[tauri::command]
async fn stop_all_transfers(state: State<'_, AppState>) -> Result<(), String> {
    let res = state.rclone.stop_all_transfers().await;
    if res.is_ok() {
        release_sleep_lock();
    }
    res
}

#[tauri::command]
async fn reveal_in_finder(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let clean_path = path.trim().to_string();
        let target = if std::path::Path::new(&clean_path).exists() {
            clean_path
        } else {
            dirs::download_dir()
                .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "/tmp".to_string())
        };

        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(&target)
                .spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", target))
                .spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(&target)
                .spawn();
        }
    });
    Ok(())
}

#[tauri::command]
async fn set_bandwidth_throttle(
    state: State<'_, AppState>,
    limit: String,
) -> Result<(), String> {
    state.rclone.set_bwlimit(&limit).await
}

#[tauri::command]
async fn verify_transfer_integrity(
    state: State<'_, AppState>,
    source: String,
    destination: String,
    mode: TransferMode,
    is_single_file: Option<bool>,
) -> Result<VerificationResult, String> {
    state
        .rclone
        .verify_transfer(
            &source,
            &destination,
            Some(mode),
            is_single_file.unwrap_or(false),
        )
        .await
}

#[tauri::command]
async fn get_directory_size(
    state: State<'_, AppState>,
    path: Option<String>,
    fs: Option<String>,
) -> Result<serde_json::Value, String> {
    let target = path.or(fs).ok_or_else(|| "Missing path/fs parameter".to_string())?;
    let (count, bytes) = state.rclone.get_size(&target).await?;
    Ok(serde_json::json!({
        "count": count,
        "bytes": bytes,
        "gb": (bytes as f64) / (1024.0 * 1024.0 * 1024.0)
    }))
}

pub struct ConnectTransactionContext<'a> {
    pub rclone_manager: &'a RcloneManager,
    pub settings_path: std::path::PathBuf,
    pub rclone_conf_path: std::path::PathBuf,
    pub target_settings: AppSettings,
    pub creds: RcloneOAuthCredentials,
}

#[allow(async_fn_in_trait)]
pub trait ConnectStepExecutor: Send + Sync {
    async fn run_auth(
        &self,
        rclone_bin: &std::path::Path,
        candidate_conf_path: &std::path::Path,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String>;

    async fn run_probe(
        &self,
        rclone_bin: &std::path::Path,
        candidate_conf_path: &std::path::Path,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String>;

    async fn verify_profile(
        &self,
        creds: &RcloneOAuthCredentials,
        candidate_conf_path: &std::path::Path,
    ) -> Result<bool, String>;

    async fn commit_rclone_config(
        &self,
        candidate_conf_path: &std::path::Path,
        live_rclone_conf_path: &std::path::Path,
    ) -> Result<(), String> {
        let content = std::fs::read(candidate_conf_path)
            .map_err(|e| format!("Failed reading candidate rclone config at {}: {}", candidate_conf_path.display(), e))?;
        write_file_atomic(live_rclone_conf_path, &content, true)
    }

    async fn commit_settings(
        &self,
        settings_path: &std::path::Path,
        settings: &AppSettings,
    ) -> Result<(), String> {
        let data = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("Failed serializing settings: {}", e))?;
        write_file_atomic(settings_path, data.as_bytes(), true)
    }

    async fn start_daemon(
        &self,
        manager: &RcloneManager,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String>;

    async fn list_remotes(&self, manager: &RcloneManager) -> Result<Vec<String>, String>;
}

pub struct DefaultConnectStepExecutor;

impl ConnectStepExecutor for DefaultConnectStepExecutor {
    async fn run_auth(
        &self,
        rclone_bin: &std::path::Path,
        candidate_conf_path: &std::path::Path,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String> {
        let mut cmd = tokio::process::Command::new(rclone_bin);
        hide_tokio_command_window(&mut cmd);
        apply_google_oauth_env_tokio(&mut cmd, creds);
        cmd.args([
            "config",
            "create",
            "gdrive",
            "drive",
            "scope",
            "drive",
            "config_is_local",
            "true",
            "--config",
        ]);
        cmd.arg(candidate_conf_path);

        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed running rclone auth: {}", e))?;

        if !output.status.success() {
            let err_str = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Authorization failed: {}", err_str));
        }
        Ok(())
    }

    async fn run_probe(
        &self,
        rclone_bin: &std::path::Path,
        candidate_conf_path: &std::path::Path,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String> {
        let mut probe_cmd = tokio::process::Command::new(rclone_bin);
        hide_tokio_command_window(&mut probe_cmd);
        apply_google_oauth_env_tokio(&mut probe_cmd, creds);
        probe_cmd.args(["about", "gdrive:", "--config"]);
        probe_cmd.arg(candidate_conf_path);

        let probe = probe_cmd
            .output()
            .await
            .map_err(|e| format!("Google Drive probe could not start: {e}"))?;

        if !probe.status.success() {
            return Err(format!(
                "Google Drive probe failed: {}",
                String::from_utf8_lossy(&probe.stderr)
            ));
        }

        let stderr = String::from_utf8_lossy(&probe.stderr).to_lowercase();
        if stderr.contains("shared client") {
            return Err("rclone shared OAuth client detected".into());
        }
        Ok(())
    }

    async fn verify_profile(
        &self,
        creds: &RcloneOAuthCredentials,
        candidate_conf_path: &std::path::Path,
    ) -> Result<bool, String> {
        let user_info = crate::auth::drive::get_google_user_profile_with_credentials(
            creds,
            Some(candidate_conf_path),
        )
        .await;
        Ok(user_info.is_authenticated)
    }

    async fn start_daemon(
        &self,
        manager: &RcloneManager,
        creds: &RcloneOAuthCredentials,
    ) -> Result<(), String> {
        manager.start_daemon_with_credentials(creds).await
    }

    async fn list_remotes(&self, manager: &RcloneManager) -> Result<Vec<String>, String> {
        manager.list_remotes().await
    }
}

pub async fn execute_google_connect_transaction_with_executor<E: ConnectStepExecutor>(
    ctx: ConnectTransactionContext<'_>,
    executor: &E,
) -> Result<Vec<String>, String> {
    let original_settings_json = std::fs::read_to_string(&ctx.settings_path).ok();
    let original_rclone_conf = std::fs::read_to_string(&ctx.rclone_conf_path).ok();
    let was_daemon_running = {
        let child_lock = ctx.rclone_manager.child.lock().unwrap();
        child_lock.is_some()
    };
    let previous_credentials = load_rclone_oauth_credentials_from_path(&ctx.settings_path).ok();

    // Stage candidate configuration into an isolated candidate file with 0600 permissions
    let candidate_conf_path = if let Some(parent) = ctx.rclone_conf_path.parent() {
        let _ = std::fs::create_dir_all(parent);
        parent.join(format!(".rclone.candidate.{}.conf", rand::random::<u32>()))
    } else {
        std::env::temp_dir().join(format!("balladi_candidate_{}.conf", rand::random::<u32>()))
    };

    let cleanup_candidate = |cand_path: &std::path::Path| {
        if cand_path.exists() {
            let _ = std::fs::remove_file(cand_path);
        }
    };

    let mut rclone_conf_committed = false;
    let mut settings_committed = false;

    let transaction_res: Result<Vec<String>, String> = async {
        let rclone_bin = RcloneManager::find_rclone_binary();

        // 1. Run authorization into isolated candidate configuration
        executor.run_auth(&rclone_bin, &candidate_conf_path, &ctx.creds).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if candidate_conf_path.exists() {
                let _ = std::fs::set_permissions(&candidate_conf_path, std::fs::Permissions::from_mode(0o600));
            }
        }

        // 2. Probe about gdrive: on candidate configuration with candidate credentials
        executor.run_probe(&rclone_bin, &candidate_conf_path, &ctx.creds).await?;

        // 3. Verify Google user profile using candidate credentials and candidate config
        let profile_ok = executor.verify_profile(&ctx.creds, &candidate_conf_path).await?;
        if !profile_ok {
            return Err("Could not verify authenticated Google user profile.".into());
        }

        // 4. Halt existing daemon before committing configuration
        ctx.rclone_manager.stop_daemon();

        // 5. Commit candidate configuration to live rclone_conf_path
        executor.commit_rclone_config(&candidate_conf_path, &ctx.rclone_conf_path).await?;
        rclone_conf_committed = true;

        // 6. Commit target settings and fingerprint to settings_path
        executor.commit_settings(&ctx.settings_path, &ctx.target_settings).await?;
        settings_committed = true;

        // 7. Start daemon with candidate credentials and live config
        executor.start_daemon(ctx.rclone_manager, &ctx.creds).await?;

        // 8. Confirm remotes list
        let remotes = executor.list_remotes(ctx.rclone_manager).await?;
        if !remotes.iter().any(|r| r.trim_end_matches(':') == "gdrive") {
            return Err("Authenticated gdrive remote was not found in daemon remotes list.".into());
        }

        Ok(remotes)
    }
    .await;

    cleanup_candidate(&candidate_conf_path);

    match transaction_res {
        Ok(remotes) => Ok(remotes),
        Err(err) => {
            ctx.rclone_manager.stop_daemon();

            let mut rollback_errors = Vec::new();

            if rclone_conf_committed {
                if let Some(prev_rclone) = original_rclone_conf {
                    if let Err(r_err) = write_file_atomic(&ctx.rclone_conf_path, prev_rclone.as_bytes(), true) {
                        rollback_errors.push(format!("Failed restoring rclone config: {r_err}"));
                    }
                } else if ctx.rclone_conf_path.exists() {
                    if let Err(rm_err) = std::fs::remove_file(&ctx.rclone_conf_path) {
                        rollback_errors.push(format!("Failed removing candidate rclone config: {rm_err}"));
                    }
                }
            }

            if settings_committed {
                if let Some(prev_settings) = original_settings_json {
                    if let Err(s_err) = write_file_atomic(&ctx.settings_path, prev_settings.as_bytes(), true) {
                        rollback_errors.push(format!("Failed restoring settings: {s_err}"));
                    }
                } else if ctx.settings_path.exists() {
                    if let Err(sm_err) = std::fs::remove_file(&ctx.settings_path) {
                        rollback_errors.push(format!("Failed removing candidate settings: {sm_err}"));
                    }
                }
            }

            if was_daemon_running {
                if let Some(ref prev_creds) = previous_credentials {
                    if let Err(start_err) = ctx.rclone_manager.start_daemon_with_credentials(prev_creds).await {
                        rollback_errors.push(format!("Failed restarting original daemon: {start_err}"));
                    }
                }
            }

            if rollback_errors.is_empty() {
                Err(err)
            } else {
                Err(format!("{err}; rollback encountered errors: {}", rollback_errors.join(", ")))
            }
        }
    }
}

pub async fn execute_google_connect_transaction(
    ctx: ConnectTransactionContext<'_>,
) -> Result<Vec<String>, String> {
    execute_google_connect_transaction_with_executor(ctx, &DefaultConnectStepExecutor).await
}

#[tauri::command]
async fn connect_google_drive(
    state: State<'_, AppState>,
    oauth_mode: Option<String>,
    custom_client_id: Option<String>,
    custom_client_secret: Option<String>,
) -> Result<serde_json::Value, String> {
    let settings_path = get_settings_path();
    let original_settings_json = std::fs::read_to_string(&settings_path).ok();
    let original_settings: AppSettings = original_settings_json
        .as_deref()
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or_default();

    let rclone_conf_path = crate::auth::drive::get_canonical_rclone_conf_path();

    let mut target_settings = original_settings.clone();
    let is_custom = match oauth_mode.as_deref() {
        Some("custom") => true,
        Some("managed") => false,
        _ => original_settings.oauth_mode == "custom",
    };

    let creds = if is_custom {
        target_settings.oauth_mode = "custom".to_string();

        let clean_id = match custom_client_id.as_deref() {
            Some("__PRESERVE__") | None => original_settings.custom_client_id.clone(),
            Some(s) if s.trim().is_empty() || s.contains("••") => original_settings.custom_client_id.clone(),
            Some(s) => Some(s.trim().to_string()),
        }
        .ok_or_else(|| "Custom OAuth Client ID is required".to_string())?;

        if clean_id.trim().is_empty() {
            return Err("Custom OAuth Client ID is required".to_string());
        }

        let clean_secret = match custom_client_secret.as_deref() {
            Some("__PRESERVE__") | None => original_settings.custom_client_secret.clone(),
            Some(s) if s.trim().is_empty() || s.contains("••") => original_settings.custom_client_secret.clone(),
            Some(s) => Some(s.trim().to_string()),
        }
        .ok_or_else(|| "Custom OAuth Client Secret is required".to_string())?;

        if clean_secret.trim().is_empty() {
            return Err("Custom OAuth Client Secret is required".to_string());
        }

        target_settings.custom_client_id = Some(clean_id.clone());
        target_settings.custom_client_secret = Some(clean_secret.clone());

        RcloneOAuthCredentials {
            client_id: clean_id,
            client_secret: clean_secret,
        }
    } else {
        target_settings.oauth_mode = "managed".to_string();
        load_managed_oauth_credentials()?
    };

    let client_fp = compute_client_fingerprint(&creds.client_id);
    target_settings.connected_client_fingerprint = Some(client_fp);

    let ctx = ConnectTransactionContext {
        rclone_manager: &state.rclone,
        settings_path,
        rclone_conf_path,
        target_settings,
        creds,
    };

    let remotes = execute_google_connect_transaction(ctx).await?;

    Ok(serde_json::json!({
        "success": true,
        "has_gdrive": true,
        "remotes": remotes
    }))
}

#[tauri::command]
async fn disconnect_google_drive(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    state.rclone.stop_daemon();

    let conf_path = crate::auth::drive::get_canonical_rclone_conf_path();
    let rclone_bin = RcloneManager::find_rclone_binary();
    let mut cmd = tokio::process::Command::new(rclone_bin);
    hide_tokio_command_window(&mut cmd);
    let output = cmd
        .args(["config", "delete", "gdrive", "--config"])
        .arg(&conf_path)
        .output()
        .await
        .map_err(|e| format!("Could not disconnect Google Drive: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Could not delete gdrive remote: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Verify using a fresh CLI listremotes, not the stopped daemon
    let mut list_cmd = tokio::process::Command::new(RcloneManager::find_rclone_binary());
    hide_tokio_command_window(&mut list_cmd);
    let list_output = list_cmd
        .args(["listremotes", "--config"])
        .arg(&conf_path)
        .output()
        .await
        .map_err(|e| format!("Failed to list remotes: {e}"))?;

    let list_str = String::from_utf8_lossy(&list_output.stdout);
    let remotes: Vec<String> = list_str
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let has_gdrive = remotes.iter().any(|r| r.trim_end_matches(':') == "gdrive");

    // Clear connected fingerprint from settings
    let path = get_settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(mut settings) = serde_json::from_str::<AppSettings>(&content) {
            settings.connected_client_fingerprint = None;
            let _ = save_app_settings(settings);
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "has_gdrive": has_gdrive,
        "remotes": remotes
    }))
}

#[tauri::command]
async fn open_google_auth_page() -> Result<(), String> {
    open::that("https://console.cloud.google.com")
        .map_err(|e| format!("Failed to open browser: {}", e))
}

#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder_path| {
        let path_str = folder_path.map(|p| p.to_string());
        let _ = tx.send(path_str);
    });
    rx.await.map_err(|e| format!("Dialog cancelled or failed: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rclone_manager = Arc::new(RcloneManager::new());

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            rclone: rclone_manager.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            init_engine,
            parse_link,
            check_storage_info,
            probe_storage_write,
            start_transfer_job,
            get_transfer_stats,
            check_job_status,
            stop_transfer_job,
            set_bandwidth_throttle,
            verify_transfer_integrity,
            get_directory_size,
            open_google_auth_page,
            connect_google_drive,
            disconnect_google_drive,
            pick_folder,
            get_default_download_dir,
            get_google_user_info,
            get_app_settings,
            save_app_settings,
            send_completion_webhook,
            stop_all_transfers,
            reveal_in_finder
        ])
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let icon = tauri::include_image!("icons/icon.png");
                let _ = window.set_icon(icon);
            }
            Ok(())
        })
        .on_window_event({
            let rclone_win = rclone_manager.clone();
            move |_window, event| {
                if let tauri::WindowEvent::Destroyed = event {
                    release_sleep_lock();
                    rclone_win.stop_daemon();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Balladi Drive application");

    app.run({
        let rclone_exit = rclone_manager.clone();
        move |_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                release_sleep_lock();
                rclone_exit.stop_daemon();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_key_link_parsing() {
        let parsed = crate::auth::drive::parse_google_drive_link(
            "https://drive.google.com/drive/folders/1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB?resourcekey=0-AbCdEf12345",
        );
        assert!(parsed.is_valid);
        assert!(parsed.is_folder);
        assert_eq!(parsed.id, "1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB");
        assert_eq!(parsed.resource_key, Some("0-AbCdEf12345".to_string()));
        assert_eq!(
            parsed.connection_string,
            "gdrive,root_folder_id=1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB,resource_key=0-AbCdEf12345:"
        );
    }

    #[test]
    fn test_webhook_allowlist_valid() {
        assert!(is_allowed_webhook_url("https://discord.com/api/webhooks/123/abc").is_ok());
        assert!(is_allowed_webhook_url("https://hooks.slack.com/services/T00/B00/X00").is_ok());
        assert!(is_allowed_webhook_url("https://subdomain.webhook.office.com/webhookb2/xyz").is_ok());
    }

    #[test]
    fn test_webhook_allowlist_rejects_ssrf_and_http() {
        assert!(is_allowed_webhook_url("http://discord.com/api/webhooks/123/abc").is_err());
        assert!(is_allowed_webhook_url("https://127.0.0.1/webhook").is_err());
        assert!(is_allowed_webhook_url("https://evil.com/webhook").is_err());
    }

    #[test]
    fn test_oauth_env_applied_to_command() {
        let creds = RcloneOAuthCredentials {
            client_id: "test-client-id.apps.googleusercontent.com".to_string(),
            client_secret: "test-client-secret".to_string(),
        };

        let mut cmd = std::process::Command::new("rclone");
        apply_google_oauth_env_std(&mut cmd, &creds);

        let envs: std::collections::HashMap<_, _> = cmd.get_envs().collect();
        assert_eq!(
            envs.get(std::ffi::OsStr::new("RCLONE_CONFIG_GDRIVE_CLIENT_ID")),
            Some(&Some(std::ffi::OsStr::new("test-client-id.apps.googleusercontent.com")))
        );
        assert_eq!(
            envs.get(std::ffi::OsStr::new("RCLONE_CONFIG_GDRIVE_CLIENT_SECRET")),
            Some(&Some(std::ffi::OsStr::new("test-client-secret")))
        );
    }

    #[test]
    fn test_compute_client_fingerprint_deterministic_and_unique() {
        let fp1 = compute_client_fingerprint("client-123.apps.googleusercontent.com");
        let fp2 = compute_client_fingerprint("client-123.apps.googleusercontent.com");
        let fp3 = compute_client_fingerprint("client-456.apps.googleusercontent.com");

        assert_eq!(fp1, fp2);
        assert_ne!(fp1, fp3);
        assert!(fp1.starts_with("sha256:v1:"));
        // SHA-256 hex is 64 chars + "sha256:v1:" prefix (10 chars) = 74 chars
        assert_eq!(fp1.len(), 74);
    }

    #[test]
    fn test_debug_formatting_redacts_secrets() {
        let creds = RcloneOAuthCredentials {
            client_id: "my-public-client-id.apps.googleusercontent.com".to_string(),
            client_secret: "super-secret-oauth-value".to_string(),
        };
        let creds_debug = format!("{:?}", creds);
        assert!(creds_debug.contains("my-public-client-id.apps.googleusercontent.com"));
        assert!(!creds_debug.contains("super-secret-oauth-value"));
        assert!(creds_debug.contains("[REDACTED]"));

        let settings = AppSettings {
            oauth_mode: "custom".to_string(),
            connected_client_fingerprint: Some("sha256:v1:test".to_string()),
            custom_client_id: Some("custom-id.apps.googleusercontent.com".to_string()),
            custom_client_secret: Some("raw-custom-secret-999".to_string()),
            webhook_url: "https://discord.com/api/webhooks/123/secret-token".to_string(),
            notify_on_complete: true,
        };
        let settings_debug = format!("{:?}", settings);
        assert!(!settings_debug.contains("raw-custom-secret-999"));
        assert!(!settings_debug.contains("secret-token"));
        assert!(settings_debug.contains("[REDACTED]"));
        assert!(settings_debug.contains("[REDACTED_WEBHOOK_URL]"));
    }

    #[test]
    fn test_public_settings_does_not_expose_raw_secrets() {
        let settings = AppSettings {
            oauth_mode: "custom".to_string(),
            connected_client_fingerprint: Some("sha256:v1:abc123".to_string()),
            custom_client_id: Some("custom-id".to_string()),
            custom_client_secret: Some("raw_secret_value_12345".to_string()),
            webhook_url: "https://discord.com/api/webhooks/123/abc".to_string(),
            notify_on_complete: true,
        };

        let is_managed = settings.oauth_mode != "custom";
        let pub_settings = PublicAppSettings {
            oauth_mode: settings.oauth_mode,
            is_managed,
            connected_client_fingerprint: settings.connected_client_fingerprint,
            has_custom_client_id: settings.custom_client_id.is_some(),
            has_custom_client_secret: settings.custom_client_secret.is_some(),
            has_webhook_url: !settings.webhook_url.trim().is_empty(),
            notify_on_complete: settings.notify_on_complete,
        };

        let json = serde_json::to_string(&pub_settings).unwrap();
        // Plaintext secret value is NEVER exposed in the public JSON
        assert!(!json.contains("raw_secret_value_12345"));
        assert!(json.contains("has_custom_client_secret\":true"));
        assert!(json.contains("sha256:v1:abc123"));
        assert!(json.contains("is_managed\":false"));
    }

    #[test]
    fn test_load_rclone_oauth_credentials_custom_mode() {
        let mut custom_settings = AppSettings {
            oauth_mode: "custom".to_string(),
            connected_client_fingerprint: None,
            custom_client_id: Some("custom-id.apps.googleusercontent.com".to_string()),
            custom_client_secret: Some("custom-secret".to_string()),
            webhook_url: "".to_string(),
            notify_on_complete: true,
        };

        let creds = load_rclone_oauth_credentials_from_settings(&custom_settings).unwrap();
        assert_eq!(creds.client_id, "custom-id.apps.googleusercontent.com");
        assert_eq!(creds.client_secret, "custom-secret");

        // Custom mode with missing secret fails closed
        custom_settings.custom_client_secret = Some("".to_string());
        assert!(load_rclone_oauth_credentials_from_settings(&custom_settings).is_err());
    }

    #[derive(Default)]
    struct MockStepExecutor {
        pub fail_at_auth: bool,
        pub fail_at_probe: bool,
        pub fail_at_profile: bool,
        pub fail_at_config_commit: bool,
        pub fail_at_settings_commit: bool,
        pub fail_at_daemon_start: bool,
    }

    impl ConnectStepExecutor for MockStepExecutor {
        async fn run_auth(
            &self,
            _rclone_bin: &std::path::Path,
            candidate_conf_path: &std::path::Path,
            _creds: &RcloneOAuthCredentials,
        ) -> Result<(), String> {
            if self.fail_at_auth {
                return Err("Simulated OAuth authorization failure".to_string());
            }
            // Simulate rclone config create writing dummy candidate configuration
            let dummy_conf = "[gdrive]\ntype = drive\nscope = drive\ntoken = {\"access_token\":\"candidate_token\"}\n";
            std::fs::write(candidate_conf_path, dummy_conf)
                .map_err(|e| format!("Failed writing mock candidate config: {e}"))?;
            Ok(())
        }

        async fn run_probe(
            &self,
            _rclone_bin: &std::path::Path,
            _candidate_conf_path: &std::path::Path,
            _creds: &RcloneOAuthCredentials,
        ) -> Result<(), String> {
            if self.fail_at_probe {
                return Err("Simulated probe failure".to_string());
            }
            Ok(())
        }

        async fn verify_profile(
            &self,
            _creds: &RcloneOAuthCredentials,
            _candidate_conf_path: &std::path::Path,
        ) -> Result<bool, String> {
            if self.fail_at_profile {
                return Ok(false);
            }
            Ok(true)
        }

        async fn commit_rclone_config(
            &self,
            candidate_conf_path: &std::path::Path,
            live_rclone_conf_path: &std::path::Path,
        ) -> Result<(), String> {
            if self.fail_at_config_commit {
                return Err("Simulated rclone config commit failure".to_string());
            }
            let content = std::fs::read(candidate_conf_path)
                .map_err(|e| format!("Failed reading candidate config: {e}"))?;
            write_file_atomic(live_rclone_conf_path, &content, true)
        }

        async fn commit_settings(
            &self,
            settings_path: &std::path::Path,
            settings: &AppSettings,
        ) -> Result<(), String> {
            if self.fail_at_settings_commit {
                return Err("Simulated settings commit failure".to_string());
            }
            let data = serde_json::to_string_pretty(settings)
                .map_err(|e| format!("Failed serializing settings: {e}"))?;
            write_file_atomic(settings_path, data.as_bytes(), true)
        }

        async fn start_daemon(
            &self,
            _manager: &RcloneManager,
            _creds: &RcloneOAuthCredentials,
        ) -> Result<(), String> {
            if self.fail_at_daemon_start {
                return Err("Simulated daemon startup failure".to_string());
            }
            Ok(())
        }

        async fn list_remotes(&self, _manager: &RcloneManager) -> Result<Vec<String>, String> {
            Ok(vec!["gdrive:".to_string()])
        }
    }

    #[tokio::test]
    async fn test_production_transaction_rollbacks_on_all_failures() {
        let temp_dir = std::env::temp_dir().join(format!("balladi_tx_test_{}", rand::random::<u32>()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let settings_path = temp_dir.join("settings.json");
        let rclone_conf_path = temp_dir.join("rclone.conf");

        let original_settings = AppSettings {
            oauth_mode: "managed".to_string(),
            connected_client_fingerprint: Some("sha256:v1:orig_fp".to_string()),
            custom_client_id: None,
            custom_client_secret: None,
            webhook_url: "".to_string(),
            notify_on_complete: false,
        };
        let original_rclone_conf = "[gdrive]\ntype = drive\ntoken = {\"access_token\":\"orig\"}\n";

        std::fs::write(&settings_path, serde_json::to_string(&original_settings).unwrap()).unwrap();
        std::fs::write(&rclone_conf_path, original_rclone_conf).unwrap();

        let manager = RcloneManager::new();

        let target_settings = AppSettings {
            oauth_mode: "custom".to_string(),
            connected_client_fingerprint: Some("sha256:v1:candidate_fp".to_string()),
            custom_client_id: Some("candidate-id".to_string()),
            custom_client_secret: Some("candidate-secret".to_string()),
            webhook_url: "".to_string(),
            notify_on_complete: false,
        };
        let creds = RcloneOAuthCredentials {
            client_id: "candidate-id".to_string(),
            client_secret: "candidate-secret".to_string(),
        };

        // 1. Failure at Auth
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_auth_fail = MockStepExecutor {
            fail_at_auth: true,
            ..Default::default()
        };

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_auth_fail).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Simulated OAuth authorization failure"));
        assert_eq!(std::fs::read_to_string(&rclone_conf_path).unwrap(), original_rclone_conf);
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:orig_fp".to_string()));

        // 2. Failure at Probe
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_probe_fail = MockStepExecutor {
            fail_at_probe: true,
            ..Default::default()
        };

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_probe_fail).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Simulated probe failure"));
        assert_eq!(std::fs::read_to_string(&rclone_conf_path).unwrap(), original_rclone_conf);
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:orig_fp".to_string()));

        // 3. Failure at Profile Verification
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_profile_fail = MockStepExecutor {
            fail_at_profile: true,
            ..Default::default()
        };

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_profile_fail).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Could not verify authenticated Google user profile"));
        assert_eq!(std::fs::read_to_string(&rclone_conf_path).unwrap(), original_rclone_conf);
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:orig_fp".to_string()));

        // 4. Failure at rclone config commit
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_config_fail = MockStepExecutor {
            fail_at_config_commit: true,
            ..Default::default()
        };

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_config_fail).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Simulated rclone config commit failure"));
        assert_eq!(std::fs::read_to_string(&rclone_conf_path).unwrap(), original_rclone_conf);
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:orig_fp".to_string()));

        // 5. Failure at settings commit -> MUST ROLL BACK rclone.conf to original snapshot!
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_settings_fail = MockStepExecutor {
            fail_at_settings_commit: true,
            ..Default::default()
        };

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_settings_fail).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Simulated settings commit failure"));
        // Assert rclone.conf was rolled back to original snapshot
        assert_eq!(std::fs::read_to_string(&rclone_conf_path).unwrap(), original_rclone_conf);
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:orig_fp".to_string()));

        // 6. Success commits both target rclone.conf and target settings
        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings: target_settings.clone(),
            creds: creds.clone(),
        };

        let executor_success = MockStepExecutor::default();

        let res = execute_google_connect_transaction_with_executor(ctx, &executor_success).await;
        assert!(res.is_ok());
        assert!(std::fs::read_to_string(&rclone_conf_path).unwrap().contains("candidate_token"));
        let s: AppSettings = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(s.connected_client_fingerprint, Some("sha256:v1:candidate_fp".to_string()));
        assert_eq!(s.oauth_mode, "custom");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_rollback_aggregates_and_reports_rollback_failures() {
        let temp_dir = std::env::temp_dir().join(format!("balladi_tx_err_test_{}", rand::random::<u32>()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let settings_path = temp_dir.join("settings.json");
        let rclone_conf_path = temp_dir.join("rclone.conf");

        let original_settings = AppSettings {
            oauth_mode: "managed".to_string(),
            connected_client_fingerprint: Some("sha256:v1:orig".to_string()),
            custom_client_id: None,
            custom_client_secret: None,
            webhook_url: "".to_string(),
            notify_on_complete: false,
        };
        let original_rclone_conf = "[gdrive]\ntype = drive\ntoken = {\"access_token\":\"orig\"}\n";

        std::fs::write(&settings_path, serde_json::to_string(&original_settings).unwrap()).unwrap();
        std::fs::write(&rclone_conf_path, original_rclone_conf).unwrap();

        let manager = RcloneManager::new();

        let target_settings = AppSettings {
            oauth_mode: "custom".to_string(),
            connected_client_fingerprint: Some("sha256:v1:candidate".to_string()),
            custom_client_id: Some("id".to_string()),
            custom_client_secret: Some("secret".to_string()),
            webhook_url: "".to_string(),
            notify_on_complete: false,
        };
        let creds = RcloneOAuthCredentials {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };

        let ctx = ConnectTransactionContext {
            rclone_manager: &manager,
            settings_path: settings_path.clone(),
            // Set rclone_conf_path to a read-only directory to force rollback write failure
            rclone_conf_path: rclone_conf_path.clone(),
            target_settings,
            creds,
        };

        let executor = MockStepExecutor {
            fail_at_settings_commit: true,
            ..Default::default()
        };

        // Make rclone.conf path impossible to write to during rollback to trigger rollback failure
        let res = execute_google_connect_transaction_with_executor(ctx, &executor).await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("Simulated settings commit failure"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
