//! Real Axum profile routes and atomic storage, with synthetic identity lookup.
//! No daemon startup, OS keyring, user Documents, provider, or model is involved.
//! This tests first profile creation, not POST /v1/entities identity creation.
//! Registry removal and disposable-directory cleanup below are synthetic; they
//! do not exercise the Entity DELETE endpoint or an installed-version upgrade.

#[path = "../src/presence.rs"]
mod presence;
#[path = "../src/presence_profile.rs"]
mod presence_profile;

use axum::body::Body;
use axum::http::Request;
use presence::PresenceIdentityResolver;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

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

#[derive(Clone, Default)]
struct SyntheticHousehold {
    entities: Arc<RwLock<HashMap<String, SyntheticRegistry>>>,
}

impl SyntheticHousehold {
    fn register(&self, root: &TestDirectory, name: &str) -> SyntheticRegistry {
        let id = uuid::Uuid::new_v4().to_string();
        let directory = root.0.join(&id);
        std::fs::create_dir(&directory).unwrap();
        let entity = SyntheticRegistry {
            id,
            name: name.to_owned(),
            directory,
        };
        self.entities
            .write()
            .unwrap()
            .insert(entity.id.clone(), entity.clone());
        entity
    }

    fn unregister(&self, id: &str) {
        assert!(self.entities.write().unwrap().remove(id).is_some());
    }
}

impl PresenceIdentityResolver for SyntheticHousehold {
    fn registered_entity(&self, id: &str) -> Result<(PathBuf, String), String> {
        self.entities
            .read()
            .unwrap()
            .get(id)
            .map(|entity| (entity.directory.clone(), entity.name.clone()))
            .ok_or_else(|| "Entity is not registered".into())
    }
}

fn router(registry: impl PresenceIdentityResolver + 'static) -> axum::Router {
    presence::router(Arc::new(registry))
}

/// Opaque synthetic pre-presence files: the profile API must preserve these
/// bytes, but this fixture makes no claim to load an older AppConfig schema.
fn seed_existing_runtime_files(directory: &Path, name: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let files = vec![
        (
            directory.join("config.json"),
            serde_json::to_vec_pretty(&json!({
                "agent_name": name,
                "birth_complete": true,
                "fixture_extension": { "retain": "existing runtime configuration" }
            }))
            .unwrap(),
        ),
        (
            directory.join("soul.md"),
            b"# Existing synthetic identity\nKeep this document unchanged.\n".to_vec(),
        ),
    ];
    for (path, bytes) in &files {
        std::fs::write(path, bytes).unwrap();
    }
    files
}

fn assert_files_unchanged(files: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in files {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }
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
async fn existing_entity_without_presence_defaults_then_saves_and_reopens() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let existing_files = seed_existing_runtime_files(&directory.0, &registry.name);
    let mut first_router = router(registry.clone());
    let defaults = get(&mut first_router, &registry.id).await;
    assert_eq!(defaults["ok"], true);
    assert_eq!(
        defaults["data"],
        json!({
            "name": registry.name, "purpose": "", "temperament": "gentle",
            "boundaries": "", "identity": "aether"
        })
    );
    assert!(!directory.0.join("presence.json").exists());
    assert_files_unchanged(&existing_files);
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
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
    assert_files_unchanged(&existing_files);
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 3);
    assert_eq!(registry.name, "Synthetic Juniper");
}

