# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Abigail is a Sovereign Entity platform built with Tauri 2.0 (Rust backend) and React (TypeScript frontend). The architecture follows a **Hive/Entity separation**: the **Hive** is the household-level control plane (secrets, identity, provider resolution) and each **Entity** is a personal agent runtime (routing, skills, memory). Both run as independent HTTP daemons composed from shared `abigail-*` crates, with the Tauri desktop app wrapping them for end users.

## Build & Run Commands

```bash
# Initial setup
cargo build                              # Build all Rust crates
cd tauri-app/src-ui && npm install       # Install frontend deps (one-time)

# Development — Tauri desktop app (from repo root)
cargo tauri dev                          # Launches app with hot-reload at localhost:1420

# Development — Hive/Entity daemons (headless)
cargo run -p hive-daemon                             # Control plane on :3141
cargo run -p entity-daemon -- --entity-id <uuid>     # Agent runtime on :3142
cargo run -p hive-cli -- status                      # CLI: query Hive
cargo run -p entity-cli -- chat "hello"              # CLI: chat with Entity

# Tests
cargo test --workspace --exclude abigail-app         # All tests (CI-equivalent)
cargo test -p abigail-core                           # Signature verification, config
cargo test -p abigail-identity                       # Identity manager
cargo test -p abigail-memory                         # SQLite schema, memory CRUD
cargo test -p abigail-router                         # Routing decisions
cargo test -p abigail-skills                         # Requires ABIGAIL_IMAP_TEST=1 + credentials

# Single test
cargo test -p abigail-core verify             # Run tests matching "verify" in abigail-core

# Linting
cargo clippy --workspace --exclude abigail-app -- -D warnings
cargo fmt --all -- --check

# Frontend only
cd tauri-app/src-ui && npm run build     # tsc + vite build

# Build installer (local)
cd tauri-app && cargo tauri build        # Output: target/release/bundle/nsis/
```

## Pre-Push Checklist

**IMPORTANT: Always run these locally before pushing to avoid wasting CI minutes.** The CI gate job requires lint, test, and frontend to all pass. Run these exact commands to match what CI runs:

```bash
# 1. Format check (auto-fix with cargo fmt --all)
cargo fmt --all -- --check

# 2. Clippy (must pass with -D warnings; excludes abigail-app which needs Tauri)
cargo clippy --workspace --exclude abigail-app -- -D warnings

# 3. Check abigail-app compiles (excluded from clippy/test but checked separately in CI)
cargo check -p abigail-app

# 4. Tests (excludes abigail-app; runs on all 3 platforms in CI)
cargo test --workspace --exclude abigail-app

# 5. Frontend build (tsc type-check + vite build)
cd tauri-app/src-ui && npm run build

# 6. Frontend tests with coverage (required in CI frontend job)
cd tauri-app/src-ui && npm run test:coverage
```

If any of these fail locally, fix them before pushing. The `gate` CI job will block the PR until all five pass.

## Environment Variables

- `OPENAI_API_KEY` — enables Ego (cloud) routing for complex queries
- `LOCAL_LLM_BASE_URL` — local LLM server (LM Studio: `http://localhost:1234`, Ollama: `http://localhost:11434`)
- `RUST_LOG=abigail_router=debug,abigail_core=info` — per-crate log levels
- See `example.env` for full list

## Architecture

### Hive/Entity Separation

The system is split into two independent daemons:

- **Hive daemon** (`hive-daemon`, port 3141) — Control plane: manages identity, secrets, and provider resolution. Wraps `IdentityManager` + `Hive` + `SecretsVault` behind an Axum REST API.
- **Entity daemon** (`entity-daemon`, port 3142) — Agent runtime: routes messages, executes skills, manages memory. Fetches provider config from Hive on startup, then runs independently.

Entity calls `GET /v1/entities/:id/provider-config` on Hive to get its LLM provider configuration, then builds providers locally via `Hive::build_providers()`.

### Rust Workspace (crates/)

The crates form a layered architecture with clear security boundaries:

**Hive layer (control plane):**

