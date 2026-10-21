# End-to-End Test Program (Current)

Date: 2026-03-07

## Objective

Validate integrated runtime behavior across daemons, skills, routing, and desktop wiring, including optional live browser-auth checks.

## E2E Categories

- `E2E-DAEMON`: hive/entity route integration and tool execution paths
- `E2E-AGENT`: agent run lifecycle and restart resilience
- `E2E-POLICY`: MCP trust and signed allowlist enforcement
- `E2E-LIVE`: browser-session persistence and desktop runtime probe

## Required Commands (Default, No External Secrets)

1. `cargo test -p entity-daemon --test integration_skills`
2. `cargo test -p entity-daemon --test chat_integration`
3. `cargo test -p entity-chat --test e2e_parity`
4. `node scripts/check_stability.mjs`

## Env-Gated Live Commands (Optional but Required for Full Release Readiness)

1. `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`
2. Daemon integration suites with `ABIGAIL_DAEMON_INTEGRATION=1`
3. `scripts/tests/run-mvp-acceptance.ps1` (concurrent Entities, streaming, scoped persistence, restart durability)

## Priority Test Cases

### E2E-DAEMON-001
- Title: Entity daemon chat plus skills integration is functional
- Priority: P0
- Evidence: `entity-daemon/tests/chat_integration.rs`, `entity-daemon/tests/integration_skills.rs`

### E2E-POLICY-001
- Title: MCP trust policy denies disallowed host before outbound call
- Priority: P0
- Evidence: `abigail-skills` MCP trust tests

### E2E-POLICY-002
- Title: Signed allowlist enforcement fails closed on invalid or untrusted state
- Priority: P0
- Evidence: policy regression command set documented in validation report

### E2E-LIVE-001
- Title: Browser session survives restart for auth-heavy workflows
- Priority: P0
- Evidence: `abigail-skills/tests/browser_persistent_auth.rs`

### E2E-LIVE-002
- Title: Hive and Entity Runtime pass MVP acceptance (concurrent Entities, streaming, restart durability)
- Priority: P0
- Evidence: `scripts/tests/run-mvp-acceptance.ps1`; installed results in `docs/MVP_WINDOWS.md`

The retired app's desktop probe (`legacy/tauri-app/scripts/tests/live_tauri_skill_secrets_e2e.ps1`) is no longer part of this program.

## Exit Criteria

- All P0 default E2E cases pass.
- For release-candidate signoff, all P0 live env-gated cases also pass or are explicitly waived with risk owner and date.
