# Validation and Gate Report (Current)

Date: 2026-03-07
Scope: Current unit, UX, stability, and E2E test program status for the active architecture.

## Canonical Sources

- `documents/tests/UNIT_TEST_PROGRAM.md`
- `documents/tests/UX_TEST_PROGRAM.md`
- `documents/tests/E2E_TEST_PROGRAM.md`
- `documents/tests/AUTOMATION_MATRIX.md`

## Current Gate Model

### Merge-Blocking CI Gates

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --exclude abigail-app -- -D warnings`
- `cargo check -p abigail-app`
- `cargo test --workspace --exclude abigail-app`
- `cd tauri-app/src-ui && npm run build`
- `cd tauri-app/src-ui && npm run test:coverage`
- `node scripts/check_stability.mjs`

### Local Required Gates

- `cd tauri-app/src-ui && npm run check:command-contract`
- `cargo test -p entity-daemon --test chat_integration`
- `cargo test -p entity-daemon --test integration_skills`
- `cargo test -p entity-chat --test e2e_parity`

### Env-Gated Release Readiness Gates

- `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`
- `.\scripts\tests\live_tauri_skill_secrets_e2e.ps1`

## Latest Validation Focus

### Unit + Compile Gates

1. `cargo check -p entity-daemon`
2. `cargo check -p abigail-app`
3. `cargo test --workspace --exclude abigail-app --no-run`

### UX + Stability Gates

1. `cd tauri-app/src-ui && npm run check:command-contract`
2. `cd tauri-app/src-ui && npm test`
3. `node scripts/check_stability.mjs`

### E2E + Live Gates

1. `cargo test -p entity-daemon --test integration_skills`
2. `cargo test -p entity-daemon --test chat_integration`
3. `cargo test -p entity-chat --test e2e_parity`
4. `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`
5. `pwsh -File scripts/tests/live_tauri_skill_secrets_e2e.ps1`

## Consolidation Actions Completed

- Removed mainline IMAP/SMTP runtime support and replaced it with Browser fallback as the supported auth-heavy workflow path.
- Added a single repo-level stability script for the cross-runtime baseline.
- Promoted removed-capability checks to explicit probe and namespace-policy coverage.
- Consolidated runtime skill registration through shared bootstrap helpers used by both Tauri and `entity-daemon`.

## Open Actions

1. Re-run the browser persistent-auth test with Playwright prerequisites installed in every target environment.
2. Keep Forge success, blocked, and error-path coverage green as the worker evolves.
3. Keep watcher hot-reload parity green for both registry changes and dynamic skill JSON changes.

## Decision

Current status: **CONDITIONAL GO**.

Reason: the baseline gates are restored and deterministic, while the live browser-auth path remains environment-dependent by design.
