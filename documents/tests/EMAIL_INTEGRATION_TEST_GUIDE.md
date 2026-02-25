# Email Integration Test Guide

## Overview

The live email integration tests in `crates/entity-chat/tests/live_email.rs` validate the full agentic email pipeline: an LLM receives a user message, recognizes the intent, calls the appropriate skill tools, and returns results. The tests exercise the exact same `entity-chat` engine used by both Tauri and entity-daemon — no HTTP server or Tauri runtime is involved.

All three tests are **environment-gated** and skip automatically unless `ABIGAIL_IMAP_TEST=1` is set. They never run in CI.

## Prerequisites

1. **Proton Mail Bridge** (or compatible IMAP/SMTP server) running locally.
   - Default bridge ports: IMAP `1143`, SMTP `1025` (STARTTLS).
   - Bridge must be logged in to the test account.

2. **Cloud LLM API key** — the tests require a real LLM that supports tool-use (function calling). Anthropic Claude is the tested provider.

3. **Rust toolchain** — `stable` channel with MSVC on Windows.

## Environment Variables

Copy the IMAP test section from `example.env` into a `.env.e2e.local` file (already gitignored) or export directly in your shell:

| Variable | Description | Example |
|----------|-------------|---------|
| `ABIGAIL_IMAP_TEST` | Set to `1` to enable tests | `1` |
| `ABIGAIL_LLM_PROVIDER` | LLM provider name | `anthropic` |
| `ABIGAIL_LLM_API_KEY` | API key for the provider | `sk-ant-...` |
| `ABIGAIL_IMAP_HOST` | IMAP server hostname | `127.0.0.1` |
| `ABIGAIL_IMAP_PORT` | IMAP server port | `1143` |
| `ABIGAIL_IMAP_USER` | IMAP login username (email) | `user@pm.me` |
| `ABIGAIL_IMAP_PASS` | IMAP login password (bridge password) | *(from bridge)* |
| `ABIGAIL_IMAP_TLS_MODE` | `starttls` or `implicit` | `starttls` |
| `ABIGAIL_SMTP_HOST` | SMTP server hostname | `127.0.0.1` |
| `ABIGAIL_SMTP_PORT` | SMTP server port | `1025` |

### PowerShell example

```powershell
$env:ABIGAIL_IMAP_TEST = "1"
$env:ABIGAIL_LLM_PROVIDER = "anthropic"
$env:ABIGAIL_LLM_API_KEY = "sk-ant-..."
$env:ABIGAIL_IMAP_HOST = "127.0.0.1"
$env:ABIGAIL_IMAP_PORT = "1143"
$env:ABIGAIL_IMAP_USER = "user@pm.me"
$env:ABIGAIL_IMAP_PASS = "bridge-password"
$env:ABIGAIL_IMAP_TLS_MODE = "starttls"
$env:ABIGAIL_SMTP_HOST = "127.0.0.1"
$env:ABIGAIL_SMTP_PORT = "1025"
```

### Bash example

```bash
export ABIGAIL_IMAP_TEST=1
export ABIGAIL_LLM_PROVIDER=anthropic
export ABIGAIL_LLM_API_KEY="sk-ant-..."
export ABIGAIL_IMAP_HOST=127.0.0.1
export ABIGAIL_IMAP_PORT=1143
export ABIGAIL_IMAP_USER="user@pm.me"
export ABIGAIL_IMAP_PASS="bridge-password"
export ABIGAIL_IMAP_TLS_MODE=starttls
export ABIGAIL_SMTP_HOST=127.0.0.1
export ABIGAIL_SMTP_PORT=1025
```

## Running the Tests

### All three turns at once

```bash
cargo test -p entity-chat --test live_email -- --nocapture
```

### Individual turns

```bash
# Turn 1: Credential storage via LLM tool-use
cargo test -p entity-chat --test live_email turn1_credential_setup -- --nocapture

# Turn 2: Fetch emails from IMAP (requires bridge running)
cargo test -p entity-chat --test live_email turn2_fetch_emails -- --nocapture

# Turn 3: Send email via SMTP (requires bridge running)
cargo test -p entity-chat --test live_email turn3_send_email -- --nocapture
```

