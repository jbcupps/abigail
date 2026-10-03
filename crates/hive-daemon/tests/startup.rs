//! Exercises installed-style helper and concurrent Entity startup, with only a
//! synthetic loopback provider and a fresh retained profile for diagnostics.

use daemon_test_harness::HiveDaemonHandle;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

struct IsolatedEnvironment(Vec<(&'static str, Option<OsString>)>);

impl IsolatedEnvironment {
    fn new(root: &std::path::Path) -> Self {
        let mut values = Vec::new();
        for (key, value) in [
            (
                "LOCALAPPDATA",
                Some(root.join("local-app-data").into_os_string()),
            ),
            (
                "APPDATA",
                Some(root.join("roaming-app-data").into_os_string()),
            ),
            (
                "ABIGAIL_DOCUMENTS_DIR",
                Some(root.join("documents").into_os_string()),
            ),
            (
                "ABIGAIL_VAULT_RAW_KEY",
                Some(OsString::from(format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                ))),
            ),
            ("ABIGAIL_CI_MODE", None),
            ("ABIGAIL_HIVE_URL", None),
            ("ABIGAIL_DATA_DIR", None),
            ("ABIGAIL_ENTITY_DAEMON_PATH", None),
            ("ABIGAIL_INTERNAL_BIN_DIR", None),
            ("ABIGAIL_VAULT_PASSPHRASE", None),
            ("ABIGAIL_VAULT_DATA_DIR", None),
            ("CLAUDECODE", None),
            ("CLAUDE_CODE_ENTRYPOINT", None),
            ("CLAUDE_CODE_SESSION_ID", None),
        ] {
            values.push((key, std::env::var_os(key)));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self(values)
    }
}

impl Drop for IsolatedEnvironment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct FixtureTask(tokio::task::JoinHandle<()>);

impl Drop for FixtureTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn api(request: reqwest::RequestBuilder) -> serde_json::Value {
    let response: serde_json::Value = request.send().await.unwrap().json().await.unwrap();
    assert_eq!(
        response["ok"], true,
        "Synthetic startup API failed: {response}"
    );
    response["data"].clone()
}

#[tokio::test]
async fn helper_and_two_distinct_entities_choose_bound_ports_and_match_identity() {
    if std::env::var_os("ABIGAIL_DAEMON_INTEGRATION").is_none() {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/manual-test")
        .join(format!(
            "supervisor-startup-{}",
            uuid::Uuid::new_v4().simple()
        ));
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let _environment = IsolatedEnvironment::new(&root);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let model_url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = axum::Router::new()
        .route("/v1/models", axum::routing::get(|| async {
            axum::Json(serde_json::json!({"object": "list", "data": [{"id": "supervisor-startup-fixture"}]}))
        }))
        .route("/v1/chat/completions", axum::routing::post(|| async {
            axum::Json(serde_json::json!({
                "id": "synthetic-startup-check", "model": "supervisor-startup-fixture",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ready"}, "finish_reason": "stop"}]
            }))
        }));
    let _fixture = FixtureTask(tokio::spawn(async move {
        axum::serve(listener, fixture).await.unwrap();
    }));
    let hive = HiveDaemonHandle::start(Duration::from_secs(45))
        .await
        .unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(50))
        .build()
        .unwrap();
    let initial = tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let status = api(client.get(format!("{}/v1/status", hive.url()))).await;
            if status["helper"]["running"] == true {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("Hive helper must start in the isolated profile");
    let helper_url = initial["helper"]["local_url"].as_str().unwrap();
    let helper_id = initial["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["is_hive"] == true)
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let helper_status = api(client.get(format!("{helper_url}/v1/status"))).await;
    assert_eq!(helper_status["entity_id"], helper_id);
    api(client
        .post(format!("{}/v1/providers/local", hive.url()))
        .json(&serde_json::json!({"base_url": model_url})))
    .await;
    let mut ids = Vec::new();
    for name in ["Startup Alpha", "Startup Beta"] {
        let entity = api(client
            .post(format!("{}/v1/entities", hive.url()))
            .json(&serde_json::json!({"name": name})))
        .await;
        let id = entity["id"].as_str().unwrap().to_string();
        api(client.post(format!("{}/v1/entities/{id}/birth", hive.url()))
            .json(&serde_json::json!({"path": "quickstart", "choices": [], "purpose": "Synthetic startup test"}))).await;
        ids.push(id);
    }
    let (alpha, beta) = tokio::join!(
        api(client
            .post(format!("{}/v1/entities/{}/open", hive.url(), ids[0]))
            .json(&serde_json::json!({}))),
        api(client
            .post(format!("{}/v1/entities/{}/open", hive.url(), ids[1]))
            .json(&serde_json::json!({}))),
    );
    let mut urls = std::collections::HashSet::from([helper_url.to_string()]);
    for (id, open) in ids.iter().zip([alpha, beta]) {
        let url = open["local_url"].as_str().unwrap();
        let parsed = reqwest::Url::parse(url).unwrap();
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert!(parsed.port().is_some_and(|port| port != 0));
        assert!(
            urls.insert(url.to_string()),
            "Each child must own a distinct bound port"
        );
        let status = api(client.get(format!("{url}/v1/status"))).await;
        assert_eq!(status["entity_id"].as_str(), Some(id.as_str()));
    }
    let status = api(client.get(format!("{}/v1/status", hive.url()))).await;
    assert_eq!(status["helper"]["running"], true);
    assert_eq!(status["helper"]["local_url"].as_str(), Some(helper_url));
    // HiveDaemonHandle kills/reaps only this fresh profile's supervised tree
    // before the environment is restored. Logs remain under the test root.
}
