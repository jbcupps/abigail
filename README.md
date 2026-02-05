# AO

AO is a local-first desktop agent built with [Tauri](https://tauri.app/), Rust, and React. It combines constitutional integrity checks, first-run identity creation, and multi-provider reasoning in a single desktop app.

## What’s current in this branch

- **Interactive birth flow** with staged onboarding (`Darkness → KeyPresentation → Ignition → Connectivity → Genesis → Emergence → Life`).
- **First-run signing key generation** with one-time private-key presentation and automatic constitutional document signing.
- **Local LLM discovery + manual connect** for Ollama/LM Studio-compatible endpoints.
- **In-app API key vaulting + validation** for cloud/model/search providers.
- **Dual persona UI modes** (surface chat + Forge mode toggle).
- **Skill-based tool execution** including web-search capability wiring.

## Architecture at a glance

| Area | Purpose |
|---|---|
| `crates/ao-core` | Config, verifier, key management, secrets, system prompt primitives |
| `crates/ao-birth` | Birth stages/prompts and orchestration logic |
| `crates/ao-memory` | SQLite-backed memory storage |
| `crates/ao-capabilities` | Provider adapters, cognitive/sensory capability modules |
| `crates/ao-router` | Id/Ego routing and provider selection |
| `crates/ao-skills` | Skill registry, executor, protocols, sandbox and events |
| `skills/skill-web-search` | Web search skill implementation |
| `tauri-app` | Tauri backend commands + app state wiring |
| `tauri-app/src-ui` | React/TypeScript UI (boot sequence, chat, modals, persona toggle) |
| `templates` | Constitutional source docs (`soul`, `ethics`, `instincts`) |
| `documents` | Runbooks, release policy, security notes, environment updates |

## Quick start

### Prerequisites

- Rust stable
- Node.js 20+
- OS dependencies required by Tauri

### Development run

```bash
cargo build
cd tauri-app/src-ui && npm install && cd ../..
cargo tauri dev
```

### Optional environment variables

- `OPENAI_API_KEY` — optional cloud provider fallback.
- `LOCAL_LLM_BASE_URL` — optional local endpoint override.
- `EXTERNAL_PUBKEY_PATH` — optional explicit pubkey path (otherwise AO auto-detects generated pubkey in app data).

See [`example.env`](example.env) and [`documents/HOW_TO_RUN_LOCALLY.md`](documents/HOW_TO_RUN_LOCALLY.md) for full details.

## Documentation map

- [How to run locally](documents/HOW_TO_RUN_LOCALLY.md)
- [Security notes](documents/SECURITY_NOTES.md)
- [Release process](documents/RELEASE.md)
- [Environment updates](documents/ENVIRONMENT_UPDATES.md)
- [MVP scope](documents/MVP_SCOPE.md)

## Common commands

```bash
# Full workspace tests
cargo test

# Focused core tests
cargo test -p ao-core

# Build installer locally
./scripts/build-installer.sh                 # macOS/Linux
powershell -File scripts/build-installer.ps1 # Windows
```

## License

MIT. See [`Cargo.toml`](Cargo.toml) under `[workspace.package]`.
