# Security Notes

## Key management

### Constitutional signing key

- Abigail generates an Ed25519 constitutional keypair during birth.
- The private key is shown once and is not persisted by the application.
- The default export is an encrypted recovery bundle.
- Plaintext recovery export exists only as an explicit advanced action and is audit logged.
- The public key is stored as `external_pubkey.bin` and is used only to verify constitutional documents.

### Stable KEK and local vaults

- Abigail uses a stable root KEK for sealed local storage.
- Preferred source is the OS keychain.
- Headless automation can use `ABIGAIL_VAULT_RAW_KEY`.
- Human fallback uses `ABIGAIL_VAULT_PASSPHRASE`, derived with Argon2id and per-install metadata in `vault.kdf.json`.
- Legacy passphrase installs without metadata remain readable and are migrated forward with explicit legacy metadata.

### Hive master key

- Hive stores an Ed25519 master signing key encrypted at rest with the stable KEK.
- The Hive master key signs each agent's `external_pubkey.bin`.
- An agent is not trusted unless that Hive signature verifies.

## Effective trust chain

Abigail does not trust a public key file just because it exists.

The enforced chain is:

`stable KEK -> Hive master key -> Hive signature over external_pubkey.bin -> constitutional document signatures`

Startup and agent load fail closed when any link in that chain fails.

## Storage security

- Sealed local files use AES-256-GCM envelope encryption.
- Key, vault, signature, and sentinel writes use atomic replace semantics.
- Unix writes apply restrictive permissions (`0600`) where supported.
- The vault sentinel is verified before passphrase-derived keys are accepted as valid.

## Secrets handling

- Provider secrets and operational skill secrets are stored in sealed vault files, not in plaintext config.
- Secret namespace validation allows only reserved provider keys or keys declared by installed skills.
- The legacy email transport has been removed from mainline Abigail.
- Legacy `password_encrypted` config fields remain only for compatibility loads and forward migration safety; they are not an active secret-storage path.

## Constitutional integrity

- Constitutional documents are signed with the user-held Ed25519 key.
- `run_startup_checks` and `load_agent` both use the shared integrity inspection path.
- Integrity results are structured as `ok`, `repairable`, or `blocked`.
- Repairable states surface document and signature issues.
- Blocked states cover KEK recovery failure, missing trust roots, or Hive signature failure.

## Archive and recovery

- Portable archive v2 derives the AEAD key from the X25519 shared secret with HKDF-SHA256.
- Archive headers are authenticated as AES-GCM additional authenticated data.
- Archive v1 remains readable for compatibility only.
- Recovery bundle export is encrypted by default; plaintext export is opt-in and explicit.

## Skill signing and trust

- Active `signed_skill_allowlist` entries are verified with Ed25519 against canonicalized `trusted_skill_signers`.
- Invalid signatures, malformed signer keys, and untrusted signers fail closed.
- External skills require a valid signed allowlist entry when trusted signers are configured.
- Trusted signer rotation and removal are exposed through dedicated Tauri commands and refresh the runtime policy immediately.

## Updater and release trust

- Release builds inject the updater verification public key at build time.
- Release scripts normalize updater minisign key boxes to the base64 format consumed by Tauri 2 before bundling.
- Published updater metadata is generated from the signed updater artifacts attached to the release.
- Official release workflows hard-fail without updater signing inputs.
- Official Windows releases require Authenticode inputs.
- Official macOS releases require Developer ID signing and notarization inputs.

## Other security controls

- Local LLM URLs are restricted to localhost / loopback HTTP(S) endpoints.
- Tauri CSP restricts script, style, network, image, font, and media origins.
- MCP HTTP endpoints are checked against the configured trust policy.
- GitHub Actions use pinned action SHAs and dependency audit workflows.
