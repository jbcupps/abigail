# Upgrade Guide

This document covers how Abigail handles updates and data preservation across versions.

## Auto-update

Abigail checks for updates on launch with the Tauri updater plugin.

- `Install & Restart` downloads the signed updater artifact, verifies it against the embedded updater public key, installs it, and relaunches Abigail.
- `Later` dismisses the banner for the current session.

### Platform support

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

All user data is preserved during both auto-update and manual installer upgrades.

### Core files

- `config.json`
- `external_pubkey.bin`
- `master.key`
- `vault.sentinel`
- `vault.kdf.json`
- `keys.vault` / legacy `keys.bin`
- `secrets.vault` / legacy `secrets.bin`
- `skills.vault` / legacy `skills.bin`
- `abigail_memory.db` plus WAL / SHM files
- `jobs.db` plus WAL / SHM files
- `docs/` signed constitutional artifacts

### Hive multi-agent files

- `global_settings.json`
- `identities/`
- per-agent configs, databases, docs, signatures, and recovery metadata

### Data directory locations

| Platform | Path |
| --- | --- |
| Windows | `%LOCALAPPDATA%\abigail\Abigail\` |
| macOS | `~/Library/Application Support/abigail/Abigail/` |
| Linux | `~/.local/share/abigail/Abigail/` |

## Schema and format compatibility

- `AppConfig` migrates older schema versions forward automatically.
- Legacy passphrase vaults remain readable and are upgraded with explicit KDF metadata.
- Archive v1 remains restore-compatible even though new exports use archive v2.
- Legacy email transport fields remain compatibility-only; the removed transport is not re-enabled during upgrade.

## Manual upgrade

If auto-update is unavailable:

1. Download the latest installer from [GitHub Releases](https://github.com/jbcupps/abigail/releases).
2. Run the installer.
3. Choose to preserve existing data when prompted.

## Downgrade

Downgrading is not supported by the updater.

1. Download the older release manually.
2. Install it separately.
3. Restore from backup if required.

Warning: newer config, database, or archive formats may not be backward compatible.
