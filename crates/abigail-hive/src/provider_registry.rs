//! Provider construction registry.
//!
//! All concrete LLM provider instantiation is centralised here so that neither
//! the router nor the Tauri app need to know how to build providers from raw
//! API keys.

use abigail_capabilities::cognitive::{
    AnthropicProvider, CandleProvider, CliLlmProvider, CliVariant, CompatibleProvider, LlmProvider,
    LocalHttpProvider, OpenAiCompatibleProvider, OpenAiProvider,
};
use std::sync::Arc;

/// Which cloud provider backs the Ego slot (mirrors `EgoProvider` in the router).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Perplexity,
    Xai,
    Google,
    ClaudeCli,
    GeminiCli,
    CodexCli,
    GrokCli,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::OpenAi => write!(f, "openai"),
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Perplexity => write!(f, "perplexity"),
            ProviderKind::Xai => write!(f, "xai"),
            ProviderKind::Google => write!(f, "google"),
            ProviderKind::ClaudeCli => write!(f, "claude-cli"),
            ProviderKind::GeminiCli => write!(f, "gemini-cli"),
            ProviderKind::CodexCli => write!(f, "codex-cli"),
            ProviderKind::GrokCli => write!(f, "grok-cli"),
        }
    }
}

/// Result of building an Ego (cloud) provider.
pub struct EgoProviderResult {
    pub provider: Option<Arc<dyn LlmProvider>>,
    pub kind: Option<ProviderKind>,
}

/// Result of building an Id (local) provider.
pub struct IdProviderResult {
    pub provider: Arc<dyn LlmProvider>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
}

/// Stateless registry that knows how to construct every provider variant.
pub struct ProviderRegistry;

