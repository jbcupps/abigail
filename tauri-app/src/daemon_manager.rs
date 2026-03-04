//! Manages the lifecycle of hive-daemon and entity-daemon child processes.
//!
//! When `RuntimeMode::Daemon` is active, the Tauri app starts both daemons
//! as child processes, waits for their `/health` endpoints, and kills them
//! on app exit. Similar pattern to `OllamaManager`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Handle for a running daemon child process.
struct DaemonProcess {
    child: Child,
    url: String,
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        tracing::info!("Shutting down daemon at {}", self.url);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Manages hive-daemon and entity-daemon as child processes.
pub struct DaemonManager {
    hive: Option<DaemonProcess>,
    entity: Option<DaemonProcess>,
    data_dir: PathBuf,
    iggy_connection: Option<String>,
}

impl DaemonManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            hive: None,
            entity: None,
            data_dir,
            iggy_connection: None,
        }
    }

    pub fn with_iggy(mut self, conn: Option<String>) -> Self {
        self.iggy_connection = conn;
        self
    }

    /// Start hive-daemon. Returns the URL it's listening on.
    pub async fn start_hive(&mut self) -> anyhow::Result<String> {
        if self.hive.is_some() {
            return Ok(self.hive_url().unwrap_or_default());
        }

        let binary = find_daemon_binary("hive-daemon")?;
        tracing::info!("Starting hive-daemon from {:?}", binary);

        let mut child = Command::new(&binary)
            .args([
                "--port",
                "0",
                "--data-dir",
                self.data_dir.to_str().unwrap_or("."),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start hive-daemon: {}", e))?;

        let url = parse_listen_url(child.stdout.take().unwrap(), STARTUP_TIMEOUT).await?;
        wait_for_health(&url, STARTUP_TIMEOUT).await?;

        tracing::info!("Hive daemon ready at {}", url);
        self.hive = Some(DaemonProcess {
            child,
            url: url.clone(),
        });
        Ok(url)
    }

    /// Start entity-daemon for the given entity ID. Requires hive to be running.
    pub async fn start_entity(&mut self, entity_id: &str) -> anyhow::Result<String> {
        let hive_url = self
            .hive_url()
            .ok_or_else(|| anyhow::anyhow!("Hive daemon must be running before entity daemon"))?;

        // Stop existing entity daemon if switching entities
        self.stop_entity();

        let binary = find_daemon_binary("entity-daemon")?;
        tracing::info!("Starting entity-daemon for {} from {:?}", entity_id, binary);

        let mut args = vec![
            "--entity-id".to_string(),
            entity_id.to_string(),
            "--hive-url".to_string(),
            hive_url,
            "--port".to_string(),
            "0".to_string(),
            "--data-dir".to_string(),
            self.data_dir.to_str().unwrap_or(".").to_string(),
        ];
        if let Some(ref conn) = self.iggy_connection {
            args.push("--iggy-connection".to_string());
            args.push(conn.clone());
        }

        let mut child = Command::new(&binary)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to start entity-daemon: {}", e))?;

        let url = parse_listen_url(child.stdout.take().unwrap(), STARTUP_TIMEOUT).await?;
        wait_for_health(&url, STARTUP_TIMEOUT).await?;

        tracing::info!("Entity daemon ready at {}", url);
        self.entity = Some(DaemonProcess {
            child,
            url: url.clone(),
        });
        Ok(url)
    }

    pub fn hive_url(&self) -> Option<String> {
        self.hive.as_ref().map(|p| p.url.clone())
    }

    pub fn entity_url(&self) -> Option<String> {
        self.entity.as_ref().map(|p| p.url.clone())
    }

    pub fn stop_entity(&mut self) {
        self.entity = None;
    }

    pub fn shutdown(&mut self) {
        self.entity = None;
        self.hive = None;
    }
}

/// Find the daemon binary next to the running Tauri executable.
fn find_daemon_binary(name: &str) -> anyhow::Result<PathBuf> {
    let exe_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // Look next to the current executable first (bundled app)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = dir.join(&exe_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // Fall back to cargo target directory (development)
    let mut dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dev_path.pop(); // up from tauri-app
    dev_path.push("target");
    if cfg!(debug_assertions) {
        dev_path.push("debug");
    } else {
        dev_path.push("release");
    }
    dev_path.push(&exe_name);
    if dev_path.exists() {
        return Ok(dev_path);
    }

    // Fall back to PATH
    if let Ok(output) = Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(&exe_name)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Err(anyhow::anyhow!(
        "Could not find {} binary. Build with `cargo build -p {}`.",
        name,
        name
    ))
}

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
        .map_err(|_| anyhow::anyhow!("Timed out waiting for daemon listen address"))?
        .map_err(|_| anyhow::anyhow!("Daemon closed before reporting listen address"))
}

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
