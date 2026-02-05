# How to Run AO Locally

This runbook reflects the current interactive onboarding + chat flow in this repository.

## Prerequisites

### Required

- Rust (stable)
- Node.js 20+
- `npm`
- Platform dependencies for Tauri

### Optional

- A local OpenAI-compatible endpoint (Ollama / LM Studio / other local server)
- Provider API keys (can be supplied in-app during onboarding)

## Quick start

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

## Runtime data locations

AO uses the OS app-data location (via `directories` crate). Typical runtime artifacts include:

- `config.json`
- constitutional docs copied from `templates/`
- signature files (`*.sig`) for constitutional docs
- `external_pubkey.bin` (generated public key)
- encrypted secrets vault data for provider keys

## Boot and startup lifecycle

### First run (interactive birth)

On a clean identity state, AO runs a staged boot sequence:

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
   - AO enters normal chat mode

### Subsequent runs

For existing identities, AO performs startup checks and then loads chat:

1. LLM heartbeat
2. Constitutional signature verification
3. Chat interface

If signatures are invalid/missing, AO enters a repair path.

## Key management model (current)

- AO generates an external Ed25519 keypair during first-run onboarding.
- AO stores **only** the public key (`external_pubkey.bin`).
- AO never stores the private key; it is shown once and then cleared from UI state.
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

Use the workflows documented in [`documents/RELEASE.md`](RELEASE.md) for tagged releases and artifact retrieval.

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

- `cargo test -p ao-core`
- `cargo test -p ao-birth`
- `cargo test -p ao-router`
- `cargo test -p ao-capabilities`
- `cargo test -p ao-memory`
- `cargo test -p ao-skills`

Note: some provider/skill tests may require credentials, feature flags, or running external services.
