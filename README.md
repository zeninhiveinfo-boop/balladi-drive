# Balladi Drive

> **High-Reliability Google Drive Media Transfer for Video Production Teams**  
> Built with **Tauri 2.0**, **Rust**, **React 19**, **TypeScript**, **Tailwind CSS**, and an embedded **rclone** transfer engine.

---

## The Problem Solved

When transferring multi-hundred-gigabyte media projects (RAW footage, proxies, audio stems, project files), Google Drive in the web browser creates dozens of fragmented 2 GB ZIP files, drops connections mid-transfer, offers zero resume capability, and lacks mathematical integrity verification.

**Balladi Drive** provides a simple, dark-themed studio desktop GUI on top of `rclone`'s high-speed transfer daemon:
* **No ZIP fragmentation**: Files stream directly into your folder structure.
* **128 MB Chunk Streams**: Satures gigabit fiber connections on large 10GB–100GB+ video files.
* **Instant Link Downloading**: Paste any Google Drive link (`/folders/ID` or `/file/d/ID`), and the app extracts the ID and downloads directly to your SSD.
* **Camera Card Safe**: Empty directories (`RDC/`, `THUMB/`) and 0-byte `.XML`/`.BUP` files are strictly retained.
* **Pre-Flight Hardware Checks**:
  * **FAT32 Trap Prevention**: Warns if an external drive is formatted as FAT32 (which fails on files > 4 GB).
  * **Mac NTFS Read-Only Check**: Probes write permissions to prevent silent failures on Windows drives plugged into a Mac.
  * **Free Space Calculation**: Calculates download size and verifies disk space before starting.
* **OS Sleep Prevention**: Holds system power assertions (`caffeinate` on macOS / Windows execution states) so long transfers continue uninterrupted when laptops are left unattended.
* **Bit-for-Bit MD5 Verification**: Verifies local files against Google Drive API's native MD5 metadata without re-downloading cloud data.
* **Unattended Webhooks**: Sends a ping to Slack or Discord when a 500 GB shoot finishes transferring.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    REACT / TS FRONTEND                  │
│  • Clean UI (Drag-and-drop, Paste Drive Link, Progress) │
│  • Human-readable error messages & verification badges   │
└───────────────────────────┬─────────────────────────────┘
                            │ Tauri IPC (Commands & Events)
┌───────────────────────────▼─────────────────────────────┐
│                     TAURI / RUST CORE                   │
│  • Single Instance Lock                                 │
│  • Sleep Inhibitor (caffeinate / power assertion)       │
│  • Storage & Filesystem Inspector (FAT32/NTFS guards)   │
│  • Child Process Guard (Zombie process elimination)     │
│  • Secure Token & Configuration Manager                 │
└───────────────────────────┬─────────────────────────────┘
                            │ Localhost HTTP (RC API with Auth)
┌───────────────────────────▼─────────────────────────────┐
│                 RCLONE ENGINE (Sidecar)                 │
│  • Tuned Flags (128M chunks, tpslimit 8, fast-list)     │
│  • Native MD5 Integrity Verification                   │
│  • Jittered Exponential Backoff & Resume               │
└───────────────────────────┬─────────────────────────────┘
                            │ HTTPS (OAuth 2.0)
┌───────────────────────────▼─────────────────────────────┐
│                      GOOGLE DRIVE                       │
└─────────────────────────────────────────────────────────┘
```

---

## Getting Started (Local Development)

### Prerequisites
* **Node.js** (v20+ or v22+)
* **Rust & Cargo** (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
* **rclone** (installed automatically or bundled in `src-tauri/bin/`)

### Running the App in Development Mode
```bash
# 1. Install dependencies
npm ci

# 2. Export Balladi Google OAuth credentials from your client_secret_*.json
export BALLADI_GOOGLE_CLIENT_ID="$(jq -r '.installed.client_id' ~/Downloads/client_secret_*.json)"
export BALLADI_GOOGLE_CLIENT_SECRET="$(jq -r '.installed.client_secret' ~/Downloads/client_secret_*.json)"

# 3. Launch Balladi Drive (hot-reloading React + Rust)
npm run tauri dev
```

### Packaging Production Installers (Mac .dmg & Windows .msi)
```bash
export BALLADI_GOOGLE_CLIENT_ID="..."
export BALLADI_GOOGLE_CLIENT_SECRET="..."
npm run tauri build
```
The compiled installer will be generated in `src-tauri/target/release/bundle/dmg/` (macOS) or `msi/` (Windows).

---

## Studio Google Cloud OAuth (Managed vs Custom)

### 1. Managed OAuth (Studio Distribution)
Production builds released by Balladi Studios embed the studio's Google Desktop Client ID and Secret at build time. Users only click **"Connect Google Account"** in the app to authenticate with dedicated studio API quotas.

### 2. Custom OAuth (Open-Source Developers)
If you clone this repository without the Balladi Studios build keys, you can use your own Google Cloud Console project:
1. Create a Google Cloud Project with the **Google Drive API** enabled.
2. Configure the OAuth consent screen (Desktop App).
3. In **Balladi Drive Settings**, open **"Advanced: Custom OAuth Project"**.
4. Enter your `Client ID` and `Client Secret`, then click **"Connect Google Account"**.

---

## Automated CI/CD Releases (GitHub Actions)

When you push a Git tag to your GitHub repository:
```bash
git tag v1.0.0
git push --tags
```
The included GitHub Actions workflow (`.github/workflows/release.yml`) will automatically:
1. Validate release environment secrets (`BALLADI_GOOGLE_CLIENT_ID`, `BALLADI_GOOGLE_CLIENT_SECRET`).
2. Compile and package the Apple Silicon macOS `.dmg` and Windows `.msi` installers.
3. Bundle the tuned `rclone` binary.
4. Publish a new GitHub Release with all download assets ready for your team.

---

## License & Credits

Released under the [MIT License](LICENSE).  
Transfer engine powered by [rclone](https://rclone.org) (MIT License). GUI powered by [Tauri 2.0](https://tauri.app) and React.