| Crate | Role |
|-------|------|
| `hive-core` | Pure DTO crate: `ApiEnvelope<T>`, `EntityInfo`, `ProviderConfig`, `HiveStatus`, request/response types |
| `abigail-identity` | `IdentityManager` — Ed25519 agent creation, signing, listing (extracted from tauri-app) |
| `abigail-hive` | `Hive` — secret resolution, provider construction, priority chain |
| `hive-daemon` | Axum HTTP server wrapping identity + hive + secrets (port 3141) |
| `hive-cli` | CLI client for hive-daemon (status, entities, secrets) |

**Entity layer (agent runtime):**

| Crate | Role |
|-------|------|
| `entity-core` | Pure DTO crate: `ChatRequest/Response`, `EntityStatus`, `ToolExecRequest/Response` |
| `abigail-router` | Id/Ego routing — classifies messages as Routine (local LLM) or Complex (cloud) |
| `abigail-capabilities` | **High-trust** functions: cognitive (LLM providers), sensory, memory, agent control |
| `abigail-skills` | **Lower-trust** plugin system: manifest-based skills with sandbox, registry, executor, event bus |
| `entity-daemon` | Axum HTTP server wrapping router + skills + executor (port 3142) |
| `entity-cli` | CLI client for entity-daemon (chat, skills, tool execution) |

**Shared foundation:**

| Crate | Role |
|-------|------|
| `abigail-core` | Foundation: AppConfig, Ed25519 crypto, keyring, vault, DPAPI secrets, document verification |
| `abigail-memory` | SQLite persistence with MemoryWeight tiers (Ephemeral/Distilled/Crystallized) |
| `abigail-birth` | First-run orchestrator: staged sequence (init → keypair → sign → verify → heartbeat → discover) |
| `abigail-keygen` | Standalone egui utility for Ed25519 keypair generation |

**Security boundary**: Hive controls secrets and identity (high trust). Entity executes skills in a sandboxed plugin system with declared permissions.

### Tauri App (tauri-app/)

The desktop app wraps the Hive/Entity architecture for end users:

- `src/lib.rs` — All `#[tauri::command]` handlers; manages `AppState` with `RwLock<AppConfig>`, `RwLock<IdEgoRouter>`, `Arc<SkillRegistry>`, `Arc<SkillExecutor>`, `Arc<EventBus>`
- `src/identity_manager.rs` — Re-exports from `abigail-identity` crate
- `src/templates.rs` — Embedded constitutional document text (soul.md, ethics.md, instincts.md)
- `src-ui/` — React frontend (Vite + Tailwind)

### Frontend State Machine (src-ui/src/App.tsx)

```
splash → loading → management → boot → chat
                 → identity_conflict → management
                 → startup_check → chat
                 → startup_failed
```

- `BootSequence.tsx` — First-run UI: intro → init soul → generate keypair → key presentation → verify → complete
- `ChatInterface.tsx` — Main chat: sends messages through `classify()` → `complete()` Tauri commands

### Id/Ego Router

The router (`abigail-router`) implements a dual-LLM pattern:
- **Id** = local LLM (LocalHttpProvider for OpenAI-compatible servers, or CandleProvider stub)
- **Ego** = cloud LLM (OpenAiProvider wrapping Azure/OpenAI API)
- `RoutingMode::IdPrimary` — local first, cloud for complex queries
- `RoutingMode::EgoPrimary` — cloud first
- In Tauri: router is rebuilt via `set_api_key`/`set_local_llm_url` commands when config changes
- In entity-daemon: router is built once at startup from `ProviderConfig` fetched from Hive

### Constitutional Documents

Templates in `templates/` (soul.md, ethics.md, instincts.md) are compiled into the binary. At first run they're written to the data directory, signed with a generated Ed25519 key, and verified at every subsequent boot.

### Skills System

Skills live in `skills/` with a `skill.toml` manifest declaring tools, permissions, and secrets. The `SkillRegistry` discovers and loads skills; `SkillExecutor` runs tool calls; `EventBus` (broadcast channel) enables inter-skill communication relayed to the frontend via Tauri events.

## Key Patterns

