use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How a service authenticates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Bearer token from SecretsVault (existing `{{secret:key}}` pattern).
    StaticToken {
        /// Key name in SecretsVault.
        secret_key: String,
    },
    /// HTTP Basic Auth from two vault keys.
    BasicAuth {
        /// Vault key for the username.
        username_key: String,
        /// Vault key for the password.
        password_key: String,
    },
    /// OAuth2 Authorization Code (Phase 3).
    OAuth2 {
        client_id: String,
        auth_url: String,
        token_url: String,
        scopes: Vec<String>,
        #[serde(default)]
        client_secret_key: Option<String>,
    },
    /// OAuth2 Device Code flow (Phase 3).
    DeviceCode {
        client_id: String,
        device_auth_url: String,
        token_url: String,
        scopes: Vec<String>,
    },
    /// API key passed as a query parameter or custom header (Phase 2+).
    ApiKey {
        secret_key: String,
        header_name: String,
    },
}

/// A resolved credential ready to attach to an HTTP request.
#[derive(Debug, Clone)]
pub struct Credential {
    /// HTTP header name (e.g. "Authorization").
    pub header_name: String,
    /// HTTP header value (e.g. "Bearer sk-...").
    pub header_value: String,
    /// When this credential expires, if known.
    pub expires_at: Option<DateTime<Utc>>,
}

impl Credential {
    /// Create a Bearer token credential.
    pub fn bearer(token: &str) -> Self {
        Self {
            header_name: "Authorization".to_string(),
            header_value: format!("Bearer {}", token),
            expires_at: None,
        }
    }

    /// Create a Basic auth credential from username and password.
    pub fn basic(username: &str, password: &str) -> Self {
        use base64::Engine;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        Self {
            header_name: "Authorization".to_string(),
            header_value: format!("Basic {}", encoded),
            expires_at: None,
        }
    }

    /// Create a credential with a custom header.
    pub fn custom_header(name: &str, value: &str) -> Self {
        Self {
            header_name: name.to_string(),
            header_value: value.to_string(),
            expires_at: None,
        }
    }

    /// Whether this credential has expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => Utc::now() >= exp,
            None => false,
        }
    }
}

/// Per-service auth configuration stored in config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAuthConfig {
    /// Unique service identifier (e.g. "github", "google-calendar").
    pub service_id: String,
    /// How this service authenticates.
    pub method: AuthMethod,
}

/// Cached token entry stored in TokenCache.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub credential: Credential,
    pub cached_at: DateTime<Utc>,
}

impl TokenInfo {
    pub fn new(credential: Credential) -> Self {
        Self {
            credential,
            cached_at: Utc::now(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.credential.is_expired()
    }
}
