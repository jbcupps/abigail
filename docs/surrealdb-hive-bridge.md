# SurrealDB Hive Bridge

This document records the current desktop-first bridge between Abigail Hive and the local embedded SurrealDB store, plus the constraints for future SAO-style replication work.

## Current Local Model

- Abigail ships with a Hive-owned local persistence root named `memory.db`.
- The desktop runtime uses embedded SurrealDB only. No external Postgres or Docker service is required.
- Shared orchestration state lives in `abigail/hive`.
- Each Entity receives an isolated database scope in `abigail/entity_<uuid>`.
- Encryption-sensitive payloads are wrapped by the persistence layer before they are written.

## Migration Boundary

- Legacy SQLite files are read only during first-launch migration.
- Supported inputs are `abigail_seed.db`, `abigail_memory.db`, `jobs.db`, `calendar.db`, and `kb.db`.
- After a successful import, Abigail archives those files with a `.legacy-imported` suffix.

## Bridge Constraints

- Desktop remains offline-first. Any future replication must be additive and must not become a boot dependency.
- Hive remains the immortal owner of the local memory root and the only writer for shared orchestration state.
- Per-Entity scoping must survive any future sync or export path.
- Reflection, graph enrichment, vector archives, and queue recovery must keep working from the local store even when no bridge is active.

## Future SAO / Cross-Node Preparation

- Treat Surreal export/import and signed archive bundles as the first replication primitive.
- Keep entity scopes stable so bridge metadata can map directly to `entity_<uuid>` databases.
- Add replication journals outside the hot chat path so mentor-chat responsiveness is unaffected.
- Preserve local conflict resolution in Hive, not in user-facing Entities.
