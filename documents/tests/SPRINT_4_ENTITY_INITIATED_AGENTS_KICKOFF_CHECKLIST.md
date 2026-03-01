# Sprint 4 Kickoff Checklist - Entity-Initiated Agents

**Date:** 2026-03-01  
**Roadmap Source:** `documents/GUI_ENTITY_STABILITY_ROADMAP.md` (Sprint 4 section)  
**Objective:** Replace stubbed agentic flows with real runtime execution, persistence, recovery, and GUI lifecycle bridging so entities can initiate agents without a GUI-originated trigger.

## Sprint 4 Scope (Locked)

- `S4-01` Wire Tauri agentic commands to `AgenticEngine`.
- `S4-02` Persist and recover agent runs across restarts.
- `S4-03` Add mentor interaction event bridge (`ask/confirm/cancel`) with deterministic transitions.
- `S4-04` Register runtime subagents at boot with delegation policy checks.
- `S4-05` Add entity-initiated run entrypoint.
- `S4-06` Re-enable orchestration UI on real backend implementation.

## Entry Criteria (Must Be True Before Sprint Work)

- Sprint 3 baseline remains green:
  - `npm run check:command-contract` passes.
  - `npm test` passes in `tauri-app/src-ui`.
  - `cargo check -p abigail-app` passes.
- Sprint 3 report accepted:
  - `documents/tests/SPRINT_3_INTERNAL_MESSAGE_BOUNDARY_REPORT.md`
- No open P0 regression in default Birth -> Chat flow.

## Implementation Checklist

### S4-01: Agentic Command Wiring
- [ ] Replace `tauri-app/src/commands/agentic.rs` stubs with real calls into `abigail-router` `AgenticEngine`.
- [ ] Implement lifecycle coverage for:
  - `start_agentic_run`
  - `get_agentic_run_status`
  - `respond_agentic_mentor`
  - `confirm_agentic_action`
  - `cancel_agentic_run`
  - `list_agentic_runs`
- [ ] Ensure command contract and error surfaces are explicit and typed.

### S4-02: Run Persistence + Recovery
- [ ] Persist run metadata/state transitions to durable storage.
- [ ] Rehydrate active/pending runs at startup.
- [ ] Implement restart recovery behavior and stale-run handling policy.
- [ ] Add migration/backfill behavior if storage schema changes.

### S4-03: Mentor Lifecycle Event Bridge
- [ ] Emit deterministic lifecycle events for mentor asks/awaiting responses/confirmations/cancels/completion.
- [ ] Bridge events into GUI with clear run correlation IDs.
- [ ] Guarantee single terminal state per run and idempotent transition handling.

### S4-04: Runtime Subagent Registration
- [ ] Register default subagent definitions during runtime bootstrap.
- [ ] Enforce delegation checks and policy validation before dispatch.
- [ ] Add visibility/diagnostics for loaded subagents and policy failures.

### S4-05: Entity-Initiated Entry Point
- [ ] Add runtime path for entity pipeline to enqueue/start agent runs without GUI trigger.
- [ ] Ensure queued/started runs participate in same persistence, eventing, and cancellation policy.
- [ ] Verify run attribution metadata (origin/entity/session correlation).

### S4-06: Orchestration UI Re-enable
- [ ] Re-enable jobs/orchestration UI only when real backend commands are wired and healthy.
- [ ] Remove temporary guards that blocked these panels in production default.
- [ ] Ensure UI state reflects backend run/job lifecycle accurately.

## Acceptance Criteria (Definition of Done)

1. Tauri agentic command surface is fully wired to real backend logic (no lifecycle stubs).
2. Agent runs survive restart and recover deterministically.
3. Mentor interaction loop is event-driven, deterministic, and correlation-safe.
4. Subagent delegation works with runtime-registered definitions and policy enforcement.
5. Entity-initiated runs work end-to-end without GUI-originated trigger.
6. Orchestration/jobs UI is connected to real backend and functionally verified.
7. Required checks pass:
   - `npm run check:command-contract`
   - `npm test`
   - `cargo check -p abigail-app`

## Exit Evidence Required

- Code diff references for:
  - agentic command wiring
  - persistence/recovery
  - mentor event bridge
  - subagent registration
  - entity-initiated entrypoint
  - orchestration UI/backend wiring
- Validation command outputs recorded in `documents/tests/VALIDATION_AND_GATE_REPORT.md`.
- Sprint 4 closure report added as:
  - `documents/tests/SPRINT_4_ENTITY_INITIATED_AGENTS_REPORT.md`

## Risks and Controls

- Risk: lifecycle race conditions create duplicate terminal states.
  - Control: run-level state machine guard with idempotent terminal transition checks.
- Risk: restart recovery replays stale or partial runs incorrectly.
  - Control: explicit recovery policy, heartbeat/lease timestamps, and stale-run reconciliation tests.
- Risk: subagent delegation bypasses policy checks.
  - Control: enforce policy in central dispatch path and assert in tests.
- Risk: UI exposes jobs panel before backend is operationally complete.
  - Control: backend-readiness capability flag gating with health checks.

## Suggested Execution Order

1. Agentic command wiring (`S4-01`)
2. Persistence + recovery (`S4-02`)
3. Mentor bridge (`S4-03`)
4. Subagent registration (`S4-04`)
5. Entity-initiated entrypoint (`S4-05`)
6. UI re-enable + integration tests (`S4-06`)
7. Gate validation + Sprint 4 report

## Sprint 4 Initiation Prompt (Copy/Paste)

```text
Execute Sprint 4: Entity-Initiated Agents exactly per documents/tests/SPRINT_4_ENTITY_INITIATED_AGENTS_KICKOFF_CHECKLIST.md and roadmap S4-01..S4-06.

Constraints:
- Validate behavior from code first; do not rely on docs assumptions.
- Keep existing stable GUI chat behavior unchanged.
- Preserve command-surface compatibility where possible; if breaking changes are required, include migration handling.
- Do not expose orchestration/jobs UI until backend wiring is real and healthy.

Required implementation:
1) Replace agentic command stubs with real AgenticEngine wiring.
2) Add durable run persistence and startup recovery.
3) Implement mentor ask/confirm/cancel event bridge with deterministic transitions.
4) Register runtime subagents at boot and enforce delegation policy checks.
5) Add entity-initiated run entrypoint (non-GUI trigger path).
6) Re-enable orchestration/jobs UI on real backend behavior.

Validation gates (must run and report):
- cd tauri-app/src-ui && npm run check:command-contract
- cd tauri-app/src-ui && npm test
- cd /Users/jamescupps/Repo/abigail/abigail && cargo check -p abigail-app

Deliverables:
- Code changes for S4-01..S4-06
- Updated validation evidence in documents/tests/VALIDATION_AND_GATE_REPORT.md
- New closure report documents/tests/SPRINT_4_ENTITY_INITIATED_AGENTS_REPORT.md with pass/fail status, residual risks, and follow-up actions.
```

