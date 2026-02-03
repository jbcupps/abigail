# How to Run Abby Locally

## Prerequisites

- Rust (stable, Windows target `x86_64-pc-windows-msvc`)
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
   - **Pre-built installers:** On push to `master`, GitHub Actions builds installers; download from the [Actions](https://github.com/YOUR_ORG/Abby/actions) tab (workflow **build-release**, artifact **abby-installer-&lt;platform&gt;**).

3. **Config / data**
   - Data dir: `%LOCALAPPDATA%\abby\Abby` (or `directories` crate default)
   - Config: `data_dir/config.json`
   - First run: birth sequence creates keyring, signed constitutional docs, and DB

## Environment

- `OPENAI_API_KEY` — optional; enables Ego (cloud) for COMPLEX routing
- See `example.env` for placeholders (no real values)

## Tests

- `cargo test -p abby-core` — tamper detection
- `cargo test -p abby-memory` — birth record
- `cargo test -p abby-router` — routing decision
- `cargo test -p abby-llm` — OpenAI (skips without `OPENAI_API_KEY`)
- `cargo test -p abby-senses` — IMAP (skips without `ABBY_IMAP_TEST` and credentials)
