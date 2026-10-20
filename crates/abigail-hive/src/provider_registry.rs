//! Provider construction registry.
//!
//! All concrete LLM provider instantiation is centralised here so that neither
//! the router nor the Tauri app need to know how to build providers from raw
//! API keys.

use crate::hive::ProviderSelection;
use abigail_capabilities::cognitive::{
    AnthropicProvider, CandleProvider, CliLlmProvider, CliPermissionMode, CliVariant,
    CompatibleProvider, LlmProvider, LocalHttpProvider, OpenAiCompatibleProvider, OpenAiProvider,
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

// None means profile preflight failed: no provider may be constructed. false
// means a validated preview, which may use only explicitly supplied API keys
// and local URLs. CLI account adapters are not isolated for preview yet.
fn provider_access() -> Option<bool> {
    abigail_core::profile::current()
        .ok()
        .map(|profile| profile.is_none())
}

fn selection_allowed(selection: Option<&ProviderSelection>, access: Option<bool>) -> bool {
    match access {
        None => false,
        Some(true) => true,
        Some(false) => selection.is_some_and(|selection| {
            !matches!(
                selection.provider_name(),
                "claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli"
            ) && selection
                .api_key()
                .is_some_and(|key| !key.trim().is_empty())
        }),
    }
}

fn explicit_local_url(url: Option<String>, access: Option<bool>) -> Option<String> {
    access?;
    url.filter(|url| !url.trim().is_empty())
}

impl ProviderRegistry {
    /// Build an Ego (cloud) LLM provider from a provider name, API key, and optional model.
    ///
    /// CLI providers (`claude-cli`, `gemini-cli`, `codex-cli`, `grok-cli`) do not
    /// require an API key — they use the CLI tool's own authentication (e.g. OAuth
    /// via `claude auth login`).  When no key is provided for a CLI variant the
    /// sentinel value `"system"` is used, which tells `CliLlmProvider` to skip
    /// injecting an env-var and rely on the tool's built-in auth.
    pub fn build_ego(
        provider_name: Option<&str>,
        api_key: Option<String>,
        ego_model: Option<String>,
    ) -> EgoProviderResult {
        let selection = provider_name.map(|provider| ProviderSelection {
            provider: provider.to_string(),
            auth: api_key
                .filter(|key| !key.trim().is_empty())
                .map(crate::hive::ProviderAuth::ApiKey)
                .unwrap_or(crate::hive::ProviderAuth::System),
        });
        Self::build_ego_with_cli_mode(selection.as_ref(), ego_model, CliPermissionMode::default())
    }

    pub fn build_ego_with_cli_mode(
        selection: Option<&ProviderSelection>,
        ego_model: Option<String>,
        cli_permission_mode: CliPermissionMode,
    ) -> EgoProviderResult {
        if !selection_allowed(selection, provider_access()) {
            return EgoProviderResult {
                provider: None,
                kind: None,
            };
        }
        let provider_name = selection.map(|selection| selection.provider_name());
        let api_key = selection.and_then(|selection| selection.api_key());
        let is_cli = matches!(
            provider_name,
            Some("claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli")
        );

        let key = match api_key.filter(|k| !k.trim().is_empty()) {
            Some(k) => k,
            None if is_cli => "system".to_string(),
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
                    ego_model.unwrap_or_else(|| "gpt-4.1".to_string()),
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
                    ego_model.unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
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
                let built = CliLlmProvider::with_permission_mode(
                    CliVariant::ClaudeCode,
                    key,
                    cli_permission_mode,
                )
                .inspect_err(|e| tracing::error!("Failed to build Claude CLI provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::ClaudeCli),
                    provider: built,
                }
            }
            Some("gemini-cli") => {
                let built = CliLlmProvider::with_permission_mode(
                    CliVariant::GeminiCli,
                    key,
                    cli_permission_mode,
                )
                .inspect_err(|e| tracing::error!("Failed to build Gemini CLI provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::GeminiCli),
                    provider: built,
                }
            }
            Some("codex-cli") => {
                let built = CliLlmProvider::with_permission_mode(
                    CliVariant::OpenAiCodex,
                    key,
                    cli_permission_mode,
                )
                .inspect_err(|e| tracing::error!("Failed to build Codex CLI provider: {}", e))
                .ok()
                .map(|p| Arc::new(p) as Arc<dyn LlmProvider>);
                EgoProviderResult {
                    kind: built.as_ref().map(|_| ProviderKind::CodexCli),
                    provider: built,
                }
            }
            Some("grok-cli") => {
                let built = CliLlmProvider::with_permission_mode(
                    CliVariant::XaiGrokCli,
                    key,
                    cli_permission_mode,
                )
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
        if let Some(url) = explicit_local_url(local_llm_base_url, provider_access()) {
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
        if let Some(url) = explicit_local_url(local_llm_base_url, provider_access()) {
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
        let access = provider_access();
        if access.is_none() || (access == Some(false) && key.trim().is_empty()) {
            return Arc::new(CandleProvider::new());
        }
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
    fn preview_provider_registry_requires_explicit_key_and_disallows_cli() {
        use crate::hive::ProviderAuth;
        for name in ["claude-cli", "gemini-cli", "codex-cli", "grok-cli"] {
            for auth in [
                ProviderAuth::System,
                ProviderAuth::ApiKey("synthetic".into()),
            ] {
                let selection = ProviderSelection {
                    provider: name.into(),
                    auth,
                };
                assert!(!selection_allowed(Some(&selection), Some(false)));
                assert!(selection_allowed(Some(&selection), Some(true)));
                assert!(!selection_allowed(Some(&selection), None));
            }
        }
        let explicit = ProviderSelection {
            provider: "openai".into(),
            auth: ProviderAuth::ApiKey("synthetic".into()),
        };
        assert!(selection_allowed(Some(&explicit), Some(false)));
        for auth in [ProviderAuth::System, ProviderAuth::ApiKey("  ".into())] {
            let missing = ProviderSelection {
                provider: "openai".into(),
                auth,
            };
            assert!(!selection_allowed(Some(&missing), Some(false)));
        }
        assert!(!selection_allowed(None, Some(false)));
    }

    #[test]
    fn preview_provider_registry_never_invents_local_url() {
        for access in [None, Some(false), Some(true)] {
            assert!(explicit_local_url(None, access).is_none());
            assert!(explicit_local_url(Some(" ".into()), access).is_none());
        }
        let url = Some("http://127.0.0.1:1".to_string());
        assert!(explicit_local_url(url.clone(), None).is_none());
        assert_eq!(explicit_local_url(url.clone(), Some(false)), url);
    }

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
}
