# How to Run AO Locally

## Prerequisites

- Rust (stable; on Windows use target `x86_64-pc-windows-msvc` if needed)
- Node.js 20+ (for frontend)
- (Optional) Local LLM server (LiteLLM, Ollama, LM Studio) for real inference
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
   - Data dir: `%LOCALAPPDATA%\ao\AO` (or `directories` crate default)
   - Config: `data_dir/config.json`
   - First run: birth sequence copies constitutional docs and creates internal keyring

## Startup flow (First Run)

When you click **Start** for the first time, AO runs through the birth sequence:

1. **Initialize** — Copies constitutional documents (soul.md, ethics.md, instincts.md) to data dir, creates internal keyring.
2. **Generate signing keypair** — Creates a new Ed25519 keypair for signing constitutional documents.
3. **CRITICAL: Save your private key** — The private key is displayed ONCE with security warnings. You MUST save it securely before proceeding. AO does NOT store this key.
4. **Sign documents** — Constitutional docs are signed with your private key.
5. **LLM heartbeat** — Verifies the local LLM is reachable (if `local_llm_base_url` is set; otherwise uses in-process stub).
6. **Signature verification** — Verifies constitutional docs against the stored public key.
7. **AO informed OK** — If all checks pass, AO engages and shows the chat interface.

## Startup flow (Subsequent Runs)

On subsequent runs (already born), AO runs these checks automatically:

1. **LLM heartbeat** — Verifies local LLM connectivity.
2. **Signature verification** — Verifies constitutional docs against stored public key.
3. **Chat** — If checks pass, shows the chat interface.

## Environment

- `OPENAI_API_KEY` — optional; enables Ego (cloud) for COMPLEX routing
- `LOCAL_LLM_BASE_URL` — optional; base URL for local LLM server (e.g. `http://localhost:1234`)
- `EXTERNAL_PUBKEY_PATH` — optional; path to external public key for signature verification
- See `example.env` for placeholders (no real values)

## External signing key

The signing keypair is now generated automatically at first run:

1. **First run:** AO generates an Ed25519 keypair and displays the private key ONCE.
2. **Save it:** Copy the private key and store it securely (password manager, encrypted drive, etc.).
3. **Public key:** Automatically saved to `{data_dir}/external_pubkey.bin` and auto-detected.
4. **Private key:** NEVER stored by AO. You are responsible for keeping it safe.

### Why save the private key?

- **Recovery:** If you reinstall AO or lose your data, you'll need the private key to re-sign documents.
- **Verification:** The private key proves you are the original mentor of this AO instance.
- **Security:** If compromised, someone could create fake constitutional documents.

### Legacy: Manual keypair generation

The `scripts/generate-signing-key.ps1` script still exists for advanced use cases (signing templates before distribution). For normal use, the automatic first-run generation is recommended.

## Building an installer

You can get a desktop installer (e.g. Windows `.exe`) that you or an end user can double-click to install AO.

**Download page:** For end users, go to the [AO download page](https://jbcupps.github.io/ao/) to get the installer for your OS; the page picks the right file and starts the download. (Requires GitHub Pages to be enabled: **Settings → Pages → Source:** Deploy from a branch, folder **/docs**.)

### Option A — From CI (no local build)

1. **One-time:** Ensure version is set in `Cargo.toml` and `tauri-app/tauri.conf.json` (e.g. `0.0.1`).
2. **Get installers:**
   - **Tag-based:** `git tag v0.0.1 && git push origin v0.0.1` → after the workflow completes, open **Releases** → draft release → download **Windows** (`.exe`), **Linux** (`.deb`), or **macOS** (`.dmg`).
   - **Manual run:** **Actions** → **build-release** → **Run workflow** (optionally set "Release version") → when done, open the workflow run → **Artifacts** → download `ao-installer-windows-latest` (contains the `.exe`), or the Linux/macOS artifacts.
3. **Run the installer:** Double-click the downloaded `.exe` (or `.dmg`/`.deb` on other platforms). On Windows, the NSIS installer runs for the current user (no admin required).

### Option B — Local one-command build

1. **One-time:** Install frontend deps: `cd tauri-app/src-ui && npm install`
2. **Build installer:** From repo root, run:
   ```bash
   cd tauri-app && cargo tauri build
   ```
   Tauri runs `beforeBuildCommand` (`cd src-ui && npm run build`) automatically, so one command builds the frontend and the app bundle.
3. **Output:** The installer is written under `tauri-app/target/release/bundle/` (or the workspace `target/` if unified). On Windows: `tauri-app/target/release/bundle/nsis/AO_0.0.1_x64-setup.exe` (version may vary). Open that folder and double-click the `.exe` to run the installer.

Alternatively, use the convenience script from repo root: `scripts/build-installer.ps1` (Windows) or `./scripts/build-installer.sh` (macOS/Linux). The script installs frontend deps if needed, runs the Tauri build, then opens the bundle folder.

## Tests

- `cargo test -p ao-core` — tamper detection
- `cargo test -p ao-memory` — birth record
- `cargo test -p ao-router` — routing decision
- `cargo test -p ao-llm` — OpenAI (skips without `OPENAI_API_KEY`)
- `cargo test -p ao-skills` — IMAP (skips without `AO_IMAP_TEST` and credentials)
