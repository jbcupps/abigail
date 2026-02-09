# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Abigail is a desktop agent application built with Tauri 2.0 (Rust backend) and React (TypeScript frontend). It implements a first-run "birth" sequence, Ed25519 signature verification of constitutional documents, Id/Ego LLM routing, and an extensible skill system.

## Build & Run Commands

```bash
# Initial setup
cargo build                              # Build all Rust crates
cd tauri-app/src-ui && npm install       # Install frontend deps (one-time)

# Development (from repo root)
cargo tauri dev                          # Launches app with hot-reload at localhost:1420

# Tests
cargo test --all                         # Run all tests
cargo test -p abigail-core                    # Signature verification, config
cargo test -p abigail-memory                  # SQLite schema, memory CRUD
cargo test -p abigail-router                  # Routing decisions
cargo test -p abigail-skills                  # Requires ABIGAIL_IMAP_TEST=1 + credentials

# Single test
cargo test -p abigail-core verify             # Run tests matching "verify" in abigail-core

# Linting
cargo clippy
cargo fmt --check

# Frontend only
cd tauri-app/src-ui && npm run build     # tsc + vite build

# Build installer (local)
cd tauri-app && cargo tauri build        # Output: target/release/bundle/nsis/
```

## Environment Variables

- `OPENAI_API_KEY` — enables Ego (cloud) routing for complex queries
- `LOCAL_LLM_BASE_URL` — local LLM server (LM Studio: `http://localhost:1234`, Ollama: `http://localhost:11434`)
- `RUST_LOG=abigail_router=debug,abigail_core=info` — per-crate log levels
- See `example.env` for full list

## Architecture

### Rust Workspace (crates/)

The crates form a layered architecture with clear security boundaries:

| Crate | Role |
|-------|------|
| `abigail-core` | Foundation: AppConfig, Ed25519 crypto, keyring, vault, DPAPI secrets, document verification |
| `abigail-memory` | SQLite persistence with MemoryWeight tiers (Ephemeral/Distilled/Crystallized) |
| `abigail-capabilities` | **High-trust** functions: cognitive (LLM providers), sensory, memory, agent control |
| `abigail-router` | Id/Ego routing — classifies messages as Routine (local LLM) or Complex (cloud), delegates accordingly |
| `abigail-birth` | First-run orchestrator: staged sequence (init → keypair → sign → verify → heartbeat → discover) |
| `abigail-skills` | **Lower-trust** plugin system: manifest-based skills with sandbox, registry, executor, event bus |
| `abigail-keygen` | Standalone egui utility for Ed25519 keypair generation |

**Security boundary**: Capabilities have vault access and run trusted code. Skills are sandboxed plugins that declare permissions in `skill.toml` manifests.

### Tauri App (tauri-app/)

- `src/lib.rs` — All `#[tauri::command]` handlers; manages `AppState` with `RwLock<AppConfig>`, `RwLock<IdEgoRouter>`, `Arc<SkillRegistry>`, `Arc<SkillExecutor>`, `Arc<EventBus>`
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
- Router is rebuilt via `set_api_key`/`set_local_llm_url` commands when config changes

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

- Tag `v0.0.x` on main triggers `.github/workflows/build-release.yml` (builds Windows NSIS, Linux deb, macOS dmg)
- Tag `deva-v0.x.x` on Deva branch triggers `build-release-deva.yml` (pre-release)
- Manual dispatch also available via GitHub Actions UI

## Data Directory

- Windows: `%LOCALAPPDATA%\abigail\Abigail\`
- macOS: `~/Library/Application Support/abigail/Abigail/`
- Linux: `~/.local/share/abigail/Abigail/`

Contains: `config.json`, `keys.bin` (DPAPI), `secrets.bin` (DPAPI), `external_pubkey.bin`, `abigail_seed.db` (SQLite), `docs/` (signed constitutional docs)

## Known Issues

### Ego Router
The Ego (cloud LLM) router has been refactored to streaming-first but may still have edge cases.
Debug with `RUST_LOG=abigail_router=debug`. Key diagnostics:
- `get_router_status` Tauri command returns current Id/Ego/Superego state
- If Ego shows as unconfigured after birth, check TrinityConfig in config.json
- CandleProvider stub returns a helpful message instead of erroring when no local LLM exists
- The `chat_stream` Ego path now streams from the start instead of doing a non-streaming initial request
