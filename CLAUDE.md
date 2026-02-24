# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Abigail is a Sovereign Entity platform built with Tauri 2.0 (Rust backend) and React (TypeScript frontend). The architecture follows a **Hive/Entity separation**: the **Hive** is the household-level control plane (secrets, identity, provider resolution) and each **Entity** is a personal agent runtime (routing, skills, memory). Both run as independent HTTP daemons composed from shared `abigail-*` crates, with the Tauri desktop app wrapping them for end users.

## Windows: GNU vs MSVC toolchain

On Windows, Rust can use two toolchains:

- **MSVC** (`x86_64-pc-windows-msvc`) — uses Microsoft’s linker and C runtime (`msvcrt.lib`). Requires Visual Studio Build Tools and a proper `LIB`/`INCLUDE` setup (e.g. “x64 Native Tools” prompt or a full “Desktop development with C++” install).
- **GNU** (`x86_64-pc-windows-gnu`) — uses MinGW/GCC. No Visual Studio needed; only MinGW-w64 on `PATH` (e.g. from [msys2](https://www.msys2.org/): `pacman -S mingw-w64-ucrt-x86_64-gcc`).

This repo sets `rust-toolchain.toml` to **GNU** so local Windows builds work without VS. Ensure MinGW is on your `PATH` (e.g. `C:\msys64\mingw64\bin` or `C:\msys64\ucrt64\bin`). CI still uses MSVC on Windows so GitHub’s runner (which has VS) is unchanged.

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
| `entity-cli` | CLI client for entity-daemon (chat, skills, tool execution, memory, scaffolding) |

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

### Tier-Based Model Routing

The router (`abigail-router`) implements a **tier-based dual-LLM pattern** with three quality tiers:
- **Fast** — cheapest, fastest models (e.g. `gpt-4.1-mini`, `claude-haiku-4-5`)
- **Standard** — balanced quality/speed (e.g. `gpt-4.1`, `claude-sonnet-4-6`)
- **Pro** — highest quality (e.g. `gpt-5.2`, `claude-opus-4-6`)

**Core components:**
- **Id** = local LLM (LocalHttpProvider for OpenAI-compatible servers, or CandleProvider stub) — **failsafe only**
- **Ego** = cloud LLM (provider selected via `active_provider_preference`)
- `CompletionRequest.model_override` — per-request model selection (single provider instance, model chosen at routing time)

**Routing modes:**
- `RoutingMode::TierBased` — classifies message complexity (score 5–95), maps to tier via `TierThresholds` (default: <35 → Fast, 35–69 → Standard, ≥70 → Pro)
- `RoutingMode::EgoPrimary` — all queries to cloud (uses Standard tier model)
- `RoutingMode::Council` — multi-provider consensus

**Force override (3 levels, highest priority first):**
1. `ForceOverride.pinned_model` — exact model ID, bypasses all tier logic
2. `ForceOverride.pinned_tier` (+ optional `pinned_provider`) — forces a specific tier
3. Normal complexity-based selection

**Config types** (`abigail-core`): `ModelTier`, `TierModels`, `TierThresholds`, `ForceOverride`

**Dynamic model registry** (`abigail-hive::ModelRegistry`):
- Discovers available models from provider APIs (OpenAI, Anthropic, Google, xAI)
- Per-provider caching with 24h TTL
- Persisted to `provider_catalog` in config.json
- Validates tier model assignments against discovered models

**Wiring:**
- In Tauri: router is rebuilt via `set_api_key`/`set_local_llm_url`/`set_force_override`/`set_tier_thresholds` commands when config changes
- In entity-daemon: router is built once at startup from `ProviderConfig` fetched from Hive
- `ChatResponse` includes `tier`, `model_used`, and `complexity_score` metadata

### Constitutional Documents

Templates in `templates/` (soul.md, ethics.md, instincts.md) are compiled into the binary. At first run they're written to the data directory, signed with a generated Ed25519 key, and verified at every subsequent boot.

### Skills System

Skills are the primary way the Entity gains capabilities beyond conversation. The system has three implementation strategies with a unified permission/sandbox model.

**Core components** (`abigail-skills`):
- `Skill` trait — contract all skills implement: `manifest()`, `tools()`, `execute_tool()`, `initialize()`, `shutdown()`
- `SkillManifest` — parsed from `skill.toml`: id, permissions, secrets, runtime config
- `SkillRegistry` — thread-safe registry with discovery (scans dirs for `*/skill.toml`), registration, secret validation
- `SkillExecutor` — execution engine with concurrency semaphore, per-tool timeouts, sandbox permission checks, capability envelope (SuperegoL2Mode)
- `SkillSandbox` — validates audit actions (network, file read/write, shell) against declared permissions
- `EventBus` — broadcast channel for inter-skill communication, relayed to UI via Tauri events

**Implementation strategies:**
1. **Native Rust** — implement `Skill` trait directly, compiled into binary. Example: `HiveManagementSkill`
2. **Dynamic API** — JSON config defining REST tools with URL/header/body templates, secret injection, response extractors. SSRF-protected. Example: preloaded GitHub/Slack/Jira integrations
3. **MCP (Model Context Protocol)** — bridge to external MCP servers via HTTP transport (`/tools/list`, `/tools/call`)

**Skill manifest format** (`skill.toml`):
```toml
[skill]
id = "com.abigail.skills.example"
name = "Example"
version = "0.1.0"
description = "What this skill does"
category = "Productivity"
keywords = ["example", "demo"]

[[permissions]]
permission = { Network = { Domains = ["api.example.com"] } }
reason = "Call Example API"

[[secrets]]
name = "api_key"
description = "API key for Example service"
required = true
```

**Permission types:** `Network` (Full/LocalOnly/Domains), `FileSystem` (Full/Read/Write with paths), `Memory` (ReadOnly/ReadWrite/Namespace), `ShellExecute`, `Notifications`, `Clipboard`, `Microphone`, `Camera`, `ScreenCapture`, `SkillInteraction`

**Trust model (layered):**
1. Registry-level: manifests loaded from disk
2. Permission-level: sandbox enforces declared permissions
3. Capability-level: SuperegoL2Mode gates high-risk actions
4. Approval-level: `approved_skill_ids` + `signed_skill_allowlist` in Tauri app
5. Execution-level: timeout + concurrency limits

**On-disk layout:** `skills/` directory contains 18+ skill subdirectories, `registry.toml` (maps skill IDs to LLM instruction files + keywords), and `instructions/` (markdown files injected into system prompt when keywords match)

**Current wiring:**
- Entity-daemon: `SkillRegistry` + `SkillExecutor` created at startup, `HiveManagementSkill` auto-registered, routes at `GET /v1/skills` and `POST /v1/tools/execute`
- Chat pipeline: `build_tool_awareness_section()` generates markdown listing all registered tools for the LLM system prompt
- Tauri app: full command surface (list, discover, execute, approve, MCP integration)

**Key gap:** No LLM tool-use loop yet — the LLM sees tools in the system prompt but there's no code to parse tool calls from LLM output, execute them via `SkillExecutor`, and feed results back into the conversation.

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

**Entity (`:3142`):** `GET /health`, `GET /v1/status`, `POST /v1/chat`, `GET /v1/skills`, `POST /v1/tools/execute`, `GET /v1/memory/stats`, `POST /v1/memory/search`, `GET /v1/memory/recent?limit=N`, `POST /v1/memory/insert`

## Development Roadmap

Current priorities, in order:

### Phase 2a: Skills Use (P1)
1. **LLM tool-use loop** — Parse tool-call blocks from LLM responses (OpenAI function-calling format), execute via `SkillExecutor`, inject results back, re-prompt until the LLM produces a final text response. Implement in `entity-daemon/src/chat_pipeline.rs`.
2. **Auto-load skills from disk** — On entity-daemon startup, scan `skills/` for `*/skill.toml`, register discovered skills in the `SkillRegistry` alongside the built-in `HiveManagementSkill`.
3. **Wire tools into LLM requests** — Convert registered `ToolDescriptor`s into the `tools` array for the OpenAI-compatible chat completion request so the LLM can invoke them natively.

### Phase 2b: Skill Creation (P2)
4. **Scaffolding CLI** — `entity-cli new-skill <name>` generates a skill directory with `skill.toml` template, boilerplate Rust or JSON config, and an instruction markdown file.
5. **Dynamic API skill authoring docs** — Document the JSON config format with examples for common patterns (REST CRUD, OAuth, webhook).
6. **Skill hot-reload** — File-watcher on `skills/` directory to re-discover and register new/updated skills without daemon restart.

### Phase 2c: Memory & Integration (P3)
7. **Memory persistence** — Wire `abigail-memory` SQLite into entity-daemon for conversation history, recall tool, and memory weight tiers.
8. **End-to-end daemon testing** — Automated integration tests: hive-daemon + entity-daemon + skill execution + memory round-trip.
9. **Tauri app → daemon delegation** — Desktop app calls daemons via HTTP instead of running everything in-process.

## Known Issues

### Tier-Based Router
The router uses tier-based routing with complexity classification. Debug with `RUST_LOG=abigail_router=debug`. Key diagnostics:
- `get_router_status` Tauri command returns current Id/Ego/Superego state
- If Ego shows as unconfigured after birth, check TrinityConfig in config.json
- CandleProvider stub returns a helpful message instead of erroring when no local LLM exists
- The `chat_stream` Ego path streams from the start
- `ChatResponse` now includes `tier`, `model_used`, and `complexity_score` — check these to verify tier routing is working
- Force overrides (`ForceOverride`) bypass complexity scoring — if routing seems stuck on one tier, check `config.json` `force_override`
- Model registry discovery runs in background at startup — check logs for `ModelRegistry:` messages
