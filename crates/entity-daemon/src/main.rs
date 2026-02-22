use std::net::SocketAddr;

use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, RunRequest, RunResponse,
    DEFAULT_ENTITY_ADDR, ENTITY_API_VERSION_PREFIX,
};
use tracing::info;

#[derive(Clone, Default)]
struct EntityState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let app = Router::new()
        .route("/v1/status", get(get_status))
        .route("/v1/run", post(run_task))
        .route("/v1/chat", post(chat))
        .route("/v1/logs", get(get_logs))
        .with_state(EntityState);

    let addr: SocketAddr = DEFAULT_ENTITY_ADDR.parse().context("invalid entity addr")?;
    info!("entity-daemon listening on http://{addr}{ENTITY_API_VERSION_PREFIX}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_status(State(_state): State<EntityState>) -> Json<ApiEnvelope<EntityStatus>> {
    Json(ApiEnvelope {
        ok: true,
        data: EntityStatus {
            service: "entity-daemon".to_string(),
            api_version: "v1".to_string(),
            mode: "daemon".to_string(),
        },
    })
}

async fn run_task(
    State(_state): State<EntityState>,
    Json(req): Json<RunRequest>,
) -> Json<ApiEnvelope<RunResponse>> {
    Json(ApiEnvelope {
        ok: true,
        data: RunResponse {
            accepted: true,
            task: req.task,
        },
    })
}

async fn chat(
    State(_state): State<EntityState>,
    Json(req): Json<ChatRequest>,
) -> Json<ApiEnvelope<ChatResponse>> {
    let reply = format!("entity daemon received: {}", req.message);
    Json(ApiEnvelope {
        ok: true,
        data: ChatResponse { reply },
    })
}

async fn get_logs() -> Json<ApiEnvelope<Vec<String>>> {
    Json(ApiEnvelope {
        ok: true,
        data: vec!["local entity logs only (hive ingest deferred)".to_string()],
    })
}
