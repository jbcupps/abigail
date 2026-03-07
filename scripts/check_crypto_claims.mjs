#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function assertIncludes(contents, expected, filePath, claimId, missing) {
  if (!contents.includes(expected)) {
    missing.push(`claim '${claimId}' missing marker in ${filePath}: ${expected}`);
  }
}

const claims = [
  {
    id: "trust-chain",
    description: "Identity verification fails closed on the KEK -> Hive -> constitutional signature chain.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "`stable KEK -> Hive master key -> Hive signature over external_pubkey.bin -> constitutional document signatures`",
          "Startup and agent load fail closed when that chain cannot be verified.",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "`stable KEK -> Hive master key -> Hive signature over external_pubkey.bin -> constitutional document signatures`",
          "Startup and agent load fail closed when any link in that chain fails.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Abigail enforces a fail-closed trust chain: stable KEK -> Hive master key -> Hive signature over `external_pubkey.bin` -> constitutional document signatures.",
        ],
      },
    ],
    implementation: [
      {
        path: "tauri-app/src/commands/identity.rs",
        markers: [
          "let unlock = HybridUnlockProvider::new();",
          "state.identity_manager.verify_agent(id)",
          "verify_constitutional_integrity(config)",
          "IntegrityStatus::Blocked",
        ],
      },
      {
        path: "crates/abigail-core/src/keyring.rs",
        markers: [
          "pub fn verify_agent_signature(",
        ],
      },
    ],
  },
  {
    id: "argon2-fallback",
    description: "Human passphrase fallback uses Argon2id metadata, while automation can supply a raw KEK.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "ABIGAIL_VAULT_RAW_KEY",
          "ABIGAIL_VAULT_PASSPHRASE",
          "Argon2id",
          "legacy HKDF metadata for compatibility only",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Headless automation can use `ABIGAIL_VAULT_RAW_KEY`.",
          "Human fallback uses `ABIGAIL_VAULT_PASSPHRASE`, derived with Argon2id and per-install metadata in `vault.kdf.json`.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Passphrase fallback uses Argon2id with per-install metadata for new vaults.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-core/src/vault/unlock.rs",
        markers: [
          "const KDF_METADATA_FILE: &str = \"vault.kdf.json\";",
          "ABIGAIL_VAULT_RAW_KEY",
          "ABIGAIL_VAULT_PASSPHRASE",
          "derive_key_from_passphrase_argon2",
          "legacy_hkdf_v1",
        ],
      },
      {
        path: "crates/abigail-core/src/vault/crypto.rs",
        markers: [
          "pub fn derive_key_from_passphrase_argon2",
          "Argon2::new",
        ],
      },
    ],
  },
  {
    id: "sealed-storage",
    description: "Sealed local storage uses AES-256-GCM and atomic writes.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "AES-256-GCM envelope storage with atomic writes and restrictive file permissions",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Sealed local files use AES-256-GCM envelope encryption.",
          "Key, vault, signature, and sentinel writes use atomic replace semantics.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Local vault storage uses AES-256-GCM with atomic writes.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-core/src/secure_fs.rs",
        markers: [
          "pub fn write_bytes_atomic",
          "pub fn write_string_atomic",
          "fn replace_file_atomic",
        ],
      },
      {
        path: "crates/abigail-core/src/encrypted_storage.rs",
        markers: [
          "secure_fs::write_bytes_atomic(path, &envelope)?;",
        ],
      },
      {
        path: "crates/abigail-core/src/keyring.rs",
        markers: [
          "secure_fs::write_bytes_atomic(&keys_file, &envelope)?;",
          "secure_fs::write_bytes_atomic(&master_key_path, &envelope)?;",
          "secure_fs::write_string_atomic(&sig_path, &json)?;",
        ],
      },
      {
        path: "crates/abigail-core/src/secrets.rs",
        markers: [
          "secure_fs::write_bytes_atomic(&self.file_path, &envelope)?;",
        ],
      },
      {
        path: "crates/abigail-core/src/vault/mod.rs",
        markers: [
          "secure_fs::write_bytes_atomic(&sentinel_path(data_root), &envelope)?;",
        ],
      },
    ],
  },
  {
    id: "recovery-export",
    description: "Recovery export defaults to an encrypted bundle, with plaintext only as an explicit advanced path.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "Default encrypted recovery bundle; optional plaintext export only as explicit advanced action",
          "`save_recovery_key`, `save_recovery_key_plaintext`, `crypto_audit.log`",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Recovery bundle export is encrypted by default; plaintext export is opt-in and explicit.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-identity/src/lib.rs",
        markers: [
          "RECOVERY_BUNDLE.abigail-recovery",
          "pub fn save_recovery_key(",
          "pub fn save_recovery_key_plaintext(",
          "append_crypto_audit(",
        ],
      },
      {
        path: "tauri-app/src-ui/src/components/BootSequence.tsx",
        markers: [
          "Save encrypted recovery bundle",
          "Save plaintext key (advanced)",
          "window.confirm(",
        ],
      },
      {
        path: "tauri-app/src/commands/identity.rs",
        markers: [
          "pub fn save_recovery_key_plaintext(",
        ],
      },
    ],
  },
  {
    id: "archive-v2",
    description: "Portable archives use v2 with HKDF-derived AEAD keys and authenticated headers while keeping v1 restore support.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "v2 uses X25519 + HKDF-SHA256 + AES-256-GCM with AAD-bound headers; v1 remains read-only compatible",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Portable archive v2 derives the AEAD key from the X25519 shared secret with HKDF-SHA256.",
          "Archive headers are authenticated as AES-GCM additional authenticated data.",
          "Archive v1 remains readable for compatibility only.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Portable archive v2 uses X25519 + HKDF-SHA256 + AES-256-GCM with authenticated headers.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-memory/src/archive.rs",
        markers: [
          "const ARCHIVE_VERSION_V2: u32 = 2;",
          "struct ArchiveHeaderV2",
          "Hkdf::<Sha256>::new",
          "Payload {",
          "aad: &out",
          "aad: &data[..header_end]",
          "fn decrypt_archive_v1(",
        ],
      },
    ],
  },
  {
    id: "skill-signers",
    description: "Signed skill allowlists are canonicalized, rotatable, and fail closed.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "Signed skill allowlist",
          "`trusted_skill_signers`",
          "trusted signer management commands",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Active `signed_skill_allowlist` entries are verified with Ed25519 against canonicalized `trusted_skill_signers`.",
          "Invalid signatures, malformed signer keys, and untrusted signers fail closed.",
          "Trusted signer rotation and removal are exposed through dedicated Tauri commands and refresh the runtime policy immediately.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Signed skill allowlist entries are verified against `trusted_skill_signers` and fail closed on malformed or untrusted input.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-skills/src/policy.rs",
        markers: [
          "pub fn normalize_trusted_signer_key",
          "trusted_signer_rotation_accepts_any_active_trusted_signer",
          "invalid_trusted_signer_configuration_fails_closed",
        ],
      },
      {
        path: "tauri-app/src/commands/skills.rs",
        markers: [
          "fn add_trusted_signer_entry",
          "fn remove_trusted_signer_entry",
          "pub fn list_trusted_skill_signers",
          "pub fn add_trusted_skill_signer",
          "pub fn remove_trusted_skill_signer",
        ],
      },
    ],
  },
  {
    id: "removed-email-secret-path",
    description: "Legacy email credentials are explicitly deprecated instead of remaining as an unsupported secret path.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "Legacy email password fields",
          "Compatibility-only tombstones in config schema",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "The legacy email transport has been removed from mainline Abigail.",
          "Legacy `password_encrypted` config fields remain only for compatibility loads and forward migration safety; they are not an active secret-storage path.",
        ],
      },
    ],
    implementation: [
      {
        path: "crates/abigail-core/src/ops.rs",
        markers: [
          "Compatibility tombstone for removed IMAP/SMTP email transport.",
          "Email transport removed from mainline Abigail.",
          "test_set_email_config_returns_removed_error",
        ],
      },
      {
        path: "crates/abigail-core/src/config.rs",
        markers: [
          "pub password_encrypted: Vec<u8>,",
        ],
      },
    ],
  },
  {
    id: "release-signing",
    description: "Updater verification and platform signing requirements are enforced in release automation.",
    docs: [
      {
        path: "documents/CRYPTO_INVENTORY.md",
        markers: [
          "`scripts/enforce_release_prereqs.sh` blocks published release builds",
          "`scripts/prepare_tauri_bundle_config.mjs` injects the updater verification public key",
          "`scripts/generate_tauri_latest_manifest.mjs` emits `latest.json`",
        ],
      },
      {
        path: "documents/SECURITY_NOTES.md",
        markers: [
          "Release builds inject the updater verification public key at build time.",
          "Published updater metadata is generated from the signed updater artifacts attached to the release.",
          "Official release workflows hard-fail without updater signing inputs.",
          "Official Windows releases require Authenticode inputs.",
          "Official macOS releases require Developer ID signing and notarization inputs.",
        ],
      },
      {
        path: ".github/SECURITY.md",
        markers: [
          "Official release workflows require updater signing inputs, Windows code signing inputs, and macOS signing/notarization inputs.",
        ],
      },
    ],
    implementation: [
      {
        path: "scripts/enforce_release_prereqs.sh",
        markers: [
          "require_var TAURI_SIGNING_PRIVATE_KEY",
          "require_var TAURI_UPDATER_PUBKEY",
          "require_var WINDOWS_SIGNING_CERT_BASE64",
          "require_var APPLE_CERTIFICATE",
        ],
      },
      {
        path: "scripts/prepare_tauri_bundle_config.mjs",
        markers: [
          "TAURI_UPDATER_PUBKEY",
          "createUpdaterArtifacts",
          "config.bundle.windows.certificateThumbprint",
          "config.bundle.windows.timestampUrl",
        ],
      },
      {
        path: "scripts/generate_tauri_latest_manifest.mjs",
        markers: [
          "latest.json",
          "windows-x86_64-nsis",
          "darwin-aarch64-app",
        ],
      },
      {
        path: ".github/workflows/release.yml",
        markers: [
          "Enforce release signing prerequisites",
          "Configure updater and signing fields in tauri.conf.json",
          "Assert updater config injection",
          "Verify updater artifacts",
        ],
      },
      {
        path: ".github/workflows/release-fast.yml",
        markers: [
          "Configure updater and signing fields in tauri.conf.json",
          "Validate updater signing key",
          "Verify updater artifacts",
        ],
      },
    ],
  },
];

const missing = [];

for (const claim of claims) {
  for (const doc of claim.docs) {
    const contents = read(doc.path);
    for (const marker of doc.markers) {
      assertIncludes(contents, marker, doc.path, claim.id, missing);
    }
  }

  for (const implementation of claim.implementation) {
    const contents = read(implementation.path);
    for (const marker of implementation.markers) {
      assertIncludes(contents, marker, implementation.path, claim.id, missing);
    }
  }
}

if (missing.length > 0) {
  console.error("Cryptography claims check failed.\n");
  for (const item of missing) {
    console.error(`- ${item}`);
  }
  process.exit(1);
}

for (const claim of claims) {
  console.log(`ok  ${claim.id}  ${claim.description}`);
}

console.log(`\nVerified ${claims.length} cryptography/security claims against docs and implementation markers.`);
