//! Hive-daemon integration tests — exercises the real binary over HTTP.
//!
//! Requires `cargo build -p hive-daemon` before running.
//! Marked `#[ignore]` so they don't slow down `cargo test --workspace`;
//! run explicitly with `cargo test -p hive-daemon --test integration -- --ignored`.

use daemon_test_harness::HiveDaemonHandle;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

async fn hive() -> HiveDaemonHandle {
    HiveDaemonHandle::start(TIMEOUT)
        .await
        .expect("hive-daemon should start")
}

#[tokio::test]
#[ignore]
async fn health_returns_200() {
    let hive = hive().await;
    let resp = reqwest::get(format!("{}/health", hive.url()))
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
#[ignore]
async fn entity_lifecycle() {
    let hive = hive().await;
    let client = reqwest::Client::new();

    // Create entity
    let resp = client
        .post(format!("{}/v1/entities", hive.url()))
        .json(&serde_json::json!({ "name": "test-agent" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
    let entity_id = body["data"]["id"].as_str().expect("entity id");

    // List entities
    let resp = client
        .get(format!("{}/v1/entities", hive.url()))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let entities = body["data"].as_array().expect("entities list");
    assert!(
        entities.iter().any(|e| e["id"].as_str() == Some(entity_id)),
        "created entity should appear in list"
    );

    // Get single entity
    let resp = client
        .get(format!("{}/v1/entities/{}", hive.url(), entity_id))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
    assert_eq!(body["data"]["id"].as_str(), Some(entity_id));
}

#[tokio::test]
#[ignore]
async fn secrets_crud() {
    let hive = hive().await;
    let client = reqwest::Client::new();

    // Store a secret
    let resp = client
        .post(format!("{}/v1/secrets", hive.url()))
        .json(&serde_json::json!({ "key": "test_key", "value": "test_value" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Get the secret
    let resp = client
        .get(format!("{}/v1/secrets/test_key", hive.url()))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
    assert_eq!(body["data"]["value"].as_str(), Some("test_value"));

    // List secrets
    let resp = client
        .get(format!("{}/v1/secrets/list", hive.url()))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys = body["data"].as_array().expect("secrets list");
    assert!(
        keys.iter().any(|k| k.as_str() == Some("test_key")),
        "stored key should appear in list"
    );
}

#[tokio::test]
#[ignore]
async fn provider_config() {
    let hive = hive().await;
    let client = reqwest::Client::new();

    // Create entity first
    let resp = client
        .post(format!("{}/v1/entities", hive.url()))
        .json(&serde_json::json!({ "name": "config-test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let entity_id = body["data"]["id"].as_str().expect("entity id");

    // Get provider config
    let resp = client
        .get(format!(
            "{}/v1/entities/{}/provider-config",
            hive.url(),
            entity_id
        ))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "provider-config should return 200"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
}
