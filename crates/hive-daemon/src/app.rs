//! The Hive daemon's HTTP surface.

use crate::state::HiveDaemonState;
use crate::{access, birth, genesis, journey, persistence, routes};
use axum::http::header;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

pub fn router(state: HiveDaemonState) -> Router {
    // Browsers never let a wildcard cover `Authorization`, so name it.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin
                .to_str()
                .ok()
                .and_then(|s| reqwest::Url::parse(s).ok())
                .is_some_and(|url| {
                    matches!(
                        url.host_str(),
                        Some("localhost" | "127.0.0.1" | "tauri.localhost" | "[::1]" | "::1")
                    )
                })
        }))
        .allow_methods(Any)
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]);

    Router::new()
        .merge(genesis::router())
        .route("/health", get(routes::health))
        .route(hive_core::access::ADOPTION_PATH, post(access::adoption))
        .route("/v1/status", get(routes::get_status))
        .route("/v1/entities", get(routes::list_entities))
        .route("/v1/entities", post(routes::create_entity))
        .route("/v1/entities/:id", get(routes::get_entity))
        .route("/v1/entities/:id/open", post(routes::open_entity))
        .route("/v1/entities/:id/close", post(routes::close_entity))
        .route("/v1/entities/:id/persistence", post(persistence::request))
        .route("/v1/entities/:id/journey", get(journey::get_journey))
        .route(
            "/v1/entities/:id/presence",
            get(journey::get_presence)
                .put(journey::put_presence)
                .layer(axum::extract::DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/v1/birth/scenarios", get(birth::get_scenarios))
        .route(
            "/v1/entities/:id/birth",
            get(birth::get_birth_document).post(birth::perform_birth),
        )
        .route(
            "/v1/entities/:id/config",
            axum::routing::patch(routes::update_entity_config),
        )
        .route(
            "/v1/entities/:id/provider-config",
            get(routes::get_provider_config),
        )
        .route("/v1/entities/:id/sign", post(routes::sign_entity))
        .route(
            "/v1/entities/:id/assignments",
            get(routes::get_skill_assignments).post(routes::set_skill_assignments),
        )
        .route(
            "/v1/entities/:id/forge-approvals",
            get(routes::get_forge_approval_jobs).post(routes::create_forge_approval_job),
        )
        .route("/v1/secrets", post(routes::store_secret))
        .route("/v1/secrets/list", get(routes::list_secrets))
        .route("/v1/secrets/:key", get(routes::get_secret))
        .route("/v1/providers/models", post(routes::discover_models))
        .route("/v1/providers/best", get(routes::get_best_model))
        .route("/v1/providers/detect", get(routes::detect_cli))
        .route("/v1/providers/hive-default", post(routes::set_hive_default))
        .route("/v1/providers/local", post(routes::set_local_provider))
        .route(
            "/v1/providers/profiles/:name",
            get(routes::get_provider_profile),
        )
        .route("/v1/runtime/sessions", post(routes::issue_runtime_session))
        .route(
            "/v1/runtime/sessions/:lease_id",
            get(routes::get_runtime_session),
        )
        .route("/v1/runtime/register", post(routes::register_runtime))
        .route(
            "/v1/runtime/heartbeat",
            post(routes::record_runtime_heartbeat),
        )
        .route("/v1/runtime/outbox/sync", post(routes::sync_runtime_outbox))
        .route(
            "/v1/entities/:id/execution/receipts",
            get(routes::get_execution_receipts),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access::authorize,
        ))
        // Outermost, so preflights are answered before authorization.
        .layer(cors)
        .with_state(state)
}
