//! Shared operational helpers for Abigail.
//!
//! These functions are used by both the Tauri app and the CLI to avoid
//! duplicating credential storage and email configuration logic.

use crate::config::{AppConfig, EmailConfig};
use crate::keyring::Keyring;
use crate::secrets::SecretsVault;

/// Store a secret in the vault after validation.
pub fn store_vault_secret(vault: &mut SecretsVault, key: &str, value: &str) -> crate::Result<()> {
    if key.is_empty() {
        return Err(crate::CoreError::Config(
            "Secret key cannot be empty".into(),
        ));
    }
    if value.is_empty() {
        return Err(crate::CoreError::Config(
            "Secret value cannot be empty".into(),
        ));
    }
    vault.set_secret(key, value);
    vault.save()?;
    Ok(())
}

/// Check if a secret exists in the vault.
pub fn check_vault_secret(vault: &SecretsVault, key: &str) -> bool {
    vault.exists(key)
}

/// Get the current email configuration from AppConfig.
pub fn get_email_config(config: &AppConfig) -> Option<&EmailConfig> {
    config.email.as_ref()
}

/// Configure email credentials, encrypting the password via DPAPI.
pub fn set_email_config(
    config: &mut AppConfig,
    address: String,
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    password: &str,
) -> crate::Result<()> {
    let password_encrypted = Keyring::encrypt_bytes(password.as_bytes())?;
    config.email = Some(EmailConfig {
        address,
        imap_host,
        imap_port,
        smtp_host,
        smtp_port,
        password_encrypted,
    });
    config
        .save(&config.config_path())
        .map_err(|e| crate::CoreError::Config(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_store_vault_secret_rejects_empty_key() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_empty_key");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        let result = store_vault_secret(&mut vault, "", "value");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_store_vault_secret_rejects_empty_value() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_empty_val");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        let result = store_vault_secret(&mut vault, "key", "");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_store_and_check_vault_secret() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_roundtrip");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        store_vault_secret(&mut vault, "test_key", "test_value").unwrap();
        assert!(check_vault_secret(&vault, "test_key"));
        assert!(!check_vault_secret(&vault, "nonexistent"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_email_config_none() {
        let config = AppConfig::default_paths();
        assert!(get_email_config(&config).is_none());
    }
}
