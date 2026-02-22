//! Hive: single authority for secret resolution and provider construction.
//!
//! The Hive owns vault references and resolves which provider/key to use
//! based on the priority chain: active preference → entity vault → hive vault
//! → trinity config → environment variables.

use crate::provider_registry::{ProviderKind, ProviderRegistry};
use abigail_capabilities::cognitive::{LlmProvider, LocalHttpProvider};
use abigail_core::{AppConfig, RoutingMode, SecretsVault, SuperegoL2Mode};
use std::sync::{Arc, Mutex};

/// Fully resolved configuration ready for provider construction.
#[derive(Debug, Clone)]
pub struct HiveConfig {
    pub local_llm_base_url: Option<String>,
    pub ego_provider_name: Option<String>,
    pub ego_api_key: Option<String>,
    pub ego_model: Option<String>,
    pub routing_mode: RoutingMode,
    pub superego_provider: Option<String>,
    pub superego_api_key: Option<String>,
    pub superego_l2_mode: SuperegoL2Mode,
}

/// All providers built and ready to be injected into a router.
pub struct BuiltProviders {
    pub id: Arc<dyn LlmProvider>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
    pub ego: Option<Arc<dyn LlmProvider>>,
    pub ego_kind: Option<ProviderKind>,
    pub superego: Option<Arc<dyn LlmProvider>>,
    pub superego_l2_mode: SuperegoL2Mode,
    pub routing_mode: RoutingMode,
}

/// The Hive owns vault references and acts as the single entry-point for
/// resolving secrets and constructing providers.
pub struct Hive {
    /// Entity-level secrets vault.
    secrets: Arc<Mutex<SecretsVault>>,
    /// Hive-level (shared) secrets vault.
    hive_secrets: Arc<Mutex<SecretsVault>>,
}

impl Hive {
    /// Create a new Hive that holds the same Arc references as AppState.
    pub fn new(secrets: Arc<Mutex<SecretsVault>>, hive_secrets: Arc<Mutex<SecretsVault>>) -> Self {
        Self {
            secrets,
            hive_secrets,
        }
    }

    /// Determine which Ego provider and key to use based on the priority chain:
    ///
    /// 1. Explicit `active_provider_preference` from Mentor/Forge menu
    /// 2. Entity-level vault scan (keys pasted in chat or Connectivity)
    /// 3. Trinity config (legacy/manual paths)
    /// 4. Environment variables (last resort)
    pub fn determine_ego_provider(
        config: &AppConfig,
        vault: &SecretsVault,
    ) -> (Option<String>, Option<String>) {
        // 1. Explicit preference from Mentor menu (Forge)
        if let Some(pref) = &config.active_provider_preference {
            if let Some(key) = vault.get_secret(pref) {
                let k = key.to_string();
                if !k.is_empty() {
                    return (Some(pref.clone()), Some(k));
                }
            }
        }

        // 2. Local Vault (keys pasted in chat or added in Connectivity)
        let provider_names = [
            "openai",
            "google",
            "xai",
            "perplexity",
            "anthropic",
            "claude-cli",
            "gemini-cli",
            "codex-cli",
            "grok-cli",
        ];
        for name in &provider_names {
            if let Some(key) = vault.get_secret(name) {
                let k = key.to_string();
                if !k.is_empty() {
                    return (Some(name.to_string()), Some(k));
                }
            }
        }

        // 3. Trinity config (legacy/manual paths)
        if let Some(trinity) = &config.trinity {
            if let Some(p) = &trinity.ego_provider {
                if let Some(k) = &trinity.ego_api_key {
                    if !k.is_empty() {
                        return (Some(p.clone()), Some(k.clone()));
                    }
                }
            }
        }

        // 4. Environment variables (last resort)
        if let Some(k) = &config.openai_api_key {
            if !k.is_empty() {
                return (Some("openai".to_string()), Some(k.clone()));
            }
        }

        (None, None)
    }

    /// Extract Superego provider config from TrinityConfig.
    fn extract_superego_config(config: &AppConfig) -> Option<(String, String)> {
        config.trinity.as_ref().and_then(|trinity| {
            match (&trinity.superego_provider, &trinity.superego_api_key) {
                (Some(provider), Some(key)) if !key.is_empty() => {
                    Some((provider.clone(), key.clone()))
                }
                _ => None,
            }
        })
    }

