use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tracing::debug;

use abigail_core::SecretsVault;

use crate::error::{AuthError, Result};
use crate::provider::AuthProvider;
use crate::types::Credential;
use crate::ui::AuthUI;

/// Provider that reads username and password from SecretsVault and returns
/// a Base64-encoded HTTP Basic Authorization header.
pub struct BasicAuthProvider {
    /// Vault key for the username.
    username_key: String,
    /// Vault key for the password.
    password_key: String,
    /// Shared reference to the vault.
    vault: Arc<Mutex<SecretsVault>>,
}

impl BasicAuthProvider {
    pub fn new(
        username_key: String,
        password_key: String,
        vault: Arc<Mutex<SecretsVault>>,
    ) -> Self {
        Self {
            username_key,
            password_key,
            vault,
        }
    }
}

#[async_trait]
impl AuthProvider for BasicAuthProvider {
    fn name(&self) -> &str {
        "Basic Auth"
    }

    async fn resolve(&self, service_id: &str, _ui: Option<&dyn AuthUI>) -> Result<Credential> {
        let vault = self
            .vault
            .lock()
            .map_err(|e| AuthError::Vault(format!("failed to lock vault: {}", e)))?;

        let username = vault
            .get_secret(&self.username_key)
            .ok_or_else(|| AuthError::SecretNotFound(self.username_key.clone()))?;

        let password = vault
            .get_secret(&self.password_key)
            .ok_or_else(|| AuthError::SecretNotFound(self.password_key.clone()))?;

        debug!(service = service_id, "resolved basic auth credential");
        Ok(Credential::basic(username, password))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_auth_resolve() {
        let tmp = std::env::temp_dir().join("abigail_auth_basic_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        vault.set_secret("jira_user", "admin");
        vault.set_secret("jira_pass", "hunter2");
        let vault = Arc::new(Mutex::new(vault));

        let provider =
            BasicAuthProvider::new("jira_user".to_string(), "jira_pass".to_string(), vault);
        let cred = provider.resolve("jira", None).await.unwrap();

        assert_eq!(cred.header_name, "Authorization");
        // "admin:hunter2" in base64 = "YWRtaW46aHVudGVyMg=="
        assert_eq!(cred.header_value, "Basic YWRtaW46aHVudGVyMg==");
        assert!(!cred.is_expired());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_basic_auth_missing_username() {
        let tmp = std::env::temp_dir().join("abigail_auth_basic_nouser");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        vault.set_secret("pass", "secret");
        let vault = Arc::new(Mutex::new(vault));

        let provider = BasicAuthProvider::new("user".to_string(), "pass".to_string(), vault);
        let err = provider.resolve("test", None).await.unwrap_err();

        assert!(matches!(err, AuthError::SecretNotFound(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_basic_auth_missing_password() {
        let tmp = std::env::temp_dir().join("abigail_auth_basic_nopass");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut vault = SecretsVault::new(tmp.clone());
        vault.set_secret("user", "admin");
        let vault = Arc::new(Mutex::new(vault));

        let provider = BasicAuthProvider::new("user".to_string(), "pass".to_string(), vault);
        let err = provider.resolve("test", None).await.unwrap_err();

        assert!(matches!(err, AuthError::SecretNotFound(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
