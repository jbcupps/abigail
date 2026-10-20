//! Builds an `IdEgoRouter` from a Hive-resolved `ProviderConfig`.
//!
//! Shared by daemon startup and the governance hot-swap path so a
//! hive-initiated provider change produces exactly the same router a
//! restart would.

use abigail_router::IdEgoRouter;
use hive_core::ProviderConfig;

pub(crate) fn is_supported_cli_provider(provider: &str) -> bool {
    matches!(provider, "claude-cli" | "codex-cli" | "grok-cli")
}

pub fn parse_routing_mode(s: &str) -> abigail_core::RoutingMode {
    match s {
        "EgoPrimary" | "TierBased" | "IdPrimary" | "Council" => {
            abigail_core::RoutingMode::EgoPrimary
        }
        "CliOrchestrator" => abigail_core::RoutingMode::CliOrchestrator,
        _ => abigail_core::RoutingMode::default(),
    }
}

/// Build providers and a router from the Hive's resolved provider config.
pub async fn build_router(provider_config: ProviderConfig) -> IdEgoRouter {
    let provider_config = match abigail_core::profile::current() {
        Ok(Some(_)) => preview_provider_config(provider_config),
        Ok(None) => provider_config,
        Err(error) => {
            tracing::error!("Provider setup refused before preview preflight: {error}");
            return IdEgoRouter::new(
                None,
                None,
                None,
                None,
                abigail_core::RoutingMode::EgoPrimary,
            );
        }
    };
    // Runtime tool approval is not exposed by the MVP. Never pass a stored
    // skip-permissions preference to an independently operating CLI model.
    let cli_permission_mode = abigail_core::CliPermissionMode::AllowListOnly;

    let ego_api_key = provider_config
        .ego_api_key
        .clone()
        .filter(|key| !key.trim().is_empty());
    let hive_config = abigail_hive::HiveConfig {
        local_llm_base_url: provider_config.local_llm_base_url,
        ego_provider: provider_config
            .ego_provider_name
            .filter(|provider| {
                if provider.ends_with("-cli") && !is_supported_cli_provider(provider) {
                    tracing::warn!(
                        "This CLI model provider has no supported read-only integration"
                    );
                    false
                } else {
                    true
                }
            })
            .map(|provider| {
                // API-key providers get their key; CLI providers use system auth.
                let auth = match ego_api_key {
                    Some(key) => abigail_hive::ProviderAuth::ApiKey(key),
                    None => abigail_hive::ProviderAuth::System,
                };
                abigail_hive::ProviderSelection { provider, auth }
            }),
        ego_model: provider_config.ego_model,
        routing_mode: parse_routing_mode(&provider_config.routing_mode),
        cli_permission_mode,
    };

    let built = abigail_hive::Hive::build_providers(&hive_config).await;
    IdEgoRouter::from_built_providers(built)
}

pub(crate) fn preview_provider_config(mut config: ProviderConfig) -> ProviderConfig {
    let explicit_key = config
        .ego_api_key
        .as_deref()
        .is_some_and(|key| !key.trim().is_empty());
    if !explicit_key
        || config
            .ego_provider_name
            .as_deref()
            .is_some_and(|provider| provider.ends_with("-cli"))
    {
        config.ego_provider_name = None;
        config.ego_api_key = None;
        config.ego_model = None;
    }
    config.local_llm_base_url = config
        .local_llm_base_url
        .filter(|url| !url.trim().is_empty());
    if config.routing_mode == "CliOrchestrator" {
        config.routing_mode = "EgoPrimary".into();
    }
    config
}

#[cfg(test)]
mod preview_provider_tests {
    use super::*;

    fn config(provider: &str, key: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            ego_provider_name: Some(provider.into()),
            ego_api_key: key.map(String::from),
            ego_model: Some("synthetic-model".into()),
            local_llm_base_url: None,
            routing_mode: "CliOrchestrator".into(),
            cli_permission_mode: None,
        }
    }

    #[test]
    fn preview_provider_selection_never_reuses_system_cli_auth() {
        for provider in ["claude-cli", "codex-cli", "grok-cli", "gemini-cli"] {
            let actual = preview_provider_config(config(provider, Some("synthetic-key")));
            assert!(actual.ego_provider_name.is_none());
            assert!(actual.ego_api_key.is_none());
            assert!(actual.ego_model.is_none());
            assert_eq!(actual.routing_mode, "EgoPrimary");
            assert!(actual.local_llm_base_url.is_none());
        }
    }

    #[test]
    fn preview_cloud_requires_explicit_key_and_preserves_explicit_local_url() {
        for key in [None, Some(""), Some(" ")] {
            assert!(preview_provider_config(config("openai", key))
                .ego_provider_name
                .is_none());
        }
        let mut explicit = config("openai", Some("synthetic-key"));
        explicit.local_llm_base_url = Some("http://127.0.0.1:8123".into());
        let actual = preview_provider_config(explicit);
        assert_eq!(actual.ego_provider_name.as_deref(), Some("openai"));
        assert_eq!(actual.ego_api_key.as_deref(), Some("synthetic-key"));
        assert_eq!(
            actual.local_llm_base_url.as_deref(),
            Some("http://127.0.0.1:8123")
        );
    }
}
