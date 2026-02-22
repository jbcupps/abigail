# How To Run Locally (Hive/Entity V1)

## Prerequisites

- Rust toolchain installed
- `cargo` available on PATH

## Build

```bash
cargo check --workspace
```

## Run Daemons

Terminal 1:

```bash
cargo run -p hive-daemon
```

Terminal 2:

```bash
cargo run -p entity-daemon
```

## Use CLIs

Hive:

```bash
cargo run -p hive-cli -- status
cargo run -p hive-cli -- entity list
cargo run -p hive-cli -- entity start demo-entity
cargo run -p hive-cli -- entity stop demo-entity
```

Entity daemon mode:

```bash
cargo run -p entity-cli -- status
cargo run -p entity-cli -- run "smoke-task"
cargo run -p entity-cli -- chat "hello"
cargo run -p entity-cli -- logs
```

Entity one-shot mode:

```bash
cargo run -p entity-cli -- --oneshot status
cargo run -p entity-cli -- --oneshot chat "hello"
```
