//! Tier resolver: maps a PromptTier to a concrete LlmProvider with the correct model.

use crate::classifier::PromptTier;
use abigail_capabilities::cognitive::{
    AnthropicProvider, CandleProvider, CompatibleProvider, LlmProvider, OpenAiCompatibleProvider,
    OpenAiProvider,
};
use abigail_core::{ModelTier, TierModels};
use std::sync::Arc;

/// Maps PromptTier to a concrete LlmProvider instance with the appropriate model.
pub struct TierResolver {
    /// Ego provider name (e.g. "openai", "anthropic")
    ego_provider_name: Option<String>,
    /// Ego API key
    ego_api_key: Option<String>,
    /// Tier-to-model mapping from config
    tier_models: TierModels,
    /// Local LLM provider for T1/fallback
    local_provider: Option<Arc<dyn LlmProvider>>,
}

impl TierResolver {
    pub fn new(
        ego_provider_name: Option<String>,
        ego_api_key: Option<String>,
        tier_models: TierModels,
        local_provider: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        Self {
            ego_provider_name,
            ego_api_key,
            tier_models,
            local_provider,
        }
    }

    /// Resolve a PromptTier to a concrete LlmProvider.
    ///
    /// Resolution logic:
    /// - T1Fast: local LLM if available, else fast cloud model
    /// - T2Standard: Ego provider's standard model
    /// - T3Pro: Ego provider's pro model
    /// - T4Specialist: Same as T3Pro (V1 has no specialized routing)
    ///
    /// Falls back through the tier chain if the target tier's provider is unavailable.
    pub fn resolve(&self, tier: PromptTier) -> Arc<dyn LlmProvider> {
        match tier {
            PromptTier::T1Fast => {
                // Prefer local LLM for fast tier
                if let Some(ref local) = self.local_provider {
                    return local.clone();
                }
                // Fall back to fast cloud model
                self.build_cloud_provider(ModelTier::Fast)
                    .unwrap_or_else(|| self.fallback_provider())
            }
            PromptTier::T2Standard => self
                .build_cloud_provider(ModelTier::Standard)
                .or_else(|| self.build_cloud_provider(ModelTier::Fast))
                .or_else(|| self.local_provider.clone())
                .unwrap_or_else(|| self.fallback_provider()),
            PromptTier::T3Pro | PromptTier::T4Specialist => self
                .build_cloud_provider(ModelTier::Pro)
                .or_else(|| self.build_cloud_provider(ModelTier::Standard))
                .or_else(|| self.build_cloud_provider(ModelTier::Fast))
                .or_else(|| self.local_provider.clone())
                .unwrap_or_else(|| self.fallback_provider()),
        }
    }

    /// Build a cloud provider for the given model tier using the configured Ego provider.
    fn build_cloud_provider(&self, tier: ModelTier) -> Option<Arc<dyn LlmProvider>> {
        let provider_name = self.ego_provider_name.as_deref()?;
        let api_key = self.ego_api_key.as_ref().filter(|k| !k.is_empty())?;

        let model = self
            .tier_models
            .get_model(provider_name, tier)
            .cloned()
            .or_else(|| {
                // If exact tier not found, use defaults
                let defaults = TierModels::defaults();
                defaults.get_model(provider_name, tier).cloned()
            })?;

        tracing::debug!(
            "TierResolver: building {} provider with model {} for tier {:?}",
            provider_name,
            model,
            tier
        );

        build_provider_with_model(provider_name, api_key, &model)
    }

    /// Fallback: CandleProvider stub.
    fn fallback_provider(&self) -> Arc<dyn LlmProvider> {
        tracing::debug!("TierResolver: using CandleProvider fallback");
        Arc::new(CandleProvider::new())
    }
}

