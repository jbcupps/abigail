# Skills Test Plan (Current)

Date: 2026-03-07

## Objective

Validate the supported skill surface after IMAP/SMTP retirement:

- shared runtime registration stays in sync across Tauri and `entity-daemon`
- skill secrets enforce the allowed namespace and reject removed email keys
- watcher hot-reload works for dynamic skill artifacts and registry updates
- Browser skill remains the supported path for authenticated web workflows

## Test Cases

### SKILL-001
- Title: Shared bootstrap registers the supported native skill inventory
- Priority: P0
- Type: Automated
- Evidence: `cargo test -p entity-daemon --test integration_skills`

### SKILL-002
- Title: Secret namespace rejects removed email transport keys
- Priority: P0
- Type: Automated
- Evidence: `cargo test -p abigail-runtime`, `cargo test -p abigail-cli`, `cargo test -p abigail-app`

### SKILL-003
- Title: Instruction registry only injects instructions for active supported skills
- Priority: P1
- Type: Automated
- Evidence: `cargo test -p abigail-skills`

### SKILL-004
- Title: Forge-created dynamic skills become discoverable and reloadable
- Priority: P0
- Type: Automated
- Evidence: `cargo test -p entity-daemon --test integration_skills`, `cargo test -p soul-forge`

### SKILL-005
- Title: Watcher detects `skill.toml`, `*.json`, and `registry.toml` changes
- Priority: P0
- Type: Automated
- Evidence: `cargo test -p abigail-skills watcher`

### SKILL-006
- Title: Browser persistent auth survives restart
- Priority: P0
- Type: EnvGated
- Evidence: `cargo test -p abigail-skills --test browser_persistent_auth -- --nocapture`

### SKILL-007
- Title: Desktop runtime probe enforces removed-capability checks
- Priority: P0
- Type: EnvGated
- Evidence: `pwsh -File scripts/tests/live_tauri_skill_secrets_e2e.ps1`

## Exit Criteria

- P0 automated cases pass in local and CI gates.
- Env-gated Browser fallback cases pass for release readiness or have an explicit waiver.
- No mainline test depends on transport email delivery or bridge-side mailbox plumbing.
