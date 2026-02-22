# CLI Improvement Options

Brainstormed options for improving the `abigail-cli` crate. Each category is independent and can be prioritized separately.

## Current State

The CLI (`crates/abigail-cli/`) currently provides:
- **8 subcommands**: `status`, `store-secret`, `check-secret`, `list-secrets`, `configure-email`, `integration-status`, `router-status`, `serve`
- **REST API** (via `serve`) with 10 endpoints on port 3141, Bearer token auth
- **Tauri integration** to start/stop the REST server from the desktop app
- **CLI provider adapters** for Claude Code, Gemini, Codex, Grok

---

## 1. New Subcommands

Add missing CLI workflows so users can do more without the desktop app.

- `chat` — send a single message from the terminal and print the response
- `birth` — run first-run setup headless (keypair generation, document signing, verification)
- `logs` — tail or query agent logs
- `config get <key>` / `config set <key> <value>` — read/write individual config fields
- `verify` — re-verify constitutional document signatures on demand
- `skills list` / `skills enable` / `skills disable` — manage skills from the CLI

## 2. Better Output Formatting

Improve readability and flexibility of CLI output.

- Colored terminal output (e.g., green for healthy, red for errors)
- Tabular formatting for list-style output (secrets, integrations, skills)
- `--json` flag on all commands for machine-readable output
- `--quiet` / `--verbose` flags for controlling detail level

## 3. Interactive Mode

Add a REPL-style experience for ongoing terminal use.

- `abigail-cli repl` — interactive chat session with prompt, streaming responses
- Tab-completion for subcommands and config keys
- Command history (persisted across sessions)
- Inline `/slash` commands within the REPL (e.g., `/status`, `/clear`, `/exit`)

## 4. Diagnostics and Debugging

Help users troubleshoot configuration and connectivity issues.

- `doctor` subcommand — run a battery of health checks (config valid, vault accessible, LLM reachable, signatures valid)
- Connectivity test for Id (local LLM) and Ego (cloud LLM) endpoints
- Report system info (OS, data directory, version, feature flags)
- Log-level override via `--log-level` flag

## 5. Scripting Support

Make the CLI a first-class citizen for automation and CI pipelines.

- Consistent, meaningful exit codes (0 = success, 1 = general error, 2 = auth error, etc.)
- Stable `--json` output schema for all commands
- Stdin support for piping input (e.g., `echo "hello" | abigail-cli chat`)
- `--no-color` flag for non-TTY environments
- `--format` flag (json, table, plain)

## 6. Other Ideas

- Shell completions generator (`abigail-cli completions bash/zsh/fish/powershell`)
- `init` subcommand to scaffold a new skills project
- `export` / `import` for config and secrets migration between machines
- Plugin/extension system for user-defined CLI commands
- `update` subcommand for self-update or version checking

---

## Decision Log

| Date | Decision | Rationale |
|------|----------|-----------|
| *(pending)* | *(pick priorities)* | |