/// Build an LlmProvider for a given provider name, API key, and model.
pub fn build_provider_with_model(
    provider_name: &str,
    api_key: &str,
    model: &str,
) -> Option<Arc<dyn LlmProvider>> {
    let result: anyhow::Result<Arc<dyn LlmProvider>> = match provider_name {
        "openai" => OpenAiProvider::with_model(Some(api_key.to_string()), model.to_string())
            .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        "anthropic" => AnthropicProvider::with_model(api_key.to_string(), model.to_string())
            .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        "perplexity" | "pplx" => OpenAiCompatibleProvider::with_config(
            CompatibleProvider::Perplexity,
            CompatibleProvider::Perplexity.base_url().to_string(),
            api_key.to_string(),
            model.to_string(),
        )
        .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        "xai" | "grok" => OpenAiCompatibleProvider::with_config(
            CompatibleProvider::Xai,
            CompatibleProvider::Xai.base_url().to_string(),
            api_key.to_string(),
            model.to_string(),
        )
        .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        "google" | "gemini" => OpenAiCompatibleProvider::with_config(
            CompatibleProvider::Google,
            CompatibleProvider::Google.base_url().to_string(),
            api_key.to_string(),
            model.to_string(),
        )
        .map(|p| Arc::new(p) as Arc<dyn LlmProvider>),
        _ => {
            tracing::warn!(
                "TierResolver: unknown provider '{}', cannot build",
                provider_name
            );
            return None;
        }
    };
    match result {
        Ok(provider) => Some(provider),
        Err(e) => {
            tracing::error!(
                "TierResolver: failed to build provider '{}': {}",
                provider_name,
                e
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::LocalHttpProvider;

    #[test]
    fn test_resolve_t1_with_local() {
        let local =
            Arc::new(LocalHttpProvider::with_url("http://localhost:1234".to_string()).unwrap())
                as Arc<dyn LlmProvider>;
        let resolver = TierResolver::new(
            Some("openai".to_string()),
            Some("test-key".to_string()),
            TierModels::defaults(),
            Some(local),
        );

        // T1Fast should use local provider
        let _provider = resolver.resolve(PromptTier::T1Fast);
        // We can't easily compare Arc<dyn> pointers, but at least it doesn't panic
    }

    #[test]
    fn test_resolve_t2_without_ego_uses_local() {
        let local =
            Arc::new(LocalHttpProvider::with_url("http://localhost:1234".to_string()).unwrap())
                as Arc<dyn LlmProvider>;
        let resolver = TierResolver::new(None, None, TierModels::defaults(), Some(local));

        // T2Standard without ego should fall back to local
        let _provider = resolver.resolve(PromptTier::T2Standard);
    }

    #[test]
    fn test_resolve_t3_without_anything_uses_candle_stub() {
        let resolver = TierResolver::new(None, None, TierModels::defaults(), None);

        // Should fall all the way back to CandleProvider
        let _provider = resolver.resolve(PromptTier::T3Pro);
    }

    #[test]
    fn test_resolve_t4_same_as_t3() {
        let resolver = TierResolver::new(
            Some("openai".to_string()),
            Some("test-key".to_string()),
            TierModels::defaults(),
            None,
        );

        // T4Specialist should resolve to a provider (pro model)
        let _provider = resolver.resolve(PromptTier::T4Specialist);
    }

    #[test]
    fn test_resolve_all_tiers_with_ego() {
        let resolver = TierResolver::new(
            Some("anthropic".to_string()),
            Some("test-key".to_string()),
            TierModels::defaults(),
            None,
        );

        // All tiers should resolve without panicking
        let _p1 = resolver.resolve(PromptTier::T1Fast);
        let _p2 = resolver.resolve(PromptTier::T2Standard);
        let _p3 = resolver.resolve(PromptTier::T3Pro);
        let _p4 = resolver.resolve(PromptTier::T4Specialist);
    }

    #[test]
    fn test_build_provider_with_model_openai() {
        let provider = build_provider_with_model("openai", "test-key", "gpt-4o");
        assert!(provider.is_some());
    }

    #[test]
    fn test_build_provider_with_model_anthropic() {
        let provider = build_provider_with_model("anthropic", "test-key", "claude-opus-4-6");
        assert!(provider.is_some());
    }

    #[test]
    fn test_build_provider_with_model_perplexity() {
        let provider = build_provider_with_model("perplexity", "test-key", "sonar-pro");
        assert!(provider.is_some());
    }

    #[test]
    fn test_build_provider_with_model_xai() {
        let provider = build_provider_with_model("xai", "test-key", "grok-3");
        assert!(provider.is_some());
    }

    #[test]
    fn test_build_provider_with_model_google() {
        let provider = build_provider_with_model("google", "test-key", "gemini-2.5-pro");
        assert!(provider.is_some());
    }

    #[test]
    fn test_build_provider_with_model_unknown() {
        let provider = build_provider_with_model("unknown", "test-key", "some-model");
        assert!(provider.is_none());
    }

    #[test]
    fn test_resolve_with_only_local() {
        let local =
            Arc::new(LocalHttpProvider::with_url("http://localhost:1234".to_string()).unwrap())
                as Arc<dyn LlmProvider>;
        let resolver = TierResolver::new(None, None, TierModels::defaults(), Some(local));

        // All tiers should resolve to something (local or fallback)
        let _p1 = resolver.resolve(PromptTier::T1Fast);
        let _p2 = resolver.resolve(PromptTier::T2Standard);
        let _p3 = resolver.resolve(PromptTier::T3Pro);
        let _p4 = resolver.resolve(PromptTier::T4Specialist);
    }
}
