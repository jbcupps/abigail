//! Builds an `IdEgoRouter` from a Hive-resolved `ProviderConfig`.
//!
//! Shared by daemon startup and the governance hot-swap path so a
//! hive-initiated provider change produces exactly the same router a
//! restart would.

use abigail_router::IdEgoRouter;
use hive_core::ProviderConfig;

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
    let cli_permission_mode = provider_config
        .cli_permission_mode
        .as_deref()
        .and_then(|s| {
            serde_json::from_str::<abigail_core::CliPermissionMode>(&format!("\"{s}\"")).ok()
        })
        .unwrap_or_default();

    let ego_api_key = provider_config
        .ego_api_key
        .clone()
        .filter(|key| !key.trim().is_empty());
    let hive_config = abigail_hive::HiveConfig {
        local_llm_base_url: provider_config.local_llm_base_url,
        ego_provider: provider_config.ego_provider_name.map(|provider| {
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
