# Message Flow Stability Test Plan

## Scope

This suite validates the GUI/Entity stabilization program:

- GUI chat transport abstraction and parity
- Frontend/backend command-surface consistency
- Agent lifecycle reliability (including restart recovery)
- Entity-initiated agent execution and subagent delegation
- Runtime trust/policy enforcement in message flow paths

Reference roadmap:
- `documents/GUI_ENTITY_STABILITY_ROADMAP.md`

---

## Coverage Matrix

| Area | Goal | Test IDs |
|---|---|---|
| Command surface parity | No frontend call to unregistered Tauri command | `STAB-001`, `STAB-002` |
| Chat transport parity | Tauri and Entity adapters produce equivalent behavior | `STAB-010`..`STAB-014` |
| Streaming lifecycle | Deterministic token/done/error sequence | `STAB-020`..`STAB-022` |
| Agent lifecycle | Start/ask/confirm/cancel/complete + recovery | `STAB-030`..`STAB-036` |
| Entity-initiated runs | Entity can enqueue/start runs without GUI action | `STAB-040`..`STAB-042` |
| Subagent path | Registered subagents are discoverable and delegate correctly | `STAB-050`..`STAB-053` |
| Policy enforcement | MCP trust + signed allowlist checks enforced | `STAB-060`..`STAB-064` |

---

## Test Cases

### STAB-001

- Test ID: `STAB-001`
- Title: Command contract check passes on baseline
- Priority: `P0`
- Type: `AutomatedHarness`
- Preconditions:
  1. CI command-contract check script is present.
- Steps:
  1. Run command-contract check against current branch.
- Expected:
  1. Script exits `0`.
  2. No unregistered commands are reported.
- Evidence:
  1. CI logs and script output artifact.
- Result: `Pass` (2026-03-01)
- Defect / Notes:
  - Evidence command: `cd tauri-app/src-ui && npm run check:command-contract`
  - Output: command-surface check passed with no missing frontend invoke registrations.

### STAB-002

- Test ID: `STAB-002`
- Title: Command contract check fails on injected mismatch
- Priority: `P0`
- Type: `AutomatedHarness`
- Preconditions:
  1. Local test branch with one synthetic mismatched command.
- Steps:
  1. Introduce temporary fake frontend invoke command.
  2. Run contract check script.
- Expected:
  1. Script exits non-zero.
  2. Output includes the fake command name.
- Evidence:
  1. Script output.
- Result: `Pass` (2026-03-01)
- Defect / Notes:
  - Injected synthetic command file: `tauri-app/src-ui/src/__contract_mismatch_tmp__.ts` with `invoke("fake_contract_command")`.
  - Evidence command: `node scripts/check_command_surface.mjs`
  - Output: non-zero exit and explicit failure line for `fake_contract_command`.

### STAB-010

- Test ID: `STAB-010`
- Title: Chat parity between Tauri and Entity gateway adapters (basic request)
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Both adapters enabled in test runtime.
- Steps:
  1. Send same user prompt through both adapters.
  2. Capture normalized response envelope fields.
- Expected:
  1. Both complete successfully.
  2. `session_id`, `execution_trace`, and reply shape are present in both outputs.
- Evidence:
  1. Adapter parity test logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-011

- Test ID: `STAB-011`
- Title: Chat parity for tool-use turn
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Tool-capable skill available in runtime.
- Steps:
  1. Send a prompt that triggers tool execution via both adapters.
- Expected:
  1. `tool_calls_made` shape is equivalent across adapters.
  2. Final response includes execution trace metadata in both paths.
- Evidence:
  1. Serialized response comparison.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-012

- Test ID: `STAB-012`
- Title: Session continuity parity
- Priority: `P1`
- Type: `Hybrid`
- Preconditions:
  1. Two-turn conversation fixture.
- Steps:
  1. Send turn 1 and turn 2 with same `session_id` via each adapter.
- Expected:
  1. Both paths preserve session continuity.
  2. Memory archive entries exist with matching session identifiers.
- Evidence:
  1. DB/session logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-020

- Test ID: `STAB-020`
- Title: Streaming success emits token(s) then done
- Priority: `P0`
- Type: `AutomatedHarness`
- Preconditions:
  1. Streaming endpoint active.
- Steps:
  1. Start streaming chat request.
  2. Record event sequence.
- Expected:
  1. One or more `token` events.
  2. Exactly one `done` event.
  3. No `error` event.
- Evidence:
  1. Event trace capture.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-021

- Test ID: `STAB-021`
- Title: Streaming failure emits error and terminates cleanly
- Priority: `P0`
- Type: `AutomatedHarness`
- Preconditions:
  1. Fault injection mode available.
- Steps:
  1. Trigger known chat failure.
  2. Observe event sequence.
- Expected:
  1. `error` event emitted.
  2. No hanging stream.
- Evidence:
  1. Event and timeout logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-030

