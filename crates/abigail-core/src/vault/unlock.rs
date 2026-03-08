//! Hybrid unlock providers for obtaining the root Key Encryption Key (KEK).
//!
//! Priority order:
//! 1. **OS credential store** (Windows Credential Manager / macOS Keychain / Linux Secret Service)
//! 2. **Passphrase** via `ABIGAIL_VAULT_PASSPHRASE` env var or explicit input
//! 3. **DPAPI** (Windows-only legacy path, read-only for migration)
//!
//! On first run, the provider generates a random 32-byte KEK and stores it in
//! the OS credential store. Subsequent runs retrieve it.

use crate::error::{CoreError, Result};
use crate::secure_fs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEK_LEN: usize = super::VAULT_KEK_LEN;
const KEYRING_SERVICE: &str = "com.abigail.vault";
const KEYRING_ACCOUNT: &str = "abigail.hive.master-kek-v1";
const PASSPHRASE_ENV: &str = "ABIGAIL_VAULT_PASSPHRASE";
const RAW_KEK_ENV: &str = "ABIGAIL_VAULT_RAW_KEY";
const PASSPHRASE_SALT: &[u8] = b"abigail-vault-passphrase-salt-v1";
const KDF_METADATA_FILE: &str = "vault.kdf.json";
const ARGON2_MEMORY_COST_KIB: u32 = 64 * 1024;
const ARGON2_TIME_COST: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_SALT_LEN: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PassphraseKdfMetadata {
    version: u8,
    algorithm: String,
    salt_base64: Option<String>,
    memory_cost_kib: Option<u32>,
    time_cost: Option<u32>,
    parallelism: Option<u32>,
}

