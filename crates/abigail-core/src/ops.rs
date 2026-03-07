//! Shared operational helpers for Abigail.
//!
//! These functions are used by both the Tauri app and the CLI to avoid
//! duplicating credential storage and email configuration logic.

use crate::config::AppConfig;
use crate::secrets::SecretsVault;

/// Provider names that are always accepted as secret keys (no skill manifest needed).
pub const RESERVED_PROVIDER_KEYS: &[&str] = &[
    "openai",
    "anthropic",
    "xai",
    "google",
    "tavily",
    "perplexity",
];

/// Validate that a secret key is non-empty and its value is non-empty.
/// Does NOT enforce namespace validation (that requires the skill registry).
pub fn validate_secret_basic(key: &str, value: &str) -> crate::Result<()> {
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
    Ok(())
}

/// Check whether a key is in the reserved provider namespace.
pub fn is_reserved_provider_key(key: &str) -> bool {
    RESERVED_PROVIDER_KEYS.contains(&key)
}

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

/// Compatibility tombstone for removed IMAP/SMTP email transport.
pub fn set_email_config(
    _config: &mut AppConfig,
    _address: String,
    _imap_host: String,
    _imap_port: u16,
    _smtp_host: String,
    _smtp_port: u16,
    _password: &str,
) -> crate::Result<()> {
    Err(crate::CoreError::Config(
        "Email transport removed from mainline Abigail. Use Browser skill fallback instead.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::test_vault;
    use std::fs;

    #[test]
    fn test_store_vault_secret_rejects_empty_key() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_empty_key");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = test_vault(tmp.clone());
        let result = store_vault_secret(&mut vault, "", "value");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_store_vault_secret_rejects_empty_value() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_empty_val");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = test_vault(tmp.clone());
        let result = store_vault_secret(&mut vault, "key", "");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_store_and_check_vault_secret() {
        let tmp = std::env::temp_dir().join("abigail_ops_test_roundtrip");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut vault = test_vault(tmp.clone());
        store_vault_secret(&mut vault, "test_key", "test_value").unwrap();
        assert!(check_vault_secret(&vault, "test_key"));
        assert!(!check_vault_secret(&vault, "nonexistent"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_set_email_config_returns_removed_error() {
        let mut config = AppConfig::default_paths();
        let err = set_email_config(
            &mut config,
            "mentor@example.com".into(),
            "imap.example.com".into(),
            993,
            "smtp.example.com".into(),
            587,
            "secret",
        )
        .expect_err("email transport should be removed");
        assert!(err.to_string().contains("removed from mainline Abigail"));
    }
}
