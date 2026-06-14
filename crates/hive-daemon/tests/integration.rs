//! Hive-daemon integration tests — exercises the real binary over HTTP.
//!
//! These tests require the `hive-daemon` binary to be pre-built.
//! Set `ABIGAIL_DAEMON_INTEGRATION=1` to enable (the CI stability job does this).

use daemon_test_harness::HiveDaemonHandle;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn should_run() -> bool {
    std::env::var("ABIGAIL_DAEMON_INTEGRATION").is_ok()
}

async fn hive() -> HiveDaemonHandle {
    HiveDaemonHandle::start(TIMEOUT)
        .await
        .expect("hive-daemon should start")
}

#[tokio::test]
async fn health_returns_200() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let resp = reqwest::get(format!("{}/health", hive.url()))
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn entity_lifecycle() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let client = reqwest::Client::new();

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
async fn secrets_crud() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/secrets", hive.url()))
        .json(&serde_json::json!({ "key": "test_key", "value": "test_value" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let resp = client
        .get(format!("{}/v1/secrets/test_key", hive.url()))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
    assert_eq!(body["data"]["value"].as_str(), Some("test_value"));

    let resp = client
        .get(format!("{}/v1/secrets/list", hive.url()))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let keys = body["data"]["keys"].as_array().expect("secrets list");
    assert!(
        keys.iter().any(|k| k.as_str() == Some("test_key")),
        "stored key should appear in list"
    );
}

#[tokio::test]
async fn provider_config() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/entities", hive.url()))
        .json(&serde_json::json!({ "name": "config-test" }))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let entity_id = body["data"]["id"].as_str().expect("entity id");

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
