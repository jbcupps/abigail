# Upgrade Guide

This document covers how Abigail handles updates and data preservation across versions.

## Auto-update

The current Abigail app (Hive plus Entity Runtime) does not ship an in-app updater. Upgrade by running a newer installer; see [Manual upgrade](#manual-upgrade). The installer removes app files only and never deletes family data.

The updater described below belonged to the retired single-agent app (now `legacy/tauri-app`) and is kept for reference only.

### Platform support (retired app)

| Platform | Auto-update | Mechanism |
| --- | --- | --- |
| Windows (NSIS) | Yes | Downloads signed `.nsis.zip` updater payload |
| Windows (MSI) | Yes | Downloads signed `.msi.zip` updater payload |
| macOS | Yes | Downloads signed `.app.tar.gz` updater payload |
| Linux (DEB) | No | Download manually from GitHub Releases |

### Release metadata

- Release workflows inject the updater verification public key into `tauri.conf.json` at build time.
- Release workflows publish signed updater payloads plus a generated `latest.json`.
- `latest.json` contains a static `platforms` map of updater URLs and signatures for the supported installer types.

## Data preserved during upgrades

All user data is preserved across installer upgrades.

### Core files

- `config.json`
- `external_pubkey.bin`
- `master.key`
- `vault.sentinel`
- `vault.kdf.json`
- `keys.vault` / legacy `keys.bin`
- `secrets.vault` / legacy `secrets.bin`
- `skills.vault` / legacy `skills.bin`
- `memory.db` (the Hive-owned embedded SurrealDB store)
- `docs/` signed constitutional artifacts

Legacy SQLite files may still appear during upgrade only:

- `abigail_seed.db`
- `abigail_memory.db`
- `jobs.db`
- `calendar.db`
- `kb.db`

On first launch after the migration, Abigail imports supported legacy SQLite data into the Hive-owned Surreal store and archives the legacy files with a `.legacy-imported` suffix after a successful import.

### Hive and Entity files

- `global_settings.json`
- `identities/<UUID>/` per Entity: config, signed constitutional docs, signatures, birth certificate, and the encrypted signing-key vault
- each Entity's records live in its own `entity_<UUID>` database inside `memory.db`

### Data directory locations

| Platform | Path |
| --- | --- |
| Windows | `%LOCALAPPDATA%\abigail\Abigail\` |
| macOS | `~/Library/Application Support/abigail/Abigail/` |
| Linux | `~/.local/share/abigail/Abigail/` |

## Schema and format compatibility

- `AppConfig` migrates older schema versions forward automatically.
- Legacy passphrase vaults remain readable and are upgraded with explicit KDF metadata.
- The runtime persistence layer is now embedded SurrealDB under the shared Hive `memory.db` path.
- Existing SQLite-backed chat history, queue state, calendar entries, and knowledge-base records are imported on first launch.
- Archive v1 remains restore-compatible even though new exports use archive v2.
- Legacy email transport fields remain compatibility-only; the removed transport is not re-enabled during upgrade.

## Manual upgrade

1. Download the latest installer from [GitHub Releases](https://github.com/jbcupps/abigail/releases).
2. Run the installer. Existing family data stays in place.

## Downgrade

Downgrading is not supported.

1. Download the older release manually.
2. Install it separately.
3. Restore from backup if required.

Warning: newer config, database, or archive formats may not be backward compatible.
