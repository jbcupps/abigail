# Abigail (Hive/Entity Restart)

Abigail is in a clean-restart phase on branch `dev-split`.

Current architecture is process-separated:

- `hive-daemon` + `hive-cli` (control plane)
- `entity-daemon` + `entity-cli` (runtime plane)
- shared contracts in `hive-core` and `entity-core`

Canonical architecture document:

- `documents/ARCHITECTURE_HIVE_ENTITY_V1.md`

## Workspace

The active workspace includes only:

- `crates/hive-core`
- `crates/entity-core`
- `crates/hive-daemon`
- `crates/hive-cli`
- `crates/entity-daemon`
- `crates/entity-cli`
- `crates/abigail-cli` (unified shell entrypoint)

Legacy Tauri/UI and harness assets were removed from the active stack.

## Quick Start

Build check:

```bash
cargo check --workspace
```

Run daemons in separate terminals:

```bash
cargo run -p hive-daemon
cargo run -p entity-daemon
```

Use CLIs:

```bash
cargo run -p abigail-cli --bin abigail
cargo run -p hive-cli -- status
cargo run -p hive-cli -- entity list
cargo run -p hive-cli -- entity birth adam --path quick-start
cargo run -p hive-cli -- entity start adam
cargo run -p entity-cli -- status
cargo run -p entity-cli -- chat "hello"
cargo run -p entity-cli -- chat --interactive
```

Entity one-shot mode:

```bash
cargo run -p entity-cli -- --oneshot chat "hello"
```

`abigail` (or `cargo run -p abigail-cli --bin abigail`) now runs an interactive shell that:

- ensures hive/entity daemons are running,
- probes local Ollama / LM Studio endpoints for Id connectivity,
- performs minimal provider key onboarding when local providers are unavailable,
- executes a simple birth dialog, then
- drops directly into mentor ↔ entity chat.

Development bootstrap for testing entity `adam`:

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/start-adam-dev-session.ps1 -EntityId adam -BirthPath direct -RestartDaemons -KeepDaemons
```

MVP runbook:

- `documents/MVP_RUN_HIVE_ENTITY.md`

4-loop validation plan and runner:

- `documents/ITERATIVE_LOOP_TEST_PLAN.md`
- `scripts/run-iterative-loops.ps1`

Automated full-shell smoke test:

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/smoke-abigail-shell.ps1
```

Hive UI + splash start screen:

- `docs/hive-ui.html`
- Start daemons first (`hive-daemon` and `entity-daemon`), then open the file in a browser.
- The page includes screens for splash/start, providers, birth dialog, and chat.

Hosted UI via `abigail-cli serve`:

- `cargo run -p abigail-cli --bin abigail -- serve --port 3141`
- Open `http://127.0.0.1:3141/ui`
