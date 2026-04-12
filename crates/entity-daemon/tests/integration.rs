//! Entity-daemon integration tests — exercises the real runtime binary over HTTP.

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
    let cluster = cluster().await;
    let resp = reqwest::get(format!("{}/health", cluster.entity_url()))
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn runtime_exposes_session_and_outbox_status() {
    let cluster = cluster().await;
    let client = reqwest::Client::new();

    let session: serde_json::Value = client
        .get(format!("{}/v1/session/status", cluster.entity_url()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(session["ok"].as_bool().unwrap_or(false));
    assert_eq!(
        session["data"]["lease"]["entity_id"].as_str(),
        Some(cluster.entity_id.as_str())
    );
    assert!(
        session["data"]["connected_to_hive"]
            .as_bool()
            .unwrap_or(false),
        "runtime should report a healthy Hive connection after startup"
    );

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
