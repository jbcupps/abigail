use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use hive_core::{EntityCreationResponse, EntityCreationState};
use std::collections::HashSet;
use tower::ServiceExt;

fn keyed(name: &str, key: &str) -> Json<CreateEntityRequest> {
    Json(CreateEntityRequest {
        name: name.into(),
        creation_key: Some(key.into()),
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_keyed_posts_replay_one_entity_and_lookup_the_receipt() {
    let (state, _) = super::tests::build_state();
    let baseline = state.identity_manager.list_agents().unwrap().len();
    let key = uuid::Uuid::new_v4().to_string();
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            let key = key.clone();
            tokio::spawn(async move { create_entity(State(state), keyed("Signal", &key)).await })
        })
        .collect();
    let mut ids = HashSet::new();
    for task in tasks {
        let (status, Json(response)) = task.await.unwrap();
        assert_eq!(status, StatusCode::OK);
        let created = response.data.unwrap();
        assert_eq!(created.creation_key.as_deref(), Some(key.as_str()));
        ids.insert(created.id);
    }
    assert_eq!(ids.len(), 1);
    assert_eq!(
        state.identity_manager.list_agents().unwrap().len(),
        baseline + 1
    );

    let (status, Json(response)) = get_entity_creation(State(state), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    let found = response.data.unwrap();
    assert_eq!(found.creation_key, key);
    assert!(matches!(
        found.status,
        EntityCreationState::Complete { name, id, .. }
            if name == "Signal" && ids.contains(&id)
    ));
}

#[tokio::test]
async fn keyed_creation_maps_validation_conflict_deletion_and_storage_failures() {
    let (state, _) = super::tests::build_state();
    let key = uuid::Uuid::new_v4().to_string();

    let (status, _) = create_entity(State(state.clone()), keyed("Signal", "bad")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = get_entity_creation(State(state.clone()), Path("bad".into())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, Json(created)) = create_entity(State(state.clone()), keyed("Signal", &key)).await;
    let id = created.data.unwrap().id;
    let (status, _) = create_entity(State(state.clone()), keyed("Different", &key)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    state.identity_manager.delete_agent(&id).unwrap();
    let (status, Json(found)) = get_entity_creation(State(state.clone()), Path(key.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(matches!(
        found.data.unwrap().status,
        EntityCreationState::Unavailable { id: found, .. } if found == id
    ));
    let (status, _) = create_entity(State(state.clone()), keyed("Signal", &key)).await;
    assert_eq!(status, StatusCode::GONE);

    let registry = abigail_core::GlobalConfig::config_path(state.identity_manager.data_root());
    std::fs::remove_file(&registry).unwrap();
    std::fs::create_dir(&registry).unwrap();
    let (status, Json(failed)) = create_entity(
        State(state),
        keyed("Storage failure", &uuid::Uuid::new_v4().to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!failed.ok);
}

#[tokio::test]
async fn no_key_keeps_the_legacy_response_shape() {
    let (state, _) = super::tests::build_state();
    let (status, Json(response)) = create_entity(
        State(state),
        Json(CreateEntityRequest {
            name: "Legacy".into(),
            creation_key: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::to_value(response.data.unwrap())
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<HashSet<_>>(),
        HashSet::from(["id".to_string(), "directory".to_string()])
    );
}

#[tokio::test]
async fn creation_lookup_requires_the_hive_control_token() {
    let (state, _) = super::tests::build_state();
    let key = uuid::Uuid::new_v4().to_string();
    let app = crate::app::router(state.clone());

    let request = |authorization: Option<String>| {
        let mut builder = Request::builder()
            .uri(format!("/v1/entity-creations/{key}"))
            .header("host", "127.0.0.1:3141");
        if let Some(value) = authorization {
            builder = builder.header("authorization", value);
        }
        builder.body(Body::empty()).unwrap()
    };
    let denied = app.clone().oneshot(request(None)).await.unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let allowed = app
        .oneshot(request(Some(format!("Bearer {}", state.control_token))))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);
    let bytes = to_bytes(allowed.into_body(), 1024 * 1024).await.unwrap();
    let envelope: ApiEnvelope<EntityCreationResponse> = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(
        envelope.data.unwrap().status,
        EntityCreationState::Absent
    ));
}
