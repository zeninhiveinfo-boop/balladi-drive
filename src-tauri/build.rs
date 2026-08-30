fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile == "release" {
        let client_id = std::env::var("BALLADI_GOOGLE_CLIENT_ID").unwrap_or_default();
        let client_secret = std::env::var("BALLADI_GOOGLE_CLIENT_SECRET").unwrap_or_default();
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            panic!(
                "BALLADI_GOOGLE_CLIENT_ID and BALLADI_GOOGLE_CLIENT_SECRET environment variables are required for release builds."
            );
        }
    }
    tauri_build::build()
}

