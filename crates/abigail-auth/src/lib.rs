//! Authentication framework for Abigail.
//!
//! Provides a trait-based auth provider system, an in-memory token cache,
//! and an `AuthManager` registry that builds providers from `AuthMethod`
//! config and resolves credentials for HTTP requests.
//!
//! Phase 1 supports:
//! - `StaticToken` — Bearer token from SecretsVault
//! - `BasicAuth` — HTTP Basic from two vault keys

pub mod cache;
pub mod error;
pub mod manager;
pub mod provider;
pub mod providers;
pub mod types;
pub mod ui;

pub use cache::TokenCache;
pub use error::{AuthError, Result};
pub use manager::AuthManager;
pub use provider::AuthProvider;
pub use providers::{BasicAuthProvider, StaticTokenProvider};
pub use types::{AuthMethod, Credential, ServiceAuthConfig, TokenInfo};
pub use ui::{AuthUI, DeviceCodePrompt};
