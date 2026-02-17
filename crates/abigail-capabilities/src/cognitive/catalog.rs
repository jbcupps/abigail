//! Provider model catalog — curated defaults and runtime model discovery.

use abigail_core::{ProviderCatalogEntry, TierModels};
use std::collections::HashMap;

/// Curated model catalog with defaults for known providers.
pub struct ProviderCatalog;

/// Result of validating a model against the catalog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum ValidationResult {
    /// Model exists and is active.
    Valid,
    /// Model exists but is deprecated — include a warning message.
    Deprecated(String),
    /// Model not found in catalog.
    NotFound,
    /// Catalog not available for this provider.
    NoCatalog,
}

impl ProviderCatalog {
    /// Return curated default catalog entries for all known providers.
    pub fn curated_defaults() -> Vec<ProviderCatalogEntry> {
        let mut entries = Vec::new();

        // OpenAI models
        for (id, name, lifecycle) in [
            ("gpt-4o", "GPT-4o", "active"),
            ("gpt-4o-mini", "GPT-4o Mini", "active"),
            ("o1", "o1", "active"),
            ("o1-mini", "o1 Mini", "active"),
            ("o3", "o3", "active"),
            ("o3-mini", "o3 Mini", "active"),
            ("gpt-4-turbo", "GPT-4 Turbo", "deprecated"),
            ("gpt-3.5-turbo", "GPT-3.5 Turbo", "deprecated"),
        ] {
            entries.push(ProviderCatalogEntry {
                provider: "openai".into(),
                model_id: id.into(),
                display_name: name.into(),
                lifecycle: Some(lifecycle.into()),
                last_fetched: None,
            });
        }

        // Anthropic models
        for (id, name, lifecycle) in [
            ("claude-opus-4-6", "Claude Opus 4.6", "active"),
            ("claude-sonnet-4-5-20250929", "Claude Sonnet 4.5", "active"),
            ("claude-haiku-4-5-20251001", "Claude Haiku 4.5", "active"),
            ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet", "active"),
            ("claude-3-haiku-20240307", "Claude 3 Haiku", "deprecated"),
        ] {
            entries.push(ProviderCatalogEntry {
                provider: "anthropic".into(),
                model_id: id.into(),
                display_name: name.into(),
                lifecycle: Some(lifecycle.into()),
                last_fetched: None,
            });
        }

        // Google models
        for (id, name, lifecycle) in [
            ("gemini-2.5-pro", "Gemini 2.5 Pro", "active"),
            ("gemini-2.0-flash", "Gemini 2.0 Flash", "active"),
            ("gemini-1.5-pro", "Gemini 1.5 Pro", "active"),
            ("gemini-1.5-flash", "Gemini 1.5 Flash", "active"),
        ] {
            entries.push(ProviderCatalogEntry {
                provider: "google".into(),
                model_id: id.into(),
                display_name: name.into(),
                lifecycle: Some(lifecycle.into()),
                last_fetched: None,
            });
        }

        // xAI models
        for (id, name, lifecycle) in [
            ("grok-3", "Grok 3", "active"),
            ("grok-2-mini", "Grok 2 Mini", "active"),
            ("grok-2", "Grok 2", "active"),
        ] {
            entries.push(ProviderCatalogEntry {
                provider: "xai".into(),
                model_id: id.into(),
                display_name: name.into(),
                lifecycle: Some(lifecycle.into()),
                last_fetched: None,
            });
        }

        // Perplexity models
        for (id, name, lifecycle) in [
            ("sonar", "Sonar", "active"),
            ("sonar-pro", "Sonar Pro", "active"),
            ("sonar-reasoning", "Sonar Reasoning", "active"),
            ("sonar-reasoning-pro", "Sonar Reasoning Pro", "active"),
        ] {
            entries.push(ProviderCatalogEntry {
                provider: "perplexity".into(),
                model_id: id.into(),
                display_name: name.into(),
                lifecycle: Some(lifecycle.into()),
                last_fetched: None,
            });
        }

        entries
    }

