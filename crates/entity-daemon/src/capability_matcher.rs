//! Capability-to-model selection for queued sub-agent jobs.

use abigail_core::ModelTier;
use abigail_queue::RequiredCapability;
use abigail_router::IdEgoRouter;
use std::sync::Arc;

/// Selection result produced by [`CapabilityMatcher`].
#[derive(Debug, Clone)]
pub struct CapabilitySelection {
    /// Provider label selected for this job (typically the active Ego provider).
    pub provider: String,
    /// Optional model hint for this job capability.
    pub model_hint: Option<String>,
    /// Tier selected by policy.
    pub tier: ModelTier,
}

/// Maps job capabilities to model tiers (and provider-specific model IDs).
///
/// This phase intentionally keeps the policy deterministic and simple:
/// - `search` -> fast
/// - `general` -> standard
/// - `code`, `reasoning`, `vision` -> pro
/// - `custom` -> standard
#[derive(Debug, Clone)]
pub struct CapabilityMatcher {
    provider: String,
    tier_models: abigail_core::TierModels,
}

impl CapabilityMatcher {
    pub fn new(provider: String, tier_models: abigail_core::TierModels) -> Self {
        Self {
            provider,
            tier_models,
        }
    }

    pub fn from_router(router: Arc<IdEgoRouter>) -> Self {
        let provider = router
            .status()
            .ego_provider
            .unwrap_or_else(|| "local".to_string());
        Self::new(provider, router.tier_models.clone())
    }

    pub fn select(&self, capability: &RequiredCapability) -> CapabilitySelection {
        let tier = match capability {
            RequiredCapability::Search => ModelTier::Fast,
            RequiredCapability::General => ModelTier::Standard,
            RequiredCapability::Code
            | RequiredCapability::Reasoning
            | RequiredCapability::Vision => ModelTier::Pro,
            RequiredCapability::Custom(_) => ModelTier::Standard,
        };

        let model_hint = self.tier_models.get_model(&self.provider, tier).cloned();
        CapabilitySelection {
            provider: self.provider.clone(),
            model_hint,
            tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_queue::RequiredCapability;

    #[test]
    fn maps_search_to_fast() {
        let matcher =
            CapabilityMatcher::new("openai".to_string(), abigail_core::TierModels::defaults());
        let selected = matcher.select(&RequiredCapability::Search);
        assert_eq!(selected.tier, ModelTier::Fast);
    }

    #[test]
    fn maps_reasoning_to_pro() {
        let matcher =
            CapabilityMatcher::new("openai".to_string(), abigail_core::TierModels::defaults());
        let selected = matcher.select(&RequiredCapability::Reasoning);
        assert_eq!(selected.tier, ModelTier::Pro);
    }
}
