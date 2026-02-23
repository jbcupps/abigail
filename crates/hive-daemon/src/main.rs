use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use hive_core::{
    ApiEnvelope, BirthEntityRequest, EntityRecord, EntityStatus, HiveStatus,
    StartStopEntityRequest, DEFAULT_HIVE_ADDR, HIVE_API_VERSION_PREFIX,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EntityRegistrySnapshot {
    entities: HashMap<String, EntityRecord>,
}

#[derive(Clone)]
struct HiveState {
    entities: Arc<RwLock<HashMap<String, EntityRecord>>>,
    registry_path: Arc<PathBuf>,
}

impl HiveState {
    fn from_env() -> anyhow::Result<Self> {
        Self::from_path(resolve_registry_path())
    }

    fn from_path(registry_path: PathBuf) -> anyhow::Result<Self> {
        let entities = load_registry_snapshot(&registry_path)?;
        Ok(Self {
            entities: Arc::new(RwLock::new(entities)),
            registry_path: Arc::new(registry_path),
        })
    }

    fn registry_path(&self) -> &Path {
        self.registry_path.as_ref().as_path()
    }

    async fn persist(&self) -> anyhow::Result<()> {
        let snapshot = {
            let entities = self.entities.read().await;
            EntityRegistrySnapshot {
                entities: entities.clone(),
            }
        };
        persist_registry_snapshot(self.registry_path(), &snapshot)
    }
}

fn resolve_registry_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HIVE_REGISTRY_PATH") {
        return PathBuf::from(path);
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("hive-entities.json")
}

fn load_registry_snapshot(path: &Path) -> anyhow::Result<HashMap<String, EntityRecord>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read hive registry at {}", path.display()))?;
    if bytes.is_empty() {
        return Ok(HashMap::new());
    }

    let snapshot: EntityRegistrySnapshot = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse hive registry at {}", path.display()))?;
    Ok(snapshot.entities)
}

fn persist_registry_snapshot(path: &Path, snapshot: &EntityRegistrySnapshot) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(snapshot)
        .with_context(|| format!("failed to serialize hive registry at {}", path.display()))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create registry directory {}",
                parent.to_string_lossy()
            )
        })?;
    }

    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, bytes).with_context(|| {
        format!(
            "failed to write temporary hive registry {}",
            tmp_path.to_string_lossy()
        )
    })?;
    if path.exists() {
        std::fs::remove_file(path).with_context(|| {
            format!(
                "failed to remove existing hive registry {}",
                path.to_string_lossy()
            )
        })?;
    }
    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to move temporary registry {} -> {}",
            tmp_path.to_string_lossy(),
            path.to_string_lossy()
        )
    })?;
    Ok(())
}

