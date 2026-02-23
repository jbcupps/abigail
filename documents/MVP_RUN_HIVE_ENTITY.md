# MVP Runbook: Hive + Entity

This runbook is the current MVP execution path for development.

## 0) Run visible loop validation (recommended first)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/run-iterative-loops.ps1 -Loop all -EnvFile .env.e2e.local
```

Expected completion lines:
- `Completed loops: 1, 2, 3, 4`
- `All requested loop checks passed.`

## 1) Start services

Terminal 1:

```powershell
cargo run -p hive-daemon
```

Terminal 2:

```powershell
cargo run -p entity-daemon
```

## 2) Birth and start the testing entity

```powershell
cargo run -p hive-cli -- entity birth adam --path direct
cargo run -p hive-cli -- entity start adam
cargo run -p hive-cli -- entity list
```

Expected list output includes:
- `adam`
- `birth_complete=true`
- `birth_path=Direct`

## 3) Chat with Adam (single + interactive)

Single message:

```powershell
cargo run -p entity-cli -- chat "Hi Adam, let's build."
```

Interactive chat:

```powershell
cargo run -p entity-cli -- chat --interactive
```

Type `exit` or `/exit` to leave interactive mode.

## 4) One-command bootstrap (recommended)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -EntityId adam -BirthPath direct -RestartDaemons -KeepDaemons
```

## 5) Verify provider/router wiring in daemon logs

```powershell
Get-Content target/dev-session-logs/entity-daemon.out.log -Tail 60
```

Look for:
- `entity router initialized mode=EgoPrimary`
- `OpenAiProvider::complete` (or equivalent provider completion line)

## 6) Hive persistence check across restart

Optional custom registry file:

```powershell
$env:HIVE_REGISTRY_PATH = "E:\Agents\abigail\target\dev-session-logs\hive-registry.json"
```

After restart, confirm Adam still exists:

```powershell
cargo run -p hive-cli -- entity list
```

## Current MVP note

Entity chat now routes through `IdEgoRouter` in `entity-daemon`.
- With provider keys available (for example via `.env.e2e.local` + bootstrap script), chat returns live provider output.
- Without provider/local-LLM availability, it falls back to a clear configuration message from the local stub.
