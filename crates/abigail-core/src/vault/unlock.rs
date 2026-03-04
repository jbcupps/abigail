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

const KEK_LEN: usize = 32;
const KEYRING_SERVICE: &str = "com.abigail.vault";
const KEYRING_ACCOUNT: &str = "root-kek";
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
        // 1. Try OS credential store
        match os_keyring_load() {
            Ok(kek) => return Ok(kek),
            Err(e) => {
                tracing::debug!("OS keyring load failed (will try alternatives): {}", e);
            }
        }

        // 2. Try passphrase from environment
        if let Ok(passphrase) = std::env::var(PASSPHRASE_ENV) {
            if !passphrase.is_empty() {
                tracing::info!("Using vault passphrase from {} env var", PASSPHRASE_ENV);
                return Ok(super::crypto::derive_key_from_passphrase(
                    &passphrase,
                    PASSPHRASE_SALT,
                ));
            }
        }

        // 3. Bootstrap: generate a fresh KEK and try to persist it
        tracing::info!("No vault KEK found — bootstrapping new root key");
        let kek = generate_random_kek();
        match os_keyring_store(&kek) {
            Ok(()) => {
                tracing::info!("Root KEK stored in OS credential store");
            }
            Err(e) => {
                tracing::warn!(
                    "Could not store KEK in OS credential store: {}. \
                     Set {} to use passphrase-based vault unlock.",
                    e,
                    PASSPHRASE_ENV
                );
            }
        }
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
