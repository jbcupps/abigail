use async_trait::async_trait;

use crate::error::Result;
use crate::types::Credential;
use crate::ui::AuthUI;

/// Trait implemented by each authentication provider.
///
/// Providers resolve credentials (tokens, API keys, etc.) for a specific
/// auth method. They may read from the SecretsVault, perform token exchanges,
/// or interact with the user via `AuthUI`.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Human-readable name for this provider (e.g. "Static Token", "OAuth2").
    fn name(&self) -> &str;

    /// Resolve a credential for the given service.
    ///
    /// `ui` is provided for interactive flows (OAuth consent, device codes).
    /// Non-interactive providers can ignore it.
    async fn resolve(&self, service_id: &str, ui: Option<&dyn AuthUI>) -> Result<Credential>;

    /// Attempt to refresh an expired credential.
    ///
    /// Default implementation returns `NotConfigured` — override for providers
    /// that support token refresh (OAuth2).
    async fn refresh(&self, service_id: &str, _ui: Option<&dyn AuthUI>) -> Result<Credential> {
        Err(crate::error::AuthError::NotConfigured(format!(
            "refresh not supported for service '{}'",
            service_id
        )))
    }

    /// Revoke/clear any stored state for this service.
    ///
    /// Default is a no-op.
    async fn revoke(&self, _service_id: &str) -> Result<()> {
        Ok(())
    }
}
