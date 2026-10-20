# Preview cold-profile recovery

Source status, 2026-10-20: the bounded recovery implementation and `abigail-profile` CLI were published at `4558fe68d9812998eb53a94ba157970af552f923` on `dev/first-signal-integration-0.0.76` and are included in `dev/preview-combined-0.0.76`. The prior slice's [component validation](../output/playwright/cold-profile-validation.md) passed 104 distinct Rust tests: 101 core and three actual SurrealKV fresh-process recovery tests, with three standalone child fixtures ignored. Seven CLI smoke checks passed separately, and independent review found no unresolved blocking findings in that bounded slice. Its publication-pending wording records the earlier checkpoint. Fresh combined checks and preparation are recorded in [combined validation](../docs/PREVIEW_COMBINED_VALIDATION.md). **Real OS credential, GUI and exact-installer acceptance remain open.** This document describes source/component facts and does not authorize normal-profile use.

Recovery targets an existing preview/beta profile on Windows, under the same user and device with its existing credentials available. It preserves the profile UUID, Entity UUIDs, creation receipt keys, shared persistence files and presentation data. Existing per-Entity import/restore is unchanged: it creates a new Entity UUID and is a separate operation.

The offline utility is a separate workspace executable. The combined candidate's installer staging still contains only Hive, Entity Runtime and both daemons; it does not bundle `abigail-profile`. Exact installed recovery needs a separately reviewed tooling and acceptance plan.

## CLI contract

The [CLI](../crates/abigail-profile/src/main.rs) provides three offline commands. These examples use illustrative synthetic paths with an existing parent directory; they were not executed against a real profile.

```powershell
abigail-profile snapshot --source "C:\AbigailRecoveryFixture\source" --output "C:\AbigailRecoveryFixture\snapshot"
abigail-profile verify --snapshot "C:\AbigailRecoveryFixture\snapshot"
abigail-profile restore --snapshot "C:\AbigailRecoveryFixture\snapshot" --destination "C:\AbigailRecoveryFixture\restored"
```

`snapshot` and `verify` return the manifest as JSON and do not access external credentials. `restore` returns a receipt after publication, including the profile/Entity IDs, creation receipt count, public credential-verification evidence and whether the recorded original source was present and locked. Commands propagate failures; they do not launch a daemon or GUI, select a provider, migrate an Entity, or create replacement credentials.

Snapshot and restore publication currently require Windows. The source must already have a compatible preview marker. Output and restore destinations must not exist, their parents must exist, and paths must be absolute and non-overlapping. The CLI supplies the known production roots for exclusion. Restore cannot replace the recorded source, even when that source directory is absent.

## What the snapshot preserves

The [snapshot implementation](../crates/abigail-core/src/profile_snapshot.rs) produces an uncompressed directory containing exactly `manifest.json` and `profile/`. Its schema-1 manifest records the profile UUID, `beta` channel, source root, creation timestamp, credential policy, exclusions, Entity IDs, receipt count and a sorted inventory. Each file has a byte length and SHA-256; directories, including empty ones, are inventoried.

All retained profile files are copied, including the entire `memory.db` directory, registry and creation receipts, identity/config/signature material, presentation files, UUID-based `Documents`, archives, and opaque WebView storage. A complete `memory.db` directory, original `master.key`, `vault.sentinel` and consistent registry/configs are required. The four excluded top-level directories are `runtime`, `tmp`, `logs` and `backups` (case-insensitive). Their contents are not recovery data in this format; in particular, old process descriptors are not restored.

Verification rejects missing, extra or changed payload files, unsupported manifest/profile schemas, unsafe or case-colliding inventory paths, symlinks/reparse points and inconsistent identity/config summaries. The semantic validator also rejects malformed or duplicate JSON keys and inconsistent creation receipts. Completed receipts for deleted Entities remain tombstones; recovery does not invent replacement IDs. The manifest is bounded to 16 MiB and the inventory to 100,000 entries.

Hashes detect accidental changes relative to the supplied manifest. They do not authenticate the author or encrypt the bundle: someone able to replace both payload and manifest can recalculate them. Existing encrypted files remain encrypted, but other retained data is readable as stored. Verification does not prove that the database can open or that credentials can decrypt it.

## Cold ownership and publication

All four current preview processes—Hive GUI, Runtime GUI, Hive daemon and Entity daemon—call shared profile initialization and retain a [shared process-use lease](../crates/abigail-core/src/profile_lease.rs) until process exit. Cold operations require exclusive acquisition of that lease and the Hive owner lock. Live OS locks establish ownership; lock-file contents and stale PIDs do not. The tool refuses an occupied profile and never kills a process or performs shutdown for the user.

