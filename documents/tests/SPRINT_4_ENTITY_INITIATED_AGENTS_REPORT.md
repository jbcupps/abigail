# Sprint 4 Entity-Initiated Agents Report

Date: 2026-03-01  
Scope: `S4-01..S4-06` from `documents/GUI_ENTITY_STABILITY_ROADMAP.md` and `documents/tests/SPRINT_4_ENTITY_INITIATED_AGENTS_KICKOFF_CHECKLIST.md`.

## Scope Decision Notes (Code-First)

- `tauri-app/src/commands/agentic.rs` was verified to be full lifecycle stubs at sprint start; the sprint replaced stubs with real runtime wiring.
- Existing GUI chat (`chat`, `chat_stream`, coordinator path) was left unchanged.
- Jobs/Staff panel visibility is now tied to backend health (`get_orchestration_backend_status`) so orchestration UI is only exposed when backend wiring is live.

## Sprint Item Closure

| Item | Status | Implementation |
|---|---|---|
| S4-01 Wire Tauri Agentic Commands to `AgenticEngine` | **Closed** | Replaced stubs in `tauri-app/src/commands/agentic.rs` with real runtime calls. Added full lifecycle support for `start`, `status`, `respond`, `confirm`, `cancel`, `list`. Added compatibility aliases (`respond_agentic_mentor`, `confirm_agentic_action`). |
| S4-02 Persist and Recover Agent Runs | **Closed** | Added `tauri-app/src/agentic_runtime.rs` durable store (`agentic_runs.json`) with schema versioning + legacy backfill (`Vec<AgenticRun>` migration path). Boot recovery marks non-terminal runs as failed with explicit restart-recovery reason and persists reconciled state. |
| S4-03 Mentor Interaction Event Bridge | **Closed** | Runtime emits engine lifecycle events via `agentic-event` and bridge events (`mentor_response_received`, `mentor_confirmation_received`) with deterministic status enforcement and terminal-state stickiness. |
| S4-04 Runtime Subagent Registration | **Closed** | Added boot-time registration of default runtime subagents in `tauri-app/src/lib.rs`. Delegation now routes through `SubagentManager::delegate` and enforces central policy checks in `crates/abigail-router/src/subagent.rs` before dispatch. |
| S4-05 Entity-Initiated Run Entry Point | **Closed** | Added non-GUI entrypoint `start_entity_initiated_agentic_run` in `tauri-app/src/commands/agentic.rs` with origin/entity/session/correlation attribution (`entity_pipeline`) persisted and surfaced in run snapshots. |
| S4-06 Re-enable Orchestration UI on Real Backend | **Closed** | Added real orchestration command surface (`tauri-app/src/commands/orchestration.rs`) and wired jobs panel to live backend operations. `SanctumDrawer` now gates staff/jobs tab visibility on backend health. |

## Code References

- Runtime + persistence/recovery: `tauri-app/src/agentic_runtime.rs`
- Agentic command wiring + entity entrypoint: `tauri-app/src/commands/agentic.rs`
- Subagent boot registration: `tauri-app/src/lib.rs`
- Delegation policy enforcement: `crates/abigail-router/src/subagent.rs`
- Delegation command path update: `tauri-app/src/commands/agent.rs`
- Orchestration backend commands: `tauri-app/src/commands/orchestration.rs`
- Jobs UI wiring and readiness gating:
  - `tauri-app/src-ui/src/components/OrchestrationPanel.tsx`
  - `tauri-app/src-ui/src/components/SanctumDrawer.tsx`
  - `tauri-app/src-ui/src/components/AgenticPanel.tsx`
  - `tauri-app/src-ui/src/browserTauriHarness.ts`

## Required Validation Gates

1. `cd tauri-app/src-ui && npm run check:command-contract` -> **PASS**
   - `Frontend commands checked: 104`
   - `Native commands registered: 147`
   - `Harness command cases checked: 86`
2. `cd tauri-app/src-ui && npm test` -> **PASS**
   - `Test Files  9 passed (9)`
   - `Tests  28 passed (28)`
3. `cd /Users/jamescupps/Repo/abigail/abigail && cargo check -p abigail-app` -> **PASS**
   - `Finished 'dev' profile ...`

## Pass/Fail Summary

- Sprint 4 Implementation: **PASS**
- Required Validation Gates: **PASS**
- Command-Surface Compatibility Constraint: **PASS** (existing command names retained; alias commands added for migration compatibility)
- GUI Chat Stability Constraint: **PASS** (no chat coordinator/transport behavior changes)

## Residual Risks

- Orchestration scheduler currently executes only manual run-now path in this sprint; continuous background cron runner is not yet enabled in desktop runtime.
- Agentic run persistence currently uses JSON snapshot persistence rather than queue-backed row-level history; high-frequency event bursts can increase write frequency.
- Existing governance commands (`get_governor_status`, `get_constraint_store`, `clear_constraints`) remain placeholders and are outside Sprint 4 scope.

## Follow-Up Actions

1. Add background orchestration polling/runner loop with explicit health telemetry and backoff control.
2. Add integration tests for restart recovery edge cases (mid-confirmation, mid-mentor-wait, and cancellation race).
3. Evaluate moving run lifecycle persistence to queue-backed storage for higher write durability and queryability.
