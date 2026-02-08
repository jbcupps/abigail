use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Signature verification failed for {document}")]
    SignatureInvalid { document: String },

    #[error("Document not found: {0}")]
    DocumentNotFound(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