impl PassphraseKdfMetadata {
    fn fresh_argon2() -> Self {
        let mut salt = [0u8; ARGON2_SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        Self {
            version: 1,
            algorithm: "argon2id".to_string(),
            salt_base64: Some(base64::engine::general_purpose::STANDARD.encode(salt)),
            memory_cost_kib: Some(ARGON2_MEMORY_COST_KIB),
            time_cost: Some(ARGON2_TIME_COST),
            parallelism: Some(ARGON2_PARALLELISM),
        }
    }

    fn legacy_hkdf() -> Self {
        Self {
            version: 1,
            algorithm: "legacy_hkdf_v1".to_string(),
            salt_base64: None,
            memory_cost_kib: None,
            time_cost: None,
            parallelism: None,
        }
    }
}

/// Trait for obtaining the root KEK that protects all vault data.
pub trait UnlockProvider: Send + Sync {
    /// Return the 32-byte root KEK, generating or bootstrapping if needed.
    fn root_kek(&self) -> Result<[u8; KEK_LEN]>;
}

/// Tries OS credential store first, then passphrase env var fallback.
pub struct HybridUnlockProvider;

impl HybridUnlockProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HybridUnlockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl UnlockProvider for HybridUnlockProvider {
    fn root_kek(&self) -> Result<[u8; KEK_LEN]> {
        if let Some(kek) = super::cached_session_root_kek() {
            return Ok(kek);
        }

        let data_root = crate::AppConfig::default_paths().data_dir;
        std::fs::create_dir_all(&data_root)?;
        let sentinel_path = super::sentinel_path(&data_root);
        let has_sentinel = sentinel_path.exists();

        // paper Sections 22-27 runtime verification:
        // stable KEK + sentinel check must succeed before runtime unlock is accepted.
        if let Some(kek) = os_keyring_load_optional()? {
            let sentinel_value = verify_or_create_sentinel(&data_root, &kek)?;
            super::cache_session_root_kek(kek, sentinel_value);
            return Ok(kek);
        }

        if let Ok(raw_secret) = std::env::var(RAW_KEK_ENV) {
            if !raw_secret.trim().is_empty() {
                let kek = decode_raw_kek(&raw_secret)?;
                let sentinel_value = verify_or_create_sentinel(&data_root, &kek)?;
                super::cache_session_root_kek(kek, sentinel_value);
                return Ok(kek);
            }
        }

        if let Ok(passphrase) = std::env::var(PASSPHRASE_ENV) {
            if !passphrase.trim().is_empty() {
                let kek = derive_passphrase_kek(&data_root, &passphrase, has_sentinel)?;
                let sentinel_value = verify_or_create_sentinel(&data_root, &kek)?;
                super::cache_session_root_kek(kek, sentinel_value);
                return Ok(kek);
            }
        }

        if has_sentinel {
            return Err(CoreError::Vault(
                "Recovery Mode: vault sentinel exists but stable KEK could not be loaded. \
                 Refusing to auto-create a new key to avoid wrong-key AES-GCM corruption."
                    .to_string(),
            ));
        }

        // First boot only (no sentinel exists): create stable KEK once.
        tracing::info!("No stable vault KEK found; bootstrapping initial root key");
        let kek = generate_random_kek();
        os_keyring_store(&kek).map_err(|e| {
            CoreError::Vault(format!(
                "Recovery Mode: failed to persist stable KEK '{}': {}",
                KEYRING_ACCOUNT, e
            ))
        })?;
        let sentinel_value = super::write_encrypted_sentinel(&data_root, &kek)?;
        super::cache_session_root_kek(kek, sentinel_value);
        Ok(kek)
    }
}

/// Provider that always derives the KEK from a fixed passphrase.
/// Used in tests and headless/daemon environments.
pub struct PassphraseUnlockProvider {
    passphrase: String,
}

impl PassphraseUnlockProvider {
    pub fn new(passphrase: impl Into<String>) -> Self {
        Self {
            passphrase: passphrase.into(),
        }
    }
}

impl UnlockProvider for PassphraseUnlockProvider {
    fn root_kek(&self) -> Result<[u8; KEK_LEN]> {
        Ok(super::crypto::derive_key_from_passphrase(
            &self.passphrase,
            PASSPHRASE_SALT,
        ))
    }
}

fn generate_random_kek() -> [u8; KEK_LEN] {
    let mut kek = [0u8; KEK_LEN];
    rand::rngs::OsRng.fill_bytes(&mut kek);
    kek
}

fn derive_passphrase_kek(
    data_root: &Path,
    passphrase: &str,
    has_sentinel: bool,
) -> Result<[u8; KEK_LEN]> {
    if let Some(metadata) = load_kdf_metadata(data_root)? {
        return derive_with_kdf_metadata(passphrase, &metadata);
    }

    if has_sentinel {
        let legacy = PassphraseKdfMetadata::legacy_hkdf();
        let kek = derive_with_kdf_metadata(passphrase, &legacy)?;
        super::decrypt_sentinel(data_root, &kek).map_err(|e| {
            CoreError::Vault(format!(
                "Recovery Mode: passphrase metadata is missing and legacy HKDF compatibility could not decrypt {}: {}",
                super::sentinel_path(data_root).display(),
                e
            ))
        })?;
        save_kdf_metadata(data_root, &legacy)?;
        return Ok(kek);
    }

    let metadata = PassphraseKdfMetadata::fresh_argon2();
    save_kdf_metadata(data_root, &metadata)?;
    derive_with_kdf_metadata(passphrase, &metadata)
}

fn derive_with_kdf_metadata(
    passphrase: &str,
    metadata: &PassphraseKdfMetadata,
) -> Result<[u8; KEK_LEN]> {
    match metadata.algorithm.as_str() {
        "argon2id" => {
            let salt = decode_metadata_salt(metadata)?;
            super::crypto::derive_key_from_passphrase_argon2(
                passphrase,
                &salt,
                metadata.memory_cost_kib.unwrap_or(ARGON2_MEMORY_COST_KIB),
                metadata.time_cost.unwrap_or(ARGON2_TIME_COST),
                metadata.parallelism.unwrap_or(ARGON2_PARALLELISM),
            )
        }
        "legacy_hkdf_v1" => Ok(super::crypto::derive_key_from_passphrase(
            passphrase,
            PASSPHRASE_SALT,
        )),
        other => Err(CoreError::Vault(format!(
            "Unsupported vault KDF algorithm '{}' in {}",
            other, KDF_METADATA_FILE
        ))),
    }
}

fn decode_metadata_salt(metadata: &PassphraseKdfMetadata) -> Result<Vec<u8>> {
    let encoded = metadata.salt_base64.as_ref().ok_or_else(|| {
        CoreError::Vault(format!(
            "Vault KDF metadata '{}' is missing a salt for Argon2id",
            KDF_METADATA_FILE
        ))
    })?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| CoreError::Vault(format!("Invalid KDF metadata salt: {}", e)))
}

fn kdf_metadata_path(data_root: &Path) -> PathBuf {
    data_root.join(KDF_METADATA_FILE)
}