- Test ID: `STAB-030`
- Title: Agent run lifecycle happy path
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Agentic commands wired to runtime engine.
- Steps:
  1. Start run.
  2. Allow completion.
  3. Query status/events.
- Expected:
  1. Status transitions from `pending/running` to terminal state.
  2. Run event history is persisted and retrievable.
- Evidence:
  1. API/status logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-031

- Test ID: `STAB-031`
- Title: Mentor ask/response path
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Run configured to request mentor input.
- Steps:
  1. Start run.
  2. Wait for ask event.
  3. Send mentor response.
- Expected:
  1. Run resumes after response.
  2. Event timeline contains ask + response handling.
- Evidence:
  1. Run event stream.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-032

- Test ID: `STAB-032`
- Title: Tool confirmation approve/deny path
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Run configured with confirmation requirement.
- Steps:
  1. Start run and wait for tool confirmation event.
  2. Deny once, then approve on next request.
- Expected:
  1. Deny path is reflected in run state/events.
  2. Approve path executes and run continues.
- Evidence:
  1. Event and status snapshots.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-035

- Test ID: `STAB-035`
- Title: Run recovery after restart
- Priority: `P0`
- Type: `ManualNative`
- Preconditions:
  1. Persistent run state enabled.
- Steps:
  1. Start a run.
  2. Restart app/runtime mid-run.
  3. Query run list/status after restart.
- Expected:
  1. In-flight or terminal run state is recoverable and visible.
- Evidence:
  1. Before/after status output.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-040

- Test ID: `STAB-040`
- Title: Entity-initiated run without GUI trigger
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Entity-initiated run entry point enabled.
- Steps:
  1. Trigger entity-side condition for autonomous run creation.
  2. Query run list and event stream.
- Expected:
  1. Run appears without direct GUI `start` action.
  2. Mentor checkpoints still enforced where policy requires.
- Evidence:
  1. Run creation logs and event stream.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-050

- Test ID: `STAB-050`
- Title: Subagent registry populated in runtime
- Priority: `P1`
- Type: `AutomatedHarness`
- Preconditions:
  1. Subagent definition source configured.
- Steps:
  1. Call list-subagents.
- Expected:
  1. Non-empty list with stable id/name/capability fields.
- Evidence:
  1. Command output.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-051

- Test ID: `STAB-051`
- Title: Subagent delegation success path
- Priority: `P1`
- Type: `Hybrid`
- Preconditions:
  1. At least one valid subagent definition.
- Steps:
  1. Delegate task to known subagent id.
- Expected:
  1. Response is returned and traceable.
  2. No fallback to unknown-subagent error.
- Evidence:
  1. Delegation command response.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-060

- Test ID: `STAB-060`
- Title: MCP trust policy blocks disallowed hosts
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. `mcp_trust_policy` configured with allow list.
- Steps:
  1. Attempt to query MCP tools on disallowed host.
- Expected:
  1. Runtime rejects request with explicit policy error.
- Evidence:
  1. Error payload/logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-061

- Test ID: `STAB-061`
- Title: MCP trust policy allows approved hosts
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Approved test host configured.
- Steps:
  1. Query MCP tools on approved host.
- Expected:
  1. Request succeeds.
- Evidence:
  1. Tool list response.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-062

- Test ID: `STAB-062`
- Title: Signed allowlist accepts valid trusted signer
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Signature verification and signer trust configuration enabled.
  2. Known trusted signer material available for test.
- Steps:
  1. Add signed allowlist entry with valid signature from trusted signer.
  2. Attempt execution of gated skill.
- Expected:
  1. Skill activation/execution is permitted.
  2. Verification metadata is emitted in logs/trace.
- Evidence:
  1. Verification log and successful execution output.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-063

- Test ID: `STAB-063`
- Title: Unsigned or unallowlisted skill execution is blocked
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Signed allowlist enforcement is set to required mode.
- Steps:
  1. Attempt execution for a skill without active trusted allowlist entry.
- Expected:
  1. Runtime denies execution with explicit policy error.
- Evidence:
  1. Error payload/logs.
- Result: `TBD`
- Defect / Notes: `TBD`

### STAB-064

- Test ID: `STAB-064`
- Title: Signed skill allowlist rejects invalid signatures
- Priority: `P0`
- Type: `Hybrid`
- Preconditions:
  1. Signature verification enforcement enabled.
- Steps:
  1. Add invalid signed allowlist entry.
  2. Attempt execution of gated skill.
- Expected:
  1. Activation/execution rejected.
- Evidence:
  1. Validation and execution error logs.
- Result: `TBD`
- Defect / Notes: `TBD`

---

## Suite Gates

- `Gate-STAB-A`: All `P0` STAB tests pass.
- `Gate-STAB-B`: No unresolved `P0` command-surface mismatch defects.
- `Gate-STAB-C`: Chat parity tests green for all production adapters.
- `Gate-STAB-D`: Agent lifecycle recovery test passes in native runtime.
- `Gate-STAB-E`: Policy enforcement tests pass (`MCP`, signed allowlist).
