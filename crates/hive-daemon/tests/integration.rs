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
async fn presence_roundtrip_is_scoped_and_does_not_change_runtime_config() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let client = reqwest::Client::new();
    let created: serde_json::Value = client
        .post(format!("{}/v1/entities", hive.url()))
        .json(&serde_json::json!({ "name": "Juniper" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["data"]["id"].as_str().unwrap();
    let url = format!("{}/v1/entities/{id}/presence", hive.url());
    let config_path = hive
        .data_dir()
        .join("identities")
        .join(id)
        .join("config.json");
    let original_config = std::fs::read(&config_path).unwrap();
    let defaults: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(defaults["data"]["name"], "Juniper");
    let profile = serde_json::json!({
        "name": "Juni", "purpose": "Help me plan", "temperament": "curious",
        "boundaries": "Ask before acting", "identity": "lumen"
    });
    let saved: serde_json::Value = client
        .put(&url)
        .json(&profile)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(saved["ok"], true);
    let reopened: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(reopened["data"], profile);
    assert_eq!(std::fs::read(&config_path).unwrap(), original_config);
    let entity: serde_json::Value = client
        .get(format!("{}/v1/entities/{id}", hive.url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(entity["data"]["name"], "Juniper");

    for invalid in [
        serde_json::json!({ "name": "Juni", "purpose": "", "temperament": "curious", "boundaries": "", "identity": "unknown" }),
        serde_json::json!({ "name": "Juni", "purpose": "", "temperament": "curious", "boundaries": "", "identity": "lumen", "permissions": "all" }),
        serde_json::json!({ "name": "Juni", "purpose": "x".repeat(33 * 1024), "temperament": "curious", "boundaries": "", "identity": "lumen" }),
    ] {
        let rejected: serde_json::Value = client
            .put(&url)
            .json(&invalid)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(rejected["ok"], false);
        assert!(rejected["error"].is_string());
    }
    let reopened: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(reopened["data"], profile);
    let missing_url = format!(
        "{}/v1/entities/{}/presence",
        hive.url(),
        uuid::Uuid::new_v4()
    );
    for response in [
        client.get(&missing_url).send().await.unwrap(),
        client
            .put(&missing_url)
            .json(&profile)
            .send()
            .await
            .unwrap(),
        client
            .get(format!("{}/v1/entities/not-an-id/presence", hive.url()))
            .send()
            .await
            .unwrap(),
    ] {
        let rejected: serde_json::Value = response.json().await.unwrap();
        assert_eq!(rejected["ok"], false);
    }
    std::fs::write(config_path.with_file_name("presence.json"), "{invalid").unwrap();
    let corrupt: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(corrupt["ok"], false);
    assert!(corrupt["error"].as_str().unwrap().contains("invalid"));
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
