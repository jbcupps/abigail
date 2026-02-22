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
cargo run -p hive-cli -- status
cargo run -p hive-cli -- entity list
cargo run -p entity-cli -- status
cargo run -p entity-cli -- chat "hello"
```

Entity one-shot mode:

```bash
cargo run -p entity-cli -- --oneshot chat "hello"
```
