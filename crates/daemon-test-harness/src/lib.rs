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
    _tmp: tempfile::TempDir,
}

impl HiveDaemonHandle {
    /// Start `hive-daemon` with an ephemeral port and a temp data dir.
    /// Blocks until `/health` returns 200 or `timeout` elapses.
    pub async fn start(timeout: Duration) -> anyhow::Result<Self> {
        let tmp = tempfile::tempdir()?;
        let binary = cargo_bin("hive-daemon");
        let mut child = Command::new(&binary)
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
            _tmp: tmp,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn data_dir(&self) -> &std::path::Path {
        self._tmp.path()
    }
}

impl Drop for HiveDaemonHandle {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
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
}

impl EntityDaemonHandle {
    /// Start `entity-daemon` connected to a running hive.
    ///
    /// If `data_dir` is None, creates a temp dir. If Some, uses the provided
    /// path (e.g. the same temp dir the Hive uses for shared identity data).
    pub async fn start(
        entity_id: &str,
        hive_url: &str,
        data_dir: Option<&std::path::Path>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        let (dir_path, tmp) = match data_dir {
            Some(p) => (p.to_path_buf(), None),
            None => {
                let t = tempfile::tempdir()?;
                let p = t.path().to_path_buf();
                (p, Some(t))
            }
        };

        let binary = cargo_bin("entity-daemon");
        let mut child = Command::new(&binary)
            .args([
                "--entity-id",
                entity_id,
                "--hive-url",
                hive_url,
                "--port",
                "0",
                "--data-dir",
                dir_path.to_str().unwrap(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start entity-daemon at {:?}: {}. Run `cargo build -p entity-daemon` first.",
                    binary, e
                )
            })?;

        let url = parse_listen_url(child.stdout.take().unwrap(), timeout).await?;

        wait_for_health(&url, timeout).await?;
        tracing::info!("Entity daemon ready at {}", url);

        Ok(Self {
            child: Some(child),
            url,
            _tmp: tmp,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for EntityDaemonHandle {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
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

        let client = reqwest::Client::new();

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
        let entity = EntityDaemonHandle::start(
            &entity_id,
            hive.url(),
            Some(hive.data_dir()),
            timeout,
        )
        .await?;

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
async fn parse_listen_url(
    stdout: std::process::ChildStdout,
    timeout: Duration,
) -> anyhow::Result<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let mut tx = Some(tx);

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(idx) = line.find("http://") {
                if let Some(sender) = tx.take() {
                    let url = line[idx..].trim().to_string();
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
