# Iterative Loop Test Plan (4 Loops)

This plan is executable through:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-iterative-loops.ps1 -Loop all
```

The runner loads provider keys from `.env.e2e.local` and prints each command as it runs.

## Latest local validation snapshot

Execution date: 2026-02-22

Command:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-iterative-loops.ps1 -Loop all -EnvFile .env.e2e.local
```

Observed completion output:
- `Completed loops: 1, 2, 3, 4`
- `All requested loop checks passed.`

Artifacts/logs:
- Loop 1 daemon logs: `target/loop-logs/loop1-hive-daemon.out.log`, `target/loop-logs/loop1-entity-daemon.out.log`
- Adam dev session logs (bootstrap script): `target/dev-session-logs/hive-daemon.out.log`, `target/dev-session-logs/entity-daemon.out.log`

## Loop 1: Hive + Entity Hookup

Goal: verify the clean-restart process split works end-to-end.

Note: loop 1 intentionally clears provider env vars during the run so lifecycle/function checks are deterministic and do not depend on external provider availability.

- Build/test active Hive/Entity crates.
- Launch `hive-daemon` and `entity-daemon`.
- Validate all current API functions:
  - Hive: `status`, `entity/list`, `entity/birth`, `entity/start`, `entity/stop`, `logs`
  - Entity: `status`, `run`, `chat`, `logs`
- Validate all CLI functions:
  - `hive-cli`: `status`, `entity list/birth/start/stop`, `logs`
  - `entity-cli`: `status`, `run`, `chat`, `logs`, `--oneshot status/run/chat/logs`

## Loop 2: Crypto Keys + KeyVault + SkillVault

Goal: verify key handling and vault behaviors, using local provider keys.

- Assert required provider env vars are present in process env:
  - `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `XAI_API_KEY`, `TAVILY_API_KEY`, `GOOGLE_API_KEY`, `PERPLEXITY_API_KEY`
- Run key and vault tests:
  - `abigail-core` secrets vault tests
  - `abigail-core` external vault tests
  - `abigail-auth` manager tests
  - `abigail-hive` provider registry tests
  - `abigail-skills` HiveManagementSkill secret/config/entity tool tests
- Explicitly validate provider construction from real env keys.

## Loop 3: Birth Cycle + All Birth Paths

Goal: exercise complete birth orchestration and all genesis paths.

- Run `abigail-birth` tests:
  - `genesis::tests` (includes all 4 paths)
  - `stages::tests` (birth stage orchestration)
  - `prompts::tests` (stage prompt contracts)

## Loop 4: Chat Capability Surface

Goal: verify router and provider chat behavior with local key context.

- Assert required provider env vars are present.
- Run chat/routing tests:
  - `abigail-router`: `router`, `orchestration`, `planner`, `council` test modules
  - `abigail-capabilities`: `local_http`, `openai_compatible` tests
  - Real-key provider smoke tests:
    - `openai::tests::test_openai_provider`
    - `anthropic::tests::test_anthropic_provider_with_real_key`
