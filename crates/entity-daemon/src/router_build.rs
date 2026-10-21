//! Builds an `IdEgoRouter` from a Hive-resolved `ProviderConfig`.
//!
//! Shared by daemon startup and the governance hot-swap path so a
//! hive-initiated provider change produces exactly the same router a
//! restart would.

use abigail_router::IdEgoRouter;
use hive_core::ProviderConfig;
use std::sync::Arc;

struct HiveApprovalGate {
    client: crate::hive_client::HiveClient,
    entity_id: String,
}

#[async_trait::async_trait]
impl abigail_hive::approved_dispatch::ApprovalGate for HiveApprovalGate {
    async fn still_approved(
        &self,
        route: &abigail_core::model_routing::ModelConnection,
    ) -> anyhow::Result<bool> {
        let fresh = self.client.get_provider_config(&self.entity_id).await?;
        Ok(fresh
            .approved_connections
            .unwrap_or_default()
            .iter()
            .any(|allowed| allowed.ready && &allowed.connection == route))
    }
}

/// Production runtime entry point: every model attempt rechecks Hive approval.
pub async fn build_router_guarded(
    mut config: ProviderConfig,
    hive_url: &str,
    entity_id: &str,
    identity: Option<&abigail_core::profile::ProcessIdentity>,
) -> IdEgoRouter {
    config.approved_connections.get_or_insert_with(Vec::new);
    let client = crate::hive_client::HiveClient::for_runtime(hive_url, identity);
    if client.is_err() {
        config.approved_connections = Some(Vec::new());
    }
    let mut router = build_router(config).await;
    if let (Ok(client), Some(dispatch)) = (client, router.approved_dispatch.as_mut()) {
        if let Some(dispatch) = Arc::get_mut(dispatch) {
            dispatch.set_gate(Arc::new(HiveApprovalGate {
                client,
                entity_id: entity_id.into(),
            }));
        } else {
            // Never retain an unguarded pool if construction changes to share it.
            router.approved_dispatch = Some(Arc::new(
                abigail_hive::approved_dispatch::ApprovedDispatch::build(&[]).await,
            ));
        }
    }
    router
}

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
            let mut router = IdEgoRouter::new(
                None,
                None,
                None,
                None,
                abigail_core::RoutingMode::EgoPrimary,
            );
            router.approved_dispatch = Some(Arc::new(
                abigail_hive::approved_dispatch::ApprovedDispatch::build(&[]).await,
            ));
            return router;
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
        approved_connections: provider_config.approved_connections,
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
    if let Some(connections) = &mut config.approved_connections {
        for resolved in connections {
            if resolved.connection.provider.ends_with("-cli")
                || (resolved.connection.provider != "local"
                    && !resolved
                        .api_key
                        .as_ref()
                        .is_some_and(|key| !key.trim().is_empty()))
            {
                resolved.ready = false;
                resolved.api_key = None;
            }
        }
    }
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
            approved_connections: None,
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

    #[test]
    fn approved_policy_does_not_enable_preview_cli_accounts() {
        let mut input = config("openai", None);
        input.approved_connections =
            Some(vec![abigail_core::model_routing::ResolvedModelConnection {
                connection: abigail_core::model_routing::ModelConnection {
                    provider: "codex-cli".into(),
                    model: None,
                    local_url: None,
                },
                api_key: Some("synthetic-fixture".into()),
                ready: true,
            }]);
        let actual = preview_provider_config(input);
        let routes = actual.approved_connections.unwrap();
        assert!(!routes[0].ready);
        assert!(routes[0].api_key.is_none());
        assert_eq!(routes[0].connection.provider, "codex-cli"); // approval is retained, availability is honest
    }

    #[tokio::test]
    async fn old_handoff_without_approval_never_enables_a_model() {
        let router = build_router_guarded(
            config("openai", Some("synthetic-fixture")),
            "http://127.0.0.1:1",
            "fixture",
            None,
        )
        .await;
        assert!(router.approved_dispatch.is_some());
        assert!(!router.has_ego());
        assert!(router
            .route_unified(abigail_router::RoutingRequest::simple(vec![]))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn production_router_rechecks_hive_before_each_actual_dispatch() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let allowed = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let checks = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mut input = config("openai", None);
        input.approved_connections =
            Some(vec![abigail_core::model_routing::ResolvedModelConnection {
                connection: abigail_core::model_routing::ModelConnection {
                    provider: "local".into(),
                    model: Some("fixture-approved".into()),
                    local_url: Some(url.clone()),
                },
                api_key: None,
                ready: true,
            }]);
        let route_config = input.clone();
        let can_use = allowed.clone();
        let gate_checks = checks.clone();
        let provider_calls = calls.clone();
        let app = axum::Router::new()
            .route("/v1/entities/fixture/provider-config", axum::routing::get(move || {
                let mut config = route_config.clone();
                let enabled = can_use.load(Ordering::SeqCst);
                gate_checks.fetch_add(1, Ordering::SeqCst);
                async move {
                    if !enabled { config.approved_connections = Some(vec![]); }
                    axum::Json(hive_core::ApiEnvelope::success(config))
                }
            }))
            .route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(body["model"], "fixture-approved");
                    axum::Json(serde_json::json!({"choices":[{"message":{"role":"assistant","content":"fixture response"}}]}))
                }
            }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let router = build_router_guarded(input, &url, "fixture", None).await;
        let diagnosis = router.diagnose("fixture question");
        assert_eq!(diagnosis.target, "local");
        assert_eq!(
            diagnosis.selected_model.as_deref(),
            Some("fixture-approved")
        );
        let first = router
            .route_unified(abigail_router::RoutingRequest::simple(vec![]))
            .await
            .unwrap();
        assert_eq!(first.completion.content, "fixture response");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        allowed.store(false, Ordering::SeqCst);
        assert!(router
            .route_unified(abigail_router::RoutingRequest::simple(vec![]))
            .await
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(checks.load(Ordering::SeqCst), 2);
        server.abort();
    }
}
