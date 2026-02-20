use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tracing::debug;

use abigail_core::SecretsVault;

use crate::error::{AuthError, Result};
use crate::provider::AuthProvider;
use crate::types::Credential;
use crate::ui::AuthUI;

/// Provider that reads a static token from SecretsVault and returns
/// a Bearer Authorization header.
///
/// This wraps the existing `{{secret:key}}` pattern used by skills.
pub struct StaticTokenProvider {
    /// The key to look up in SecretsVault.
    secret_key: String,
    /// Shared reference to the vault.
    vault: Arc<Mutex<SecretsVault>>,
}

impl StaticTokenProvider {
    pub fn new(secret_key: String, vault: Arc<Mutex<SecretsVault>>) -> Self {
        Self { secret_key, vault }
    }
}

#[async_trait]
impl AuthProvider for StaticTokenProvider {
    fn name(&self) -> &str {
        "Static Token"
    }

    async fn resolve(&self, service_id: &str, _ui: Option<&dyn AuthUI>) -> Result<Credential> {
        let vault = self
            .vault
            .lock()
            .map_err(|e| AuthError::Vault(format!("failed to lock vault: {}", e)))?;

        match vault.get_secret(&self.secret_key) {
            Some(token) => {
                debug!(service = service_id, "resolved static token credential");
                Ok(Credential::bearer(token))
            }
            None => Err(AuthError::SecretNotFound(self.secret_key.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_token_resolve() {
        let tmp = std::env::temp_dir().join("abigail_auth_static_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        vault.set_secret("github_token", "ghp_test123");
        let vault = Arc::new(Mutex::new(vault));

        let provider = StaticTokenProvider::new("github_token".to_string(), vault);
        let cred = provider.resolve("github", None).await.unwrap();

        assert_eq!(cred.header_name, "Authorization");
        assert_eq!(cred.header_value, "Bearer ghp_test123");
        assert!(!cred.is_expired());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_static_token_missing_secret() {
        let tmp = std::env::temp_dir().join("abigail_auth_static_missing");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vault = SecretsVault::new(tmp.clone());
        let vault = Arc::new(Mutex::new(vault));

        let provider = StaticTokenProvider::new("nonexistent".to_string(), vault);
        let err = provider.resolve("test", None).await.unwrap_err();

        assert!(matches!(err, AuthError::SecretNotFound(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