fn load_kdf_metadata(data_root: &Path) -> Result<Option<PassphraseKdfMetadata>> {
    let path = kdf_metadata_path(data_root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let metadata = serde_json::from_str(&content).map_err(|e| {
        CoreError::Vault(format!("Invalid KDF metadata at {}: {}", path.display(), e))
    })?;
    Ok(Some(metadata))
}

fn save_kdf_metadata(data_root: &Path, metadata: &PassphraseKdfMetadata) -> Result<()> {
    let path = kdf_metadata_path(data_root);
    let content = serde_json::to_string_pretty(metadata)?;
    secure_fs::write_string_atomic(&path, &content)
}

fn decode_raw_kek(raw_secret: &str) -> Result<[u8; KEK_LEN]> {
    let trimmed = raw_secret.trim();
    let bytes = if trimmed.len() == KEK_LEN * 2 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = Vec::with_capacity(KEK_LEN);
        for idx in (0..trimmed.len()).step_by(2) {
            let byte = u8::from_str_radix(&trimmed[idx..idx + 2], 16).map_err(|e| {
                CoreError::Vault(format!("Invalid hex raw KEK in {}: {}", RAW_KEK_ENV, e))
            })?;
            out.push(byte);
        }
        out
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .map_err(|e| {
                CoreError::Vault(format!("Invalid base64 raw KEK in {}: {}", RAW_KEK_ENV, e))
            })?
    };

    let kek: [u8; KEK_LEN] = bytes.try_into().map_err(|_| {
        CoreError::Vault(format!(
            "{} must decode to exactly {} bytes",
            RAW_KEK_ENV, KEK_LEN
        ))
    })?;
    Ok(kek)
}

// ---------------------------------------------------------------------------
// OS credential store helpers
// ---------------------------------------------------------------------------

fn os_keyring_load() -> Result<[u8; KEK_LEN]> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| CoreError::Keyring(format!("keyring entry creation failed: {}", e)))?;
    let password = entry
        .get_password()
        .map_err(|e| CoreError::Keyring(format!("keyring get failed: {}", e)))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&password)
        .map_err(|e| CoreError::Keyring(format!("keyring value decode failed: {}", e)))?;
    let kek: [u8; KEK_LEN] = bytes
        .try_into()
        .map_err(|_| CoreError::Keyring("keyring value wrong length".into()))?;
    Ok(kek)
}

fn os_keyring_load_optional() -> Result<Option<[u8; KEK_LEN]>> {
    match os_keyring_load() {
        Ok(kek) => Ok(Some(kek)),
        Err(e) => {
            let msg = e.to_string().to_ascii_lowercase();
            if msg.contains("no entry")
                || msg.contains("no matching entry")
                || msg.contains("not found")
                || msg.contains("no password")
                || msg.contains("platform secure storage")
            {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

fn os_keyring_store(kek: &[u8; KEK_LEN]) -> Result<()> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(kek);
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| CoreError::Keyring(format!("keyring entry creation failed: {}", e)))?;
    entry
        .set_password(&encoded)
        .map_err(|e| CoreError::Keyring(format!("keyring store failed: {}", e)))?;
    Ok(())
}

fn verify_or_create_sentinel(data_root: &Path, kek: &[u8; KEK_LEN]) -> Result<String> {
    let sentinel_path = super::sentinel_path(data_root);
    if sentinel_path.exists() {
        super::decrypt_sentinel(data_root, kek).map_err(|e| {
            CoreError::Vault(format!(
                "Recovery Mode: failed to decrypt {} with stable KEK '{}': {}",
                sentinel_path.display(),
                KEYRING_ACCOUNT,
                e
            ))
        })
    } else {
        super::write_encrypted_sentinel(data_root, kek)
    }
}

use base64::Engine as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_provider_deterministic() {
        let p = PassphraseUnlockProvider::new("test-phrase");
        let k1 = p.root_kek().unwrap();
        let k2 = p.root_kek().unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_passphrases_different_keks() {
        let a = PassphraseUnlockProvider::new("alpha");
        let b = PassphraseUnlockProvider::new("beta");
        assert_ne!(a.root_kek().unwrap(), b.root_kek().unwrap());
    }

    #[test]
    fn fresh_passphrase_bootstrap_creates_argon2_metadata() {
        let dir = std::env::temp_dir().join("abigail_unlock_argon2_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let kek = derive_passphrase_kek(&dir, "strong-passphrase", false).unwrap();
        assert_ne!(kek, [0u8; KEK_LEN]);

        let metadata = load_kdf_metadata(&dir).unwrap().unwrap();
        assert_eq!(metadata.algorithm, "argon2id");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_passphrase_without_metadata_remains_readable() {
        let dir = std::env::temp_dir().join("abigail_unlock_legacy_metadata");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let legacy_kek =
            super::super::crypto::derive_key_from_passphrase("legacy-pass", PASSPHRASE_SALT);
        super::super::write_encrypted_sentinel(&dir, &legacy_kek).unwrap();

        let derived = derive_passphrase_kek(&dir, "legacy-pass", true).unwrap();
        assert_eq!(derived, legacy_kek);

        let metadata = load_kdf_metadata(&dir).unwrap().unwrap();
        assert_eq!(metadata.algorithm, "legacy_hkdf_v1");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
