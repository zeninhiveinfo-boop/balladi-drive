use std::path::PathBuf;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedDriveLink {
    pub is_valid: bool,
    pub is_folder: bool,
    pub is_file: bool,
    pub id: String,
    pub resource_key: Option<String>,
    pub connection_string: String,
    pub error: Option<String>,
}

/// Parses any Google Drive URL into an ID and rclone connection string
pub fn parse_google_drive_link(url: &str) -> ParsedDriveLink {
    let trimmed = url.trim();

    // Check resource key if present in query params (?resourcekey=xxx or &resourcekey=xxx)
    let rk_re = Regex::new(r"[?&]resourcekey=([a-zA-Z0-9_-]+)").unwrap();
    let resource_key = rk_re
        .captures(trimmed)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string());

    // Pattern 1: Folder link (e.g. /folders/1A2B3C... or /folders/1A2B3C?usp=sharing)
    let folder_re = Regex::new(r"/folders/([a-zA-Z0-9_-]+)").unwrap();
    if let Some(cap) = folder_re.captures(trimmed) {
        if let Some(id_match) = cap.get(1) {
            let id = id_match.as_str().to_string();
            let conn = build_connection_string(&id, resource_key.as_deref());
            return ParsedDriveLink {
                is_valid: true,
                is_folder: true,
                is_file: false,
                id,
                resource_key,
                connection_string: conn,
                error: None,
            };
        }
    }

    // Pattern 2: Single file link (e.g. /file/d/1A2B3C.../view)
    let file_re = Regex::new(r"/file/d/([a-zA-Z0-9_-]+)").unwrap();
    if let Some(cap) = file_re.captures(trimmed) {
        if let Some(id_match) = cap.get(1) {
            let id = id_match.as_str().to_string();
            let conn = build_connection_string(&id, resource_key.as_deref());
            return ParsedDriveLink {
                is_valid: true,
                is_folder: false,
                is_file: true,
                id,
                resource_key,
                connection_string: conn,
                error: None,
            };
        }
    }

    // Pattern 3: Legacy open?id=1A2B3C... or uc?id=...
    let id_re = Regex::new(r"[?&]id=([a-zA-Z0-9_-]+)").unwrap();
    if let Some(cap) = id_re.captures(trimmed) {
        if let Some(id_match) = cap.get(1) {
            let id = id_match.as_str().to_string();
            let conn = build_connection_string(&id, resource_key.as_deref());
            return ParsedDriveLink {
                is_valid: true,
                is_folder: true, // assume folder unless proven otherwise
                is_file: false,
                id,
                resource_key,
                connection_string: conn,
                error: None,
            };
        }
    }

    // If already raw ID (alphanumeric with - and _, 20-50 chars)
    let raw_id_re = Regex::new(r"^[a-zA-Z0-9_-]{20,50}$").unwrap();
    if raw_id_re.is_match(trimmed) {
        let conn = build_connection_string(trimmed, None);
        return ParsedDriveLink {
            is_valid: true,
            is_folder: true,
            is_file: false,
            id: trimmed.to_string(),
            resource_key: None,
            connection_string: conn,
            error: None,
        };
    }

    ParsedDriveLink {
        is_valid: false,
        is_folder: false,
        is_file: false,
        id: String::new(),
        resource_key: None,
        connection_string: String::new(),
        error: Some("Invalid Google Drive URL. Please paste a valid folder or file link.".into()),
    }
}

