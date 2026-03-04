pub mod capability_envelope;
pub mod config;
pub mod document;
pub mod dpapi;
pub mod encrypted_storage;
pub mod error;
pub mod global_config;
pub mod key_detection;
pub mod keyring;
pub mod local_llm_url;
pub mod ops;
pub mod sao_bridge;
pub mod secrets;
pub mod structured_failure;
pub mod system_prompt;
pub mod templates;
pub mod vault;
pub mod verifier;

pub use capability_envelope::{
    evaluate_gate, CapabilityEnvelope, CapabilityGateResult, RequestedCapability,
};
pub use config::{
    AppConfig, CliPermissionMode, EmailAccountConfig, EmailConfig, McpServerDefinition,
    McpTrustPolicy, ProviderCatalogEntry, RoutingMode, RuntimeMode, TrinityConfig,
    CONFIG_SCHEMA_VERSION,
};
pub use document::{CoreDocument, DocumentTier};
pub use error::{CoreError, Result};
pub use global_config::{AgentEntry, GlobalConfig};
pub use key_detection::redact_secrets;
pub use keyring::{
    generate_external_keypair, generate_master_key, load_master_key, parse_private_key,
    sign_agent_key, sign_constitutional_documents, sign_document, verify_agent_signature,
    ExternalKeypairResult, Keyring, MasterKeyResult, SignatureMetadata,
};
pub use local_llm_url::validate_local_llm_url;
pub use ops::{is_reserved_provider_key, RESERVED_PROVIDER_KEYS};
pub use sao_bridge::{AgentState, SaoBridgeClient, SaoBridgeError};
pub use secrets::{test_vault, SecretsVault};
pub use structured_failure::StructuredFailure;
pub use vault::external::{ExternalVault, ReadOnlyFileVault};
pub use vault::scoped::{ScopedVault, VaultScope};
pub use vault::unlock::{HybridUnlockProvider, PassphraseUnlockProvider, UnlockProvider};
pub use verifier::{write_sig_file, Verifier};
