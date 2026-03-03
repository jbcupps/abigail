//! Capability-to-provider selection for queued sub-agent jobs.

use abigail_queue::RequiredCapability;
use abigail_router::IdEgoRouter;
use std::sync::Arc;

/// Selection result produced by [`CapabilityMatcher`].
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CapabilitySelection {
    /// Provider label selected for this job (typically the active Ego provider).
    pub provider: String,
    /// Optional model hint for this job capability.
    pub model_hint: Option<String>,
    /// Capability label used for logging/prompt context.
    pub capability_label: String,
}

/// Maps job capabilities to the active provider.
///
/// With tier-based routing removed, the matcher always selects the ego
/// provider. The capability label is retained for sub-agent prompt context.
#[derive(Debug, Clone)]
pub struct CapabilityMatcher {
    provider: String,
}

impl CapabilityMatcher {
    pub fn new(provider: String) -> Self {
        Self { provider }
    }

    pub fn from_router(router: Arc<IdEgoRouter>) -> Self {
        let provider = router
            .status()
            .ego_provider
            .unwrap_or_else(|| "local".to_string());
        Self::new(provider)
    }

    pub fn select(&self, capability: &RequiredCapability) -> CapabilitySelection {
        CapabilitySelection {
            provider: self.provider.clone(),
            model_hint: None,
            capability_label: capability.as_str().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_queue::RequiredCapability;

    #[test]
    fn selects_provider_for_search() {
        let matcher = CapabilityMatcher::new("openai".to_string());
        let selected = matcher.select(&RequiredCapability::Search);
        assert_eq!(selected.provider, "openai");
        assert_eq!(selected.capability_label, "search");
    }

    #[test]
    fn selects_provider_for_reasoning() {
        let matcher = CapabilityMatcher::new("openai".to_string());
        let selected = matcher.select(&RequiredCapability::Reasoning);
        assert_eq!(selected.provider, "openai");
        assert_eq!(selected.capability_label, "reasoning");
    }
}
