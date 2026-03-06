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
use rand::RngCore;
use std::path::Path;

const KEK_LEN: usize = super::VAULT_KEK_LEN;
const KEYRING_SERVICE: &str = "com.abigail.vault";
const KEYRING_ACCOUNT: &str = "abigail.hive.master-kek-v1";
const PASSPHRASE_ENV: &str = "ABIGAIL_VAULT_PASSPHRASE";
const PASSPHRASE_SALT: &[u8] = b"abigail-vault-passphrase-salt-v1";

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

        if let Ok(passphrase) = std::env::var(PASSPHRASE_ENV) {
            if !passphrase.trim().is_empty() {
                let kek = super::crypto::derive_key_from_passphrase(&passphrase, PASSPHRASE_SALT);
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
}
