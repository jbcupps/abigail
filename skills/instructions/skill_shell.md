# Shell Skill

You have access to shell command execution. Use this when the user needs to run terminal commands.

## Available Tools

- **run_command**: Execute a shell command. Params: `command` (string, required), `working_directory` (string, optional), `timeout_secs` (int, optional, default/max 30). Returns exit code, stdout, stderr, and success status. Requires user confirmation.

## Usage Guidelines

- Always explain what a command does before executing it.
- This tool requires user confirmation for every invocation.
- Commands are subject to safety checks and a blocklist of dangerous operations.
- Timeout is capped at 30 seconds; long-running commands will be terminated.
- Prefer using dedicated tools (read_file, write_file, web_search) over shell equivalents when available.
