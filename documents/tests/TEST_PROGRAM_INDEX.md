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

Sprint reports and the consolidated gate report from the retired single-agent app program are archived in `documents/archive/legacy-tauri-app/`.

## Program Tracks

1. Unit: crate-level logic and invariants (Rust + TypeScript unit/component tests)
2. UX: user-visible desktop/browser-harness behavior and regressions
3. End-to-End: daemon/runtime integration, live tool-use, and policy checks

## Program Gates

- `Gate-U1`: `cargo test --workspace` passes.
- `Gate-U2`: `cargo clippy --workspace -- -D warnings` passes.
- `Gate-U3`: `cargo check -p abigail-hive-app -p abigail-entity-runtime-app` passes.
- `Gate-X1`: `npm run build` passes in `hive-app/src-ui` and `entity-runtime-app/src-ui`.
- `Gate-X2`: `npm test` passes in `hive-app/src-ui` and `entity-runtime-app/src-ui`.
- `Gate-E1`: release-lane and trust-claim checks pass (`node scripts/check_unsigned_stabilization.mjs`, `node scripts/check_crypto_claims.mjs`).
- `Gate-E2`: env-gated daemon integration and live E2E suites pass when enabled (`ABIGAIL_DAEMON_INTEGRATION=1`, browser persistent auth).
- `Gate-S1`: `node scripts/check_stability.mjs` passes.

The retired app's command-contract check (`node legacy/tauri-app/scripts/check_command_surface.mjs`) is no longer a program gate.

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
5. Release decision from CI and the release evidence in `docs/MVP_WINDOWS.md`
