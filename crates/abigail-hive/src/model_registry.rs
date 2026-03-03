//! Dynamic model registry — discovers, caches, and validates provider models.
//!
//! Wraps the low-level `discover_models()` from `abigail-capabilities` with:
//! - Per-provider caching with configurable TTL (default 24h)
//! - Persistence to `AppConfig.provider_catalog`
//! - Tier model validation (warns if assigned model not found in registry)

use abigail_capabilities::cognitive::validation::{discover_models, ModelInfo};
use abigail_core::ProviderCatalogEntry;
use chrono::Utc;
use std::collections::HashMap;

/// Default cache TTL: 24 hours.
const DEFAULT_TTL_SECS: i64 = 86_400;

/// A cached set of models for a single provider.
#[derive(Debug, Clone)]
pub struct ProviderModelCache {
    /// The provider name (e.g. "openai", "anthropic").
    pub provider: String,
    /// Discovered models.
    pub models: Vec<ModelInfo>,
    /// When the cache was last refreshed (UTC ISO 8601).
    pub last_fetched: String,
}

impl ProviderModelCache {
    /// Check if this cache entry has expired based on TTL.
    pub fn is_expired(&self, ttl_secs: i64) -> bool {
        let Ok(fetched) = chrono::DateTime::parse_from_rfc3339(&self.last_fetched) else {
            return true;
        };
        let elapsed = Utc::now().signed_duration_since(fetched);
        elapsed.num_seconds() > ttl_secs
    }
}

/// In-memory model registry with per-provider caching.
#[derive(Debug, Default)]
pub struct ModelRegistry {
    /// Cached models keyed by provider name.
    cache: HashMap<String, ProviderModelCache>,
    /// TTL in seconds for cache entries.
    ttl_secs: i64,
}

