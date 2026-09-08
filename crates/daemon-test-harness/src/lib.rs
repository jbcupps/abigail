//! Daemon test harness — start hive-daemon and entity-daemon as child
//! processes with temp data directories, wait for health, kill on drop.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Handle for a running hive-daemon process.
pub struct HiveDaemonHandle {
    child: Option<Child>,
    url: String,
    token: String,
    _tmp: tempfile::TempDir,
}

impl HiveDaemonHandle {
    /// Start `hive-daemon` with an ephemeral port and a temp data dir.
    /// Blocks until `/health` returns 200 or `timeout` elapses.
    pub async fn start(timeout: Duration) -> anyhow::Result<Self> {
        let tmp = tempfile::tempdir()?;
        let binary = cargo_bin("hive-daemon");
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut child = Command::new(&binary)
            .env("ABIGAIL_LOCAL_AUTH_TOKEN", &token)
            .env_remove("ABIGAIL_PERSISTENCE_URL")
            .env_remove("OPENAI_API_KEY").env_remove("ANTHROPIC_API_KEY")
            .args([
                "--port",
                "0",
                "--data-dir",
                tmp.path().to_str().unwrap(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start hive-daemon at {:?}: {}. Run `cargo build -p hive-daemon` first.",
                    binary, e
                )
            })?;

        let url = parse_listen_url(child.stdout.take().unwrap(), timeout).await?;

        wait_for_health(&url, timeout).await?;
        tracing::info!("Hive daemon ready at {}", url);

        Ok(Self {
            child: Some(child),
            url,
            token,
            _tmp: tmp,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn client(&self) -> reqwest::Client {
        authed_client(&self.token)
    }

    pub fn data_dir(&self) -> &std::path::Path {
        self._tmp.path()
    }
}

impl Drop for HiveDaemonHandle {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            #[cfg(windows)]
            if child.try_wait().ok().flatten().is_none() {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(0x0800_0000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Handle for a running entity-daemon process.
pub struct EntityDaemonHandle {
    child: Option<Child>,
    url: String,
    _tmp: Option<tempfile::TempDir>,
    token: String,
}

impl EntityDaemonHandle {
    /// Open through the real supervisor, including per-Entity credentials and shared storage.
    pub async fn open(entity_id: &str, hive: &HiveDaemonHandle) -> anyhow::Result<Self> {
        let value: serde_json::Value = hive
            .client()
            .post(format!("{}/v1/entities/{entity_id}/open", hive.url()))
            .send()
            .await?
            .json()
            .await?;
        let url = value["data"]["local_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to open Entity: {}", value["error"]))?
            .to_string();
        let token = value["data"]["auth_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing Entity caller credential"))?
            .to_string();
        Ok(Self {
            child: None,
            url,
            token,
            _tmp: None,
        })
    }
    pub fn client(&self) -> reqwest::Client {
        authed_client(&self.token)
    }
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for EntityDaemonHandle {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            #[cfg(windows)]
            if child.try_wait().ok().flatten().is_none() {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .creation_flags(0x0800_0000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Convenience wrapper: spins up hive + creates an entity + starts entity-daemon.
pub struct TestCluster {
    pub hive: HiveDaemonHandle,
    pub entity: EntityDaemonHandle,
    pub entity_id: String,
    pub client: reqwest::Client,
}

impl TestCluster {
    /// Boot a full hive + entity cluster with a shared temp data dir.
    pub async fn start(timeout: Duration) -> anyhow::Result<Self> {
        let hive = HiveDaemonHandle::start(timeout).await?;

        let client = hive.client();

        // Create an entity in the Hive
        let resp = client
            .post(format!("{}/v1/entities", hive.url()))
            .json(&serde_json::json!({ "name": "test-entity" }))
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        let entity_id = body["data"]["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No entity id in response: {}", body))?
            .to_string();

        // Start entity-daemon using the same data dir
        let entity = EntityDaemonHandle::open(&entity_id, &hive).await?;

        let client = entity.client();
        Ok(Self {
            hive,
            entity,
            entity_id,
            client,
        })
    }

    pub fn hive_url(&self) -> &str {
        self.hive.url()
    }

    pub fn entity_url(&self) -> &str {
        self.entity.url()
    }
}

fn authed_client(token: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    let mut value: reqwest::header::HeaderValue = format!("Bearer {token}").parse().unwrap();
    value.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, value);
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap()
}

// ── Helpers ────────────────────────────────────────────────────────

/// Find the daemon binary path in the cargo target directory.
fn cargo_bin(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Up from crates/daemon-test-harness to workspace root
    path.pop();
    path.pop();
    path.push("target");
    // Use the same profile as the test binary
    if cfg!(debug_assertions) {
        path.push("debug");
    } else {
        path.push("release");
    }
    path.push(if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    });
    path
}

/// Read stdout lines from a child process until we find the "listening on http://..." URL.
///
/// Anchored on the "listening on " marker: daemon startup logs mention other
/// URLs first (e.g. the entity daemon logs the hive URL it connects to), so
/// matching any "http://" would lock onto the wrong daemon's address.
async fn parse_listen_url(
    stdout: std::process::ChildStdout,
    timeout: Duration,
) -> anyhow::Result<String> {
    const MARKER: &str = "listening on http://";
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let mut tx = Some(tx);

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(idx) = line.find(MARKER) {
                if let Some(sender) = tx.take() {
                    let url_start = idx + MARKER.len() - "http://".len();
                    let url = line[url_start..].trim().to_string();
                    let _ = sender.send(url);
                }
            }
        }
    });

    tokio::time::timeout(timeout, rx)
        .await
        .map_err(|_| anyhow::anyhow!("Timed out waiting for daemon to report listening address"))?
        .map_err(|_| anyhow::anyhow!("Daemon stdout closed before reporting listen address"))
}

/// Poll `/health` until it returns 200 or timeout elapses.
async fn wait_for_health(base_url: &str, timeout: Duration) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/health", base_url);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Timed out waiting for {} to become healthy", url);
        }
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cargo_bin_path_exists_or_reasonable() {
        let path = cargo_bin("hive-daemon");
        // The path should point into target/debug or target/release
        assert!(
            path.to_string_lossy().contains("target"),
            "cargo_bin should resolve to target dir: {:?}",
            path
        );
    }
}
