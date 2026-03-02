# End-to-End Test Program (Current)

Date: 2026-03-02

## Objective

Validate integrated runtime behavior across daemons, skills, routing, and desktop wiring, including env-gated live checks.

## E2E Categories

- `E2E-DAEMON`: hive/entity route integration and tool execution paths
- `E2E-AGENT`: agent run lifecycle and restart resilience
- `E2E-POLICY`: MCP trust and signed allowlist enforcement
- `E2E-LIVE`: live email tool-use and desktop runtime probe

## Required Commands (Default, No External Secrets)

1. `cargo test -p entity-daemon --test integration_skills`
2. `cargo test -p entity-daemon --test chat_integration`
3. `cargo test -p entity-chat --test e2e_parity`
4. `cd tauri-app/src-ui && npm run check:command-contract`

## Env-Gated Live Commands (Optional but Required for Full Release Readiness)

1. `cargo test -p entity-chat --test live_email -- --nocapture`
2. `.\scripts\tests\live_tauri_skill_secrets_e2e.ps1`

## Priority Test Cases

### E2E-DAEMON-001
- Title: Entity daemon chat + skills integration is functional
- Priority: P0
- Evidence: `entity-daemon/tests/chat_integration.rs`, `integration_skills.rs`

### E2E-POLICY-001
- Title: MCP trust policy denies disallowed host before outbound call
- Priority: P0
- Evidence: `abigail-skills` MCP trust tests

### E2E-POLICY-002
- Title: Signed allowlist enforcement fails closed on invalid/untrusted state
- Priority: P0
- Evidence: policy regression command set documented in validation report

### E2E-LIVE-001
- Title: Live email Turn 1 stores IMAP/SMTP secrets through LLM tool-use loop
- Priority: P0
- Evidence: `entity-chat/tests/live_email.rs` (`turn1_credential_setup`)

### E2E-LIVE-002
- Title: Desktop runtime probe validates instruction bootstrap and skill namespace checks
- Priority: P0
- Evidence: `scripts/tests/live_tauri_skill_secrets_e2e.ps1`

## Redundant E2E Policy

Legacy case identifiers from `STAB-*` are now informational only unless mapped to the `E2E-*` IDs above. Unmapped legacy cases are non-blocking and should be removed in a follow-up cleanup PR.

## Exit Criteria

- All P0 default E2E cases pass.
- For release-candidate signoff, all P0 live env-gated cases also pass or are explicitly waived with risk owner and date.
