# GUI/Entity Code Review Report (Code-First)

**Date:** 2026-03-01  
**Method:** Implementation review directly from source code (not documentation-first)

---

## Scope Reviewed

- `tauri-app/src` (command handlers, state wiring, startup graph)
- `tauri-app/src-ui/src` (GUI command usage and runtime chat surfaces)
- `crates/entity-chat` (shared chat/tool-use engine)
- `crates/entity-daemon` and `crates/hive-daemon` (HTTP runtime/control-plane)
- `crates/abigail-router` (routing, agentic engine, orchestration, subagents)
- `crates/abigail-skills` (tool execution and policy checks)

---

## What the Application Does (Verified in Code)

Abigail is a local-first sovereign-entity platform with two operating topologies:

1. **Desktop all-in-one runtime (Tauri)**  
   The GUI invokes Rust commands that run routing, chat, memory, skill execution, identity, and config operations in-process.

2. **Split daemon runtime (Hive + Entity)**  
   - `hive-daemon`: control-plane for identity and provider config.
   - `entity-daemon`: chat/tool/memory runtime per entity, with HTTP + SSE endpoints.

Core behaviors confirmed in code:

- **Identity lifecycle** (`tauri-app/src/commands/identity.rs`, `crates/abigail-identity`)
- **Chat and tool-use loop** (`tauri-app/src/commands/chat.rs`, `crates/entity-chat/src/lib.rs`)
- **Tier/routing execution traces** (`crates/entity-core/src/lib.rs`, `crates/abigail-router`)
- **Skill registry/execution** (`tauri-app/src/lib.rs`, `crates/abigail-skills`)
- **Memory persistence** (`abigail_memory::MemoryStore` usage in Tauri and entity-daemon)

---

## Architecture (As Implemented)

### Runtime boundaries

- **GUI/desktop:** `tauri-app`
- **Shared chat engine:** `crates/entity-chat` (used by Tauri and entity-daemon)
- **Routing and agentic/orchestration primitives:** `crates/abigail-router`
- **Skills and tool execution:** `crates/abigail-skills`
- **Control plane:** `crates/hive-daemon`
- **Entity runtime plane:** `crates/entity-daemon`
- **Contracts:** `crates/entity-core`, `crates/hive-core`

### Desktop startup model

At boot, Tauri creates a large shared `AppState` containing:

- router (`IdEgoRouter`)
- memory store
- skill registry/executor + event bus
- identity manager and auth manager
- subagent manager
- config and secrets

This is wired in `tauri-app/src/lib.rs` and held in `tauri-app/src/state.rs`.

### Daemon model

- `hive-daemon` exposes identity/provider/secret APIs.
- `entity-daemon` fetches provider config from hive, builds router, registers skills, and exposes `/v1/chat`, `/v1/chat/stream`, skill and memory APIs.

---

## How Chat Works End-to-End

### Desktop GUI chat path

1. UI invokes `chat_stream` and subscribes to `chat-token`, `chat-done`, `chat-error` events (`ChatInterface.tsx`).
2. Tauri command `chat_stream` (`tauri-app/src/commands/chat.rs`) builds prompt/messages/tools.
3. Shared pipeline runs via `entity_chat::stream_chat_pipeline(...)`.
4. Router executes model calls; tool-use loop executes skill tools through `SkillExecutor`.
5. User and assistant turns are persisted to memory.
6. Token/done/error events are emitted back to UI.

### Entity daemon chat path

`POST /v1/chat` and `POST /v1/chat/stream` in `crates/entity-daemon/src/routes.rs` use the same shared `entity-chat` engine and persist memory similarly.

---

## Code-Observed Stability Gaps

### 1. Command-surface drift between frontend and Tauri handlers

From source comparison of frontend `invoke("...")` calls vs `generate_handler![]`, production UI code currently references commands not registered in Tauri:

- `archive_identity`
- `backup_sqlite`
- `check_existing_identity`
- `create_orchestration_job`
- `delete_orchestration_job`
- `enable_orchestration_job`
- `get_mcp_app_content`
- `list_orchestration_jobs`
- `refresh_provider_catalog`
- `run_orchestration_job_now`
- `set_tier_models`
- `skip_to_life_for_mvp`
- `wipe_identity`

Impact: runtime command-not-found failures on exposed or reachable GUI paths.

### 2. Agentic command surface is present but intentionally stubbed

`tauri-app/src/commands/agentic.rs` returns "not wired to AgenticEngine yet" for lifecycle commands (`start_agentic_run`, status, mentor response, confirmation, cancel, list).

Impact: staff/agentic UI paths are visible but backend is non-functional.

### 3. Orchestration scheduler exists in crate, not wired to Tauri

`crates/abigail-router/src/orchestration.rs` contains scheduler/job logic, but Tauri command handlers for orchestration are missing.

Impact: jobs UI invokes unavailable commands.

### 4. Subagent framework exists without runtime registration path

`SubagentManager` is initialized, but no runtime registration source is wired for default definitions.

Impact: `list_subagents` is empty by default; delegation depends on external/manual registration that is not currently bootstrapped.

### 5. `target` semantics are inconsistent in chat contracts

- Request contracts still carry `target`.
- `entity-chat` streaming pipeline currently marks target as reserved and does not enforce it.
- Tauri and daemon commands accept `target`, but behavior is effectively router-driven.

Impact: API contract ambiguity between request intent and effective execution.

### 6. Security policy configuration exists with incomplete runtime enforcement

- `mcp_trust_policy` exists in config but is not enforced in MCP command path.
- Signed allowlist entries are stored as metadata, but signature verification is not enforced before runtime use.
- Chat tool-use path executes through `SkillExecutor` directly; command-level allowlist checks do not protect this path.

Impact: policy posture is partially declarative rather than end-to-end enforced.

### 7. Message abstraction crates are not integrated into desktop path

`abigail-streaming` and `abigail-queue` exist as reusable boundaries, but desktop runtime chat remains direct command/event coupling.

Impact: harder transport abstraction and lifecycle durability for entity-initiated runs.

---

## Improvement Opportunities (Priority)

1. **Immediate (stability):**
- Add CI gate for command contract parity.
- Remove or feature-gate exposed GUI paths that call missing handlers.
- Register or remove legacy identity commands currently referenced by UI.

2. **Near-term (architecture):**
- Introduce frontend `ChatGateway` interface and adapters (Tauri + entity-daemon).
- Normalize chat envelope and trace/session behavior across adapters.
- Make Tauri handlers thin adapters over an internal coordinator boundary.

3. **Agent lifecycle completion:**
- Wire Tauri agentic commands to `AgenticEngine`.
- Add persisted run state + restart recovery.
- Add entity-initiated run entrypoint and GUI event bridge.

4. **Policy hardening:**
- Enforce MCP trust policy at host/tool resolution.
- Verify signed allowlist cryptographically before skill activation.
- Ensure policy enforcement applies uniformly to direct command execution and chat tool-use loop.

---

## Program Artifacts

- Stability execution roadmap: `documents/GUI_ENTITY_STABILITY_ROADMAP.md`
- Stability suite plan: `documents/tests/MESSAGE_FLOW_STABILITY_TEST_PLAN.md`
- Program index: `documents/tests/TEST_PROGRAM_INDEX.md`

