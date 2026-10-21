# Legacy

Nothing in this directory is part of the Abigail framework build, installer, or CI.

## `tauri-app/` (package `abigail-app`)

The retired single-agent desktop app. It ran one agent in one Tauri process
with an in-process boot/birth sequence, its own command surface, and its own
installer scripts. Abigail is now a framework for creating and hosting many
persistent Entities:

- `hive-app` + `hive-daemon` create, birth, sign, and supervise Entities.
- `entity-runtime-app` + `entity-daemon` run each Entity, with its own
  `entity_<UUID>` database inside the Hive-owned `memory.db`.

See the [root README](../README.md) and [`docs/architecture.md`](../docs/architecture.md).

### Why it is kept instead of deleted

- It is still the only UI for encrypted recovery-bundle export and the only
  command surface for trusted skill-signer rotation. `scripts/check_crypto_claims.mjs`
  checks those markers here until Hive exposes them.
- It is the only consumer of `abigail-birth` and `abigail-soul-crystallization`
  (conversational birth and soul crystallization), which remain framework
  crates for a future Hive birth path.
- Its history is useful when porting behavior into the split apps.

### Building it

It is a standalone Cargo workspace with its own `Cargo.lock`, so it never
affects the framework workspace or lockfile:

```bash
cargo check --manifest-path legacy/tauri-app/Cargo.toml
cd legacy/tauri-app/src-ui && npm ci && npm test
```

Its helper scripts live in `tauri-app/scripts/` (`build-installer.*`,
`build-release-windows.ps1`, `fix_tauri_build_blockers.sh`,
`check_command_surface.mjs`, and `tests/live_tauri_skill_secrets_e2e.ps1`).
They build the retired app only. Do not use them for family installers; use
`scripts/build-mvp-windows.ps1` or `scripts/build-signed-installer.ps1`.

Historical plans and reports written against this app are in
[`documents/archive/legacy-tauri-app/`](../documents/archive/legacy-tauri-app/README.md).
