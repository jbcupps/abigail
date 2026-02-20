use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::RwLock;
use tracing::{debug, warn};

use abigail_core::SecretsVault;

use crate::cache::TokenCache;
use crate::error::{AuthError, Result};
use crate::provider::AuthProvider;
use crate::providers::{BasicAuthProvider, StaticTokenProvider};
use crate::types::{AuthMethod, Credential, TokenInfo};
use crate::ui::AuthUI;

/// Central auth registry. Holds providers per service, a shared token cache,
/// and a reference to the SecretsVault for building providers.
pub struct AuthManager {
    providers: RwLock<HashMap<String, Arc<dyn AuthProvider>>>,
    cache: TokenCache,
    vault: Arc<Mutex<SecretsVault>>,
}

impl AuthManager {
    pub fn new(vault: Arc<Mutex<SecretsVault>>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            cache: TokenCache::new(),
            vault,
        }
    }

    /// Build an AuthProvider from an AuthMethod enum variant.
    ///
    /// Phase 1 supports StaticToken and BasicAuth. Other variants
    /// return NotConfigured.
    pub fn build_provider(&self, method: &AuthMethod) -> Result<Arc<dyn AuthProvider>> {
        match method {
            AuthMethod::StaticToken { secret_key } => Ok(Arc::new(StaticTokenProvider::new(
                secret_key.clone(),
                self.vault.clone(),
            ))),
            AuthMethod::BasicAuth {
                username_key,
                password_key,
            } => Ok(Arc::new(BasicAuthProvider::new(
                username_key.clone(),
                password_key.clone(),
                self.vault.clone(),
            ))),
            other => {
                let type_name = match other {
                    AuthMethod::OAuth2 { .. } => "oauth2",
                    AuthMethod::DeviceCode { .. } => "device_code",
                    AuthMethod::ApiKey { .. } => "api_key",
                    _ => "unknown",
                };
                Err(AuthError::NotConfigured(format!(
                    "auth method '{}' not yet implemented",
                    type_name
                )))
            }
        }
    }

    /// Register a provider for a service. If an `AuthMethod` is given,
    /// builds the provider automatically.
    pub async fn register_method(&self, service_id: &str, method: &AuthMethod) -> Result<()> {
        let provider = self.build_provider(method)?;
        self.register(service_id, provider).await;
        Ok(())
    }

    /// Blocking variant of `register_method` for use during sync startup.
    /// Builds the provider and inserts it directly using `blocking_write`.
    pub fn register_method_blocking(&self, service_id: &str, method: &AuthMethod) -> Result<()> {
        let provider = self.build_provider(method)?;
        debug!(
            service = service_id,
            provider = provider.name(),
            "registering auth provider (blocking)"
        );
        let mut providers = self.providers.blocking_write();
        providers.insert(service_id.to_string(), provider);
        Ok(())
    }

    /// Register a pre-built provider for a service.
    pub async fn register(&self, service_id: &str, provider: Arc<dyn AuthProvider>) {
        debug!(
            service = service_id,
            provider = provider.name(),
            "registering auth provider"
        );
        let mut providers = self.providers.write().await;
        providers.insert(service_id.to_string(), provider);
    }

    /// Unregister a provider and clear its cached token.
    pub async fn unregister(&self, service_id: &str) {
        let mut providers = self.providers.write().await;
        providers.remove(service_id);
        drop(providers);
        self.cache.remove(service_id).await;
    }

    /// List all registered service IDs.
    pub async fn list_services(&self) -> Vec<String> {
        let providers = self.providers.read().await;
        providers.keys().cloned().collect()
    }

    /// Resolve a credential for a service.
    ///
    /// 1. Check cache for a non-expired token.
    /// 2. If expired, try refresh.
    /// 3. If no cache hit, call the provider's resolve.
    /// 4. Cache the result.
    pub async fn resolve(&self, service_id: &str, ui: Option<&dyn AuthUI>) -> Result<Credential> {
        // 1. Check cache
        if let Some(info) = self.cache.get(service_id).await {
            debug!(service = service_id, "cache hit");
            return Ok(info.credential);
        }

        // 2. Get provider
        let provider = {
            let providers = self.providers.read().await;
            providers
                .get(service_id)
                .cloned()
                .ok_or_else(|| AuthError::ProviderNotFound(service_id.to_string()))?
        };

        // 3. Resolve
        let credential = provider.resolve(service_id, ui).await?;

        // 4. Cache
        self.cache
            .put(service_id, TokenInfo::new(credential.clone()))
            .await;

        Ok(credential)
    }

    /// Force-refresh a credential (bypass cache, call provider refresh or resolve).
    pub async fn refresh(&self, service_id: &str, ui: Option<&dyn AuthUI>) -> Result<Credential> {
        let provider = {
            let providers = self.providers.read().await;
            providers
                .get(service_id)
                .cloned()
                .ok_or_else(|| AuthError::ProviderNotFound(service_id.to_string()))?
        };

        // Try refresh first, fall back to resolve
        let credential = match provider.refresh(service_id, ui).await {
            Ok(cred) => cred,
            Err(_) => {
                warn!(
                    service = service_id,
                    "refresh not supported, falling back to resolve"
                );
                provider.resolve(service_id, ui).await?
            }
        };

        self.cache
            .put(service_id, TokenInfo::new(credential.clone()))
            .await;

        Ok(credential)
    }

    /// Clear cached token for a service.
    pub async fn clear_cache(&self, service_id: &str) {
        self.cache.remove(service_id).await;
    }

    /// Clear all cached tokens.
    pub async fn clear_all_cache(&self) {
        self.cache.clear().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vault(dir_name: &str) -> (Arc<Mutex<SecretsVault>>, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(dir_name);
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let vault = SecretsVault::new(tmp.clone());
        (Arc::new(Mutex::new(vault)), tmp)
    }

    #[tokio::test]
    async fn test_register_and_resolve_static_token() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_static");
        vault.lock().unwrap().set_secret("my_token", "tok_abc123");

        let manager = AuthManager::new(vault);
        manager
            .register_method(
                "my-service",
                &AuthMethod::StaticToken {
                    secret_key: "my_token".to_string(),
                },
            )
            .await
            .unwrap();

        let cred = manager.resolve("my-service", None).await.unwrap();
        assert_eq!(cred.header_value, "Bearer tok_abc123");

        // Second call should be a cache hit
        let cred2 = manager.resolve("my-service", None).await.unwrap();
        assert_eq!(cred2.header_value, "Bearer tok_abc123");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_register_and_resolve_basic_auth() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_basic");
        {
            let mut v = vault.lock().unwrap();
            v.set_secret("svc_user", "alice");
            v.set_secret("svc_pass", "p@ssw0rd");
        }

        let manager = AuthManager::new(vault);
        manager
            .register_method(
                "my-api",
                &AuthMethod::BasicAuth {
                    username_key: "svc_user".to_string(),
                    password_key: "svc_pass".to_string(),
                },
            )
            .await
            .unwrap();

        let cred = manager.resolve("my-api", None).await.unwrap();
        assert_eq!(cred.header_name, "Authorization");
        assert!(cred.header_value.starts_with("Basic "));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_resolve_unknown_service() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_unknown");
        let manager = AuthManager::new(vault);

        let err = manager.resolve("nonexistent", None).await.unwrap_err();
        assert!(matches!(err, AuthError::ProviderNotFound(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_unregister_clears_cache() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_unreg");
        vault.lock().unwrap().set_secret("tok", "val");

        let manager = AuthManager::new(vault);
        manager
            .register_method(
                "svc",
                &AuthMethod::StaticToken {
                    secret_key: "tok".to_string(),
                },
            )
            .await
            .unwrap();

        // Populate cache
        manager.resolve("svc", None).await.unwrap();

        // Unregister
        manager.unregister("svc").await;

        let err = manager.resolve("svc", None).await.unwrap_err();
        assert!(matches!(err, AuthError::ProviderNotFound(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_list_services() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_list");
        vault.lock().unwrap().set_secret("a", "1");
        vault.lock().unwrap().set_secret("b", "2");

        let manager = AuthManager::new(vault);
        manager
            .register_method(
                "svc-a",
                &AuthMethod::StaticToken {
                    secret_key: "a".to_string(),
                },
            )
            .await
            .unwrap();
        manager
            .register_method(
                "svc-b",
                &AuthMethod::StaticToken {
                    secret_key: "b".to_string(),
                },
            )
            .await
            .unwrap();

        let mut services = manager.list_services().await;
        services.sort();
        assert_eq!(services, vec!["svc-a", "svc-b"]);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_unsupported_method() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_unsupported");
        let manager = AuthManager::new(vault);

        let err = manager
            .register_method(
                "oauth-svc",
                &AuthMethod::OAuth2 {
                    client_id: "id".to_string(),
                    auth_url: "https://example.com/auth".to_string(),
                    token_url: "https://example.com/token".to_string(),
                    scopes: vec!["read".to_string()],
                    client_secret_key: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::NotConfigured(_)));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_bootstrap_pattern() {
        // Simulates the startup bootstrap: register providers before secrets exist,
        // then store secrets and verify resolve works.
        let (vault, tmp) = test_vault("abigail_auth_mgr_bootstrap");
        let manager = AuthManager::new(vault.clone());

        // Register providers (secrets don't exist yet — registration succeeds,
        // but resolve would fail when trying to read the secret)
        manager
            .register_method(
                "github",
                &AuthMethod::StaticToken {
                    secret_key: "github_token".to_string(),
                },
            )
            .await
            .unwrap();

        // Simulate user storing credentials later
        vault
            .lock()
            .unwrap()
            .set_secret("github_token", "ghp_test123");

        // Clear cache and re-register
        manager.clear_cache("github").await;
        manager
            .register_method(
                "github",
                &AuthMethod::StaticToken {
                    secret_key: "github_token".to_string(),
                },
            )
            .await
            .unwrap();

        // Now resolve should work
        let cred = manager.resolve("github", None).await.unwrap();
        assert_eq!(cred.header_value, "Bearer ghp_test123");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_register_method_blocking() {
        let (vault, tmp) = test_vault("abigail_auth_mgr_blocking");
        vault
            .lock()
            .unwrap()
            .set_secret("my_tok", "tok_blocking_test");

        let manager = AuthManager::new(vault);
        manager
            .register_method_blocking(
                "svc-blocking",
                &AuthMethod::StaticToken {
                    secret_key: "my_tok".to_string(),
                },
            )
            .unwrap();

        // Verify via async resolve using a runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        let cred = rt.block_on(manager.resolve("svc-blocking", None)).unwrap();
        assert_eq!(cred.header_value, "Bearer tok_blocking_test");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
