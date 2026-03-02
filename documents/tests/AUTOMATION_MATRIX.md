# Automation Matrix (Current)

Date: 2026-03-02

## Legend

- `CI-Required`: blocks merge via required checks
- `CI-Advisory`: runs in CI but does not block merge
- `Local-Required`: required pre-push/local gate, not currently required in CI
- `Env-Gated`: runs only when credentials/services are configured

## Current Matrix

| Track | Test Group | Mode | Status | Command / Evidence |
|---|---|---|---|---|
| Unit | Rust workspace unit/integration | CI-Required | Active | `cargo test --workspace --exclude abigail-app` |
| Unit | Rust lint and compile safety | CI-Required | Active | `cargo clippy --workspace --exclude abigail-app -- -D warnings`, `cargo check -p abigail-app` |
| Unit | Frontend unit/component tests | CI-Required | Active | `cd tauri-app/src-ui && npm run test:coverage` |
| UX | Frontend build and type safety | CI-Required | Active | `cd tauri-app/src-ui && npm run build` |
| E2E | Command surface contract | Local-Required | Active | `cd tauri-app/src-ui && npm run check:command-contract` |
| E2E | Daemon chat/skills integration | Local-Required | Active | `cargo test -p entity-daemon --test chat_integration`, `integration_skills` |
| E2E | Shared chat parity integration | Local-Required | Active | `cargo test -p entity-chat --test e2e_parity` |
| E2E | Live email tool-use | Env-Gated | Active | `cargo test -p entity-chat --test live_email -- --nocapture` |
| E2E | Desktop runtime probe | Env-Gated | Active | `.\scripts\tests\live_tauri_skill_secrets_e2e.ps1` |
| Security | Cargo/npm advisory scans | CI-Advisory | Active | `audit` job in `.github/workflows/ci.yml` |
| Security | CodeQL JS/TS analysis | CI-Advisory | Active | `codeql` job in `.github/workflows/ci.yml` |

## Deprecated Matrix Entries

The following ID families are deprecated from active planning and should not be expanded:

- `BIRTH-*`
- `CRYS-*`
- `OPER-*`
- `SKILL-*`
- `SPAWN-*`
- `STAB-*`

Use `UNIT-*`, `UX-*`, and `E2E-*` IDs from the new canonical program docs instead.