- **RwLock for shared state** — AppState fields use `RwLock`, not `Mutex`. Drop locks before `await` boundaries (RwLock is not Send).
- **Trait-based providers** — `LlmProvider`, `Skill`, `Capability`, `ExternalVault` traits enable swappable implementations.
- **Idempotent init** — `init_soul` and birth stages are safe to call multiple times.
- **DPAPI on Windows** — Keyring and email passwords encrypted via Windows DPAPI (user scope). Other platforms use plaintext stub (dev only).

## Version Management

Version must be updated in two places:
1. Root `Cargo.toml` → `[workspace.package]` → `version`
2. `tauri-app/tauri.conf.json` → `"version"`

All workspace crates inherit via `version.workspace = true`.

## Deploy / PR Process

- **Branch protection**: Direct pushes to `main` are blocked. All changes must go through a pull request.
- **Workflow**: Create a feature branch, push it, then open a PR via `gh pr create`.
- **Typical flow**:
  1. Make changes on `main` (or a feature branch)
  2. Commit with descriptive message
  3. `git stash --include-untracked && git pull --rebase && git stash pop` (sync with remote)
  4. `git checkout -b <branch-name>` (create feature branch)
  5. `git push -u origin <branch-name>` (push branch)
  6. `gh pr create --title "..." --body "..."` (open PR)
- **Branch naming**: `refactor/`, `fix/`, `feat/`, `ci/` prefixes matching commit type.

## Release Process

- Tags matching `v*` trigger `.github/workflows/release.yml` (builds and publishes Windows NSIS/MSI, Linux deb, macOS universal dmg, and npm package).
- `workflow_dispatch` is supported for manual releases and optional version override.
- CI quality gate and advisory scans run via `.github/workflows/ci.yml` (`gate` is the protected check).

## Data Directory

- Windows: `%LOCALAPPDATA%\abigail\Abigail\`
- macOS: `~/Library/Application Support/abigail/Abigail/`
- Linux: `~/.local/share/abigail/Abigail/`

Contains: `config.json`, `keys.bin` (DPAPI), `secrets.bin` (DPAPI), `external_pubkey.bin`, `abigail_seed.db` (SQLite), `docs/` (signed constitutional docs)

## Change Tracking

**Every commit** must include exactly one changelog line appended to the `### Added` section of `CHANGELOG.md` under `## [Unreleased]`. Use this exact format:

```
- YYYY-MM-DD HH:MM EST: [brief one-line description of what changed]
```

Example:
```
- 2026-02-19 21:45 EST: Fix credential storage refusal and add abigail-cli REST troubleshooting API
```

Rules:
- One line per commit, no matter how many files changed. Consolidate into a single summary.
- Always include the date AND time with EST timezone.
- Keep it brief — one sentence describing the "what", not the "how".
- This applies to every commit: code, test, refactor, docs, even one-line tweaks.

## Daemon Development

### Running daemons locally

```bash
# Terminal 1: Start Hive control plane
cargo run -p hive-daemon -- --port 3141

# Terminal 2: Start Entity agent runtime (needs a registered entity UUID)
cargo run -p entity-daemon -- --entity-id <uuid> --hive-url http://127.0.0.1:3141 --port 3142

# Terminal 3: Interact via CLI
cargo run -p hive-cli -- --url http://127.0.0.1:3141 status
cargo run -p entity-cli -- --url http://127.0.0.1:3142 chat "hello"
```

### Key daemon endpoints

**Hive (`:3141`):** `GET /health`, `GET /v1/status`, `GET /v1/entities`, `POST /v1/entities`, `GET /v1/entities/:id/provider-config`, `POST /v1/secrets`

**Entity (`:3142`):** `GET /health`, `GET /v1/status`, `POST /v1/chat`, `GET /v1/skills`, `POST /v1/tools/execute`

## Known Issues

### Ego Router
The Ego (cloud LLM) router has been refactored to streaming-first but may still have edge cases.
Debug with `RUST_LOG=abigail_router=debug`. Key diagnostics:
- `get_router_status` Tauri command returns current Id/Ego/Superego state
- If Ego shows as unconfigured after birth, check TrinityConfig in config.json
- CandleProvider stub returns a helpful message instead of erroring when no local LLM exists
- The `chat_stream` Ego path now streams from the start instead of doing a non-streaming initial request