    /// Resolve the full provider configuration from AppConfig + vaults.
    ///
    /// Acquires locks on `secrets` then `hive_secrets` (in documented order).
    pub fn resolve_config(&self, config: &AppConfig) -> Result<HiveConfig, String> {
        let (ego_name, ego_key) = {
            let vault = self.secrets.lock().map_err(|e| e.to_string())?;
            let (name, key) = Self::determine_ego_provider(config, &vault);
            if name.is_some() {
                tracing::info!("Using Ego provider from preference/vault: {:?}", name);
                (name, key)
            } else {
                drop(vault);
                let hive = self.hive_secrets.lock().map_err(|e| e.to_string())?;
                let (h_name, h_key) = Self::determine_ego_provider(config, &hive);
                tracing::info!("Using Ego provider from Hive vault: {:?}", h_name);
                (h_name, h_key)
            }
        };

        let ego_model = ego_name.as_ref().and_then(|name| {
            let model = config
                .tier_models
                .as_ref()
                .and_then(|tm| tm.standard.get(name).cloned());
            tracing::info!("Model for {:?} found in TierModels: {:?}", name, model);
            model
        });

        let superego_config = Self::extract_superego_config(config);

        tracing::debug!(
            "Resolved config: local_url={:?}, ego_name={:?}, ego_model={:?}, has_ego_key={}, mode={:?}",
            config.local_llm_base_url,
            ego_name,
            ego_model,
            ego_key.is_some(),
            config.routing_mode
        );

        Ok(HiveConfig {
            local_llm_base_url: config.local_llm_base_url.clone(),
            ego_provider_name: ego_name,
            ego_api_key: ego_key,
            ego_model,
            routing_mode: config.routing_mode,
            superego_provider: superego_config.as_ref().map(|(p, _)| p.clone()),
            superego_api_key: superego_config.map(|(_, k)| k),
            superego_l2_mode: config.superego_l2_mode,
        })
    }

    /// Build all providers from a resolved HiveConfig (no locking).
    pub async fn build_providers(hive_config: &HiveConfig) -> BuiltProviders {
        let ego_result = ProviderRegistry::build_ego(
            hive_config.ego_provider_name.as_deref(),
            hive_config.ego_api_key.clone(),
            hive_config.ego_model.clone(),
        );

        let id_result =
            ProviderRegistry::build_id_auto_detect(hive_config.local_llm_base_url.clone()).await;

        let superego = match (
            &hive_config.superego_provider,
            &hive_config.superego_api_key,
        ) {
            (Some(provider), Some(key)) if !key.is_empty() => {
                Some(ProviderRegistry::build_superego(provider, key))
            }
            _ => None,
        };

        BuiltProviders {
            id: id_result.provider,
            local_http: id_result.local_http,
            ego: ego_result.provider,
            ego_kind: ego_result.kind,
            superego,
            superego_l2_mode: hive_config.superego_l2_mode,
            routing_mode: hive_config.routing_mode,
        }
    }

