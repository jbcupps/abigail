# Sprint 2 Kickoff Checklist - Chat Gateway Abstraction

**Date:** 2026-03-01  
**Roadmap Source:** `documents/GUI_ENTITY_STABILITY_ROADMAP.md` (Sprint 2 section)  
**Objective:** Decouple GUI chat from direct Tauri command/event wiring by introducing a transport-agnostic gateway and validating parity across adapters.

## Sprint 2 Scope (Locked)

- `S2-01` Introduce frontend `ChatGateway` interface.
- `S2-02` Implement Tauri gateway adapter.
- `S2-03` Implement Entity HTTP/SSE gateway adapter.
- `S2-04` Move `ChatInterface` to gateway-only integration.
- `S2-05` Add parity tests for response + telemetry contract.

## Entry Criteria (Must Be True Before Sprint Work)

- Sprint 1 baseline remains green:
  - `npm run check:command-contract` passes.
  - `npm test` passes in `tauri-app/src-ui`.
  - `cargo check -p abigail-app` passes.
- No open P0 defects in default Birth -> Connectivity -> Chat flow.
- `documents/tests/SPRINT_1_GUI_ENTITY_STABILITY_REPORT.md` accepted as baseline.

## Implementation Checklist

### S2-01: Define Contract
- [ ] Add `ChatGateway` TypeScript interface with:
  - `send(message, context)` entrypoint
  - token stream callback support
  - done/error lifecycle callbacks
  - cancellation support
  - response telemetry fields (`provider`, `tier`, `model_used`, `execution_trace`, `session_id`)
- [ ] Define gateway-level DTOs shared by adapters and UI.
- [ ] Add explicit mapping for interruption (`Interrupted by user`).

### S2-02: Tauri Adapter
- [ ] Implement `TauriChatGateway` that wraps:
  - `invoke("chat_stream", ...)`
  - `listen("chat-token" | "chat-done" | "chat-error")`
  - `invoke("cancel_chat_stream")`
- [ ] Ensure listener lifecycle cleanup is deterministic on done/error/cancel/unmount.
- [ ] Preserve current telemetry and error normalization behavior.

### S2-03: Entity HTTP/SSE Adapter
- [ ] Implement `EntityHttpChatGateway` for daemon mode:
  - HTTP submit call
  - SSE token/done/error stream handling
  - cancellation semantics via abort controller (and endpoint hook if present)
- [ ] Normalize entity-daemon response shape to gateway DTO.
- [ ] Add transport timeout + reconnect/error fallback behavior.

### S2-04: ChatInterface Migration
- [ ] Remove direct `invoke/listen` streaming logic from `ChatInterface`.
- [ ] Inject/select gateway by runtime mode/config.
- [ ] Keep UI behavior unchanged for:
  - streaming token rendering
  - stop button behavior
  - partial response retention on interrupt
  - routing detail badges + telemetry display

### S2-05: Parity Test Coverage
- [ ] Add adapter parity tests validating same logical outputs for both adapters:
  - response text behavior
  - provider/tier/model/session fields
  - execution trace surface
  - tool call metadata
  - interruption/cancel lifecycle
- [ ] Add negative tests for timeout/error and listener cleanup leaks.
- [ ] Keep existing browser harness lifecycle tests green.

## Acceptance Criteria (Definition of Done)

1. `ChatInterface` has no direct dependency on `@tauri-apps/api` invoke/listen for chat streaming.
2. Both adapters satisfy the same `ChatGateway` contract and pass parity tests.
3. Cancel works consistently in both adapters and surfaces as non-fatal interrupt in UI.
4. Existing UI behavior is preserved (no regressions in chat rendering/status panels).
5. All required checks pass:
   - `npm run check:command-contract`
   - `npm test`
   - `cargo check -p abigail-app`

## Exit Evidence Required

- PR/commit diff references for:
  - gateway interface
  - tauri adapter
  - entity adapter
  - `ChatInterface` migration
  - parity tests
- Test run outputs recorded in `documents/tests/VALIDATION_AND_GATE_REPORT.md`.
- Sprint 2 closure report added as:
  - `documents/tests/SPRINT_2_CHAT_GATEWAY_REPORT.md`

## Risks and Controls

- Risk: adapter divergence in telemetry fields.
  - Control: shared DTO + parity snapshot assertions.
- Risk: cancellation races causing duplicate terminal events.
  - Control: single-terminal-state guard in gateway base/adapter layer.
- Risk: lifecycle leaks from stale listeners.
  - Control: explicit unlisten/dispose path tested under rapid send/cancel loops.

## Suggested Execution Order

1. Contract + DTOs (`S2-01`)
2. Tauri adapter + migration (`S2-02`, `S2-04`)
3. Entity adapter (`S2-03`)
4. Parity and regression tests (`S2-05`)
5. Validate gates + write closure report

## Sprint 2 Initiation Prompt (Copy/Paste)

```text
Execute Sprint 2: Chat Gateway Abstraction exactly per documents/tests/SPRINT_2_CHAT_GATEWAY_KICKOFF_CHECKLIST.md and roadmap S2-01..S2-05.

Constraints:
- Do not trust documentation blindly; verify behavior from code paths before editing.
- Keep existing GUI behavior unchanged while decoupling transport.
- Preserve chat interrupt semantics and telemetry fields.
- Do not expose experimental panels by default.

Required implementation:
1) Add transport-agnostic ChatGateway interface + shared DTOs.
2) Implement TauriChatGateway using chat_stream/chat-token/chat-done/chat-error and cancel_chat_stream.
3) Implement EntityHttpChatGateway using HTTP/SSE with cancellation.
4) Refactor ChatInterface to gateway-only chat transport.
5) Add parity tests proving both adapters return equivalent functional + telemetry outputs.

Validation gates (must run and report):
- cd tauri-app/src-ui && npm run check:command-contract
- cd tauri-app/src-ui && npm test
- cd /Users/jamescupps/Repo/abigail/abigail && cargo check -p abigail-app

Deliverables:
- Code changes for S2-01..S2-05
- Updated validation evidence in documents/tests/VALIDATION_AND_GATE_REPORT.md
- New closure report documents/tests/SPRINT_2_CHAT_GATEWAY_REPORT.md with pass/fail status and residual risks.
```

