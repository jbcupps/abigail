# UAT Operations Runbook

## Overview

The Tabula Rasa UAT validates the full Abigail daemon stack from a clean state: build, Hive startup, entity creation, LLM chat, weather currentness, and IMAP email inbox verification.

## Prerequisites

- Rust toolchain (`cargo`, `rustfmt`, `clippy`)
- PowerShell 5.1+ (Windows) or PowerShell Core 7+ (cross-platform)
- Network access to the configured LLM provider API (e.g. OpenAI)
- Local IMAP bridge running (for email stage) on configured host/port

## Quick Start

```powershell
# 1. Copy the keyset template and fill in real values
cp scripts/uat/uat-keys.env.template scripts/uat/uat-keys.env
# Edit uat-keys.env with your API key and IMAP credentials

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
| `-SkipEmail` | false | Skip email/IMAP stage |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | `PASS` — all stages green |
| 10 | `SOFT_FAIL_RECOVERED` — passed after retries |
| 20 | `HARD_FAIL` — unrecoverable, fix required |

## Run Artifacts

Each run creates `target/uat-runs/{runId}/` containing:

- `summary.json` — final result, entity_id, timing
- `timeline.log` — chronological event log
- `http/` — redacted HTTP request/response traces per stage
- `process/` — Hive and Entity daemon stdout/stderr logs
- `assertions/` — per-stage pass/fail with detail
- `failure-plan.md` + `failure-plan.json` — generated on any failure

## Stages

### Stage 0: Preflight
Validates keyset, port availability, and IMAP bridge reachability.

### Stage 1: Build
Runs `cargo fmt --check`, `cargo clippy`, `cargo build` (workspace minus tauri-app).

### Stage 2: Hive Bootstrap
Starts `hive-daemon --data-dir {uatDataDir}` and waits for `/health`.

### Stage 3: Entity Create
`POST /v1/entities` with naming convention `uat-{date}-{time}-{index}`.

### Stage 4: Secret Seeding
Seeds provider API key and IMAP credentials into Hive via `POST /v1/secrets`.

### Stage 5: Entity Bootstrap
Starts `entity-daemon` with `--data-dir` pointing at UAT root and waits for `has_ego=true`.

### Stage 6: Chat Sanity
Sends "hello" + 3 simple questions. Asserts real LLM response (not stub).

### Stage 7: Weather
Fetches ground truth from Open-Meteo API, asks entity for weather, compares.

### Stage 8: Email
Verifies email skill is registered, calls `fetch_emails`, asserts inbox data.

## Failure Handling

Every stage follows: `Detect -> Troubleshoot -> Remediate -> Retry -> Classify`.

- **Soft failure**: recovered within retry budget (max 2 transient, 1 config).
- **Hard failure**: unrecoverable in-run. Run stops, artifacts freeze, `failure-plan.md` emitted.

### Hard Failure Recovery

1. Read `failure-plan.md` in the run's artifact folder.
2. Fix the root cause (code, config, or environment).
3. Re-run from scratch with a new runId:
   ```powershell
   powershell -File scripts/uat/run-uat.ps1 -SkipBuild
   ```

## Keyset File Format

```env
# Provider key (at least one required)
OPENAI_API_KEY=sk-...

# IMAP bridge credentials
UAT_IMAP_HOST=127.0.0.1
UAT_IMAP_PORT=7654
UAT_IMAP_USER=user@example.com
UAT_IMAP_PASSWORD=secret
UAT_IMAP_SECURITY=STARTTLS
```

## Entity Naming Convention

Format: `uat-{yyyyMMdd}-{HHmm}-{runIndex}`

Example: `uat-20260224-2145-01`

## Security

- Keyset file is gitignored (`scripts/uat/uat-keys.env`).
- Secrets are never printed to console or written to artifact files.
- HTTP traces redact secret values.
- Only the keyset template (`.env.template`) is committed.
