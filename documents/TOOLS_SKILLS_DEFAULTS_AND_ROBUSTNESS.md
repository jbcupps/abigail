# Tools and Skills: Current Defaults and Robustness

Summary of what the codebase uses today and where hardening would help.

---

## 1. Default skills loaded

| Source | IDs / location | Where |
|--------|----------------|-------|
| **Built-in** | `builtin.hive_management`, `builtin.skill_factory` | Tauri app + entity-daemon |
| **Preloaded (embedded)** | `dynamic.github_api`, `dynamic.slack`, `dynamic.jira` | From `abigail-skills::build_preloaded_skills()`; Tauri bootstraps into config, entity-daemon registers at startup |
| **Discovered dynamic** | `{data_dir}/skills/*.json` (IDs must start with `dynamic.` or `custom.`) | Tauri: `data_dir/skills`; entity-daemon: `entity_dir/skills` |
| **MCP servers** | From config; exposed as skills via HTTP transport | Tauri app only (entity-daemon has no MCP wiring yet) |

- **SkillRegistry**: `SkillRegistry::new()` (no secrets) or `SkillRegistry::with_secrets(…)` (Tauri). Registration is insert-only; re-registering the same ID overwrites.
- Failed preloaded/discovered registrations are logged and skipped; no aggregate health or “required skills” check.

---

## 2. Tool definitions (LLM-facing)

- **Source**: `entity_chat::build_tool_definitions(registry)` walks all registered skills and collects each skill’s `tools()`.
- **Qualified name**: `{skill_id}::{tool_name}` (e.g. `builtin.skill_factory::author_skill`).
- **Validation**: Only check is `parameters.type == "object"` (OpenAI compatibility). Tools without that are **skipped with a warning**; no schema validation, no `required`/`properties` checks.
- **Defaults**: None at the engine level. Each skill fills `ToolDescriptor` (name, description, parameters, returns, cost_estimate, required_permissions, autonomous, requires_confirmation). Dynamic skills use fixed cost/permissions in code; native skills set them per tool.

**Robustness gaps**: Malformed or underspecified `parameters` only cause a skip in `build_tool_definitions`; the LLM can still receive an incomplete tool list. No central validation or normalization of tool schemas.

---

## 3. SkillExecutor and resource limits

- **Construction**: `SkillExecutor::new(registry)` uses `ResourceLimits::default()`.
- **ResourceLimits (default)**:
  - `max_cpu_ms`: **30_000** (30 s per tool call)
  - `max_concurrency`: **10** (global across all skills)
  - `max_memory_bytes`: **256** MB (documented for sandbox/capability layers; not enforced in executor)
  - `storage_quota`: **100** MB (documented; not enforced in executor)
  - `network_bandwidth`: **None**
- **Timeout**: Applied in the executor with `tokio::time::timeout(max_cpu_ms, skill.execute_tool(…))`. On timeout, returns `SkillError::ToolFailed("Tool X exceeded timeout (30000 ms)")`.
- **Sandbox**: Each execution builds a `SkillSandbox` with `ResourceLimits::default()` again (limits are not taken from the executor instance). Permission check is against the tool’s `required_permissions`; capability envelope (Superego L2) is applied before execution.

**Robustness gaps**: Sandbox and executor can get out of sync if executor is built with `with_limits(…)` (sandbox still uses default). Memory/storage limits are not enforced in the current executor path.

---

## 4. Tool-use loop (entity-chat)

- **Max rounds**: `MAX_TOOL_ROUNDS = 8`; after that, loop exits with a fixed message and whatever tool results were collected.
- **Per call**:
  - Qualified name parsing: `split_qualified_tool_name`; if invalid, returns JSON `{ "error": "Invalid tool name format: …" }` and a failed `ToolCallRecord`; execution continues for other calls.
  - Arguments: If `tc.arguments` is not a JSON object, **fallback to empty `ToolParams`** (no crash, no retry).
  - Execution: `executor.execute(skill_id, tool_name, params)`. Errors are turned into JSON `{ "error": "…" }` and a failed record; loop does not retry.
