# Cryptography Inventory

This document maps each cryptographic surface in Abigail to the actual runtime or CI control that enforces it.

## Trust chain

`stable KEK -> Hive master key -> Hive signature over external_pubkey.bin -> constitutional document signatures`

- Startup and agent load fail closed when that chain cannot be verified.
- Recovery flows can repair document signatures, but they do not bypass Hive or KEK verification.

## Inventory

| Surface | Algorithm / Format | Trust root | Enforcement |
| --- | --- | --- | --- |
| Vault root KEK | OS keychain by default; `ABIGAIL_VAULT_RAW_KEY` for headless automation; `ABIGAIL_VAULT_PASSPHRASE` via Argon2id for human fallback; legacy HKDF metadata for compatibility only | Local device or operator-provided raw secret | `HybridUnlockProvider::root_kek()`, `vault.sentinel`, startup integrity inspection |
| Local encrypted files | AES-256-GCM envelope storage with atomic writes and restrictive file permissions | Root KEK | `secure_fs`, `encrypted_storage`, `SecretsVault`, `Keyring`, vault sentinel |
| Hive master key | Ed25519 signing key encrypted at rest | Stable KEK | `generate_master_key`, `load_master_key`, identity bootstrap |
| Agent registration | Ed25519 signature over `external_pubkey.bin` by Hive master key | Hive master key | `IdentityManager::verify_agent`, startup and agent-load integrity checks |
| Constitutional documents | Ed25519 signatures over canonical document payloads | User-held constitutional private key | `verify_constitutional_integrity`, repair flow, startup and agent-load fail-closed path |
| Recovery export | Default encrypted recovery bundle; optional plaintext export only as explicit advanced action | Root KEK for bundle encryption; operator custody for plaintext | `save_recovery_key`, `save_recovery_key_plaintext`, `crypto_audit.log` |
| Portable archives | v2 uses X25519 + HKDF-SHA256 + AES-256-GCM with AAD-bound headers; v1 remains read-only compatible | User recovery key | `ArchiveExporter::export`, `ArchiveExporter::restore` |
| Provider and skill secrets | AES-256-GCM sealed storage in `SecretsVault` / skills vault | Root KEK | `store_secret`, `SecretsVault`, namespace validation |
| Legacy email password fields | Compatibility-only tombstones in config schema | None; feature removed | Email configure endpoint returns gone; docs explicitly deprecate old fields |
| Signed skill allowlist | Ed25519 signatures over canonical allowlist payloads | `trusted_skill_signers` | `SkillExecutionPolicy`, trusted signer management commands, fail-closed verification |
| Updater verification | Minisign updater signatures verified with embedded updater pubkey | Release pipeline managed updater keypair | Tauri updater plugin, release workflow pubkey injection, generated `latest.json` |
| Platform release signing | Windows Authenticode, macOS Developer ID + notarization | Release signing credentials | `.github/workflows/release.yml`, `.github/workflows/release-fast.yml`, release prerequisite checks |

## Compatibility rules

- Existing passphrase-derived vaults remain readable. First successful legacy unlock writes `vault.kdf.json` with `legacy_hkdf_v1` metadata.
- Existing portable archives remain readable through archive v1 restore support.
- Legacy config email password fields remain loadable only so old configs can migrate forward safely; active email transport is removed from mainline Abigail.

## Release controls

- `scripts/enforce_release_prereqs.sh` blocks published release builds when updater signing keys, updater public key, Windows signing inputs, or macOS signing/notarization inputs are missing.
- `scripts/prepare_tauri_bundle_config.mjs` injects the updater verification public key and signing-related bundle fields at build time.
- `scripts/generate_tauri_latest_manifest.mjs` emits `latest.json` from the signed updater artifacts that are actually attached to the release.
