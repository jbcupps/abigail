# How to Run Abby Locally

## Prerequisites

- Rust (stable; on Windows use target `x86_64-pc-windows-msvc` if needed)
- Node.js 20+ (for frontend)
- (Optional) OpenAI API key for Ego routing

## Build and run

1. **Workspace build**
   ```bash
   cargo build
   ```

2. **Tauri app (desktop)**
   - Install frontend deps: `cd tauri-app/src-ui && npm install`
   - From repo root: `cargo tauri dev` (or from tauri-app if your CLI supports it)
   - See **Building an installer** below for producing a clickable installer (CI or local).

3. **Config / data**
   - Data dir: `%LOCALAPPDATA%\abby\Abby` (or `directories` crate default)
   - Config: `data_dir/config.json`
   - First run: birth sequence creates keyring, signed constitutional docs, and DB

## Environment

- `OPENAI_API_KEY` — optional; enables Ego (cloud) for COMPLEX routing
- See `example.env` for placeholders (no real values)

## Building an installer

You can get a desktop installer (e.g. Windows `.exe`) that you or an end user can double-click to install Abby.

**Download page:** For end users, go to the [Abby download page](https://jbcupps.github.io/abby/) to get the installer for your OS; the page picks the right file and starts the download. (Requires GitHub Pages to be enabled: **Settings → Pages → Source:** Deploy from a branch, folder **/docs**.)

### Option A — From CI (no local build)

1. **One-time:** Ensure version is set in `Cargo.toml` and `tauri-app/tauri.conf.json` (e.g. `0.0.1`).
2. **Get installers:**
   - **Tag-based:** `git tag v0.0.1 && git push origin v0.0.1` → after the workflow completes, open **Releases** → draft release → download **Windows** (`.exe`), **Linux** (`.deb`), or **macOS** (`.dmg`).
   - **Manual run:** **Actions** → **build-release** → **Run workflow** (optionally set "Release version") → when done, open the workflow run → **Artifacts** → download `abby-installer-windows-latest` (contains the `.exe`), or the Linux/macOS artifacts.
3. **Run the installer:** Double-click the downloaded `.exe` (or `.dmg`/`.deb` on other platforms). On Windows, the NSIS installer runs for the current user (no admin required).

### Option B — Local one-command build

1. **One-time:** Install frontend deps: `cd tauri-app/src-ui && npm install`
2. **Build installer:** From repo root, run:
   ```bash
   cd tauri-app && cargo tauri build
   ```
   Tauri runs `beforeBuildCommand` (`cd src-ui && npm run build`) automatically, so one command builds the frontend and the app bundle.
3. **Output:** The installer is written under `tauri-app/target/release/bundle/` (or the workspace `target/` if unified). On Windows: `tauri-app/target/release/bundle/nsis/Abby_0.0.1_x64-setup.exe` (version may vary). Open that folder and double-click the `.exe` to run the installer.

Alternatively, use the convenience script from repo root: `scripts/build-installer.ps1` (Windows) or `./scripts/build-installer.sh` (macOS/Linux). The script installs frontend deps if needed, runs the Tauri build, then opens the bundle folder.

## Tests

- `cargo test -p abby-core` — tamper detection
- `cargo test -p abby-memory` — birth record
- `cargo test -p abby-router` — routing decision
- `cargo test -p abby-llm` — OpenAI (skips without `OPENAI_API_KEY`)
- `cargo test -p abby-senses` — IMAP (skips without `ABBY_IMAP_TEST` and credentials)
