# Development Chat Session: Testing Entity `adam`

Use this flow to create a testing entity, run a birth path, and verify chat is working.

## Prerequisites

- Provider keys are present in `.env.e2e.local` (used by the bootstrap script).
- Rust/Cargo toolchain is installed.

## One-command bootstrap

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -EntityId adam -BirthPath direct -RestartDaemons -KeepDaemons
```

This does:
- Starts `hive-daemon` and `entity-daemon` (if not already running)
- Births entity `adam` via Hive
- Starts `adam`
- Runs a chat check via `entity-cli`

## Choose a birth path

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -BirthPath direct
```

Allowed values:
- `quick_start`
- `direct`
- `soul_crystallization`
- `soul_forge`

## Keep daemons alive for active development

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -KeepDaemons
```

## Restart daemons after code/config changes

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -RestartDaemons -KeepDaemons
```

Then continue chatting:

```powershell
cargo run -p entity-cli -- chat "Hi Adam, let's continue."
```

Interactive chat session:

```powershell
cargo run -p entity-cli -- chat --interactive
```

## Expected verification signals

- `cargo run -p hive-cli -- status` prints `service=hive-daemon api=v1`.
- `cargo run -p entity-cli -- status` prints `service=entity-daemon api=v1 mode=daemon`.
- `cargo run -p entity-cli -- chat "Hi Adam"` returns a provider response (not local echo text).
- Interactive mode shows `you>` and `adam>` prompts.

## Troubleshooting

- `os error 10061` (`target machine actively refused it`) means `entity-daemon` is not listening on `127.0.0.1:7702`.
- Run:
  - `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -RestartDaemons -KeepDaemons`
- Re-check:
  - `cargo run -p entity-cli -- status`
  - `cargo run -p entity-cli -- chat "ping"`
- `-KeepDeamons` (legacy typo) is accepted as an alias for `-KeepDaemons`.
