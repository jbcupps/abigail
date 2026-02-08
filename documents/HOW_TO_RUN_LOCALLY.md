# How to Run Abigail Locally

This runbook reflects the current interactive onboarding + chat flow in this repository.

## Prerequisites

### Required (native development)

- Rust (stable)
- Node.js 20+
- `npm`
- Platform dependencies for Tauri:
  - **Windows**: NSIS (for installer builds)
  - **macOS**: Xcode Command Line Tools
  - **Ubuntu 22.04+**: `libwebkit2gtk-4.1-dev libappindicator3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev`

### Required (Docker development)

- Docker 20+ and Docker Compose v2. The `docker/` directory contains `Dockerfile` and `docker-compose.yml` for containerized build and test (Docker-first option).

### Optional

- A local OpenAI-compatible endpoint (Ollama / LM Studio / other local server)
- Provider API keys (can be supplied in-app during onboarding)

## Quick start (native)

From the repository root:

```bash
cargo build
cd tauri-app/src-ui && npm install && cd ../..
cargo tauri dev
```

If `cargo tauri` is not available:

```bash
cargo install tauri-cli
```

## Quick start (Docker)

Use the development container to avoid installing Rust/Node.js/system deps on your host:

```bash
# Build the dev image (from repo root; context is parent directory)
docker compose -f docker/docker-compose.yml build

# Start the dev container
docker compose -f docker/docker-compose.yml up -d abigail-dev

# Open a shell inside the container
docker compose -f docker/docker-compose.yml exec abigail-dev bash

# Inside the container:
cd tauri-app/src-ui && npm install && cd ../..
cargo build
cargo test --all
```

The container bind-mounts the repo so edits on your host appear immediately. Cargo build cache is stored in `.cargo-docker/` at the workspace root.

**Note**: Running the GUI (`cargo tauri dev`) inside Docker requires X11/Wayland forwarding, which is platform-specific and not covered here. Use Docker for building, testing, and CI validation; use native development for GUI work.

### Build validation (one-shot)

To run a full build + test in an isolated container (mimics CI):

```bash
docker compose -f docker/docker-compose.yml run --rm abigail-build
```

## Runtime data locations

Abigail uses the OS app-data location (via `directories` crate). Typical runtime artifacts include:

- `config.json`
- constitutional docs copied from `templates/`
- signature files (`*.sig`) for constitutional docs
- `external_pubkey.bin` (generated public key)
- encrypted secrets vault data for provider keys

## Boot and startup lifecycle

### First run (interactive birth)

On a clean identity state, Abigail runs a staged boot sequence:

1. **Darkness**
   - Initializes soul docs and internal keyring state
   - Checks identity status
2. **KeyPresentation**
   - Generates external Ed25519 signing identity
   - Displays private key once (must be saved by the user)
   - Signs constitutional docs
3. **Ignition**
   - Detects or accepts a local LLM endpoint
4. **Connectivity**
   - Optionally stores + validates API keys (OpenAI, Anthropic, xAI, Google, Tavily)
5. **Genesis / SoulPreview**
   - Captures identity prompts and crystallizes initial persona text
6. **Emergence**
   - Completes constitutional signing/transition
7. **Life**
   - Abigail enters normal chat mode

### Subsequent runs

For existing identities, Abigail performs startup checks and then loads chat:

1. LLM heartbeat
2. Constitutional signature verification
3. Chat interface

If signatures are invalid/missing, Abigail enters a repair path.

## Key management model (current)

- Abigail generates an external Ed25519 keypair during first-run onboarding.
- Abigail stores **only** the public key (`external_pubkey.bin`).
- Abigail never stores the private key; it is shown once and then cleared from UI state.
- The private key is required to re-sign/recover identity integrity after data loss or reset.

Recommended handling:

- Store private key in a password manager or encrypted vault.
- Treat loss of this key as loss of signing authority for that identity.

## Provider configuration

You can configure providers in two ways:

- **Environment variables** (`OPENAI_API_KEY`, `LOCAL_LLM_BASE_URL`, `EXTERNAL_PUBKEY_PATH`)
- **In-app during onboarding/chat** (encrypted provider-key storage + optional validation)

Reference template: [`example.env`](../example.env)

## Installer builds

### CI/release builds

Use the workflows documented in [`documents/RELEASE.md`](RELEASE.md) for tagged releases and artifact retrieval. CI builds for all three platforms:

| Platform | Artifact | Runner |
|----------|----------|--------|
| Windows (x64) | `Abigail-windows-x64-setup.exe` | `windows-latest` |
| macOS (Universal) | `Abigail-macos-universal.dmg` | `macos-latest` |
| Ubuntu (x64) | `Abigail-linux-x64.deb` | `ubuntu-22.04` |

### npm install (end users)

Users with Node.js 18+ can install with:

```bash
npx abigail-desktop
```

This downloads and runs the correct platform installer automatically.

### Local installer build

```bash
cd tauri-app/src-ui && npm install && cd ../..
cd tauri-app && cargo tauri build
```

Helper scripts from repo root:

- `./scripts/build-installer.sh` (macOS/Linux)
- `powershell -File scripts/build-installer.ps1` (Windows)

## Validation commands

Run from repo root:

```bash
cargo test
```

Focused crate tests:

- `cargo test -p abigail-core`
- `cargo test -p abigail-birth`
- `cargo test -p abigail-router`
- `cargo test -p abigail-capabilities`
- `cargo test -p abigail-memory`
- `cargo test -p abigail-skills`

Note: some provider/skill tests may require credentials, feature flags, or running external services.