    /// Fetch available models from a provider's API.
    /// Returns updated catalog entries with `last_fetched` timestamps.
    pub async fn fetch_catalog(
        provider: &str,
        api_key: &str,
    ) -> anyhow::Result<Vec<ProviderCatalogEntry>> {
        let now = chrono::Utc::now().to_rfc3339();
        let client = reqwest::Client::new();

        match provider {
            "openai" => {
                let resp = client
                    .get("https://api.openai.com/v1/models")
                    .bearer_auth(api_key)
                    .send()
                    .await?;
                let body: serde_json::Value = resp.json().await?;
                let models = body["data"].as_array().cloned().unwrap_or_default();
                Ok(models
                    .iter()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?;
                        // Filter to chat-capable models
                        if id.starts_with("gpt-") || id.starts_with("o1") || id.starts_with("o3") {
                            Some(ProviderCatalogEntry {
                                provider: "openai".into(),
                                model_id: id.into(),
                                display_name: id.into(),
                                lifecycle: Some("active".into()),
                                last_fetched: Some(now.clone()),
                            })
                        } else {
                            None
                        }
                    })
                    .collect())
            }
            "anthropic" => {
                let resp = client
                    .get("https://api.anthropic.com/v1/models")
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .send()
                    .await?;
                let body: serde_json::Value = resp.json().await?;
                let models = body["data"].as_array().cloned().unwrap_or_default();
                Ok(models
                    .iter()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?;
                        let name = m["display_name"].as_str().unwrap_or(id);
                        Some(ProviderCatalogEntry {
                            provider: "anthropic".into(),
                            model_id: id.into(),
                            display_name: name.into(),
                            lifecycle: Some("active".into()),
                            last_fetched: Some(now.clone()),
                        })
                    })
                    .collect())
            }
            _ => {
                // For providers without a models API, return curated defaults
                Ok(Self::curated_defaults()
                    .into_iter()
                    .filter(|e| e.provider == provider)
                    .map(|mut e| {
                        e.last_fetched = Some(now.clone());
                        e
                    })
                    .collect())
            }
        }
    }

    /// Validate a model ID against the catalog.
    pub fn validate_model(
        provider: &str,
        model_id: &str,
        catalog: &[ProviderCatalogEntry],
    ) -> ValidationResult {
        let provider_entries: Vec<&ProviderCatalogEntry> =
            catalog.iter().filter(|e| e.provider == provider).collect();

        if provider_entries.is_empty() {
            return ValidationResult::NoCatalog;
        }

        for entry in &provider_entries {
            if entry.model_id == model_id {
                if entry.lifecycle.as_deref() == Some("deprecated") {
                    return ValidationResult::Deprecated(format!(
                        "Model '{}' is deprecated. Consider upgrading.",
                        model_id
                    ));
                }
                return ValidationResult::Valid;
            }
        }

        ValidationResult::NotFound
    }

    /// Get the default tier model assignments.
    pub fn default_tier_models() -> TierModels {
        TierModels::defaults()
    }

    /// Validate all tier model assignments against the catalog.
    /// Returns a map of provider → tier → validation result for any issues found.
    pub fn validate_tier_models(
        tier_models: &TierModels,
        catalog: &[ProviderCatalogEntry],
    ) -> HashMap<String, Vec<(String, ValidationResult)>> {
        let mut issues: HashMap<String, Vec<(String, ValidationResult)>> = HashMap::new();

        for (tier_name, tier_map) in [
            ("fast", &tier_models.fast),
            ("standard", &tier_models.standard),
            ("pro", &tier_models.pro),
        ] {
            for (provider, model_id) in tier_map {
                let result = Self::validate_model(provider, model_id, catalog);
                if result != ValidationResult::Valid {
                    issues
                        .entry(provider.clone())
                        .or_default()
                        .push((tier_name.to_string(), result));
                }
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curated_defaults_not_empty() {
        let catalog = ProviderCatalog::curated_defaults();
        assert!(!catalog.is_empty());
        // Should have entries for all 5 providers
        let providers: std::collections::HashSet<&str> =
            catalog.iter().map(|e| e.provider.as_str()).collect();
        assert!(providers.contains("openai"));
        assert!(providers.contains("anthropic"));
        assert!(providers.contains("google"));
        assert!(providers.contains("xai"));
        assert!(providers.contains("perplexity"));
    }

    #[test]
    fn test_validate_model_valid() {
        let catalog = ProviderCatalog::curated_defaults();
        let result = ProviderCatalog::validate_model("openai", "gpt-4o", &catalog);
        assert_eq!(result, ValidationResult::Valid);
    }

    #[test]
    fn test_validate_model_deprecated() {
        let catalog = ProviderCatalog::curated_defaults();
        let result = ProviderCatalog::validate_model("openai", "gpt-4-turbo", &catalog);
        assert!(matches!(result, ValidationResult::Deprecated(_)));
    }

    #[test]
    fn test_validate_model_not_found() {
        let catalog = ProviderCatalog::curated_defaults();
        let result = ProviderCatalog::validate_model("openai", "nonexistent-model", &catalog);
        assert_eq!(result, ValidationResult::NotFound);
    }

    #[test]
    fn test_validate_model_no_catalog() {
        let catalog = ProviderCatalog::curated_defaults();
        let result = ProviderCatalog::validate_model("unknown_provider", "model", &catalog);
        assert_eq!(result, ValidationResult::NoCatalog);
    }

    #[test]
    fn test_validate_tier_models_defaults_pass() {
        let catalog = ProviderCatalog::curated_defaults();
        let tiers = TierModels::defaults();
        let issues = ProviderCatalog::validate_tier_models(&tiers, &catalog);
        // Default tier models should all be valid
        assert!(issues.is_empty(), "Unexpected issues: {:?}", issues);
    }

    #[test]
    fn test_default_tier_models() {
        let tiers = ProviderCatalog::default_tier_models();
        assert!(!tiers.fast.is_empty());
        assert!(!tiers.standard.is_empty());
        assert!(!tiers.pro.is_empty());
    }
}
