//! Real Axum profile routes and atomic storage, with synthetic identity lookup.
//! No daemon startup, OS keyring, user Documents, provider, or model is involved.
//! This tests first profile creation, not POST /v1/entities identity creation.

#[path = "../src/presence.rs"]
mod presence;
#[path = "../src/presence_profile.rs"]
mod presence_profile;

use axum::body::Body;
use axum::http::Request;
use presence::PresenceIdentityResolver;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("abigail-profile-http-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct SyntheticRegistry {
    id: String,
    name: String,
    directory: PathBuf,
}

impl SyntheticRegistry {
    fn new(directory: &TestDirectory) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Synthetic Juniper".into(),
            directory: directory.0.clone(),
        }
    }
}

impl PresenceIdentityResolver for SyntheticRegistry {
    fn registered_entity(&self, id: &str) -> Result<(PathBuf, String), String> {
        if id == self.id {
            Ok((self.directory.clone(), self.name.clone()))
        } else {
            Err("Entity is not registered".into())
        }
    }
}

fn router(registry: SyntheticRegistry) -> axum::Router {
    presence::router(Arc::new(registry))
}

fn profile() -> Value {
    json!({
        "name": "Juni", "purpose": "Synthetic planning companion",
        "temperament": "curious", "boundaries": "Ask before acting",
        "identity": "lumen"
    })
}

/// Axum's public extension trait exposes its Service supertrait here, avoiding
/// a new test dependency. These are real router requests with no TCP listener.
async fn request<S>(router: &mut S, method: &str, id: &str, body: String) -> Value
where
    S: axum::ServiceExt<
        Request<Body>,
        Response = axum::response::Response,
        Error = std::convert::Infallible,
    >,
{
    let request = Request::builder()
        .method(method)
        .uri(format!("/v1/entities/{id}/presence"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    std::future::poll_fn(|context| router.poll_ready(context))
        .await
        .unwrap();
    let response = router.call(request).await.unwrap();
    assert!(response.status().is_success());
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn get(router: &mut axum::Router, id: &str) -> Value {
    request(router, "GET", id, String::new()).await
}

async fn put(router: &mut axum::Router, id: &str, value: &Value) -> Value {
    request(router, "PUT", id, serde_json::to_string(value).unwrap()).await
}

#[tokio::test]
async fn first_profile_save_survives_router_recreation() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let config_path = directory.0.join("config.json");
    std::fs::write(
        &config_path,
        "synthetic runtime configuration stays unchanged",
    )
    .unwrap();
    let mut first_router = router(registry.clone());
    let defaults = get(&mut first_router, &registry.id).await;
    assert_eq!(defaults["ok"], true);
    assert_eq!(defaults["data"]["name"], registry.name);
    assert!(!directory.0.join("presence.json").exists());
    assert_eq!(
        put(&mut first_router, &registry.id, &profile()).await["data"],
        profile()
    );
    drop(first_router);

    let mut reopened_router = router(registry.clone());
    let reopened = get(&mut reopened_router, &registry.id).await;
    assert_eq!(reopened["ok"], true);
    assert_eq!(reopened["data"], profile());
    let persisted: Value =
        serde_json::from_slice(&std::fs::read(directory.0.join("presence.json")).unwrap()).unwrap();
    assert_eq!(persisted, profile());
    assert_eq!(
        std::fs::read_to_string(config_path).unwrap(),
        "synthetic runtime configuration stays unchanged"
    );
    assert_eq!(registry.name, "Synthetic Juniper");
}

#[tokio::test]
async fn rejected_http_updates_keep_the_saved_profile() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let mut router = router(registry.clone());
    assert_eq!(put(&mut router, &registry.id, &profile()).await["ok"], true);
    let previous = std::fs::read(directory.0.join("presence.json")).unwrap();
    for (field, value) in [
        ("name", json!(" ")),
        ("purpose", json!("x".repeat(2001))),
        ("identity", json!("unknown")),
    ] {
        let mut invalid = profile();
        invalid[field] = value;
        let result = put(&mut router, &registry.id, &invalid).await;
        assert_eq!(result["ok"], false);
        assert!(result["error"].is_string());
        assert_eq!(
            std::fs::read(directory.0.join("presence.json")).unwrap(),
            previous
        );
    }
    let malformed = request(&mut router, "PUT", &registry.id, "{unfinished".into()).await;
    assert_eq!(malformed["ok"], false);
    // Valid fields with excess JSON whitespace isolate the HTTP body limit
    // from the profile's independent field-length validation.
    let oversized_json = format!(
        "{}{}",
        serde_json::to_string(&profile()).unwrap(),
        " ".repeat(presence_profile::MAX_PROFILE_BYTES)
    );
    let oversized = request(&mut router, "PUT", &registry.id, oversized_json).await;
    assert_eq!(oversized["ok"], false);
    assert_eq!(
        std::fs::read(directory.0.join("presence.json")).unwrap(),
        previous
    );
    assert_eq!(get(&mut router, &registry.id).await["data"], profile());
}

#[tokio::test]
async fn corrupt_saved_profile_returns_a_visible_error() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let mut router = router(registry.clone());
    assert_eq!(put(&mut router, &registry.id, &profile()).await["ok"], true);
    let path = directory.0.join("presence.json");
    std::fs::write(&path, "{interrupted file").unwrap();
    let response = get(&mut router, &registry.id).await;
    assert_eq!(response["ok"], false);
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("Saved presence is invalid"));
    assert!(response.get("data").is_none());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{interrupted file");
}

#[tokio::test]
async fn failed_save_reports_error_and_can_be_retried() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let mut router = router(registry.clone());
    let obstruction = directory.0.join("presence.json");
    std::fs::create_dir(&obstruction).unwrap();
    let response = put(&mut router, &registry.id, &profile()).await;
    assert_eq!(response["ok"], false);
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("Could not save presence"));
    std::fs::remove_dir(&obstruction).unwrap();
    assert_eq!(put(&mut router, &registry.id, &profile()).await["ok"], true);
    assert_eq!(get(&mut router, &registry.id).await["data"], profile());
}

#[tokio::test]
async fn unregistered_identity_has_no_profile() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let mut router = router(registry);
    let unknown_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(get(&mut router, &unknown_id).await["ok"], false);
    assert_eq!(put(&mut router, &unknown_id, &profile()).await["ok"], false);
    assert!(!directory.0.join("presence.json").exists());
}
