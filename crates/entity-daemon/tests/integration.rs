//! Entity-daemon integration tests — exercises the real runtime binary over HTTP.
//!
//! These tests require the `hive-daemon` and `entity-daemon` binaries to be
//! pre-built.  The CI stability job handles that explicitly; the generic
//! `cargo test --workspace` run skips them to avoid flaky build-order races.

use daemon_test_harness::TestCluster;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(45);

async fn cluster() -> TestCluster {
    TestCluster::start(TIMEOUT)
        .await
        .expect("hive + entity cluster should start")
}

#[tokio::test]
async fn health_returns_200() {
    if std::env::var("ABIGAIL_DAEMON_INTEGRATION").is_err() {
        eprintln!("Skipping: set ABIGAIL_DAEMON_INTEGRATION=1 to run daemon integration tests");
        return;
    }
    let cluster = cluster().await;
    let resp = reqwest::get(format!("{}/health", cluster.entity_url()))
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn runtime_protects_session_lease_and_exposes_outbox_status() {
    if std::env::var("ABIGAIL_DAEMON_INTEGRATION").is_err() {
        eprintln!("Skipping: set ABIGAIL_DAEMON_INTEGRATION=1 to run daemon integration tests");
        return;
    }
    let cluster = cluster().await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/v1/session/status", cluster.entity_url()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    let session: serde_json::Value = response.json().await.unwrap();
    assert!(
        session["data"].is_null(),
        "runtime lease must not be public"
    );
    let response = client
        .post(format!("{}/v1/runtime/sessions", cluster.hive_url()))
        .json(&serde_json::json!({"entity_id": cluster.entity_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

    let outbox: serde_json::Value = client
        .get(format!("{}/v1/outbox/status", cluster.entity_url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(outbox["ok"].as_bool().unwrap_or(false));
    assert_eq!(outbox["data"]["queued_records"].as_u64(), Some(0));

    let acks: serde_json::Value = client
        .get(format!("{}/v1/skills/acks", cluster.entity_url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(acks["ok"].as_bool().unwrap_or(false));
    assert!(acks["data"]["acknowledgements"].is_array());
}