The `--nocapture` flag shows the diagnostic `eprintln!` output, which is essential for understanding what the LLM did.

## What Each Turn Tests

### Turn 1: `turn1_credential_setup`

**Purpose**: Verify the LLM recognizes a credential-setup intent and calls `store_secret` via the tool-use loop.

**Flow**:
1. User message provides IMAP/SMTP connection details.
2. `entity-chat` augments the system prompt with skill instructions and available tools.
3. The LLM calls `builtin.hive_management::store_secret` for each credential.
4. `TestHiveOps` (in-memory vault) captures the stored secrets.

**Pass criteria**:
- `store_secret` was called at least once.
- All `store_secret` calls succeeded.
- The in-memory vault contains `imap_user` and `imap_password`.

**Does NOT require**: Proton Mail Bridge (no IMAP/SMTP connection is made).

### Turn 2: `turn2_fetch_emails`

**Purpose**: Verify the LLM calls `fetch_emails` when asked to check mail, and the skill returns real messages from the IMAP server.

**Flow**:
1. Vault is pre-populated with credentials (simulating Turn 1 completion).
2. `ProtonMailSkill` is initialized with a 15-second timeout (for IMAP connection).
3. User message asks to find emails from a specific sender.
4. The LLM calls `com.abigail.skills.proton-mail::fetch_emails`.

**Pass criteria**:
- `fetch_emails` was called.
- The LLM response mentions the target sender.

**Requires**: Proton Mail Bridge running and accepting IMAP connections.

### Turn 3: `turn3_send_email`

**Purpose**: Verify the LLM calls `send_email` when asked to send mail, and the SMTP transport delivers it.

**Flow**:
1. Vault is pre-populated with credentials.
2. `ProtonMailSkill` is initialized with a 15-second timeout.
3. User message asks to send a test email to a specific address.
4. The LLM calls `com.abigail.skills.proton-mail::send_email`.

**Pass criteria**:
- `send_email` was called.
- At least one `send_email` call succeeded.

**Requires**: Proton Mail Bridge running and accepting SMTP connections.

## Expected Output

### Turn 1 passing

```
--- Turn 1: sending credential setup message ---
Router: has_ego=true, provider=Some("anthropic")
Tools available: ["builtin_hive_management__store_secret", ...]
Router sanity check OK: "Hello! ..."
Turn 1 response: I've stored your email configuration...
Turn 1 tool calls: ["builtin.hive_management::store_secret (ok=true)", ...]
Vault contents: ["imap_user", "imap_password", "imap_host", ...]
test turn1_credential_setup ... ok
```

### Turn 2/3 skipping (bridge not running)

```
Skipping Turn 2 — IMAP init timed out after 15s (is the mail bridge running?)
test turn2_fetch_emails ... ok
```

This is a graceful skip, not a failure. The test exits early and reports `ok`.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| All tests print "Skipping: ... not set" | Environment variables missing | Set all vars per table above |
| Turn 1 fails with "Router must have Ego configured" | `ABIGAIL_LLM_API_KEY` is empty or provider unrecognized | Verify the key and `ABIGAIL_LLM_PROVIDER` value |
| Turn 1 returns "I need a cloud API key or local LLM" | Provider built but tool-use call failed | Check LLM provider API status; enable `RUST_LOG=abigail_router=debug` |
| Turn 2/3 "IMAP init timed out after 15s" | Proton Mail Bridge not running or wrong port | Start bridge, verify `ABIGAIL_IMAP_HOST`/`PORT` match bridge settings |
| Turn 2/3 "ProtonMailSkill init failed" | Wrong credentials or TLS mode | Verify bridge password and `ABIGAIL_IMAP_TLS_MODE` |
| Linker error `LNK1104` after killing a test | Old test binary locked by zombie process | Kill `live_email-*.exe` processes, delete the binary from `target/`, retry |

## CI Safety

These tests are fully isolated from CI:
- They skip unless `ABIGAIL_IMAP_TEST=1` is set (never set in GitHub Actions).
- They use environment variables for all credentials (no hardcoded secrets).
- The `TestHiveOps` struct provides an in-memory vault (no filesystem side effects).
- Timeouts prevent hangs if external services are unavailable.