async fn persist_or_log(state: &HiveState, action: &str) {
    if let Err(err) = state.persist().await {
        error!(
            error = %err,
            path = %state.registry_path().display(),
            "failed to persist hive registry after {}",
            action
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let state = HiveState::from_env().context("failed to initialize hive state")?;
    let loaded_entities = state.entities.read().await.len();
    info!(
        "loaded {} entities from {}",
        loaded_entities,
        state.registry_path().display()
    );
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/v1/status", get(get_status))
        .route("/v1/entity/list", get(list_entities))
        .route("/v1/entity/birth", post(birth_entity))
        .route("/v1/entity/start", post(start_entity))
        .route("/v1/entity/stop", post(stop_entity))
        .route("/v1/logs", get(get_logs))
        .layer(cors)
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
    let mut items: Vec<EntityRecord> = state.entities.read().await.values().cloned().collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    Json(ApiEnvelope {
        ok: true,
        data: items,
    })
}

async fn start_entity(
    State(state): State<HiveState>,
    Json(req): Json<StartStopEntityRequest>,
) -> Json<ApiEnvelope<EntityRecord>> {
    let record = {
        let mut lock = state.entities.write().await;
        lock.entry(req.id.clone())
            .and_modify(|e| e.status = EntityStatus::Running)
            .or_insert(EntityRecord {
                id: req.id,
                status: EntityStatus::Running,
                birth_complete: false,
                birth_path: None,
            })
            .clone()
    };
    persist_or_log(&state, "start_entity").await;
    Json(ApiEnvelope {
        ok: true,
        data: record,
    })
}

async fn stop_entity(
    State(state): State<HiveState>,
    Json(req): Json<StartStopEntityRequest>,
) -> Json<ApiEnvelope<EntityRecord>> {
    let record = {
        let mut lock = state.entities.write().await;
        lock.entry(req.id.clone())
            .and_modify(|e| e.status = EntityStatus::Stopped)
            .or_insert(EntityRecord {
                id: req.id,
                status: EntityStatus::Stopped,
                birth_complete: false,
                birth_path: None,
            })
            .clone()
    };
    persist_or_log(&state, "stop_entity").await;
    Json(ApiEnvelope {
        ok: true,
        data: record,
    })
}

async fn birth_entity(
    State(state): State<HiveState>,
    Json(req): Json<BirthEntityRequest>,
) -> Json<ApiEnvelope<EntityRecord>> {
    let record = {
        let mut lock = state.entities.write().await;
        lock.entry(req.id.clone())
            .and_modify(|e| {
                e.birth_complete = true;
                e.birth_path = Some(req.path);
            })
            .or_insert(EntityRecord {
                id: req.id,
                status: EntityStatus::Stopped,
                birth_complete: true,
                birth_path: Some(req.path),
            })
            .clone()
    };
    persist_or_log(&state, "birth_entity").await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::BirthPath;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_registry_path(test_name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "abigail_hive_registry_{}_{}_{}.json",
            test_name,
            std::process::id(),
            ts
        ))
    }

    fn test_state(test_name: &str) -> (HiveState, PathBuf) {
        let path = unique_registry_path(test_name);
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        (HiveState::from_path(path.clone()).unwrap(), path)
    }

    #[tokio::test]
    async fn birth_creates_entity_with_path() {
        let (state, registry_path) = test_state("birth");
        let res = birth_entity(
            State(state),
            Json(BirthEntityRequest {
                id: "adam".to_string(),
                path: BirthPath::Direct,
            }),
        )
        .await;

        assert!(res.0.ok);
        assert_eq!(res.0.data.id, "adam");
        assert_eq!(res.0.data.status, EntityStatus::Stopped);
        assert!(res.0.data.birth_complete);
        assert_eq!(res.0.data.birth_path, Some(BirthPath::Direct));
        if registry_path.exists() {
            std::fs::remove_file(registry_path).unwrap();
        }
    }

    #[tokio::test]
    async fn start_after_birth_preserves_birth_metadata() {
        let (state, registry_path) = test_state("preserve_birth");
        let shared = State(state.clone());
        let _ = birth_entity(
            shared.clone(),
            Json(BirthEntityRequest {
                id: "adam".to_string(),
                path: BirthPath::SoulForge,
            }),
        )
        .await;
        let started = start_entity(
            shared.clone(),
            Json(StartStopEntityRequest {
                id: "adam".to_string(),
            }),
        )
        .await;
        let stopped = stop_entity(
            shared,
            Json(StartStopEntityRequest {
                id: "adam".to_string(),
            }),
        )
        .await;

        assert_eq!(started.0.data.status, EntityStatus::Running);
        assert!(started.0.data.birth_complete);
        assert_eq!(started.0.data.birth_path, Some(BirthPath::SoulForge));
        assert_eq!(stopped.0.data.status, EntityStatus::Stopped);
        assert!(stopped.0.data.birth_complete);
        assert_eq!(stopped.0.data.birth_path, Some(BirthPath::SoulForge));
        if registry_path.exists() {
            std::fs::remove_file(registry_path).unwrap();
        }
    }

    #[tokio::test]
    async fn start_without_birth_defaults_to_unbirthed_record() {
        let (state, registry_path) = test_state("start_without_birth");
        let started = start_entity(
            State(state),
            Json(StartStopEntityRequest {
                id: "no-birth-yet".to_string(),
            }),
        )
        .await;

        assert_eq!(started.0.data.status, EntityStatus::Running);
        assert!(!started.0.data.birth_complete);
        assert!(started.0.data.birth_path.is_none());
        if registry_path.exists() {
            std::fs::remove_file(registry_path).unwrap();
        }
    }

    #[tokio::test]
    async fn registry_survives_restart() {
        let (state, registry_path) = test_state("restart");
        let shared = State(state.clone());

        let _ = birth_entity(
            shared.clone(),
            Json(BirthEntityRequest {
                id: "adam".to_string(),
                path: BirthPath::Direct,
            }),
        )
        .await;
        let _ = start_entity(
            shared,
            Json(StartStopEntityRequest {
                id: "adam".to_string(),
            }),
        )
        .await;

        let reloaded = HiveState::from_path(registry_path.clone()).unwrap();
        let listed = list_entities(State(reloaded)).await;
        assert_eq!(listed.0.data.len(), 1);
        let adam = listed.0.data.first().unwrap();
        assert_eq!(adam.id, "adam");
        assert_eq!(adam.status, EntityStatus::Running);
        assert!(adam.birth_complete);
        assert_eq!(adam.birth_path, Some(BirthPath::Direct));

        if registry_path.exists() {
            std::fs::remove_file(registry_path).unwrap();
        }
    }
}