fn build_connection_string(id: &str, resource_key: Option<&str>) -> String {
    match resource_key {
        Some(rk) => format!("gdrive,root_folder_id={},resource_key={}:", id, rk),
        None => format!("gdrive,root_folder_id={}:", id),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GoogleUserInfo {
    pub is_authenticated: bool,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub photo_link: Option<String>,
    pub storage_total: Option<u64>,
    pub storage_used: Option<u64>,
}

pub fn find_rclone_conf_path() -> Option<PathBuf> {
    let candidate_paths = [
        dirs::home_dir().map(|h| h.join(".config/rclone/rclone.conf")),
        dirs::config_dir().map(|c| c.join("rclone/rclone.conf")),
        dirs::home_dir().map(|h| h.join("Library/Application Support/rclone/rclone.conf")),
    ];
    candidate_paths.into_iter().flatten().find(|p| p.exists())
}

pub fn extract_gdrive_access_token(content: &str) -> Option<String> {
    let mut in_gdrive = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = &trimmed[1..trimmed.len() - 1];
            in_gdrive = section.eq_ignore_ascii_case("gdrive");
            continue;
        }
        if in_gdrive && trimmed.starts_with("token") {
            if let Some(pos) = trimmed.find('=') {
                let json_part = trimmed[pos + 1..].trim();
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_part) {
                    if let Some(tok) = val.get("access_token").and_then(|v| v.as_str()) {
                        return Some(tok.to_string());
                    }
                }
            }
        }
    }
    // Fallback regex specifically anchored to gdrive
    let token_re = Regex::new(r#"(?s)\[gdrive\][^\[]*?token\s*=\s*(\{.*?\})"#).ok()?;
    let cap = token_re.captures(content)?;
    let json_str = cap.get(1)?.as_str();
    let val: serde_json::Value = serde_json::from_str(json_str).ok()?;
    val["access_token"].as_str().map(|s| s.to_string())
}

pub async fn get_google_access_token_with_credentials(
    creds: &crate::RcloneOAuthCredentials,
    rclone_conf_path: Option<&std::path::Path>,
) -> Option<String> {
    let conf_path = match rclone_conf_path {
        Some(p) => p.to_path_buf(),
        None => find_rclone_conf_path()?,
    };

    let read_token = |path: &std::path::Path| -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        extract_gdrive_access_token(&content)
    };

    let mut access_token = read_token(&conf_path)?;

    // Health check token with quick Drive call
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap_or_default();

    let res = client
        .get("https://www.googleapis.com/drive/v3/about?fields=user")
        .bearer_auth(&access_token)
        .send()
        .await;

    if let Ok(r) = res {
        if r.status() == reqwest::StatusCode::UNAUTHORIZED {
            let bin = crate::engine::rclone::RcloneManager::find_rclone_binary();
            let mut refresh_cmd = tokio::process::Command::new(&bin);
            crate::apply_google_oauth_env_tokio(&mut refresh_cmd, creds);
            if let Some(cfg) = rclone_conf_path {
                refresh_cmd.arg("--config").arg(cfg);
            }
            let out = refresh_cmd
                .args(["about", "gdrive:"])
                .output()
                .await
                .ok()?;

            if !out.status.success() {
                return None;
            }

            let stderr_str = String::from_utf8_lossy(&out.stderr).to_lowercase();
            if stderr_str.contains("shared client") {
                return None;
            }

            access_token = read_token(&conf_path)?;
        }
    }

    Some(access_token)
}

pub async fn get_google_access_token() -> Option<String> {
    let creds = crate::load_rclone_oauth_credentials().ok()?;
    get_google_access_token_with_credentials(&creds, None).await
}