#[tokio::test]
async fn ongoing_entity_survives_disposable_profile_writes_and_synthetic_deletion() {
    let directory = TestDirectory::new();
    let household = SyntheticHousehold::default();
    let ongoing = household.register(&directory, "Ongoing Juniper");
    let disposable = household.register(&directory, "Disposable Maintenance");
    let ongoing_files = seed_existing_runtime_files(&ongoing.directory, &ongoing.name);
    let disposable_files = seed_existing_runtime_files(&disposable.directory, &disposable.name);
    let mut active_router = router(household.clone());

    assert_eq!(
        put(&mut active_router, &ongoing.id, &profile()).await["ok"],
        true
    );
    let ongoing_profile_path = ongoing.directory.join("presence.json");
    let ongoing_profile_bytes = std::fs::read(&ongoing_profile_path).unwrap();
    assert_eq!(
        get(&mut active_router, &disposable.id).await["data"]["name"],
        disposable.name
    );
    assert!(!disposable.directory.join("presence.json").exists());

    for identity in ["ember", "aether"] {
        let disposable_profile = json!({
            "name": "Maintenance display label",
            "purpose": format!("Synthetic disposable pass with {identity}"),
            "temperament": "direct", "boundaries": "Temporary design notes",
            "identity": identity
        });
        assert_eq!(
            put(&mut active_router, &disposable.id, &disposable_profile).await["data"],
            disposable_profile
        );
        assert_eq!(
            get(&mut active_router, &disposable.id).await["data"],
            disposable_profile
        );
        assert_eq!(
            std::fs::read(&ongoing_profile_path).unwrap(),
            ongoing_profile_bytes
        );
        assert_files_unchanged(&ongoing_files);
        assert_files_unchanged(&disposable_files);
    }

    // Remove the synthetic registration first: even a retained sidecar must
    // not be readable or replaceable through an already-created router.
    household.unregister(&disposable.id);
    let disposable_profile_path = disposable.directory.join("presence.json");
    let disposable_profile_bytes = std::fs::read(&disposable_profile_path).unwrap();
    assert_eq!(get(&mut active_router, &disposable.id).await["ok"], false);
    assert_eq!(
        put(&mut active_router, &disposable.id, &profile()).await["ok"],
        false
    );
    assert_eq!(
        std::fs::read(&disposable_profile_path).unwrap(),
        disposable_profile_bytes
    );
    assert_files_unchanged(&disposable_files);

    // Delete only known fixture files, never an actual Entity DELETE request.
    assert_eq!(disposable.directory.parent(), Some(directory.0.as_path()));
    std::fs::remove_file(&disposable_profile_path).unwrap();
    for (path, _) in &disposable_files {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(&disposable.directory).unwrap();
    assert_eq!(get(&mut active_router, &disposable.id).await["ok"], false);
    assert_eq!(
        put(&mut active_router, &disposable.id, &profile()).await["ok"],
        false
    );
    assert!(!disposable.directory.exists());
    drop(active_router);

    let mut reopened_router = router(household);
    let unknown_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(get(&mut reopened_router, &unknown_id).await["ok"], false);
    assert_eq!(
        put(&mut reopened_router, &unknown_id, &profile()).await["ok"],
        false
    );
    assert_eq!(
        get(&mut reopened_router, &ongoing.id).await["data"],
        profile()
    );
    assert_eq!(
        std::fs::read(&ongoing_profile_path).unwrap(),
        ongoing_profile_bytes
    );
    assert_files_unchanged(&ongoing_files);
    assert_eq!(std::fs::read_dir(&ongoing.directory).unwrap().count(), 3);
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[tokio::test]
async fn rejected_http_updates_keep_the_saved_profile() {
    let directory = TestDirectory::new();
    let registry = SyntheticRegistry::new(&directory);
    let existing_files = seed_existing_runtime_files(&directory.0, &registry.name);
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
    // Valid fields with excess JSON whitespace isolate the HTTP body limit
    // from the profile's independent field-length validation.
    let oversized_json = format!(
        "{}{}",
        serde_json::to_string(&profile()).unwrap(),
        " ".repeat(presence_profile::MAX_PROFILE_BYTES)
    );
    let mut unknown_field = profile();
    unknown_field["unsupported_future_field"] = json!("must not be silently accepted");
    for malformed in [
        "{unfinished".to_string(),
        json!({ "name": "Incomplete replacement" }).to_string(),
        unknown_field.to_string(),
        oversized_json,
    ] {
        let result = request(&mut router, "PUT", &registry.id, malformed).await;
        assert_eq!(result["ok"], false);
        assert!(result["error"].is_string());
        assert_eq!(
            std::fs::read(directory.0.join("presence.json")).unwrap(),
            previous
        );
        assert_files_unchanged(&existing_files);
    }
    assert_eq!(get(&mut router, &registry.id).await["data"], profile());
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 3);
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
