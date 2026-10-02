//! Hive is the only process opening the embedded SurrealKV store. Runtime
//! persistence calls are authorized against the lease's Entity scope.

use crate::state::HiveDaemonState;
use abigail_identity::HiveEntity;
use abigail_persistence::{EntityScope, PersistenceHandle, RemoteOperation, RemoteResponse};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;

pub async fn request(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    headers: HeaderMap,
    Json(operation): Json<RemoteOperation>,
) -> (StatusCode, Json<RemoteResponse>) {
    let lease_id = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authorized = lease_id
        .and_then(|id| state.runtime_control.lock().ok()?.session_status(id))
        .is_some_and(|session| session.lease.entity_id == entity_id);
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(RemoteResponse {
                ok: false,
                result: Value::Null,
                error: Some("A runtime lease for this Entity is required".into()),
            }),
        );
    }
    let path = HiveEntity::memory_db_path(state.identity_manager.data_root());
    let result = tokio::task::spawn_blocking(move || {
        let handle = PersistenceHandle::open(path, EntityScope::Entity(entity_id))?;
        operation.execute(&handle)
    })
    .await;
    match result {
        Ok(Ok(value)) => (
            StatusCode::OK,
            Json(RemoteResponse {
                ok: true,
                result: value,
                error: None,
            }),
        ),
        Ok(Err(error)) => (
            StatusCode::OK,
            Json(RemoteResponse {
                ok: false,
                result: Value::Null,
                error: Some(error.to_string()),
            }),
        ),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(RemoteResponse {
                ok: false,
                result: Value::Null,
                error: Some(error.to_string()),
            }),
        ),
    }
}
