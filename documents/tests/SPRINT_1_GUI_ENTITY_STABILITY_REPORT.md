# Sprint 1 GUI/Entity Stability Report

**Date:** 2026-03-01  
**Scope:** Sprint 1 from `documents/GUI_ENTITY_STABILITY_ROADMAP.md`

## Gap-Closure Checklist (S1-01..S1-05)

| ID | Requirement | Status | Implementation | Evidence |
|---|---|---|---|---|
| S1-01 | Command contract CI gate | **Closed** | Added `scripts/check_command_surface.mjs` and wired `npm run check:command-contract` in `tauri-app/src-ui/package.json`. Gate checks frontend invoke commands against Tauri `generate_handler![]`. | Baseline run: `npm run check:command-contract` -> PASS |
| S1-02 | Remove/feature-gate broken frontend invokes | **Closed** | Removed or disabled unregistered command calls from default UI paths: `skip_to_life_for_mvp`, `backup_sqlite`, orchestration command set, `get_mcp_app_content`, `set_tier_models`/`refresh_provider_catalog`/`validate_tier_models`. | Contract gate passes with no missing frontend invoke registrations. |
| S1-03 | Destructive identity action safety fix | **Closed** | Registered `check_existing_identity`, `archive_identity`, `wipe_identity` in Tauri handler. Hardened `archive_identity`/`wipe_identity` to block active-agent execution and reset runtime state consistently (active agent, birth state, config persistence, router rebuild). | `cargo check -p abigail-app` -> PASS; commands now in `generate_handler![]`. |
| S1-04 | Hide experimental agentic/orchestration panels by default | **Closed** | Added `isExperimentalUiEnabled()` flag in `runtimeMode.ts`; `SanctumDrawer` now hides `staff` and `jobs` tabs unless explicitly enabled via query/localStorage/env. | Default UI no longer exposes unfinished panels. |
| S1-05 | Align harness command mocks with native command registry | **Closed** | Command-surface gate now validates harness switch-case command names against native registry with explicit allowlist for harness-only debug/plugin commands. | `npm run check:command-contract` includes harness alignment check -> PASS. |

## Sprint 1 Exit Criteria

| Exit Criterion | Result | Notes |
|---|---|---|
| No runtime `command not found` in default GUI flows | **PASS** | Broken/unregistered default-flow invokes were removed or replaced; command contract gate is green. |
| Experimental/unwired paths are not exposed by default | **PASS** | Staff/agentic and orchestration/job panels are hidden unless experimental UI is explicitly enabled. |

## STAB Test Mapping

- `STAB-001` -> **Pass** (baseline contract gate)
- `STAB-002` -> **Pass** (synthetic mismatch causes non-zero fail)

## Verification Commands Run

- `cd tauri-app/src-ui && npm run check:command-contract` -> **PASS**
- `cd tauri-app/src-ui && npm test` -> **PASS** (`24` tests)
- `cargo check -p abigail-app` -> **PASS**
- Synthetic mismatch check:
  - create temporary invoke with `fake_contract_command`
  - run `node scripts/check_command_surface.mjs`
  - expected non-zero + missing-command output -> **PASS**
