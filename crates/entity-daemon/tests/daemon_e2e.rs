//! Cross-daemon E2E tests — starts real hive-daemon + entity-daemon processes
//! and exercises the full HTTP API round-trip.
//!
//! Requires `cargo build -p hive-daemon -p entity-daemon` before running.
//! Marked `#[ignore]` so they don't slow down `cargo test --workspace`;
//! run explicitly with `cargo test -p entity-daemon --test daemon_e2e -- --ignored`.

use daemon_test_harness::TestCluster;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(45);

async fn cluster() -> TestCluster {
    TestCluster::start(TIMEOUT)
        .await
        .expect("test cluster should start")
}

#[tokio::test]
#[ignore]
async fn cluster_starts_and_both_healthy() {
    let c = cluster().await;

    let hive_health = c
        .client
        .get(format!("{}/health", c.hive_url()))
        .send()
        .await
        .unwrap();
    assert!(hive_health.status().is_success());

    let entity_health = c
        .client
        .get(format!("{}/health", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(entity_health.status().is_success());
}

#[tokio::test]
#[ignore]
async fn entity_status() {
    let c = cluster().await;

    let resp = c
        .client
        .get(format!("{}/v1/status", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
    assert_eq!(body["data"]["entity_id"].as_str(), Some(c.entity_id.as_str()));
}

#[tokio::test]
#[ignore]
async fn skill_listing() {
    let c = cluster().await;

    let resp = c
        .client
        .get(format!("{}/v1/skills", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));

    let skills = body["data"].as_array().expect("skills array");
    assert!(
        !skills.is_empty(),
        "entity should have at least built-in skills"
    );
}

#[tokio::test]
#[ignore]
async fn memory_round_trip() {
    let c = cluster().await;

    // Insert a memory
    let resp = c
        .client
        .post(format!("{}/v1/memory/insert", c.entity_url()))
        .json(&serde_json::json!({
            "content": "E2E test memory entry",
            "weight": "Crystallized"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "memory insert should succeed: {}",
        resp.status()
    );

    // Search for it
    let resp = c
        .client
        .post(format!("{}/v1/memory/search", c.entity_url()))
        .json(&serde_json::json!({ "query": "E2E test" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    let results = body["data"].as_array().unwrap_or(&Vec::new()).clone();
    assert!(
        results.iter().any(|r| {
            r["content"]
                .as_str()
                .map(|c| c.contains("E2E test"))
                .unwrap_or(false)
        }),
        "search should find the inserted memory"
    );

    // Recent
    let resp = c
        .client
        .get(format!("{}/v1/memory/recent?limit=5", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Stats
    let resp = c
        .client
        .get(format!("{}/v1/memory/stats", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
#[ignore]
async fn tool_execution() {
    let c = cluster().await;

    // Execute a built-in tool (hive_management::list_secrets should work)
    let resp = c
        .client
        .post(format!("{}/v1/tools/execute", c.entity_url()))
        .json(&serde_json::json!({
            "skill_id": "builtin.hive_management",
            "tool_name": "list_secrets",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "tool execution should succeed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
}

#[tokio::test]
#[ignore]
async fn governance_endpoints() {
    let c = cluster().await;

    // Get constraints
    let resp = c
        .client
        .get(format!("{}/v1/governance/constraints", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Get governance status
    let resp = c
        .client
        .get(format!("{}/v1/governance/status", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
#[ignore]
async fn job_endpoints() {
    let c = cluster().await;

    // List jobs (should be empty)
    let resp = c
        .client
        .get(format!("{}/v1/jobs", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["ok"].as_bool().unwrap_or(false));
}

#[tokio::test]
#[ignore]
async fn routing_diagnose() {
    let c = cluster().await;

    let resp = c
        .client
        .get(format!("{}/v1/routing/diagnose", c.entity_url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}
