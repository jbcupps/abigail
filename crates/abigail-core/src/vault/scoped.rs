//! Scoped encrypted vault for secrets storage.
//!
//! Each `ScopedVault` uses a scope-derived encryption key (via HKDF from the
//! root KEK) so that Hive secrets, entity secrets, and skills secrets are
//! cryptographically isolated even though they may share the same root key.
//!
//! File format: the versioned AES-256-GCM envelope from [`super::crypto`].
//! Payload: JSON-serialised `HashMap<String, String>`.

use super::crypto;
use super::unlock::UnlockProvider;
use crate::error::{CoreError, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Labels that feed into HKDF `info` to produce scope-specific DEKs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultScope {
    /// Hive-level shared secrets (API keys visible to all entities).
    Hive,
    /// Per-entity secrets.
    Entity(String),
    /// Skills operational secrets (browser fallback sessions, Jira tokens, etc.).
    Skills,
    /// Arbitrary custom scope (for tests or future use).
    Custom(String),
}

impl VaultScope {
    fn label(&self) -> String {
        match self {
            Self::Hive => "hive".to_string(),
            Self::Entity(id) => format!("entity:{}", id),
            Self::Skills => "skills".to_string(),
            Self::Custom(s) => s.clone(),
        }
    }
}

/// Encrypted key-value vault scoped to a specific purpose.
///
/// Thread-safe: wrap in `Arc<Mutex<ScopedVault>>` for shared access (same
/// pattern as the legacy `SecretsVault`).
pub struct ScopedVault {
    file_path: PathBuf,
    scope: VaultScope,
    unlock: Arc<dyn UnlockProvider>,
    secrets: HashMap<String, String>,
}

impl ScopedVault {
    /// Open (or create) a scoped vault backed by `file_path`.
    ///
    /// If the file exists it is decrypted with the scope-derived key.
    /// If the file does not exist, the vault starts empty.
    pub fn open(
        file_path: PathBuf,
        scope: VaultScope,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        let secrets = if file_path.exists() {
            let root_kek = unlock.root_kek()?;
            let dek = crypto::derive_scope_key(&root_kek, &scope.label());
            let envelope = std::fs::read(&file_path)?;
            let plaintext = crypto::open(&dek, &envelope)?;
            serde_json::from_slice(&plaintext)
                .map_err(|e| CoreError::Keyring(format!("vault payload parse failed: {}", e)))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            file_path,
            scope,
            unlock,
            secrets,
        })
    }

    /// Persist the current in-memory secrets to the encrypted file.
    pub fn save(&self) -> Result<()> {
        let root_kek = self.unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, &self.scope.label());
        let plaintext = serde_json::to_vec(&self.secrets)?;
        let envelope = crypto::seal(&dek, &plaintext)?;

        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.file_path, envelope)?;
        Ok(())
    }

    pub fn get_secret(&self, key: &str) -> Option<&str> {
        self.secrets.get(key).map(|s| s.as_str())
    }

    pub fn set_secret(&mut self, key: &str, value: &str) {
        self.secrets.insert(key.to_string(), value.to_string());
    }

    pub fn remove_secret(&mut self, key: &str) -> bool {
        self.secrets.remove(key).is_some()
    }

    pub fn exists(&self, key: &str) -> bool {
        self.secrets.contains_key(key)
    }

    pub fn list_keys(&self) -> Vec<&str> {
        self.secrets.keys().map(|s| s.as_str()).collect()
    }

    /// Number of stored secrets.
    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Delete the backing file and clear in-memory secrets.
    pub fn reset(&mut self) -> Result<()> {
        self.secrets.clear();
        if self.file_path.exists() {
            std::fs::remove_file(&self.file_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::unlock::PassphraseUnlockProvider;

    fn test_unlock() -> Arc<dyn UnlockProvider> {
        Arc::new(PassphraseUnlockProvider::new("test-passphrase"))
    }

    #[test]
    fn roundtrip_scoped_vault() {
        let tmp = std::env::temp_dir().join("abigail_scoped_vault_rt");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("test.vault");
        let unlock = test_unlock();

        {
            let mut v = ScopedVault::open(path.clone(), VaultScope::Hive, unlock.clone()).unwrap();
            v.set_secret("openai", "sk-abc");
            v.set_secret("anthropic", "sk-xyz");
            v.save().unwrap();
        }

        {
            let v = ScopedVault::open(path.clone(), VaultScope::Hive, unlock.clone()).unwrap();
            assert_eq!(v.get_secret("openai"), Some("sk-abc"));
            assert_eq!(v.get_secret("anthropic"), Some("sk-xyz"));
            assert_eq!(v.get_secret("missing"), None);
            assert_eq!(v.len(), 2);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn wrong_scope_cannot_decrypt() {
        let tmp = std::env::temp_dir().join("abigail_scoped_vault_scope");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("test.vault");
        let unlock = test_unlock();

        {
            let mut v = ScopedVault::open(path.clone(), VaultScope::Hive, unlock.clone()).unwrap();
            v.set_secret("key", "value");
            v.save().unwrap();
        }

        let result = ScopedVault::open(
            path.clone(),
            VaultScope::Entity("other".into()),
            unlock.clone(),
        );
        assert!(result.is_err(), "Different scope should fail to decrypt");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn remove_and_reset() {
        let tmp = std::env::temp_dir().join("abigail_scoped_vault_rm");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("test.vault");
        let unlock = test_unlock();

        let mut v = ScopedVault::open(path.clone(), VaultScope::Skills, unlock).unwrap();
        v.set_secret("a", "1");
        v.set_secret("b", "2");
        v.save().unwrap();

        assert!(v.remove_secret("a"));
        assert!(!v.remove_secret("a"));
        assert_eq!(v.len(), 1);

        v.reset().unwrap();
        assert!(v.is_empty());
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_vault_opens_without_file() {
        let tmp = std::env::temp_dir().join("abigail_scoped_vault_empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let path = tmp.join("nonexistent.vault");
        let unlock = test_unlock();
        let v = ScopedVault::open(path, VaultScope::Hive, unlock).unwrap();
        assert!(v.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
