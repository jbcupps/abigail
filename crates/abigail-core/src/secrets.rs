//! SecretsVault: cross-platform encrypted storage for provider API keys.
//!
//! Separate from Keyring (identity keys) and ExternalVault (pubkey verification).
//!
//! **New format (v2):** Uses the versioned AES-256-GCM envelope from `vault::crypto`,
//! unlocked via `vault::unlock::HybridUnlockProvider` (OS keychain + passphrase fallback).
//! File extension: `.vault` (e.g. `secrets.vault`).
//!
//! **Legacy format:** DPAPI-encrypted JSON blob (`.bin`). Detected and rejected with
//! explicit remediation instructions (fresh-start policy — no auto-migration).

use crate::error::{CoreError, Result};
use crate::vault::crypto;
use crate::vault::unlock::{HybridUnlockProvider, PassphraseUnlockProvider, UnlockProvider};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

const VAULT_EXT: &str = "vault";

/// Cross-platform encrypted storage for provider API keys and other secrets.
///
/// Uses AES-256-GCM with HKDF-scoped keys derived from a root KEK obtained
/// via the OS credential store or passphrase fallback.
pub struct SecretsVault {
    file_path: PathBuf,
    scope_label: String,
    unlock: Arc<dyn UnlockProvider>,
    secrets: HashMap<String, String>,
}

impl SecretsVault {
    /// Create a new empty vault at `{data_dir}/{filename}` using the hybrid unlock provider.
    /// The file extension is forced to `.vault`.
    pub fn new_custom(data_dir: PathBuf, filename: &str) -> Self {
        let vault_filename = force_vault_extension(filename);
        Self {
            file_path: data_dir.join(&vault_filename),
            scope_label: scope_from_filename(&vault_filename),
            unlock: Arc::new(HybridUnlockProvider::new()),
            secrets: HashMap::new(),
        }
    }

    /// Create a new empty vault with the default `secrets.vault`.
    pub fn new(data_dir: PathBuf) -> Self {
        Self::new_custom(data_dir, "secrets.vault")
    }

    /// Load secrets from encrypted storage. Uses the hybrid unlock provider.
    pub fn load(data_dir: PathBuf) -> Result<Self> {
        Self::load_custom(data_dir, "secrets.vault")
    }

    /// Load secrets with a custom filename. The extension is forced to `.vault`.
    pub fn load_custom(data_dir: PathBuf, filename: &str) -> Result<Self> {
        let vault_filename = force_vault_extension(filename);
        let file_path = data_dir.join(&vault_filename);
        let unlock: Arc<dyn UnlockProvider> = Arc::new(HybridUnlockProvider::new());

        // Check for legacy `.bin` files and give a clear error
        let legacy_bin = data_dir.join(filename.replace(".vault", ".bin"));
        if !file_path.exists() && legacy_bin.exists() {
            tracing::warn!(
                "Legacy DPAPI vault detected at {}. \
                 The new cross-platform vault format uses {}. \
                 Re-enter your secrets to populate the new vault.",
                legacy_bin.display(),
                file_path.display()
            );
        }

        let secrets = if file_path.exists() {
            let root_kek = unlock.root_kek()?;
            let scope = scope_from_filename(&vault_filename);
            let dek = crypto::derive_scope_key(&root_kek, &scope);
            let envelope = std::fs::read(&file_path)?;
            let plaintext = crypto::open(&dek, &envelope)?;
            serde_json::from_slice(&plaintext)
                .map_err(|e| CoreError::Keyring(format!("vault payload parse failed: {}", e)))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            file_path,
            scope_label: scope_from_filename(&vault_filename),
            unlock,
            secrets,
        })
    }

    /// Open a vault with an explicit unlock provider (for tests or daemon mode).
    pub fn open_with_provider(
        data_dir: PathBuf,
        filename: &str,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        let vault_filename = force_vault_extension(filename);
        let file_path = data_dir.join(&vault_filename);
        let scope = scope_from_filename(&vault_filename);

        let secrets = if file_path.exists() {
            let root_kek = unlock.root_kek()?;
            let dek = crypto::derive_scope_key(&root_kek, &scope);
            let envelope = std::fs::read(&file_path)?;
            let plaintext = crypto::open(&dek, &envelope)?;
            serde_json::from_slice(&plaintext)
                .map_err(|e| CoreError::Keyring(format!("vault payload parse failed: {}", e)))?
        } else {
            HashMap::new()
        };

        Ok(Self {
            file_path,
            scope_label: scope,
            unlock,
            secrets,
        })
    }

    /// Persist secrets to the encrypted vault file.
    pub fn save(&self) -> Result<()> {
        let root_kek = self.unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, &self.scope_label);
        let plaintext = serde_json::to_vec(&self.secrets)?;
        let envelope = crypto::seal(&dek, &plaintext)?;

        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.file_path, envelope)?;
        Ok(())
    }

    /// Get a secret by provider name.
    pub fn get_secret(&self, provider: &str) -> Option<&str> {
        self.secrets.get(provider).map(|s| s.as_str())
    }

    /// Set a secret for a provider. Call `save()` to persist.
    pub fn set_secret(&mut self, provider: &str, key: &str) {
        self.secrets.insert(provider.to_string(), key.to_string());
    }

    /// Remove a secret. Call `save()` to persist.
    pub fn remove_secret(&mut self, provider: &str) -> bool {
        self.secrets.remove(provider).is_some()
    }

    /// Returns true if a secret exists for the given key.
    pub fn exists(&self, key: &str) -> bool {
        self.secrets.contains_key(key)
    }

    /// List all provider names that have stored secrets.
    pub fn list_providers(&self) -> Vec<&str> {
        self.secrets.keys().map(|s| s.as_str()).collect()
    }
}

