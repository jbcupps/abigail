# AO

[![CI](https://github.com/jbcupps/ao/actions/workflows/ci.yml/badge.svg)](https://github.com/jbcupps/ao/actions/workflows/ci.yml)
[![Security Audit](https://github.com/jbcupps/ao/actions/workflows/security-audit.yml/badge.svg)](https://github.com/jbcupps/ao/actions/workflows/security-audit.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

AO is a local-first desktop agent built with [Tauri 2.0](https://tauri.app/), Rust, and React. It combines constitutional integrity checks, first-run identity creation, and multi-provider LLM reasoning in a single desktop app.

## Features

- **Interactive birth flow** with staged onboarding (Darkness, KeyPresentation, Ignition, Connectivity, Genesis, Emergence, Life).
- **First-run signing key generation** with one-time private-key presentation and automatic constitutional document signing.
- **Local LLM discovery + manual connect** for Ollama/LM Studio-compatible endpoints.
- **In-app API key vaulting + validation** for cloud/model/search providers.
- **Dual persona UI modes** (surface chat + Forge mode toggle).
- **Id/Ego routing** -- local LLM for routine queries, cloud LLM for complex queries.
- **Skill-based tool execution** including web-search capability.
- **Constitutional integrity** -- Ed25519 signed documents verified at every boot.

## System Requirements

| Platform | Status | Notes |
|----------|--------|-------|
| Windows 10+ | Supported | Primary target. Secrets encrypted via DPAPI. |
| macOS 10.15+ | Supported | Universal binary (Intel + Apple Silicon). Not notarized -- right-click to open on first launch. |
| Ubuntu 22.04+ | Supported | Requires `libwebkit2gtk-4.1-0` and `libayatana-appindicator3-1`. |

## Quick Start

### For End Users

Download the latest installer from [GitHub Releases](https://github.com/jbcupps/ao/releases/latest) or install via npm:

```bash
npx ao-desktop install
```

### For Developers

**Prerequisites**: Rust stable, Node.js 20+, and platform-specific Tauri dependencies.

```bash
# Clone and build
git clone https://github.com/jbcupps/ao.git
cd ao
cargo build

# Install frontend dependencies (one-time)
cd tauri-app/src-ui && npm install && cd ../..

# Launch with hot-reload
cargo tauri dev
```

For Docker-based development, see [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md).

### Environment Variables (Optional)

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | Cloud provider fallback (Ego routing) |
| `LOCAL_LLM_BASE_URL` | Local LLM endpoint override (e.g., `http://localhost:1234`) |
| `EXTERNAL_PUBKEY_PATH` | Explicit public key path (otherwise auto-detected) |

See [`example.env`](example.env) for the full list.

## Architecture

| Area | Purpose |
|------|---------|
| `crates/ao-core` | Config, verifier, key management, secrets, system prompt primitives |
| `crates/ao-birth` | Birth stages/prompts and orchestration logic |
| `crates/ao-memory` | SQLite-backed memory storage |
| `crates/ao-capabilities` | Provider adapters, cognitive/sensory capability modules |
| `crates/ao-router` | Id/Ego routing and provider selection |
| `crates/ao-skills` | Skill registry, executor, protocols, sandbox and events |
| `skills/skill-web-search` | Web search skill implementation |
| `tauri-app` | Tauri backend commands + app state wiring |
| `tauri-app/src-ui` | React/TypeScript UI (boot sequence, chat, modals, persona toggle) |
| `templates` | Constitutional source docs (soul, ethics, instincts) |
| `documents` | Runbooks, release policy, security notes, environment updates |

For a detailed architecture reference (crate responsibilities, security boundaries, Id/Ego routing model), see [CLAUDE.md](CLAUDE.md).

## Common Commands

```bash
# Full workspace tests
cargo test --all

# Focused core tests
cargo test -p ao-core

# Lint and format
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings

# Build installer locally
./scripts/build-installer.sh                 # macOS/Linux
powershell -File scripts/build-installer.ps1 # Windows
```

## Troubleshooting

**App does not start on macOS**: The app is not notarized. Right-click the app and select "Open" on first launch to bypass Gatekeeper.

**Missing Linux libraries**: Install the required WebKit and GTK dependencies:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-0 libayatana-appindicator3-1
```

**Local LLM not detected**: Ensure your LLM server is running and accessible at the configured URL. AO validates that the URL points to localhost/loopback only (SSRF protection).

**Birth sequence stuck**: If the birth flow hangs, check the developer console (F12) for errors. Ensure you have network connectivity if using a cloud provider.

**Build failures**: Run `cargo clean` and rebuild. Ensure you have the latest Rust stable toolchain (`rustup update stable`).

## Documentation

- [How to Run Locally](documents/HOW_TO_RUN_LOCALLY.md)
- [Security Notes](documents/SECURITY_NOTES.md)
- [Release Process](documents/RELEASE.md)
- [Environment Updates](documents/ENVIRONMENT_UPDATES.md)
- [MVP Scope](documents/MVP_SCOPE.md)
- [GitHub Settings Checklist](documents/GITHUB_SETTINGS.md)

## Contributing

We welcome contributions. Please read our [Contributing Guide](CONTRIBUTING.md) before submitting a pull request.

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).

For security vulnerabilities, see our [Security Policy](.github/SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
