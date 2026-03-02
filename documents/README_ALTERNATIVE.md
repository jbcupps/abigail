# Abigail (Runtime-Accurate README Alternative)

This document is a code-aligned alternative to the root README. It reflects the implementation in the current repository as of **2026-03-01**.

## What Abigail is today

Abigail is a local-first sovereign-entity platform with:

- A desktop runtime (`tauri-app`) for identity onboarding, chat, and operations UI
- A headless split-runtime mode (`hive-daemon` + `entity-daemon`)
- A shared chat engine (`entity-chat`) with tool-use, execution tracing, and memory archiving
- A skill system with built-in Rust skills, dynamic API skills, and optional MCP HTTP skill runtimes

Current release line: **0.0.1** (workspace version).
Target milestone: **working beta by March 31, 2026**.

## Current user flow (desktop app)

The implemented UI flow in `tauri-app/src-ui/src/App.tsx` is:

1. Splash screen
2. Soul Registry (always entered first)
3. Choose one:
   - Create/load an existing soul
   - Resolve legacy identity conflict/migration path
4. Birth path (`BootSequence`) for incomplete identities
5. Startup checks (`run_startup_checks`): provider heartbeat + signature verification
6. Chat mode (`ChatInterface`)
7. Sanctum drawer for operations (Forge, identity settings, diagnostics, agentic/orchestration panels)

### Birth sequence details

`BootSequence` currently includes these staged states:

- Darkness
- KeyPresentation
- Ignition
- Connectivity
- Genesis / GenesisChat / GenesisForge
- Crystallization / SoulPreview
- Emergence
- Life
- Repair

Crystallization currently supports multiple paths (fast template, guided dialog, image archetypes, psych/moral, editable template).

## Runtime architecture

Abigail runs in two supported topologies.

### A) Desktop integrated runtime

`tauri-app` hosts:

- Router and provider resolution
- Skill registry/executor
- Memory store
- Agentic runtime
- Orchestration scheduler state
- UI command surface via Tauri `generate_handler![]`

Chat in this mode is routed through the frontend `ChatGateway` abstraction and currently uses a Tauri transport by default.

### B) Headless split daemons

- `hive-daemon` (default `:3141`) is the control plane
- `entity-daemon` (default `:3142`) is the agent runtime
- `entity-daemon` pulls resolved provider config from `hive-daemon` on startup

Core HTTP APIs:

- Hive: `/v1/status`, `/v1/entities`, `/v1/entities/:id/provider-config`, `/v1/secrets/*`
- Entity: `/v1/status`, `/v1/chat`, `/v1/chat/stream`, `/v1/skills`, `/v1/tools/execute`, `/v1/memory/*`

## Chat architecture (as implemented)

### Frontend transport boundary

`ChatInterface` talks only to `ChatGateway` (`src-ui/src/chat/*`), with implementations for:

- `TauriChatGateway` (active default)
- `EntityHttpChatGateway` (supported adapter for daemon/SSE transport)

### Internal desktop chat boundary

Tauri commands (`chat`, `chat_stream`) are thin adapters over `ChatCoordinator`.
Streaming uses internal envelopes (`chat-internal-envelope`) with:

- `request`
- `metadata`
- `token`
- `done`
- `error`

### Important contract note

`target` input is currently **deprecated/ignored** in desktop coordinator logic and normalized to `AUTO` (`target_policy: deprecated_ignored`).

## Skills and tool execution model

At startup, desktop runtime registers:

- Built-in operational skills (`builtin.hive_management`, `builtin.skill_factory`)
- Preloaded dynamic skills
- Dynamic API skills discovered from data directory
- Native compiled skills (filesystem, shell, git, code analysis, database, calendar, document, image, web search, perplexity search, clipboard, system monitor, notification, HTTP, knowledge base)
- Proton Mail skill (registered always; initializes transport when credentials exist)
- MCP HTTP servers as runtime skills, with trust-policy URL checks before activation

Tool calls are executed through `SkillExecutor` and reported in chat/tool-use traces.

## Agentic and orchestration surfaces

Desktop command surface includes:

- Agentic lifecycle (`start_agentic_run`, status/list/cancel, mentor response, confirmation)
- Entity-initiated agentic entry point (`start_entity_initiated_agentic_run`)
- Orchestration jobs (list/enable/delete/run-now/logs)

Sanctum "Staff" and "Jobs" tabs are shown only when backend is healthy, unless experimental UI is forced via runtime flags.

## Data and security posture (current)

- Identity and provider/skill secrets are persisted in encrypted vault files
- Chat turns are archived in `MemoryStore` with provider/model/tier metadata when available
- Constitutional signature checks are part of startup checks
- Birth key handling keeps private key transient in UI; recovery flow exists for broken identity signatures

## Developer quick start

### Desktop

```bash
cargo build
cd tauri-app/src-ui && npm install && cd ../..
cargo tauri dev
```

### Headless daemons

```bash
# Terminal 1
cargo run -p hive-daemon

# Terminal 2
cargo run -p hive-cli -- create "MyEntity"
cargo run -p entity-daemon -- --entity-id <uuid>

# Terminal 3
cargo run -p entity-cli -- status
cargo run -p entity-cli -- chat "hello"
```

## Validation commands

```bash
# Rust workspace tests (excluding tauri desktop crate)
cargo test --workspace --exclude abigail-app

# Frontend tests
cd tauri-app/src-ui
npm run test
npm run check:command-contract
```

## Known implementation realities

- Desktop chat uses the new gateway/coordinator boundaries; direct UI command coupling is no longer the intended path.
- Orchestration supports persisted jobs and run-now execution; continuous scheduler behavior should be treated as an active hardening area unless explicitly validated for your build.
- Some governance-related command endpoints exist but remain limited compared to chat/agentic core paths.

