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

    // The entity daemon may still be initialising routes right after the
    // health endpoint becomes available.  Retry a few times to avoid flaky
    // "EOF while parsing" failures on slower CI runners.
    let mut session: serde_json::Value = serde_json::Value::Null;
    for attempt in 0..5u32 {
        let resp = client
            .get(format!("{}/v1/session/status", cluster.entity_url()))
            .send()
            .await;
        if let Ok(r) = resp {
            if let Ok(v) = r.json::<serde_json::Value>().await {
                session = v;
                break;
            }
        }
        if attempt < 4 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    assert!(
        !session.is_null(),
        "session/status never returned a valid JSON response"
    );
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
