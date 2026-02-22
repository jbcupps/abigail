# Contributing

## Scope

Contributions should target the active Hive/Entity restart architecture:

- `hive-core`
- `entity-core`
- `hive-daemon`
- `hive-cli`
- `entity-daemon`
- `entity-cli`

Do not add new dependencies on removed Tauri/UI paths.

## Basic Flow

1. Create a branch from `dev-split`.
2. Keep changes scoped and commit in logical steps.
3. Run local checks:

```bash
cargo check --workspace
cargo test --workspace
```

4. Open a PR with risk/rollback notes for behavior-changing work.

## PR Expectations

- API changes must stay versioned under `/v1` unless explicitly planned otherwise.
- Keep process boundaries clear: Hive control-plane logic vs Entity runtime logic.
- Prefer additive contracts in `hive-core`/`entity-core` over ad-hoc payloads.