- **No retries**: A failed or timed-out tool call is reported once to the LLM; there is no automatic retry or backoff.

**Robustness gaps**: Malformed args are silently defaulted to empty; no schema validation of args against the tool’s `parameters`. No retry policy or idempotency handling.

---

## 5. Dynamic skills (config and sandbox)

- **ID**: Must start with `dynamic.` or `custom.`; alphanumeric, `.`, `_` only.
- **Tools**: 1–10 tools per skill; duplicate tool names rejected at load.
- **Permissions**: In code, `from_config` sets `Network(Full)` for all dynamic skills. No per-tool or per-URL permission in the manifest.
- **SSRF**: Blocked hosts (e.g. metadata endpoints), private IP checks, no file-URL by default (see `dynamic.rs`).
- **Validation**: `validate_config` at load time; invalid configs fail registration.

**Robustness gaps**: No rate limiting or per-domain caps; all dynamic skills get full network. Secrets resolved at call time; no “required secrets present” check before registering.

---

## 6. Tool execution path (executor)

- **Lookup**: `registry.get_skill(skill_id)` then `skill.tools().find(|t| t.name == tool_name)`.
- **Order of checks**: Capability envelope (L2 + mentor_confirmed) → sandbox permission (audit actions from `required_permissions`) → concurrency permit → timeout-wrapped `execute_tool`.
- **Output**: `ToolOutput { success, data, error, metadata }`; latency stored in metadata by executor.
- **Errors**: `SkillError` (e.g. ToolFailed, PermissionDenied, InitFailed) propagated to caller; entity-chat maps to JSON and continues the loop.

**Robustness gaps**: No per-skill or per-tool overrides for timeout/concurrency. No circuit breaker or failure counting. “Unknown tool” after lookup can happen if the skill’s `tools()` list and actual execution are out of sync (e.g. dynamic skill changed).

---

## 7. Summary table

| Area | Current default / behavior | Possible robustness improvements |
|------|----------------------------|-----------------------------------|
| **Skills loaded** | Built-in + preloaded + discovered JSON; failed reg = skip + log | Health check, required-skills list, version checks |
| **Tool schema** | Only `type: object`; malformed → skip | Validate/fix schema; optional `required`/`properties`; central normalization |
| **Tool args** | Non-object → empty params | Validate against tool schema; reject or coerce with clear error |
| **Timeouts** | 30 s global default; sandbox uses default too | Per-tool/per-skill overrides; align sandbox limits with executor |
| **Concurrency** | 10 global | Per-skill or per-tool caps; backpressure |
| **Retries** | None | Optional retry with backoff and idempotency |
| **Tool-use rounds** | 8 max | Configurable; optional “escalate to user” message |
| **Dynamic permissions** | All get Network(Full) | Per-tool or per-URL allowlists; rate/domain limits |
| **Memory/storage** | Documented limits only | Enforce in executor or capability layer |
| **Errors to LLM** | JSON `{ "error": "…" }` | Structured codes; optional retry hints |

---

## 8. Key files

- **Registration / discovery**: `tauri-app/src/lib.rs` (setup), `entity-daemon/src/main.rs` (startup).
- **Tool definitions**: `entity-chat/src/lib.rs` (`build_tool_definitions`, `run_tool_use_loop`, `execute_single_tool_call`).
- **Execution and limits**: `abigail-skills/src/executor.rs`, `abigail-skills/src/sandbox.rs` (`ResourceLimits`, `SkillSandbox`).
- **Tool descriptor and trait**: `abigail-skills/src/skill.rs` (`ToolDescriptor`, `Skill`).
- **Dynamic skills**: `abigail-skills/src/dynamic.rs` (validation, SSRF, permissions).
- **Preloaded**: `abigail-skills/src/preloaded.rs`.

Invariants and sequencing are documented in `documents/LLM_ROUTING_SKILLS_INVARIANTS.md`.
