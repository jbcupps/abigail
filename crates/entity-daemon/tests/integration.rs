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

#[cfg(windows)]
#[tokio::test]
async fn startup_preserves_an_unchanged_skill_vault_without_delete_sharing() {
    use daemon_test_harness::{EntityDaemonHandle, HiveDaemonHandle};
    use std::os::windows::fs::OpenOptionsExt;

    if std::env::var("ABIGAIL_DAEMON_INTEGRATION").is_err() {
        eprintln!("Skipping: set ABIGAIL_DAEMON_INTEGRATION=1 to run daemon integration tests");
        return;
    }

    async fn api(request: reqwest::RequestBuilder) -> serde_json::Value {
        let response = request.send().await.unwrap();
        assert!(response.status().is_success());
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["ok"], true, "Hive request failed: {body}");
        body["data"].clone()
    }

    let hive = HiveDaemonHandle::start(TIMEOUT).await.unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(TIMEOUT)
        .build()
        .unwrap();
    // Let the initial helper finish its own vault startup before seeding the
    // shared fixture, so it cannot overwrite the envelope under examination.
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let status = api(client.get(format!("{}/v1/status", hive.url()))).await;
            if status["helper"]["running"] == true {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("Hive helper should become ready before seeding the skill vault");

    api(client
        .post(format!("{}/v1/secrets", hive.url()))
        .json(&serde_json::json!({"key": "tavily", "value": "synthetic-startup-secret"})))
    .await;
    let skill_dir = hive.data_dir().join("skill_secrets");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let vault_path = skill_dir.join("secrets.vault");
    // Both vaults use the same filename-derived encryption scope and the
    // child's Hive-owned key, without changing this test process's key cache.
    std::fs::copy(
        hive.data_dir().join("hive_secrets").join("secrets.vault"),
        &vault_path,
    )
    .unwrap();
    let original = std::fs::read(&vault_path).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2) // FILE_SHARE_READ | FILE_SHARE_WRITE, no FILE_SHARE_DELETE.
        .open(&vault_path)
        .unwrap();

    let created = api(client
        .post(format!("{}/v1/entities", hive.url()))
        .json(&serde_json::json!({"name": "locked-vault-startup"})))
    .await;
    let entity_id = created["id"].as_str().unwrap();
    let entity = EntityDaemonHandle::start(entity_id, hive.url(), None, TIMEOUT)
        .await
        .expect("An Entity with no missing skill secrets must start with the vault held open");
    let status = api(client.get(format!("{}/v1/status", entity.url()))).await;
    assert_eq!(status["entity_id"].as_str(), Some(entity_id));
    assert_eq!(
        std::fs::read(&vault_path).unwrap(),
        original,
        "Startup must not rewrite a skill vault when no new secrets were synced"
    );
    drop(held);
}
