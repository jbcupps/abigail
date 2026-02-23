# Upgrade Guide

This document covers how Abigail handles updates and data preservation across versions.

## Auto-Update (v0.0.2+)

Abigail checks for updates on launch using the Tauri 2.0 updater plugin. When a new version is available, a banner appears at the top of the chat screen with the new version number and two options:

- **Install & Restart** — downloads the update, installs it, and relaunches Abigail
- **Later** — dismisses the banner for the current session

### Platform Support

| Platform | Auto-update | Mechanism |
|----------|-------------|-----------|
| Windows (NSIS) | Yes | Downloads `.nsis.zip`, runs installer passively |
| macOS (DMG) | Yes | Downloads `.app.tar.gz`, replaces app bundle |
| Linux (AppImage) | Yes | Downloads `.AppImage.tar.gz` |
| Linux (DEB) | No | Check silently fails; download manually from GitHub Releases |

### How It Works

1. On app launch, the frontend calls the Tauri updater `check()` endpoint
2. The updater fetches `latest.json` from the latest GitHub Release
3. `latest.json` contains per-platform download URLs and Ed25519 signatures
4. If a newer version exists, the update banner is shown
5. The update is verified against the embedded public key before installation
6. The signing key is separate from platform code signing (no notarization needed)

## Data Preserved During Upgrades

All user data is preserved during both auto-update and manual NSIS upgrade:

### Core Files
- `config.json` — application settings and configuration (schema v0-v12 auto-migrated)
- `abigail_seed.db` — SQLite database (conversations, memories, birth record)
- `abigail_seed.db-wal` / `abigail_seed.db-shm` — SQLite WAL journal files
- `external_pubkey.bin` — external verification public key
- `secrets.bin` — DPAPI-encrypted secrets (Windows)
- `keys.bin` — DPAPI-encrypted signing keys (Windows)
- `docs/` — signed constitutional documents (soul.md, ethics.md, instincts.md)

### Hive Multi-Agent Files
- `global_settings.json` — shared Hive settings
- `master.key` — Hive master encryption key
- `hive_secrets.bin` — Hive-level encrypted secrets
- `identities/` — per-agent identity directories (config, keys, databases)

### Data Directory Locations

| Platform | Path |
|----------|------|
| Windows | `%LOCALAPPDATA%\abigail\Abigail\` |
| macOS | `~/Library/Application Support/abigail/Abigail/` |
| Linux | `~/.local/share/abigail/Abigail/` |

## SQLite Schema Migrations

The SQLite database uses a `schema_versions` table to track applied migrations. On each startup, the migration runner:

1. Creates the `schema_versions` table if it doesn't exist
2. Checks which migration versions have already been applied
3. Runs any pending migrations in order
4. Records each applied migration with a timestamp

Version 1 is the baseline schema (memories + birth tables). Future schema changes will be added as numbered migrations.

## Config Schema Migration

`AppConfig` automatically migrates from older config formats (v0 through v12) on startup. No manual intervention is needed — the app reads the old format and writes back the current format.

## Manual Upgrade

If auto-update is not available (Linux DEB, network issues):

1. Download the latest installer from [GitHub Releases](https://github.com/jbcupps/abigail/releases)
2. Run the installer — it detects the existing installation
3. Choose "Yes" to preserve your data when prompted
4. The installer backs up your data, installs the new version, and restores data

## Daemon Upgrades

When running the Hive/Entity daemons directly (outside the Tauri desktop app):

1. Stop all running daemons (`entity-daemon` first, then `hive-daemon`)
2. Build the new version: `cargo build -p hive-daemon -p entity-daemon --release`
3. Start `hive-daemon` with the same `--data-dir` as before
4. Start `entity-daemon` with the same `--entity-id`

The daemons share the same data directory and config format as the Tauri desktop app. No separate migration is needed.

## Downgrade

Downgrading is not supported via auto-update. To downgrade manually:

1. Download the older version from GitHub Releases
2. Run the installer with a fresh install (data may not be compatible with older schemas)
3. Restore a backup if available

**Warning**: Running an older version against a database with newer schema migrations may cause errors. Back up your data directory before attempting a downgrade.