Snapshot retains Windows read handles for every included file without write/delete sharing while it copies. An incompatible surviving writer, including a WebView writer, causes refusal. It compares the source inventory before and after copying and verifies the copy. Restore similarly guards the bundle payload and manifest while copying and re-verifies its input.

When the manifest's original source still exists, restore checks its compatible marker and matching profile UUID, then holds its cold lease through publication. This ownership inspection deliberately does not require damaged active configs to be valid: recovery must be able to exclude a broken original. A missing or mismatched marker refuses recovery; a genuinely absent original is allowed. This check only covers the recorded original path. It neither discovers unknown clones nor permanently prevents the original and restored roots from being opened separately after recovery.

The locks coordinate participating processes. The Hive owner lock also excludes an older Hive that uses that lock, but this is not a universal detector for older binaries or arbitrary external writers. File guards and inventory checks do not establish global filesystem quiescence or provide a security boundary against a hostile writer.

Both operations build in a unique sibling `.abigail-cold-<UUID>.partial` directory, validate before publication, then use a same-parent Windows move without replacement. Existing targets are never overwritten. Ordinary failure attempts to remove only the operation's staging directory; a crash or cleanup failure can leave incomplete staging for inspection. Current normal startup rejects that reserved staging path and its descendants. A leftover partial directory is not a successfully published snapshot or recovered profile.

## Credentials and path rebasing

The CLI's [system credential probe](../crates/abigail-core/src/vault/unlock.rs) reads the existing profile-scoped OS entry, or a decryptable copied same-device fallback when no OS entry is available. The snapshot does not export the OS keyring. Restore therefore depends on the original user's existing preview credential or usable DPAPI fallback; this is not cross-user/device credential migration.

Before publication the probe decrypts the profile-scoped vault sentinel and original master-key envelope and returns public evidence, including the master public key. Missing, malformed, unavailable or wrong credential material causes an explicit failure. The probe adds no key generation, credential repair, cache update or credential write. It does not prove provider authentication, inference, or decryption of every encrypted file.

[State rebasing](../crates/abigail-core/src/profile_snapshot/state.rs) changes only these validated declared application paths:

| Location | Rebased fields |
| --- | --- |
| Root and active Entity configs | `data_dir`, `docs_dir`, `models_dir`, `db_path`, optional `external_pubkey_path` |
| Global registry | `master_key_path`, optional `hive_secrets_path` |

Registry directories remain UUID-relative. User text, other JSON values, presentation, encrypted bytes and opaque WebView data are preserved rather than subjected to blanket string replacement. Rewritten config/registry JSON is serialized again, so formatting is not promised to remain byte-identical. An old endpoint retained in a browser creation journal stays old; current creation recovery explicitly fails on endpoint mismatch while retaining the exact key. Automatic endpoint rebinding is not part of this slice.

## Evidence boundaries

The [fresh-process recovery fixture](../crates/abigail-persistence/tests/cold_profile_restore.rs) passed three tests using the actual embedded SurrealKV store, synthetic records and fake credentials. Its 15 recorded child phases have distinct process IDs. Reopen verifies two unchanged Entity UUIDs, three store scopes, four exact conversation turns, three receipts and two signatures, alongside the profile UUID, scoped jobs/memories, presentation, Documents, archives and retained synthetic browser-journal bytes. Missing/wrong credential cases preserve the bundle and do not publish a destination. The interrupted-restore case intentionally exits a child with code 73 before publication, then retries successfully; that proves a tested abrupt-process boundary, not power-loss recovery.

That fixture is not GUI reopening, a real WebView-store compatibility test, provider-account acceptance, or an OS keyring/DPAPI integration run. Its completed evidence, the final 101 core tests and seven CLI smoke checks are linked in [final component validation](../output/playwright/cold-profile-validation.md). The CLI checks cover help/parser behavior and missing-snapshot refusal without reaching credential probing; they do not perform a successful real-credential CLI restore. Exact installed cold recovery, real OS credentials, signed-installer coexistence and normal GUI/daemon lifecycle acceptance remain separate.

Read-only review of `ca6a` identified existing preview list/get/update config guards for future or corrupt configs. Forge's claim concerned older Linux source and is not a newly reproduced defect in this Windows recovery slice; neither review supplies fresh HTTP proof here. Stable metadata fallback is a different path, and a status response is process metadata, not attestation that every on-disk config has just been revalidated.