    /// Convenience: resolve config + build providers in one call.
    pub async fn build_providers_from_config(
        &self,
        config: &AppConfig,
    ) -> Result<BuiltProviders, String> {
        let hive_config = self.resolve_config(config)?;
        Ok(Self::build_providers(&hive_config).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp_vault() -> Arc<Mutex<SecretsVault>> {
        let dir = std::env::temp_dir().join(format!(
            "hive_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Mutex::new(SecretsVault::new(dir)))
    }

    fn default_config() -> AppConfig {
        AppConfig::default_paths()
    }

    #[test]
    fn determine_ego_prefers_active_preference() {
        let mut config = default_config();
        config.active_provider_preference = Some("anthropic".to_string());
        let vault = temp_vault();
        vault
            .lock()
            .unwrap()
            .set_secret("anthropic", "anthro-key-123");
        vault.lock().unwrap().set_secret("openai", "openai-key-456");

        let (name, key) = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert_eq!(name.as_deref(), Some("anthropic"));
        assert_eq!(key.as_deref(), Some("anthro-key-123"));
    }

    #[test]
    fn determine_ego_scans_vault_order() {
        let config = default_config();
        let vault = temp_vault();
        // Only xai has a key
        vault.lock().unwrap().set_secret("xai", "xai-key");

        let (name, _key) = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert_eq!(name.as_deref(), Some("xai"));
    }

    #[test]
    fn determine_ego_falls_to_trinity() {
        let mut config = default_config();
        config.trinity = Some(abigail_core::TrinityConfig {
            id_url: None,
            ego_provider: Some("google".to_string()),
            ego_api_key: Some("google-key".to_string()),
            superego_provider: None,
            superego_api_key: None,
        });
        let vault = temp_vault();

        let (name, key) = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert_eq!(name.as_deref(), Some("google"));
        assert_eq!(key.as_deref(), Some("google-key"));
    }

    #[test]
    fn determine_ego_falls_to_env_key() {
        let mut config = default_config();
        config.openai_api_key = Some("env-key".to_string());
        let vault = temp_vault();

        let (name, key) = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert_eq!(name.as_deref(), Some("openai"));
        assert_eq!(key.as_deref(), Some("env-key"));
    }

    #[test]
    fn determine_ego_returns_none_when_empty() {
        let config = default_config();
        let vault = temp_vault();

        let (name, key) = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert!(name.is_none());
        assert!(key.is_none());
    }

    #[test]
    fn resolve_config_prefers_entity_vault_over_hive() {
        let mut config = default_config();
        config.active_provider_preference = None;

        let entity_vault = temp_vault();
        entity_vault
            .lock()
            .unwrap()
            .set_secret("openai", "entity-key");

        let hive_vault = temp_vault();
        hive_vault
            .lock()
            .unwrap()
            .set_secret("anthropic", "hive-key");

        let hive = Hive::new(entity_vault, hive_vault);
        let resolved = hive.resolve_config(&config).unwrap();

        assert_eq!(resolved.ego_provider_name.as_deref(), Some("openai"));
        assert_eq!(resolved.ego_api_key.as_deref(), Some("entity-key"));
    }

    #[test]
    fn resolve_config_falls_to_hive_vault() {
        let config = default_config();

        let entity_vault = temp_vault();
        let hive_vault = temp_vault();
        hive_vault
            .lock()
            .unwrap()
            .set_secret("anthropic", "hive-key");

        let hive = Hive::new(entity_vault, hive_vault);
        let resolved = hive.resolve_config(&config).unwrap();

        assert_eq!(resolved.ego_provider_name.as_deref(), Some("anthropic"));
        assert_eq!(resolved.ego_api_key.as_deref(), Some("hive-key"));
    }

    #[test]
    fn extract_superego_config_present() {
        let mut config = default_config();
        config.trinity = Some(abigail_core::TrinityConfig {
            id_url: None,
            ego_provider: None,
            ego_api_key: None,
            superego_provider: Some("anthropic".to_string()),
            superego_api_key: Some("se-key".to_string()),
        });

        let result = Hive::extract_superego_config(&config);
        assert_eq!(
            result,
            Some(("anthropic".to_string(), "se-key".to_string()))
        );
    }

    #[test]
    fn extract_superego_config_missing() {
        let config = default_config();
        assert!(Hive::extract_superego_config(&config).is_none());
    }

    #[test]
    fn extract_superego_config_empty_key() {
        let mut config = default_config();
        config.trinity = Some(abigail_core::TrinityConfig {
            id_url: None,
            ego_provider: None,
            ego_api_key: None,
            superego_provider: Some("anthropic".to_string()),
            superego_api_key: Some("".to_string()),
        });

        assert!(Hive::extract_superego_config(&config).is_none());
    }

    #[tokio::test]
    async fn build_providers_from_config_no_keys() {
        let config = default_config();
        let hive = Hive::new(temp_vault(), temp_vault());
        let built = hive.build_providers_from_config(&config).await.unwrap();

        assert!(built.ego.is_none());
        assert!(built.ego_kind.is_none());
        assert!(built.superego.is_none());
        assert!(built.local_http.is_none());
    }

    #[tokio::test]
    async fn build_providers_from_config_with_ego() {
        let config = default_config();
        let entity_vault = temp_vault();
        entity_vault
            .lock()
            .unwrap()
            .set_secret("openai", "test-key");

        let hive = Hive::new(entity_vault, temp_vault());
        let built = hive.build_providers_from_config(&config).await.unwrap();

        assert!(built.ego.is_some());
        assert_eq!(built.ego_kind, Some(ProviderKind::OpenAi));
    }
}
