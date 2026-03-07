//! Hive: single authority for secret resolution and provider construction.
//!
//! The Hive owns vault references and resolves which provider/key to use
//! based on the priority chain: active preference → entity vault → hive vault
//! → trinity config → environment variables.

use crate::provider_registry::{ProviderKind, ProviderRegistry};
use abigail_capabilities::cognitive::{
    detect_all_cli_providers, CliDetectionResult, LlmProvider, LocalHttpProvider,
};
use abigail_core::{AppConfig, CliPermissionMode, RoutingMode, SecretsVault};
use std::sync::{Arc, Mutex};

/// Check whether a binary is reachable on the system PATH.
pub fn is_binary_on_path(name: &str) -> bool {
    #[cfg(windows)]
    let check = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new("where");
        cmd.arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        cmd.status()
    };
    #[cfg(not(windows))]
    let check = std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    check.map(|s| s.success()).unwrap_or(false)
}

/// Detect all CLI tools with full verification (official + auth status).
pub fn detect_cli_providers_full() -> Vec<CliDetectionResult> {
    detect_all_cli_providers()
}

/// Fully resolved configuration ready for provider construction.
#[derive(Debug, Clone)]
pub struct HiveConfig {
    pub local_llm_base_url: Option<String>,
    pub ego_provider: Option<ProviderSelection>,
    pub ego_model: Option<String>,
    pub routing_mode: RoutingMode,
    /// Permission mode for CLI tool invocations.
    pub cli_permission_mode: CliPermissionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAuth {
    ApiKey(String),
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelection {
    pub provider: String,
    pub auth: ProviderAuth,
}

impl ProviderSelection {
    pub fn provider_name(&self) -> &str {
        &self.provider
    }

    pub fn api_key(&self) -> Option<String> {
        match &self.auth {
            ProviderAuth::ApiKey(key) => Some(key.clone()),
            ProviderAuth::System => None,
        }
    }
}

/// All providers built and ready to be injected into a router.
pub struct BuiltProviders {
    pub id: Arc<dyn LlmProvider>,
    pub local_http: Option<Arc<LocalHttpProvider>>,
    pub ego: Option<Arc<dyn LlmProvider>>,
    pub ego_kind: Option<ProviderKind>,
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

    /// CLI provider names that can operate without a stored API key
    /// (they use their own auth, e.g. `claude auth login`).
    const CLI_PROVIDERS: &'static [&'static str] =
        &["claude-cli", "gemini-cli", "codex-cli", "grok-cli"];

    fn is_cli_provider(name: &str) -> bool {
        Self::CLI_PROVIDERS.contains(&name)
    }

    /// Determine which Ego provider and key to use based on the priority chain:
    ///
    /// 1. Explicit `active_provider_preference` from Mentor/Forge menu
    /// 2. Entity-level vault scan (keys pasted in chat or Connectivity)
    /// 3. Trinity config (legacy/manual paths)
    /// 4. Environment variables (last resort)
    /// 5. Auto-detect installed CLI tools on PATH
    pub fn determine_ego_provider(
        config: &AppConfig,
        vault: &SecretsVault,
    ) -> Option<ProviderSelection> {
        // Helper to avoid propagating full secrets into logs or debug output.
        fn redact_secret(secret: &str) -> String {
            let trimmed = secret.trim();
            if trimmed.len() <= 4 {
                "***redacted***".to_string()
            } else {
                let suffix = &trimmed[trimmed.len() - 4..];
                format!("***{}", suffix)
            }
        }

        // 1. Explicit preference from Mentor menu (Forge)
        if let Some(pref) = &config.active_provider_preference {
            // CLI providers work without a stored key (OAuth / built-in auth)
            if Self::is_cli_provider(pref) {
                return Some(
                    vault
                        .get_secret(pref)
                        .map(str::trim)
                        .filter(|k| !k.is_empty())
                        // Do not propagate the actual secret to avoid accidental logging.
                        .map(|_key| ProviderSelection {
                            auth: ProviderAuth::ApiKey(redact_secret(key)),
                            auth: ProviderAuth::ApiKey("<redacted>".to_string()),
                        })
                        .unwrap_or_else(|| ProviderSelection {
                            provider: pref.clone(),
                            auth: ProviderAuth::System,
                        }),
                );
            }
            if let Some(selection) = vault
                .get_secret(pref)
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(|key| ProviderSelection {
                    auth: ProviderAuth::ApiKey(redact_secret(key)),
                    auth: ProviderAuth::ApiKey(key.to_string()),
                })
            {
                return Some(selection);
            }
        }

        // 2. Local Vault (keys pasted in chat or added in Connectivity)
            if let Some(raw_key) = vault.get_secret(name) {
                let key = raw_key.trim();
                if !key.is_empty() {
                    let selection = ProviderSelection {
                        provider: (*name).to_string(),
                        auth: ProviderAuth::ApiKey(key.to_string()),
                    };
                    return Some(selection);
                }
        ];
        for name in &provider_names {
            if let Some(selection) = vault
                .get_secret(name)
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(|key| ProviderSelection {
                    auth: ProviderAuth::ApiKey(redact_secret(key)),
                    auth: ProviderAuth::ApiKey(key.to_string()),
                })
            {
                return Some(selection);
            }
        }

        // 3. Trinity config (legacy/manual paths)
        if let Some(trinity) = &config.trinity {
            if let Some(p) = &trinity.ego_provider {
                if let Some(k) = &trinity.ego_api_key {
                    if !k.is_empty() {
                        return Some(ProviderSelection {
                            auth: ProviderAuth::ApiKey(redact_secret(k)),
                            auth: ProviderAuth::ApiKey(k.clone()),
                        });
                    }
                }
            }
        }

        // 4. Environment variables (last resort)
        if let Some(k) = &config.openai_api_key {
            if !k.is_empty() {
                return Some(ProviderSelection {
                    auth: ProviderAuth::ApiKey(redact_secret(k)),
                    auth: ProviderAuth::ApiKey(k.clone()),
                });
            }
        }

        None
    }

    /// Detect CLI tools installed on PATH that can serve as Ego providers
    /// via their own authentication (OAuth / `claude auth login`).
    fn detect_cli_on_path() -> Option<ProviderSelection> {
        let cli_binaries = [
            ("claude-cli", "claude"),
            ("gemini-cli", "gemini"),
            ("codex-cli", "codex"),
            ("grok-cli", "grok"),
        ];
        for (provider, binary) in &cli_binaries {
            if is_binary_on_path(binary) {
                tracing::info!(
                    "Auto-detected {} on PATH — selecting {} provider (OAuth auth)",
                    binary,
                    provider
                );
                return Some(ProviderSelection {
                    provider: provider.to_string(),
                    auth: ProviderAuth::System,
                });
            }
        }
        None
    }

    /// Resolve the full provider configuration from AppConfig + vaults.
    ///
    /// Acquires locks on `secrets` then `hive_secrets` (in documented order).
    pub fn resolve_config(&self, config: &AppConfig) -> Result<HiveConfig, String> {
        let ego_provider = {
            let vault = self.secrets.lock().map_err(|e| e.to_string())?;
            let selection = Self::determine_ego_provider(config, &vault);
            if selection.is_some() {
                tracing::info!(
                    "Using Ego provider from preference/vault: {:?}",
                    selection.as_ref().map(|s| s.provider_name())
                );
                selection
            } else {
                drop(vault);
                let hive = self.hive_secrets.lock().map_err(|e| e.to_string())?;
                let selection = Self::determine_ego_provider(config, &hive);
                if selection.is_some() {
                    tracing::info!(
                        "Using Ego provider from Hive vault: {:?}",
                        selection.as_ref().map(|s| s.provider_name())
                    );
                    selection
                } else {
                    // Last resort: auto-detect CLI tools on PATH (OAuth auth)
                    Self::detect_cli_on_path()
                }
            }
        };

        tracing::debug!(
            "Resolved config: local_url={:?}, ego_name={:?}, has_ego_key={}, mode={:?}",
            config.local_llm_base_url,
            ego_provider.as_ref().map(|s| s.provider_name()),
            ego_provider
                .as_ref()
                .map(|selection| matches!(selection.auth, ProviderAuth::ApiKey(_)))
                .unwrap_or(false),
            config.routing_mode
        );

        Ok(HiveConfig {
            local_llm_base_url: config.local_llm_base_url.clone(),
            ego_provider,
            ego_model: None,
            routing_mode: config.routing_mode,
            cli_permission_mode: config.cli_permission_mode,
        })
    }

    /// Build all providers from a resolved HiveConfig (no locking).
    pub async fn build_providers(hive_config: &HiveConfig) -> BuiltProviders {
        let ego_result = ProviderRegistry::build_ego_with_cli_mode(
            hive_config.ego_provider.as_ref(),
            hive_config.ego_model.clone(),
            hive_config.cli_permission_mode,
        );

        let id_result =
            ProviderRegistry::build_id_auto_detect(hive_config.local_llm_base_url.clone()).await;

        BuiltProviders {
            id: id_result.provider,
            local_http: id_result.local_http,
            ego: ego_result.provider,
            ego_kind: ego_result.kind,
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

        let selection = Hive::determine_ego_provider(&config, &vault.lock().unwrap()).unwrap();
        assert_eq!(selection.provider_name(), "anthropic");
        assert_eq!(selection.api_key().as_deref(), Some("anthro-key-123"));
    }

    #[test]
    fn determine_ego_scans_vault_order() {
        let config = default_config();
        let vault = temp_vault();
        // Only xai has a key
        vault.lock().unwrap().set_secret("xai", "xai-key");

        let selection = Hive::determine_ego_provider(&config, &vault.lock().unwrap()).unwrap();
        assert_eq!(selection.provider_name(), "xai");
    }

    #[test]
    fn determine_ego_falls_to_trinity() {
        let mut config = default_config();
        config.trinity = Some(abigail_core::TrinityConfig {
            id_url: None,
            ego_provider: Some("google".to_string()),
            ego_api_key: Some("google-key".to_string()),
        });
        let vault = temp_vault();

        let selection = Hive::determine_ego_provider(&config, &vault.lock().unwrap()).unwrap();
        assert_eq!(selection.provider_name(), "google");
        assert_eq!(selection.api_key().as_deref(), Some("google-key"));
    }

    #[test]
    fn determine_ego_falls_to_env_key() {
        let mut config = default_config();
        config.openai_api_key = Some("env-key".to_string());
        let vault = temp_vault();

        let selection = Hive::determine_ego_provider(&config, &vault.lock().unwrap()).unwrap();
        assert_eq!(selection.provider_name(), "openai");
        assert_eq!(selection.api_key().as_deref(), Some("env-key"));
    }

    #[test]
    fn determine_ego_returns_none_when_empty() {
        let config = default_config();
        let vault = temp_vault();

        let selection = Hive::determine_ego_provider(&config, &vault.lock().unwrap());
        assert!(selection.is_none());
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

        let selection = resolved.ego_provider.unwrap();
        assert_eq!(selection.provider_name(), "openai");
        assert_eq!(selection.api_key().as_deref(), Some("entity-key"));
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

        let selection = resolved.ego_provider.unwrap();
        assert_eq!(selection.provider_name(), "anthropic");
        assert_eq!(selection.api_key().as_deref(), Some("hive-key"));
    }

    #[tokio::test]
    async fn build_providers_from_config_no_keys() {
        let config = default_config();
        let hive = Hive::new(temp_vault(), temp_vault());
        let built = hive.build_providers_from_config(&config).await.unwrap();

        // If a CLI tool (e.g. `claude`) is on PATH, auto-detection will
        // select it as the Ego provider even with no stored keys.
        // Otherwise Ego remains None.
        if built.ego.is_some() {
            assert!(
                matches!(
                    built.ego_kind,
                    Some(ProviderKind::ClaudeCli)
                        | Some(ProviderKind::GeminiCli)
                        | Some(ProviderKind::CodexCli)
                        | Some(ProviderKind::GrokCli)
                ),
                "auto-detected ego should be a CLI provider, got {:?}",
                built.ego_kind
            );
        } else {
            assert!(built.ego_kind.is_none());
        }
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