impl ModelRegistry {
    /// Create a new empty registry with default TTL.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            ttl_secs: DEFAULT_TTL_SECS,
        }
    }

    /// Create a registry with a custom TTL.
    pub fn with_ttl(ttl_secs: i64) -> Self {
        Self {
            cache: HashMap::new(),
            ttl_secs,
        }
    }

    /// Load cached models from persisted `ProviderCatalogEntry` list.
    ///
    /// Groups entries by provider and populates the in-memory cache.
    pub fn load_from_catalog(&mut self, catalog: &[ProviderCatalogEntry]) {
        let mut grouped: HashMap<String, Vec<ModelInfo>> = HashMap::new();
        let mut timestamps: HashMap<String, String> = HashMap::new();

        for entry in catalog {
            grouped
                .entry(entry.provider.clone())
                .or_default()
                .push(ModelInfo {
                    id: entry.model_id.clone(),
                    display_name: if entry.display_name.is_empty() {
                        None
                    } else {
                        Some(entry.display_name.clone())
                    },
                    created: None,
                });
            if let Some(ref ts) = entry.last_fetched {
                timestamps
                    .entry(entry.provider.clone())
                    .or_insert_with(|| ts.clone());
            }
        }

        for (provider, models) in grouped {
            let last_fetched = timestamps
                .remove(&provider)
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            self.cache.insert(
                provider.clone(),
                ProviderModelCache {
                    provider,
                    models,
                    last_fetched,
                },
            );
        }
    }

    /// Export the current cache to a flat list of `ProviderCatalogEntry` for persistence.
    pub fn to_catalog(&self) -> Vec<ProviderCatalogEntry> {
        let mut entries = Vec::new();
        for cache in self.cache.values() {
            for model in &cache.models {
                entries.push(ProviderCatalogEntry {
                    provider: cache.provider.clone(),
                    model_id: model.id.clone(),
                    display_name: model
                        .display_name
                        .clone()
                        .unwrap_or_else(|| model.id.clone()),
                    lifecycle: None,
                    last_fetched: Some(cache.last_fetched.clone()),
                });
            }
        }
        entries
    }

    /// Discover models for a single provider (network call).
    ///
    /// Updates the cache on success. On failure, retains the existing cache
    /// entry (if any) and returns the error.
    pub async fn refresh_provider(
        &mut self,
        provider: &str,
        api_key: &str,
    ) -> Result<&ProviderModelCache, String> {
        let models = discover_models(provider, api_key).await?;
        let now = Utc::now().to_rfc3339();
        let cache_entry = ProviderModelCache {
            provider: provider.to_string(),
            models,
            last_fetched: now,
        };
        self.cache.insert(provider.to_string(), cache_entry);
        Ok(self.cache.get(provider).unwrap())
    }

    /// Get cached models for a provider, refreshing if expired.
    ///
    /// Returns `None` if the provider has no cache entry and discovery fails.
    pub async fn get_or_refresh(
        &mut self,
        provider: &str,
        api_key: &str,
    ) -> Option<&ProviderModelCache> {
        let needs_refresh = self
            .cache
            .get(provider)
            .map(|c| c.is_expired(self.ttl_secs))
            .unwrap_or(true);

        if needs_refresh {
            match self.refresh_provider(provider, api_key).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        "Model discovery failed for {}: {}; using cached data if available",
                        provider,
                        e
                    );
                }
            }
        }

        self.cache.get(provider)
    }

    /// Get cached models for a provider without any network calls.
    pub fn get_cached(&self, provider: &str) -> Option<&ProviderModelCache> {
        self.cache.get(provider)
    }

    /// Check if a specific model exists in the registry for a provider.
    pub fn has_model(&self, provider: &str, model_id: &str) -> bool {
        self.cache
            .get(provider)
            .map(|c| c.models.iter().any(|m| m.id == model_id))
            .unwrap_or(false)
    }

    /// List all providers that have cached models.
    pub fn providers(&self) -> Vec<&str> {
        self.cache.keys().map(|s| s.as_str()).collect()
    }

    /// Get the total number of models across all providers.
    pub fn total_models(&self) -> usize {
        self.cache.values().map(|c| c.models.len()).sum()
    }

    /// Clear the entire cache.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_registry_is_empty() {
        let reg = ModelRegistry::new();
        assert!(reg.providers().is_empty());
        assert_eq!(reg.total_models(), 0);
    }

    #[test]
    fn test_load_from_catalog() {
        let catalog = vec![
            ProviderCatalogEntry {
                provider: "openai".to_string(),
                model_id: "gpt-4.1".to_string(),
                display_name: "GPT 4.1".to_string(),
                lifecycle: None,
                last_fetched: Some(Utc::now().to_rfc3339()),
            },
            ProviderCatalogEntry {
                provider: "openai".to_string(),
                model_id: "gpt-4.1-mini".to_string(),
                display_name: "GPT 4.1 Mini".to_string(),
                lifecycle: None,
                last_fetched: Some(Utc::now().to_rfc3339()),
            },
            ProviderCatalogEntry {
                provider: "anthropic".to_string(),
                model_id: "claude-sonnet-4-6".to_string(),
                display_name: "Claude Sonnet 4.6".to_string(),
                lifecycle: None,
                last_fetched: Some(Utc::now().to_rfc3339()),
            },
        ];

        let mut reg = ModelRegistry::new();
        reg.load_from_catalog(&catalog);

        assert_eq!(reg.providers().len(), 2);
        assert_eq!(reg.total_models(), 3);
        assert!(reg.has_model("openai", "gpt-4.1"));
        assert!(reg.has_model("openai", "gpt-4.1-mini"));
        assert!(reg.has_model("anthropic", "claude-sonnet-4-6"));
        assert!(!reg.has_model("openai", "nonexistent"));
    }

    #[test]
    fn test_to_catalog_roundtrip() {
        let catalog = vec![ProviderCatalogEntry {
            provider: "openai".to_string(),
            model_id: "gpt-4.1".to_string(),
            display_name: "GPT 4.1".to_string(),
            lifecycle: None,
            last_fetched: Some(Utc::now().to_rfc3339()),
        }];

        let mut reg = ModelRegistry::new();
        reg.load_from_catalog(&catalog);

        let exported = reg.to_catalog();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].provider, "openai");
        assert_eq!(exported[0].model_id, "gpt-4.1");
    }

    #[test]
    fn test_cache_expiry() {
        // Create a cache entry that's already expired
        let old_time = "2020-01-01T00:00:00Z".to_string();
        let cache = ProviderModelCache {
            provider: "openai".to_string(),
            models: vec![],
            last_fetched: old_time,
        };
        assert!(cache.is_expired(DEFAULT_TTL_SECS));

        // Create a cache entry that's fresh
        let fresh_time = Utc::now().to_rfc3339();
        let cache = ProviderModelCache {
            provider: "openai".to_string(),
            models: vec![],
            last_fetched: fresh_time,
        };
        assert!(!cache.is_expired(DEFAULT_TTL_SECS));
    }

    #[test]
    fn test_clear_cache() {
        let catalog = vec![ProviderCatalogEntry {
            provider: "openai".to_string(),
            model_id: "gpt-4.1".to_string(),
            display_name: "GPT 4.1".to_string(),
            lifecycle: None,
            last_fetched: Some(Utc::now().to_rfc3339()),
        }];

        let mut reg = ModelRegistry::new();
        reg.load_from_catalog(&catalog);
        assert_eq!(reg.total_models(), 1);

        reg.clear();
        assert_eq!(reg.total_models(), 0);
        assert!(reg.providers().is_empty());
    }

    #[tokio::test]
    async fn test_refresh_curated_provider() {
        // Anthropic uses curated list — doesn't require real API key
        let mut reg = ModelRegistry::new();
        let result = reg.refresh_provider("anthropic", "dummy-key").await;
        assert!(result.is_ok());

        let cache = result.unwrap();
        assert!(!cache.models.is_empty());
        assert!(cache.models.iter().any(|m| m.id.contains("claude")));
        assert!(reg.has_model("anthropic", "claude-sonnet-4-6"));
    }

    #[tokio::test]
    async fn test_refresh_perplexity_curated() {
        let mut reg = ModelRegistry::new();
        let result = reg.refresh_provider("perplexity", "dummy-key").await;
        assert!(result.is_ok());
        assert!(reg.has_model("perplexity", "sonar"));
    }

    #[tokio::test]
    async fn test_get_or_refresh_fresh_cache_no_network() {
        let mut reg = ModelRegistry::new();
        // Pre-populate with fresh cache
        let _ = reg.refresh_provider("anthropic", "dummy-key").await;

        // Second call should use cache (no network needed, even with bad key)
        let cached = reg.get_or_refresh("anthropic", "").await;
        assert!(cached.is_some());
    }
}