pub async fn get_google_user_profile_with_credentials(
    creds: &crate::RcloneOAuthCredentials,
    rclone_conf_path: Option<&std::path::Path>,
) -> GoogleUserInfo {
    let conf_path = match rclone_conf_path {
        Some(p) => p.to_path_buf(),
        None => match find_rclone_conf_path() {
            Some(p) => p,
            None => {
                return GoogleUserInfo {
                    is_authenticated: false,
                    display_name: None,
                    email: None,
                    photo_link: None,
                    storage_total: None,
                    storage_used: None,
                };
            }
        },
    };

    let read_token = |path: &std::path::Path| -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        extract_gdrive_access_token(&content)
    };

    let mut access_token = match read_token(&conf_path) {
        Some(t) => t,
        None => {
            return GoogleUserInfo {
                is_authenticated: false,
                display_name: None,
                email: None,
                photo_link: None,
                storage_total: None,
                storage_used: None,
            };
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .unwrap_or_default();

    let mut res = client
        .get("https://www.googleapis.com/drive/v3/about?fields=user,storageQuota")
        .bearer_auth(&access_token)
        .send()
        .await;

    // If unauthorized / token expired, ask rclone to refresh the token and retry once
    if let Ok(ref r) = res {
        if r.status() == reqwest::StatusCode::UNAUTHORIZED {
            let bin = crate::engine::rclone::RcloneManager::find_rclone_binary();
            let mut refresh_cmd = tokio::process::Command::new(&bin);
            crate::apply_google_oauth_env_tokio(&mut refresh_cmd, creds);
            if let Some(cfg) = rclone_conf_path {
                refresh_cmd.arg("--config").arg(cfg);
            }
            let out = refresh_cmd
                .args(["about", "gdrive:"])
                .output()
                .await;

            if let Ok(output) = out {
                if output.status.success() {
                    let stderr_str = String::from_utf8_lossy(&output.stderr).to_lowercase();
                    if !stderr_str.contains("shared client") {
                        if let Some(refreshed_token) = read_token(&conf_path) {
                            access_token = refreshed_token;
                            res = client
                                .get("https://www.googleapis.com/drive/v3/about?fields=user,storageQuota")
                                .bearer_auth(&access_token)
                                .send()
                                .await;
                        }
                    }
                }
            }
        }
    }

    if let Ok(r) = res {
        if r.status().is_success() {
            if let Ok(json) = r.json::<serde_json::Value>().await {
                let user = &json["user"];
                let quota = &json["storageQuota"];

                let display_name = user["displayName"].as_str().map(|s| s.to_string());
                let email = user["emailAddress"].as_str().map(|s| s.to_string());
                let photo_link = user["photoLink"].as_str().map(|s| s.to_string());
                let storage_total = quota["limit"].as_str().and_then(|s| s.parse::<u64>().ok());
                let storage_used = quota["usage"].as_str().and_then(|s| s.parse::<u64>().ok());

                return GoogleUserInfo {
                    is_authenticated: true,
                    display_name,
                    email,
                    photo_link,
                    storage_total,
                    storage_used,
                };
            }
        }
    }

    GoogleUserInfo {
        is_authenticated: false,
        display_name: None,
        email: None,
        photo_link: None,
        storage_total: None,
        storage_used: None,
    }
}

pub async fn get_google_user_profile() -> GoogleUserInfo {
    let creds = match crate::load_rclone_oauth_credentials() {
        Ok(c) => c,
        Err(_) => {
            return GoogleUserInfo {
                is_authenticated: false,
                display_name: None,
                email: None,
                photo_link: None,
                storage_total: None,
                storage_used: None,
            };
        }
    };

    get_google_user_profile_with_credentials(&creds, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_folder_links() {
        let parsed = parse_google_drive_link(
            "https://drive.google.com/drive/folders/1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB?usp=sharing",
        );
        assert!(parsed.is_valid);
        assert!(parsed.is_folder);
        assert!(!parsed.is_file);
        assert_eq!(parsed.id, "1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB");
        assert_eq!(
            parsed.connection_string,
            "gdrive,root_folder_id=1AaN85zoEOEDxc_zgVYIFdWVwX9wn7_aB:"
        );
    }

    #[test]
    fn test_parse_single_file_links() {
        let parsed = parse_google_drive_link(
            "https://drive.google.com/file/d/1X9wn7_aB1AaN85zoEOEDxc_zgVYIFdWVw/view",
        );
        assert!(parsed.is_valid);
        assert!(!parsed.is_folder);
        assert!(parsed.is_file);
        assert_eq!(parsed.id, "1X9wn7_aB1AaN85zoEOEDxc_zgVYIFdWVw");
    }

    #[test]
    fn test_parse_invalid_links() {
        let parsed = parse_google_drive_link("https://example.com/not-drive");
        assert!(!parsed.is_valid);
    }

    #[test]
    fn test_extract_gdrive_access_token_multi_section() {
        let conf = r#"
[other_remote]
type = s3
token = {"access_token":"s3_token_ignore"}

[gdrive]
type = drive
scope = drive
token = {"access_token":"valid_gdrive_access_token_xyz","token_type":"Bearer"}

[backup]
type = drive
token = {"access_token":"backup_token"}
"#;
        let token = extract_gdrive_access_token(conf);
        assert_eq!(token, Some("valid_gdrive_access_token_xyz".to_string()));
    }

    #[test]
    fn test_extract_gdrive_access_token_missing() {
        let conf = r#"
[other]
type = drive
"#;
        let token = extract_gdrive_access_token(conf);
        assert_eq!(token, None);
    }
}
