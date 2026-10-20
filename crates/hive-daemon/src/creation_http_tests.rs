//! Direct Axum-handler tests with the real IdentityManager and temporary files.
//! No socket, service, vault unlock, credential provider, or model is started.

use super::*;
use abigail_core::global_config::{AgentCreationRecord, AgentCreationState};
use abigail_core::{GlobalConfig, SecretsVault};
use abigail_hive::Hive;
use abigail_identity::IdentityManager;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("abigail-creation-http-{}", uuid::Uuid::new_v4())))
    }

    fn state(&self) -> HiveDaemonState {
        let identity_manager =
            Arc::new(IdentityManager::isolated_creation_test_manager(self.0.clone()).unwrap());
        // These constructors only hold empty values. No load/save/unlock is called.
        let entity_secrets = Arc::new(Mutex::new(SecretsVault::new(self.0.join("entity-secrets"))));
        let hive_secrets = Arc::new(Mutex::new(SecretsVault::new(self.0.join("hive-secrets"))));
        HiveDaemonState {
            identity_manager,
            hive: Arc::new(Hive::new(entity_secrets, hive_secrets.clone())),
            hive_secrets,
            hive_url: "http://127.0.0.1:43141".to_string(),
            runtime_control: Arc::new(Mutex::new(
                crate::runtime_registry::RuntimeControlPlane::default(),
            )),
            helper_url: Arc::new(Mutex::new(None)),
            supervisor: crate::supervisor::HiveSupervisor::new(
                "http://127.0.0.1:43141".to_string(),
                self.0.clone(),
            ),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(name: &str, key: &str) -> Json<CreateEntityRequest> {
    Json(CreateEntityRequest {
        name: name.to_string(),
        creation_key: Some(key.to_string()),
    })
}

#[tokio::test]
async fn creation_absent_capability_probe_is_read_only() {
    let fixture = Fixture::new();
    let state = fixture.state();
    let key = uuid::Uuid::new_v4().to_string();
    let (status, Json(body)) = get_entity_creation(State(state), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::to_value(body).unwrap(),
        serde_json::json!({
            "ok": true, "data": {"creation_key": key, "state": "absent"}
        })
    );
    assert!(!GlobalConfig::config_path(&fixture.0).exists());
    assert!(!fixture.0.join("identities").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn creation_concurrent_handler_replay_and_restart_return_one_entity() {
    let fixture = Fixture::new();
    let state = fixture.state();
    let key = uuid::Uuid::new_v4().to_string();
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            let key = key.clone();
            tokio::spawn(async move { create_entity(State(state), request("Signal", &key)).await })
        })
        .collect();
    let mut ids = Vec::new();
    for task in tasks {
        let (status, Json(body)) = task.await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert!(body.ok);
        let data = body.data.unwrap();
        assert_eq!(data.creation_key.as_deref(), Some(key.as_str()));
        ids.push(data.id);
    }
    assert!(ids.iter().all(|id| id == &ids[0]));
    drop(state);
    let restarted = fixture.state();
    let (status, Json(replay)) =
        create_entity(State(restarted.clone()), request("Signal", &key)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay.data.unwrap().id, ids[0]);
    let (status, Json(found)) = get_entity_creation(State(restarted), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    let data = found.data.unwrap();
    assert_eq!(data.creation_key, key);
    assert!(
        matches!(data.status, EntityCreationState::Complete { id, name, .. } if id == ids[0] && name == "Signal")
    );
    assert_eq!(GlobalConfig::load(&fixture.0).unwrap().agents.len(), 1);
}

#[tokio::test]
async fn creation_reserved_recovery_resumes_the_persisted_id() {
    let fixture = Fixture::new();
    let state = fixture.state();
    let key = uuid::Uuid::new_v4().to_string();
    let reserved_id = uuid::Uuid::new_v4().to_string();
    {
        let mut gc = state.identity_manager.global_config().write().unwrap();
        gc.creation_attempts.insert(
            key.clone(),
            AgentCreationRecord {
                name: "Signal".to_string(),
                entity_id: reserved_id.clone(),
                state: AgentCreationState::Reserved,
            },
        );
        gc.save(&fixture.0).unwrap();
    }
    drop(state);
    let restarted = fixture.state();
    let (status, Json(found)) =
        get_entity_creation(State(restarted.clone()), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(found.data.unwrap().status, EntityCreationState::Reserved { name } if name == "Signal")
    );
    assert!(!fixture.0.join("identities").exists());
    let (status, Json(created)) = create_entity(State(restarted), request("Signal", &key)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created.data.unwrap().id, reserved_id);
}

#[tokio::test]
async fn creation_bad_request_conflict_and_deleted_completion_have_distinct_statuses() {
    let fixture = Fixture::new();
    let state = fixture.state();
    let key = uuid::Uuid::new_v4().to_string();
    let (status, Json(bad)) =
        create_entity(State(state.clone()), request("Signal", "invalid")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!bad.ok);
    assert!(bad.data.is_none());
    let (status, Json(bad_lookup)) =
        get_entity_creation(State(state.clone()), Path("invalid".into())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!bad_lookup.ok);
    let (_, Json(created)) = create_entity(State(state.clone()), request("Signal", &key)).await;
    let id = created.data.unwrap().id;
    let (status, Json(conflict)) =
        create_entity(State(state.clone()), request("Different", &key)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!conflict.ok);
    state.identity_manager.delete_agent(&id).unwrap();
    drop(state);
    let restarted = fixture.state();
    let (status, Json(found)) =
        get_entity_creation(State(restarted.clone()), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(found.data.unwrap().status, EntityCreationState::Unavailable { id: found_id, .. } if found_id == id)
    );
    let (status, Json(gone)) = create_entity(State(restarted), request("Signal", &key)).await;
    assert_eq!(status, StatusCode::GONE);
    assert!(!gone.ok);
    assert!(gone.data.is_none());
    assert!(GlobalConfig::load(&fixture.0).unwrap().agents.is_empty());
    assert!(!fixture.0.join("identities").join(id).exists());
}

#[tokio::test]
async fn creation_storage_failure_is_never_a_success_envelope() {
    let fixture = Fixture::new();
    let state = fixture.state();
    std::fs::create_dir(GlobalConfig::config_path(&fixture.0)).unwrap();
    let (status, Json(body)) = create_entity(
        State(state.clone()),
        request("Signal", &uuid::Uuid::new_v4().to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!body.ok);
    assert!(body.data.is_none());
    assert!(body.error.is_some());
    assert!(state
        .identity_manager
        .global_config()
        .read()
        .unwrap()
        .agents
        .is_empty());
}

#[tokio::test]
async fn creation_legacy_post_keeps_the_old_response_shape() {
    let fixture = Fixture::new();
    let (status, Json(body)) = create_entity(
        State(fixture.state()),
        Json(CreateEntityRequest {
            name: "Legacy".into(),
            creation_key: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.ok);
    let data = serde_json::to_value(body.data.unwrap()).unwrap();
    assert!(data["id"].is_string());
    assert!(data["directory"].is_string());
    assert!(data.get("creation_key").is_none());
}
