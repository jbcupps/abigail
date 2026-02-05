pub mod config;
pub mod document;
pub mod dpapi;
pub mod error;
pub mod keyring;
pub mod secrets;
pub mod superego;
pub mod system_prompt;
pub mod templates;
pub mod vault;
pub mod verifier;

pub use config::{AppConfig, EmailConfig, RoutingMode, TrinityConfig};
pub use document::{CoreDocument, DocumentTier};
pub use error::{CoreError, Result};
pub use keyring::{
    generate_external_keypair, parse_private_key, sign_constitutional_documents,
    sign_document, ExternalKeypairResult, Keyring, SignatureMetadata,
};
pub use secrets::SecretsVault;
pub use vault::{ExternalVault, ReadOnlyFileVault};
pub use verifier::{write_sig_file, Verifier};
