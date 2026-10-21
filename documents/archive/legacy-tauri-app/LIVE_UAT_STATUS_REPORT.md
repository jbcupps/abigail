# Live UAT Status Report (Current)

Date: 2026-03-07
Status: CONDITIONAL GO

## Scope

This report tracks the live, environment-dependent checks that sit on top of the baseline stabilization gates:

- Browser persistent-auth regression
- Desktop runtime probe against an isolated data directory
- Human-supervised daemon and GUI validation when a real provider or signed-in browser session is available

## Baseline Assumption

The release candidate is not considered for live UAT until these baseline gates are green:

- `cargo check -p entity-daemon`
- `cargo check -p abigail-app`
- `cargo test --workspace --exclude abigail-app --no-run`
- `cd tauri-app/src-ui && npm run check:command-contract`
- `cd tauri-app/src-ui && npm test`

## Current Live Cases

### LIVE-001
- Title: Browser persistent auth survives restart
- Command: `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`
- Result: Environment-gated
- Success signal: authenticated browser session remains usable after skill restart

### LIVE-002
- Title: Desktop runtime probe rejects removed email secrets and validates instruction bootstrap
- Command: `pwsh -File scripts/tests/live_tauri_skill_secrets_e2e.ps1`
- Result: Environment-gated
- Success signal: probe passes instruction bootstrap, native-skill registration, and removed-capability checks

### LIVE-003
- Title: Human-supervised browser fallback flow
- Interface: Tauri or browser harness
- Result: Manual
- Success signal: signed-in web workflow succeeds through Browser skill without any IMAP/SMTP transport dependency

## Risks

- Browser-auth checks depend on Playwright prerequisites and a valid signed-in session.
- Manual live validation is still required for any site-specific webmail or OAuth flow before release signoff.

## Decision

Current status remains **CONDITIONAL GO** because the core gates are deterministic and restored, while live browser-auth validation is intentionally environment-dependent.