impl ProviderRegistry {
    /// Build an Ego (cloud) LLM provider from a provider name, API key, and optional model.
    pub fn build_ego(
        provider_name: Option<&str>,
        api_key: Option<String>,
        ego_model: Option<String>,
    ) -> EgoProviderResult {
        let key = match api_key.filter(|k| !k.trim().is_empty()) {
            Some(k) => k,
            None => {
                tracing::debug!("build_ego: no API key provided for {:?}", provider_name);
                return EgoProviderResult {
                    provider: None,
                    kind: None,
                };
            }
        };

        tracing::info!(
            "Initializing Ego provider: {:?} with model: {:?}",
            provider_name,
            ego_model
        );

        match provider_name {
            Some("openai") => {
                let built = OpenAiProvider::with_model(
                    Some(key),
                    ego_model.unwrap_or_else(|| "gpt-4o-mini".to_string()),
                )
                .inspect_err(|e| tracing::error!("Failed to build OpenAI provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::OpenAi),
                    provider: built,
                }
            }
            Some("anthropic") => {
                let built = AnthropicProvider::with_model(
                    key,
                    ego_model.unwrap_or_else(|| "claude-sonnet-4-20250514".to_string()),
                )
                .inspect_err(|e| tracing::error!("Failed to build Anthropic provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::Anthropic),
                    provider: built,
                }
            }
            Some("perplexity") => {
                let built = OpenAiCompatibleProvider::with_config(
                    CompatibleProvider::Perplexity,
                    CompatibleProvider::Perplexity.base_url().to_string(),
                    key,
                    ego_model.unwrap_or_else(|| {
                        CompatibleProvider::Perplexity.default_model().to_string()
                    }),
                )
                .inspect_err(|e| tracing::error!("Failed to build Perplexity provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::Perplexity),
                    provider: built,
                }
            }
            Some("xai") => {
                let built = OpenAiCompatibleProvider::with_config(
                    CompatibleProvider::Xai,
                    CompatibleProvider::Xai.base_url().to_string(),
                    key,
                    ego_model
                        .unwrap_or_else(|| CompatibleProvider::Xai.default_model().to_string()),
                )
                .inspect_err(|e| tracing::error!("Failed to build xAI provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::Xai),
                    provider: built,
                }
            }
            Some("google") => {
                let built = OpenAiCompatibleProvider::with_config(
                    CompatibleProvider::Google,
                    CompatibleProvider::Google.base_url().to_string(),
                    key,
                    ego_model
                        .unwrap_or_else(|| CompatibleProvider::Google.default_model().to_string()),
                )
                .inspect_err(|e| tracing::error!("Failed to build Google provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::Google),
                    provider: built,
                }
            }
            Some("claude-cli") => {
                let built = CliLlmProvider::new(CliVariant::ClaudeCode, key)
                    .inspect_err(|e| tracing::error!("Failed to build Claude CLI provider: {}", e))
                    .ok()
                    .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::ClaudeCli),
                    provider: built,
                }
            }
            Some("gemini-cli") => {
                let built = CliLlmProvider::new(CliVariant::GeminiCli, key)
                    .inspect_err(|e| tracing::error!("Failed to build Gemini CLI provider: {}", e))
                    .ok()
                    .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::GeminiCli),
                    provider: built,
                }
            }
            Some("codex-cli") => {
                let built = CliLlmProvider::new(CliVariant::OpenAiCodex, key)
                    .inspect_err(|e| tracing::error!("Failed to build Codex CLI provider: {}", e))
                    .ok()
                    .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::CodexCli),
                    provider: built,
                }
            }
            Some("grok-cli") => {
                let built = CliLlmProvider::new(CliVariant::XaiGrokCli, key)
                    .inspect_err(|e| tracing::error!("Failed to build Grok CLI provider: {}", e))
                    .ok()
                    .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::GrokCli),
                    provider: built,
                }
            }
            _ => {
                tracing::debug!("build_ego: unknown provider name {:?}", provider_name);
                EgoProviderResult {
                    provider: None,
                    kind: None,
                }
            }
        }
    }

    /// Build an Id (local) provider from an optional LLM base URL.
    pub fn build_id(local_llm_base_url: Option<String>) -> IdProviderResult {
        if let Some(url) = local_llm_base_url.filter(|u| !u.trim().is_empty()) {
            if let Ok(p) = LocalHttpProvider::with_url(url) {
                let p = Arc::new(p);
                return IdProviderResult {
                    provider: p.clone() as Arc<dyn LlmProvider>,
                    local_http: Some(p),
                };
            }
        }
        IdProviderResult {
            provider: Arc::new(CandleProvider::new()),
            local_http: None,
        }
    }

    /// Build an Id (local) provider with auto-detected model name.
    pub async fn build_id_auto_detect(local_llm_base_url: Option<String>) -> IdProviderResult {
        if let Some(url) = local_llm_base_url.filter(|u| !u.trim().is_empty()) {
            if let Ok(p) = LocalHttpProvider::with_url_auto_model(url).await {
                let p = Arc::new(p);
                return IdProviderResult {
                    provider: p.clone() as Arc<dyn LlmProvider>,
                    local_http: Some(p),
                };
            }
        }
        IdProviderResult {
            provider: Arc::new(CandleProvider::new()),
            local_http: None,
        }
    }

    /// Build a Superego safety provider from a provider name and key.
    pub fn build_superego(provider: &str, key: &str) -> Arc<dyn LlmProvider> {
        let fallback = || match OpenAiProvider::new(Some(key.to_string())) {
            Ok(p) => Arc::new(p) as Arc<dyn LlmProvider>,
            Err(e) => {
                tracing::error!(
                    "Failed to create OpenAI fallback provider for Superego: {}",
                    e
                );
                Arc::new(CandleProvider::new()) as Arc<dyn LlmProvider>
            }
        };
        match provider {
            "anthropic" => match AnthropicProvider::new(key.to_string()) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    tracing::error!("Failed to create Anthropic provider: {}", e);
                    fallback()
                }
            },
            _ => fallback(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ego_openai() {
        let r = ProviderRegistry::build_ego(Some("openai"), Some("test-key".to_string()), None);
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::OpenAi));
    }

    #[test]
    fn build_ego_anthropic() {
        let r = ProviderRegistry::build_ego(Some("anthropic"), Some("test-key".to_string()), None);
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::Anthropic));
    }

    #[test]
    fn build_ego_perplexity() {
        let r = ProviderRegistry::build_ego(Some("perplexity"), Some("test-key".to_string()), None);
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::Perplexity));
    }

    #[test]
    fn build_ego_xai() {
        let r = ProviderRegistry::build_ego(Some("xai"), Some("test-key".to_string()), None);
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::Xai));
    }

    #[test]
    fn build_ego_google() {
        let r = ProviderRegistry::build_ego(Some("google"), Some("test-key".to_string()), None);
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::Google));
    }

    #[test]
    fn build_ego_empty_key_returns_none() {
        let r = ProviderRegistry::build_ego(Some("openai"), Some("".to_string()), None);
        assert!(r.provider.is_none());
        assert!(r.kind.is_none());
    }

    #[test]
    fn build_ego_whitespace_key_returns_none() {
        let r = ProviderRegistry::build_ego(Some("openai"), Some("   ".to_string()), None);
        assert!(r.provider.is_none());
        assert!(r.kind.is_none());
    }

    #[test]
    fn build_ego_no_key_returns_none() {
        let r = ProviderRegistry::build_ego(Some("openai"), None, None);
        assert!(r.provider.is_none());
        assert!(r.kind.is_none());
    }

    #[test]
    fn build_ego_unknown_provider_returns_none() {
        let r =
            ProviderRegistry::build_ego(Some("unknown-provider"), Some("key".to_string()), None);
        assert!(r.provider.is_none());
        assert!(r.kind.is_none());
    }

    #[test]
    fn build_ego_none_provider_returns_none() {
        let r = ProviderRegistry::build_ego(None, Some("key".to_string()), None);
        assert!(r.provider.is_none());
        assert!(r.kind.is_none());
    }

    #[test]
    fn build_ego_with_custom_model() {
        let r = ProviderRegistry::build_ego(
            Some("openai"),
            Some("test-key".to_string()),
            Some("gpt-4-turbo".to_string()),
        );
        assert!(r.provider.is_some());
        assert_eq!(r.kind, Some(ProviderKind::OpenAi));
    }

    #[test]
    fn build_id_no_url_returns_candle() {
        let r = ProviderRegistry::build_id(None);
        assert!(r.local_http.is_none());
    }

    #[test]
    fn build_id_empty_url_returns_candle() {
        let r = ProviderRegistry::build_id(Some("".to_string()));
        assert!(r.local_http.is_none());
    }

    #[test]
    fn build_id_whitespace_url_returns_candle() {
        let r = ProviderRegistry::build_id(Some("   ".to_string()));
        assert!(r.local_http.is_none());
    }

    #[test]
    fn build_id_valid_url() {
        let r = ProviderRegistry::build_id(Some("http://localhost:1234".to_string()));
        assert!(r.local_http.is_some());
    }

    #[tokio::test]
    async fn build_id_auto_detect_no_url() {
        let r = ProviderRegistry::build_id_auto_detect(None).await;
        assert!(r.local_http.is_none());
    }

    #[test]
    fn build_superego_anthropic() {
        let p = ProviderRegistry::build_superego("anthropic", "test-key");
        // Should succeed (provider is constructed)
        let _ = p;
    }

    #[test]
    fn build_superego_openai_fallback() {
        let p = ProviderRegistry::build_superego("openai", "test-key");
        let _ = p;
    }

    #[test]
    fn build_superego_unknown_uses_openai_fallback() {
        let p = ProviderRegistry::build_superego("unknown", "test-key");
        let _ = p;
    }

    #[test]
    fn provider_kind_display() {
        assert_eq!(ProviderKind::OpenAi.to_string(), "openai");
        assert_eq!(ProviderKind::Anthropic.to_string(), "anthropic");
        assert_eq!(ProviderKind::Perplexity.to_string(), "perplexity");
        assert_eq!(ProviderKind::Xai.to_string(), "xai");
        assert_eq!(ProviderKind::Google.to_string(), "google");
        assert_eq!(ProviderKind::ClaudeCli.to_string(), "claude-cli");
        assert_eq!(ProviderKind::GeminiCli.to_string(), "gemini-cli");
        assert_eq!(ProviderKind::CodexCli.to_string(), "codex-cli");
        assert_eq!(ProviderKind::GrokCli.to_string(), "grok-cli");
    }

    #[test]
    fn build_ego_cli_providers() {
        for (name, expected) in [
            ("claude-cli", ProviderKind::ClaudeCli),
            ("gemini-cli", ProviderKind::GeminiCli),
            ("codex-cli", ProviderKind::CodexCli),
            ("grok-cli", ProviderKind::GrokCli),
        ] {
            let r = ProviderRegistry::build_ego(Some(name), Some("test-key".to_string()), None);
            assert!(r.provider.is_some(), "provider should be Some for {}", name);
            assert_eq!(r.kind, Some(expected), "kind mismatch for {}", name);
        }
    }

    #[test]
    fn build_ego_from_env_provider_keys() {
        let provider_envs = [
            ("openai", "OPENAI_API_KEY", ProviderKind::OpenAi),
            ("anthropic", "ANTHROPIC_API_KEY", ProviderKind::Anthropic),
            ("xai", "XAI_API_KEY", ProviderKind::Xai),
            ("google", "GOOGLE_API_KEY", ProviderKind::Google),
            ("perplexity", "PERPLEXITY_API_KEY", ProviderKind::Perplexity),
        ];

        let mut tested = 0usize;
        for (provider_name, env_var, expected_kind) in provider_envs {
            let Ok(key) = std::env::var(env_var) else {
                continue;
            };
            if key.trim().is_empty() {
                continue;
            }

            tested += 1;
            let r = ProviderRegistry::build_ego(Some(provider_name), Some(key), None);
            assert!(
                r.provider.is_some(),
                "expected provider to build for '{}' using {}",
                provider_name,
                env_var
            );
            assert_eq!(
                r.kind,
                Some(expected_kind),
                "expected provider kind {:?} for {}",
                expected_kind,
                provider_name
            );
        }

        assert!(
            tested > 0,
            "no provider keys detected in environment; load .env.e2e.local before running this test"
        );
    }
}
