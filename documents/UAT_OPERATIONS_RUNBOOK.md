# UAT Operations Runbook

## Overview

The Tabula Rasa UAT validates the full Abigail daemon stack from a clean state: build, Hive startup, entity creation, LLM chat, and weather currentness.

## Prerequisites

- Rust toolchain (`cargo`, `rustfmt`, `clippy`)
- PowerShell 5.1+ (Windows) or PowerShell Core 7+ (cross-platform)
- Network access to the configured LLM provider API (for example OpenAI)

## Quick Start

```powershell
# 1. Copy the keyset template and fill in real values
cp scripts/uat/uat-keys.env.template scripts/uat/uat-keys.env
# Edit uat-keys.env with your API key

# 2. Run the full UAT
powershell -File scripts/uat/run-uat.ps1

# 3. Check results
cat target/uat-runs/uat-*/summary.json
```

## Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `-KeysetFile` | `scripts/uat/uat-keys.env` | Path to keyset file |
| `-HivePort` | 3141 | Hive daemon port |
| `-EntityPort` | 3142 | Entity daemon port |
| `-SkipBuild` | false | Skip build stage (use after a hard-failure fix) |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | `PASS` - all stages green |
| 10 | `SOFT_FAIL_RECOVERED` - passed after retries |
| 20 | `HARD_FAIL` - unrecoverable, fix required |

## Run Artifacts

Each run creates `target/uat-runs/{runId}/` containing:

- `summary.json` - final result, entity_id, timing
- `timeline.log` - chronological event log
- `http/` - redacted HTTP request/response traces per stage
- `process/` - Hive and Entity daemon stdout/stderr logs
- `assertions/` - per-stage pass/fail with detail
- `failure-plan.md` + `failure-plan.json` - generated on any failure

## Stages

### Stage 0: Preflight
Validates keyset and port availability.

### Stage 1: Build
Runs `cargo fmt --check`, `cargo clippy`, `cargo build` (workspace minus `abigail-app`).

### Stage 2: Hive Bootstrap
Starts `hive-daemon --data-dir {uatDataDir}` and waits for `/health`.

### Stage 3: Entity Create
`POST /v1/entities` with naming convention `uat-{date}-{time}-{index}`.

### Stage 4: Secret Seeding
Seeds the provider API key into Hive via `POST /v1/secrets`.

### Stage 5: Entity Bootstrap
Starts `entity-daemon` with `--data-dir` pointing at the UAT root and waits for `has_ego=true`.

### Stage 6: Chat Sanity
Sends "hello" plus 3 simple questions. Asserts a real LLM response, not the stub fallback.

### Stage 7: Weather
Fetches ground truth from Open-Meteo API, asks the entity for weather, and compares.

## Failure Handling

Every stage follows: `Detect -> Troubleshoot -> Remediate -> Retry -> Classify`.

- **Soft failure**: recovered within retry budget (max 2 transient, 1 config).
- **Hard failure**: unrecoverable in-run. The run stops, artifacts freeze, and `failure-plan.md` is emitted.

### Hard Failure Recovery

1. Read `failure-plan.md` in the run's artifact folder.
2. Fix the root cause (code, config, or environment).
3. Re-run from scratch with a new run ID:

```powershell
powershell -File scripts/uat/run-uat.ps1 -SkipBuild
```

## Keyset File Format

```env
# Provider key (at least one required)
OPENAI_API_KEY=sk-...
```

## Entity Naming Convention

Format: `uat-{yyyyMMdd}-{HHmm}-{runIndex}`

Example: `uat-20260224-2145-01`

## Security

- Keyset file is gitignored (`scripts/uat/uat-keys.env`).
- Secrets are never printed to console or written to artifact files.
- HTTP traces redact secret values.
- Only the keyset template is committed.