fn force_vault_extension(filename: &str) -> String {
    let stem = filename
        .strip_suffix(".bin")
        .or_else(|| filename.strip_suffix(".vault"))
        .unwrap_or(filename);
    format!("{}.{}", stem, VAULT_EXT)
}

fn scope_from_filename(filename: &str) -> String {
    let stem = filename.strip_suffix(".vault").unwrap_or(filename);
    format!("secrets:{}", stem)
}

/// Create a test-friendly vault using the passphrase provider (no OS keychain needed).
pub fn test_vault(data_dir: PathBuf) -> SecretsVault {
    let unlock: Arc<dyn UnlockProvider> =
        Arc::new(PassphraseUnlockProvider::new("test-passphrase"));
    SecretsVault {
        file_path: data_dir.join("secrets.vault"),
        scope_label: "secrets:secrets".to_string(),
        unlock,
        secrets: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_test_vault(dir: &Path) -> SecretsVault {
        test_vault(dir.to_path_buf())
    }

    #[test]
    fn test_secrets_vault_roundtrip() {
        let tmp = std::env::temp_dir().join("abigail_secrets_v2_test_rt");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = make_test_vault(&tmp);
        vault.set_secret("openai", "sk-test-key-123");
        vault.set_secret("anthropic", "sk-ant-key-456");
        vault.save().unwrap();

        let loaded = SecretsVault::open_with_provider(
            tmp.clone(),
            "secrets",
            Arc::new(PassphraseUnlockProvider::new("test-passphrase")),
        )
        .unwrap();
        assert_eq!(loaded.get_secret("openai"), Some("sk-test-key-123"));
        assert_eq!(loaded.get_secret("anthropic"), Some("sk-ant-key-456"));
        assert_eq!(loaded.get_secret("missing"), None);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_secrets_vault_empty_load() {
        let tmp = std::env::temp_dir().join("abigail_secrets_v2_test_empty");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vault = SecretsVault::open_with_provider(
            tmp.clone(),
            "secrets",
            Arc::new(PassphraseUnlockProvider::new("test-passphrase")),
        )
        .unwrap();
        assert!(vault.list_providers().is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_secrets_vault_remove() {
        let tmp = std::env::temp_dir().join("abigail_secrets_v2_test_rm");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = make_test_vault(&tmp);
        vault.set_secret("openai", "sk-test");
        assert!(vault.remove_secret("openai"));
        assert!(!vault.remove_secret("openai"));
        assert_eq!(vault.get_secret("openai"), None);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_secrets_vault_list_providers() {
        let tmp = std::env::temp_dir().join("abigail_secrets_v2_test_lp");
        let _ = std::fs::remove_dir_all(&tmp);

        let mut vault = make_test_vault(&tmp);
        vault.set_secret("openai", "key1");
        vault.set_secret("anthropic", "key2");

        let mut providers = vault.list_providers();
        providers.sort();
        assert_eq!(providers, vec!["anthropic", "openai"]);
    }

    #[test]
    fn test_force_vault_extension() {
        assert_eq!(force_vault_extension("secrets.bin"), "secrets.vault");
        assert_eq!(force_vault_extension("secrets.vault"), "secrets.vault");
        assert_eq!(force_vault_extension("skills"), "skills.vault");
        assert_eq!(force_vault_extension("custom.bin"), "custom.vault");
    }
}
