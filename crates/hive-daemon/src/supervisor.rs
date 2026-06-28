//! Process supervisor for entity-daemons spawned by the Hive.
//!
//! The Hive control plane spawns entity-daemons as child processes: the immortal
//! "Abigail Hive" identity as the persistent family helper (kept alive for the
//! lifetime of the control plane), and family entity-daemons on demand when a
//! family member opens an Entity. Spawned daemons are given a cleaned
//! environment so a dev or agent shell can't leak `CLAUDECODE` into them, which
//! would otherwise make the claude-cli provider refuse to run inside a nested
//! Claude Code session.

use anyhow::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Locate the `entity-daemon` executable next to the current (hive-daemon) exe.
/// In both dev (`target/debug`) and a packaged install the two binaries are
/// siblings, so this resolves correctly without any configured path.
fn entity_daemon_binary() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let dir = exe
        .parent()
        .context("current executable has no parent directory")?;
    let name = if cfg!(windows) {
        "entity-daemon.exe"
    } else {
        "entity-daemon"
    };
    Ok(dir.join(name))
}

/// Bind to an ephemeral port to discover a free one, then release it for the
/// child to claim. A brief TOCTOU window is acceptable for local-only daemons.
async fn pick_free_port() -> anyhow::Result<u16> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Poll `{url}/health` until it returns success or the timeout elapses.
async fn wait_for_health(url: &str, timeout: Duration) -> bool {
    let client = reqwest::Client::new();
    let health = format!("{}/health", url);
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Ok(resp) = client
            .get(&health)
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if resp.status().is_success() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

/// Kill a child process and reap it in the background so it never lingers as a
/// zombie (Unix) or an orphan (we spawn with `kill_on_drop(false)`).
fn kill_and_reap(mut child: tokio::process::Child) {
    let _ = child.start_kill();
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
}

/// Spawn an `entity-daemon` for `entity_id` on a free local port, returning its
/// child handle and base URL once it reports healthy.
pub async fn spawn_entity_daemon(
    entity_id: &str,
    hive_url: &str,
    data_root: &Path,
) -> anyhow::Result<(tokio::process::Child, String)> {
    let bin = entity_daemon_binary()?;
    let port = pick_free_port().await?;
    let local_url = format!("http://127.0.0.1:{}", port);

    let mut command = tokio::process::Command::new(&bin);
    command
        .arg("--entity-id")
        .arg(entity_id)
        .arg("--hive-url")
        .arg(hive_url)
        .arg("--port")
        .arg(port.to_string())
        .arg("--data-dir")
        .arg(data_root)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .kill_on_drop(false);

    let child = command
        .spawn()
        .with_context(|| format!("spawning entity-daemon at {}", bin.display()))?;

    if wait_for_health(&local_url, Duration::from_secs(45)).await {
        Ok((child, local_url))
    } else {
        // Never reached healthy — don't leak the process.
        kill_and_reap(child);
        Err(anyhow::anyhow!(
            "entity-daemon for {} did not become healthy at {}",
            entity_id,
            local_url
        ))
    }
}

/// A running family entity-daemon tracked by the supervisor.
struct RunningEntity {
    local_url: String,
    child: tokio::process::Child,
}

/// Supervises on-demand family entity-daemons (start, reuse, stop). The
/// persistent Hive helper is managed separately by [`supervise_hive_helper`] and
/// is intentionally NOT tracked here, so it is never stopped by entity lifecycle.
pub struct HiveSupervisor {
    hive_url: String,
    data_root: PathBuf,
    running: Mutex<HashMap<String, RunningEntity>>,
}

impl HiveSupervisor {
    pub fn new(hive_url: String, data_root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            hive_url,
            data_root,
            running: Mutex::new(HashMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunningEntity>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Ensure an entity-daemon is running for `entity_id`, returning its URL.
    /// Idempotent: reuses a live daemon if one is already tracked, otherwise
    /// spawns a fresh one. A tracked-but-dead daemon is replaced.
    pub async fn ensure_entity_running(&self, entity_id: &str) -> anyhow::Result<String> {
        let existing = {
            let mut map = self.lock();
            match map.get_mut(entity_id) {
                Some(entry) => match entry.child.try_wait() {
                    Ok(None) => Some(entry.local_url.clone()), // still alive
                    _ => {
                        map.remove(entity_id); // exited — replace below
                        None
                    }
                },
                None => None,
            }
        };

        if let Some(url) = existing {
            if wait_for_health(&url, Duration::from_secs(3)).await {
                return Ok(url);
            }
            // Tracked but unhealthy — kill the old daemon, then respawn.
            if let Some(stale) = self.lock().remove(entity_id) {
                kill_and_reap(stale.child);
            }
        }

        let (child, url) = spawn_entity_daemon(entity_id, &self.hive_url, &self.data_root).await?;
        self.lock().insert(
            entity_id.to_string(),
            RunningEntity {
                local_url: url.clone(),
                child,
            },
        );
        tracing::info!("Entity {} running at {}", entity_id, url);
        Ok(url)
    }

    /// Stop and forget the entity-daemon for `entity_id`, if running.
    pub fn stop_entity(&self, entity_id: &str) {
        if let Some(entry) = self.lock().remove(entity_id) {
            tracing::info!("Stopped entity {} at {}", entity_id, entry.local_url);
            kill_and_reap(entry.child);
        }
    }
}

/// Keep the immortal Hive helper running for the lifetime of the Hive daemon. If
/// it exits, restart it (with exponential backoff on repeated spawn failures).
/// Runs out-of-band so the control plane serves `/health` immediately. The
/// helper's live URL is published into `helper_url` so `/v1/status` and the Hive
/// app's helper chat can reach it.
pub fn supervise_hive_helper(
    hive_entity_id: String,
    hive_url: String,
    data_root: PathBuf,
    helper_url: Arc<Mutex<Option<String>>>,
) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            match spawn_entity_daemon(&hive_entity_id, &hive_url, &data_root).await {
                Ok((mut child, url)) => {
                    backoff = Duration::from_secs(1);
                    tracing::info!("Hive helper running at {}", url);
                    if let Ok(mut guard) = helper_url.lock() {
                        *guard = Some(url.clone());
                    }
                    let status = child.wait().await;
                    if let Ok(mut guard) = helper_url.lock() {
                        *guard = None;
                    }
                    tracing::warn!("Hive helper exited ({:?}); restarting shortly", status);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to start Hive helper: {:#}; retrying in {:?}",
                        e,
                        backoff
                    );
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    });
}
