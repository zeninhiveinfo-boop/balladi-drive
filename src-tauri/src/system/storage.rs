use serde::{Deserialize, Serialize};
use std::fs::{remove_file, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::Disks;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StorageInfo {
    pub path: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub free_gb: f64,
    pub total_gb: f64,
    pub file_system: String,
    pub is_writable: bool,
    pub is_fat32: bool,
    pub error: Option<String>,
}

/// Inspect a local path for disk space, filesystem type, and optional write permissions
pub fn inspect_storage(target_path: &str, probe_write: bool) -> StorageInfo {
    // If target is a cloud remote (e.g. gdrive:), return a cloud storage placeholder
    if target_path.starts_with("gdrive:") || target_path.starts_with("gdrive,") {
        return StorageInfo {
            path: target_path.to_string(),
            mount_point: "Google Drive (Cloud)".to_string(),
            total_bytes: 0,
            free_bytes: 0,
            total_gb: 0.0,
            free_gb: 0.0,
            file_system: "Cloud".to_string(),
            is_writable: true,
            is_fat32: false,
            error: None,
        };
    }

    let path_obj = Path::new(target_path);
    let resolved_path = path_obj.canonicalize().unwrap_or_else(|_| {
        if let Some(parent) = path_obj.parent() {
            parent.canonicalize().unwrap_or_else(|_| path_obj.to_path_buf())
        } else {
            path_obj.to_path_buf()
        }
    });

    let mut disks = Disks::new_with_refreshed_list();
    disks.refresh(true);

    let mut matched_mount = String::new();
    let mut total_bytes: u64 = 0;
    let mut free_bytes: u64 = 0;
    let mut fs_type = String::from("Unknown");
    let mut best_match_len = 0;

    for disk in disks.list() {
        let mount = disk.mount_point();
        if resolved_path.starts_with(mount) || path_obj.starts_with(mount) {
            let len = mount.as_os_str().len();
            if len > best_match_len {
                best_match_len = len;
                matched_mount = mount.to_string_lossy().to_string();
                total_bytes = disk.total_space();
                free_bytes = disk.available_space();
                fs_type = disk.file_system().to_string_lossy().to_string();
            }
        }
    }

    let is_fat32 = fs_type.to_lowercase().contains("fat") || fs_type.to_lowercase().contains("vfat");

    let mut is_writable = true;
    let mut write_error: Option<String> = None;

    if probe_write {
        // Generate a cryptographically randomized, PID-scoped temporary filename
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let probe_filename = format!(".balladi_probe_{}_{}", pid, now_ts);

        let target_dir = if path_obj.is_dir() {
            path_obj
        } else {
            path_obj.parent().unwrap_or(path_obj)
        };
        let test_file_path = target_dir.join(probe_filename);

        // create_new(true) guarantees atomic creation without overwriting or truncating any existing file
        match OpenOptions::new().write(true).create_new(true).open(&test_file_path) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(b"probe") {
                    is_writable = false;
                    write_error = Some(format!("Write test failed: {}", e));
                }
                drop(f);
                let _ = remove_file(&test_file_path);
            }
            Err(e) => {
                is_writable = false;
                write_error = Some(format!("Destination directory is not writable: {}", e));
            }
        }
    }

    StorageInfo {
        path: target_path.to_string(),
        mount_point: matched_mount,
        total_bytes,
        free_bytes,
        total_gb: (total_bytes as f64) / (1024.0 * 1024.0 * 1024.0),
        free_gb: (free_bytes as f64) / (1024.0 * 1024.0 * 1024.0),
        file_system: fs_type,
        is_writable,
        is_fat32,
        error: write_error,
    }
}
