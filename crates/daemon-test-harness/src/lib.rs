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

        let stdout = child.stdout.take().unwrap();
        // Own the process before either readiness wait so errors and cancelled
        // startup futures also clean up their supervisor and managed children.
        let mut handle = Self {
            child: Some(child),
            url: String::new(),
            _tmp: tmp,
        };
        handle.url = parse_listen_url(stdout, timeout).await?;
        wait_for_health(&handle.url, timeout).await?;
        tracing::info!("Hive daemon ready at {}", handle.url);
        Ok(handle)
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
        #[cfg(windows)]
        stop_owned_entity_children(self._tmp.path());
    }
}

/// A force-killed Hive cannot reap its helper. Find only children using this
/// harness's executable and unique temporary data directory, including orphans.
#[cfg(windows)]
fn stop_owned_entity_children(data_dir: &std::path::Path) {
    const CLEANUP: &str = r#"
$ErrorActionPreference = 'Stop'
$expected = [IO.Path]::GetFullPath($env:ABIGAIL_HARNESS_ENTITY_EXE)
$data = [IO.Path]::GetFullPath($env:ABIGAIL_HARNESS_DATA_DIR)
$pattern = '(?:^|\s)--data-dir(?:\s+|=)(?:"([^"]*)"|(\S+))(?=\s|$)'
foreach ($snapshot in @(Get-CimInstance Win32_Process -Filter "Name = 'entity-daemon.exe'")) {
    if (-not $snapshot.ExecutablePath -or -not $expected.Equals([IO.Path]::GetFullPath($snapshot.ExecutablePath), [StringComparison]::OrdinalIgnoreCase)) { continue }
    if (-not $snapshot.CommandLine -or $snapshot.CommandLine -notmatch $pattern) { continue }
    $argument = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
    if (-not $data.Equals([IO.Path]::GetFullPath($argument), [StringComparison]::OrdinalIgnoreCase)) { continue }
    $process = $null
    try {
        $process = [Diagnostics.Process]::GetProcessById($snapshot.ProcessId)
        [void]$process.Handle
        $current = Get-CimInstance Win32_Process -Filter "ProcessId = $($snapshot.ProcessId)"
        if (-not $current -or $current.CreationDate -ne $snapshot.CreationDate -or $current.ExecutablePath -ne $snapshot.ExecutablePath -or $current.CommandLine -ne $snapshot.CommandLine) { continue }
        $process.Kill()
        if (-not $process.WaitForExit(5000)) { throw 'Owned Entity daemon failed to stop' }
    } catch [ArgumentException] {
    } finally {
        if ($process) { $process.Dispose() }
    }
}
"#;
    let result = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", CLEANUP])
        .env("ABIGAIL_HARNESS_ENTITY_EXE", cargo_bin("entity-daemon"))
        .env("ABIGAIL_HARNESS_DATA_DIR", data_dir)
        .output();
    match result {
        Ok(output) if output.status.success() => {}
        Ok(output) => eprintln!(
            "Harness child cleanup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => eprintln!("Harness child cleanup failed: {}", error),
    }
}

/// Handle for a running entity-daemon process.
pub struct EntityDaemonHandle {
    url: String,
    close_url: String,
}

impl EntityDaemonHandle {
    /// Open an Entity through its Hive supervisor, using the same private
    /// runtime bootstrap authorization as an installed Abigail app.
    pub async fn start(
        entity_id: &str,
        hive_url: &str,
        _data_dir: Option<&std::path::Path>,
        timeout: Duration,
    ) -> anyhow::Result<Self> {
        let response: serde_json::Value = reqwest::Client::new()
            .post(format!("{}/v1/entities/{}/open", hive_url, entity_id))
            .timeout(timeout)
            .send()
            .await?
            .json()
            .await?;
        let url = response["data"]["local_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Hive could not open Entity: {}", response))?
            .to_string();

        wait_for_health(&url, timeout).await?;
        tracing::info!("Entity daemon ready at {}", url);

        Ok(Self {
            url,
            close_url: format!("{}/v1/entities/{}/close", hive_url, entity_id),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for EntityDaemonHandle {
    fn drop(&mut self) {
        let close_url = self.close_url.clone();
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async move {
                    let _ = reqwest::Client::new()
                        .post(close_url)
                        .timeout(Duration::from_secs(3))
                        .send()
                        .await;
                });
            }
        })
        .join();
    }
}

/// Convenience wrapper: spins up hive + creates an entity + starts entity-daemon.
pub struct TestCluster {
    pub entity: EntityDaemonHandle,
    pub hive: HiveDaemonHandle,
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
        let entity =
            EntityDaemonHandle::start(&entity_id, hive.url(), Some(hive.data_dir()), timeout)
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
