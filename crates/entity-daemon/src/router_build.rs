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
