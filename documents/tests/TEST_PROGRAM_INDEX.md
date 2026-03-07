# Abigail Test Program Index (Current)

Date: 2026-03-07

## Scope

This is the canonical test-program index for the current architecture:

- Hive/Entity split daemons
- Gateway-based desktop chat flow
- Simplified routing with `ego_primary` and `cli_orchestrator`
- Skill trust and signed allowlist enforcement
- Browser fallback for authenticated web workflows

## Canonical Program Documents

- `documents/tests/UNIT_TEST_PROGRAM.md`
- `documents/tests/UX_TEST_PROGRAM.md`
- `documents/tests/E2E_TEST_PROGRAM.md`
- `documents/tests/AUTOMATION_MATRIX.md`
- `documents/tests/VALIDATION_AND_GATE_REPORT.md`

## Program Tracks

1. Unit: crate-level logic and invariants (Rust + TypeScript unit/component tests)
2. UX: user-visible desktop/browser-harness behavior and regressions
3. End-to-End: daemon/runtime integration, live tool-use, and policy checks

## Program Gates

- `Gate-U1`: `cargo test --workspace --exclude abigail-app` passes.
- `Gate-U2`: `cargo clippy --workspace --exclude abigail-app -- -D warnings` passes.
- `Gate-U3`: `cargo check -p abigail-app` passes.
- `Gate-X1`: `cd tauri-app/src-ui && npm run build` passes.
- `Gate-X2`: `cd tauri-app/src-ui && npm run test:coverage` passes.
- `Gate-E1`: command-contract check passes (`npm run check:command-contract`).
- `Gate-E2`: env-gated live E2E suites pass when enabled (browser persistent auth + Tauri probe).
- `Gate-S1`: `node scripts/check_stability.mjs` passes.

## Legacy Suite Status

The following documents remain for historical traceability but are no longer the canonical planning surface:

- `documents/tests/BIRTH_UI_TEST_PLAN.md`
- `documents/tests/CRYSTALLIZATION_UI_TEST_PLAN.md`
- `documents/tests/OPERATIONAL_HIVE_UI_TEST_PLAN.md`
- `documents/tests/SKILLS_TEST_PLAN.md`
- `documents/tests/AGENT_SPAWNING_TEST_PLAN.md`
- `documents/tests/MESSAGE_FLOW_STABILITY_TEST_PLAN.md`

Legacy IDs are consolidated into the `UNIT-*`, `UX-*`, and `E2E-*` tracks.

## Execution Order

1. Unit gates (`Gate-U1`..`Gate-U3`)
2. UX gates (`Gate-X1`..`Gate-X2`)
3. E2E gates (`Gate-E1`..`Gate-E2`)
4. Stability gate (`Gate-S1`)
5. Release decision from `documents/tests/VALIDATION_AND_GATE_REPORT.md`
