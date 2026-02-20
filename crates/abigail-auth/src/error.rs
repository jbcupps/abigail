use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Auth provider not configured for service: {0}")]
    NotConfigured(String),

    #[error("Auth provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Secret not found in vault: {0}")]
    SecretNotFound(String),

    #[error("Token expired for service: {0}")]
    TokenExpired(String),

    #[error("Token refresh failed: {0}")]
    RefreshFailed(String),

    #[error("Invalid credential: {0}")]
    InvalidCredential(String),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("User interaction required: {0}")]
    InteractionRequired(String),

    #[error("Auth error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AuthError>;
