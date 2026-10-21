//! Hive-daemon integration tests — exercises the real binary over HTTP.
//!
//! These tests require the `hive-daemon` binary to be pre-built.
//! Set `ABIGAIL_DAEMON_INTEGRATION=1` to enable (the CI stability job does this).

use daemon_client::HiveDaemonClient;
use daemon_test_harness::HiveDaemonHandle;
use std::sync::{Arc, Mutex};
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
    let client = hive.client();

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
    let client = hive.client();

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
    let client = hive.client();

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

#[tokio::test]
async fn control_plane_requires_this_installs_token() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let anonymous = reqwest::Client::new();
    for path in ["/v1/status", "/v1/entities", "/v1/secrets/list"] {
        for bearer in [None, Some("0".repeat(64))] {
            let mut request = anonymous.get(format!("{}{}", hive.url(), path));
            if let Some(bearer) = &bearer {
                request = request.bearer_auth(bearer);
            }
            let status = request.send().await.unwrap().status();
            assert_eq!(status, reqwest::StatusCode::UNAUTHORIZED, "{path}");
        }
    }
    let resp = hive
        .client()
        .get(format!("{}/v1/status", hive.url()))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn desktop_adopts_only_a_daemon_that_proves_this_installs_token() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let channel = abigail_core::channel::current().name();
    let token = daemon_client::read_control_token(hive.data_dir()).expect("control token");
    assert_eq!(token, hive.control_token());

    let adopted = HiveDaemonClient::adopt(hive.url(), &token, channel)
        .await
        .unwrap()
        .expect("the daemon holding this install's token is adopted");
    assert!(adopted.list_entities().await.is_ok());

    let other_channel = if channel == "stable" {
        "preview"
    } else {
        "stable"
    };
    assert!(
        HiveDaemonClient::adopt(hive.url(), &"ab".repeat(32), channel)
            .await
            .unwrap()
            .is_none()
    );
    assert!(HiveDaemonClient::adopt(hive.url(), &token, other_channel)
        .await
        .unwrap()
        .is_none());

    let other_install = HiveDaemonHandle::start(TIMEOUT).await.unwrap();
    assert!(
        HiveDaemonClient::adopt(other_install.url(), &token, channel)
            .await
            .unwrap()
            .is_none()
    );
}

/// Serves `/health` like any daemon, forwards adoption challenges to `relay_to`
/// when set, and records the Authorization header of every request.
async fn impostor(relay_to: Option<String>) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = seen.clone();
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route(
            "/v1/hive/adoption",
            axum::routing::post(move |body: String| {
                let relay_to = relay_to.clone();
                async move {
                    match relay_to {
                        Some(url) => reqwest::Client::new()
                            .post(format!("{url}/v1/hive/adoption"))
                            .header("content-type", "application/json")
                            .body(body)
                            .send()
                            .await
                            .unwrap()
                            .text()
                            .await
                            .unwrap(),
                        None => serde_json::json!({
                            "ok": true,
                            "data": {"channel": abigail_core::channel::current().name(), "proof": "00".repeat(32)}
                        })
                        .to_string(),
                    }
                }
            }),
        )
        .fallback(|| async { "ok" })
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let recorder = recorder.clone();
                async move {
                    recorder.lock().unwrap().push(
                        request
                            .headers()
                            .get("authorization")
                            .map(|value| value.to_str().unwrap_or_default().to_string()),
                    );
                    next.run(request).await
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

#[tokio::test]
async fn a_server_that_merely_answers_health_never_receives_the_token() {
    let (url, seen) = impostor(None).await;
    let token = "cd".repeat(32);
    let channel = abigail_core::channel::current().name();
    assert!(HiveDaemonClient::new(&url).health().await.unwrap());
    assert!(HiveDaemonClient::adopt(&url, &token, channel)
        .await
        .unwrap()
        .is_none());
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(Option::is_none), "{seen:?}");
}

#[tokio::test]
async fn a_relay_cannot_replay_the_real_daemons_proof() {
    if !should_run() {
        return;
    }
    let hive = hive().await;
    let (relay_url, seen) = impostor(Some(hive.url().to_string())).await;
    let channel = abigail_core::channel::current().name();
    assert!(
        HiveDaemonClient::adopt(&relay_url, hive.control_token(), channel)
            .await
            .unwrap()
            .is_none()
    );
    assert!(seen.lock().unwrap().iter().all(Option::is_none));
}
