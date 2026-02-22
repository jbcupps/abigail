use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use hive_core::{
    ApiEnvelope, EntityRecord, EntityStatus, HiveStatus, StartStopEntityRequest,
    DEFAULT_HIVE_ADDR, HIVE_API_VERSION_PREFIX,
};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone, Default)]
struct HiveState {
    entities: Arc<RwLock<HashMap<String, EntityRecord>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let state = HiveState::default();
    let app = Router::new()
        .route("/v1/status", get(get_status))
        .route("/v1/entity/list", get(list_entities))
        .route("/v1/entity/start", post(start_entity))
        .route("/v1/entity/stop", post(stop_entity))
        .route("/v1/logs", get(get_logs))
        .with_state(state);

    let addr: SocketAddr = DEFAULT_HIVE_ADDR.parse().context("invalid hive addr")?;
    info!("hive-daemon listening on http://{addr}{HIVE_API_VERSION_PREFIX}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_status(State(state): State<HiveState>) -> Json<ApiEnvelope<HiveStatus>> {
    let managed_entities = state.entities.read().await.len();
    Json(ApiEnvelope {
        ok: true,
        data: HiveStatus {
            service: "hive-daemon".to_string(),
            api_version: "v1".to_string(),
            managed_entities,
        },
    })
}

async fn list_entities(State(state): State<HiveState>) -> Json<ApiEnvelope<Vec<EntityRecord>>> {
    let items = state.entities.read().await.values().cloned().collect();
    Json(ApiEnvelope {
        ok: true,
        data: items,
    })
}

async fn start_entity(
    State(state): State<HiveState>,
    Json(req): Json<StartStopEntityRequest>,
) -> Json<ApiEnvelope<EntityRecord>> {
    let mut lock = state.entities.write().await;
    let record = lock
        .entry(req.id.clone())
        .and_modify(|e| e.status = EntityStatus::Running)
        .or_insert(EntityRecord {
            id: req.id,
            status: EntityStatus::Running,
        })
        .clone();
    Json(ApiEnvelope {
        ok: true,
        data: record,
    })
}

async fn stop_entity(
    State(state): State<HiveState>,
    Json(req): Json<StartStopEntityRequest>,
) -> Json<ApiEnvelope<EntityRecord>> {
    let mut lock = state.entities.write().await;
    let record = lock
        .entry(req.id.clone())
        .and_modify(|e| e.status = EntityStatus::Stopped)
        .or_insert(EntityRecord {
            id: req.id,
            status: EntityStatus::Stopped,
        })
        .clone();
    Json(ApiEnvelope {
        ok: true,
        data: record,
    })
}

async fn get_logs() -> Json<ApiEnvelope<Vec<String>>> {
    Json(ApiEnvelope {
        ok: true,
        data: vec!["local logs only (hive ingest deferred)".to_string()],
    })
}
