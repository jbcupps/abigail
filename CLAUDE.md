# CLAUDE.md

## Active Architecture

Repository is in Hive/Entity clean-restart mode on `dev-split`.

Active workspace members:

- `crates/hive-core`
- `crates/entity-core`
- `crates/hive-daemon`
- `crates/hive-cli`
- `crates/entity-daemon`
- `crates/entity-cli`

Legacy Tauri/UI stack is removed from active development.

## Build & Check

```bash
cargo check --workspace
cargo test --workspace
```

## Run

```bash
cargo run -p hive-daemon
cargo run -p entity-daemon
```

```bash
cargo run -p hive-cli -- status
cargo run -p entity-cli -- chat "hello"
```

```bash
cargo run -p entity-cli -- --oneshot status
```

## Contract Rules

- Keep daemon endpoints versioned under `/v1`.
- Keep Hive control-plane and Entity runtime responsibilities separated.
- Prefer shared payloads in `hive-core` / `entity-core`.
