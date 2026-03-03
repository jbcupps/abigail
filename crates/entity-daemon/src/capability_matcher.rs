//! Capability-to-provider selection for queued sub-agent jobs.
//!
//! Routes each `RequiredCapability` to the optimal provider/model pair.
//! Defaults are inferred from the active ego provider with specialized
//! overrides for non-chat capabilities (image gen, search, etc.).

use abigail_queue::{ExecutionMode, RequiredCapability};
use abigail_router::IdEgoRouter;
use std::collections::HashMap;
use std::sync::Arc;

/// Selection result produced by [`CapabilityMatcher`].
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CapabilitySelection {
    pub provider: String,
    pub model_hint: Option<String>,
    pub capability_label: String,
    pub execution_mode: ExecutionMode,
}

/// Per-capability routing override.
#[derive(Debug, Clone)]
pub struct CapabilityRoute {
    pub provider: String,
    pub model: Option<String>,
    pub execution_mode: ExecutionMode,
}

/// Maps job capabilities to provider/model pairs.
///
/// Starts with sensible defaults (ego provider for chat-capable tasks,
/// specialized providers for generation tasks) and accepts overrides.
#[derive(Debug, Clone)]
pub struct CapabilityMatcher {
    default_provider: String,
    routes: HashMap<String, CapabilityRoute>,
}

impl CapabilityMatcher {
    pub fn new(default_provider: String) -> Self {
        let mut routes = HashMap::new();

        routes.insert(
            "image_generation".into(),
            CapabilityRoute {
                provider: "openai".into(),
                model: Some("dall-e-3".into()),
                execution_mode: ExecutionMode::Direct,
            },
        );
        routes.insert(
            "video_generation".into(),
            CapabilityRoute {
                provider: "openai".into(),
                model: Some("sora".into()),
                execution_mode: ExecutionMode::Direct,
            },
        );
        routes.insert(
            "transcription".into(),
            CapabilityRoute {
                provider: "openai".into(),
                model: Some("whisper-1".into()),
                execution_mode: ExecutionMode::Direct,
            },
        );

        Self {
            default_provider,
            routes,
        }
    }

    pub fn from_router(router: Arc<IdEgoRouter>) -> Self {
        let provider = router
            .status()
            .ego_provider
            .unwrap_or_else(|| "local".to_string());
        Self::new(provider)
    }

    /// Register or replace a route for a given capability.
    #[allow(dead_code)]
    pub fn set_route(&mut self, capability: &str, route: CapabilityRoute) {
        self.routes.insert(capability.to_string(), route);
    }

    /// Register a search provider override (e.g. perplexity).
    #[allow(dead_code)]
    pub fn with_search_provider(mut self, provider: &str, model: Option<&str>) -> Self {
        self.routes.insert(
            "search".into(),
            CapabilityRoute {
                provider: provider.into(),
                model: model.map(|m| m.into()),
                execution_mode: ExecutionMode::Mediated,
            },
        );
        self
    }

    pub fn select(&self, capability: &RequiredCapability) -> CapabilitySelection {
        let key = capability.as_str();
        if let Some(route) = self.routes.get(key) {
            return CapabilitySelection {
                provider: route.provider.clone(),
                model_hint: route.model.clone(),
                capability_label: key.to_string(),
                execution_mode: route.execution_mode.clone(),
            };
        }

        CapabilitySelection {
            provider: self.default_provider.clone(),
            model_hint: None,
            capability_label: key.to_string(),
            execution_mode: ExecutionMode::Mediated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_queue::RequiredCapability;

    #[test]
    fn selects_default_for_general() {
        let matcher = CapabilityMatcher::new("openai".to_string());
        let sel = matcher.select(&RequiredCapability::General);
        assert_eq!(sel.provider, "openai");
        assert!(sel.model_hint.is_none());
        assert_eq!(sel.execution_mode, ExecutionMode::Mediated);
    }

    #[test]
    fn routes_image_generation_to_dall_e() {
        let matcher = CapabilityMatcher::new("anthropic".to_string());
        let sel = matcher.select(&RequiredCapability::ImageGeneration);
        assert_eq!(sel.provider, "openai");
        assert_eq!(sel.model_hint.as_deref(), Some("dall-e-3"));
        assert_eq!(sel.execution_mode, ExecutionMode::Direct);
    }

    #[test]
    fn search_override_applies() {
        let matcher = CapabilityMatcher::new("openai".to_string())
            .with_search_provider("perplexity", Some("sonar-pro"));
        let sel = matcher.select(&RequiredCapability::Search);
        assert_eq!(sel.provider, "perplexity");
        assert_eq!(sel.model_hint.as_deref(), Some("sonar-pro"));
        assert_eq!(sel.execution_mode, ExecutionMode::Mediated);
    }

    #[test]
    fn custom_capability_falls_to_default() {
        let matcher = CapabilityMatcher::new("google".to_string());
        let sel = matcher.select(&RequiredCapability::Custom("exotic".into()));
        assert_eq!(sel.provider, "google");
        assert!(sel.model_hint.is_none());
    }

    #[test]
    fn set_route_overrides_default() {
        let mut matcher = CapabilityMatcher::new("openai".to_string());
        matcher.set_route(
            "reasoning",
            CapabilityRoute {
                provider: "anthropic".into(),
                model: Some("claude-opus-4-6".into()),
                execution_mode: ExecutionMode::Mediated,
            },
        );
        let sel = matcher.select(&RequiredCapability::Reasoning);
        assert_eq!(sel.provider, "anthropic");
        assert_eq!(sel.model_hint.as_deref(), Some("claude-opus-4-6"));
    }
}
