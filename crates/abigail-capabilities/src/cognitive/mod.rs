//! Cognitive capabilities — LLM providers, reasoning, classification.

pub mod anthropic;
pub mod candle;
pub mod cli_provider;
pub mod download;
pub mod local_http;
pub mod openai;
pub mod openai_compatible;
pub mod provider;
pub mod types;
pub mod validation;

pub use abigail_core::CliPermissionMode;
pub use anthropic::AnthropicProvider;
pub use candle::CandleProvider;
pub use cli_provider::{
    detect_all_cli_providers, CliDetectionResult, CliLlmProvider, CliVariant, ALL_CLI_VARIANTS,
};
pub use download::ModelDownloader;
pub use local_http::{stub_heartbeat, LocalHttpProvider};
pub use openai::OpenAiProvider;
pub use openai_compatible::{CompatibleProvider, OpenAiCompatibleProvider};
pub use provider::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, StreamEvent, ToolCall,
    ToolDefinition,
};
pub use types::*;
pub use validation::ModelInfo;

use abigail_core::secrets::SecretsVault;

/// Update a provider API key in the secure vault.
/// Called when the LLM recognizes the user is providing an API key.
pub fn update_provider_key(
    secrets: &mut SecretsVault,
    provider: &str,
    key: &str,
) -> anyhow::Result<()> {
    secrets.set_secret(provider, key);
    secrets.save()?;
    Ok(())
}

/// Function schema for LLM tool-calling: describes the update_provider_key function
/// so the LLM can recognize and call it when the user provides an API key.
pub fn update_provider_key_schema() -> serde_json::Value {
    serde_json::json!({
        "name": "update_provider_key",
        "description": "Store or update an API key for an LLM provider in the secure vault. Use when the user provides an API key.",
        "parameters": {
            "type": "object",
            "properties": {
                "provider": {
                    "type": "string",
                    "description": "Provider name, e.g. 'openai', 'anthropic'",
                    "enum": ["openai", "anthropic", "perplexity", "xai", "google", "mistral", "claude-cli", "gemini-cli", "codex-cli", "grok-cli"]
                },
                "key": {
                    "type": "string",
                    "description": "The API key to store"
                }
            },
            "required": ["provider", "key"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_provider_key() {
        let tmp = std::env::temp_dir().join("abigail_cap_provider_key_v2");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let unlock: std::sync::Arc<dyn abigail_core::UnlockProvider> =
            std::sync::Arc::new(abigail_core::PassphraseUnlockProvider::new("test"));
        let mut vault =
            SecretsVault::open_with_provider(tmp.clone(), "secrets", unlock.clone()).unwrap();
        update_provider_key(&mut vault, "openai", "sk-test-key-123").unwrap();

        let loaded =
            SecretsVault::open_with_provider(tmp.clone(), "secrets", unlock).unwrap();
        assert_eq!(loaded.get_secret("openai"), Some("sk-test-key-123"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_update_provider_key_overwrite() {
        let tmp = std::env::temp_dir().join("abigail_cap_provider_key_overwrite_v2");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let unlock: std::sync::Arc<dyn abigail_core::UnlockProvider> =
            std::sync::Arc::new(abigail_core::PassphraseUnlockProvider::new("test"));
        let mut vault =
            SecretsVault::open_with_provider(tmp.clone(), "secrets", unlock.clone()).unwrap();
        update_provider_key(&mut vault, "openai", "sk-old-key").unwrap();
        update_provider_key(&mut vault, "openai", "sk-new-key").unwrap();

        let loaded =
            SecretsVault::open_with_provider(tmp.clone(), "secrets", unlock).unwrap();
        assert_eq!(loaded.get_secret("openai"), Some("sk-new-key"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_update_provider_key_schema_valid() {
        let schema = update_provider_key_schema();
        assert_eq!(schema["name"], "update_provider_key");
        assert!(schema["parameters"]["properties"]["provider"].is_object());
        assert!(schema["parameters"]["properties"]["key"].is_object());
    }
}
