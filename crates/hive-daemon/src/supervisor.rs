//! Process supervisor for entity-daemons spawned by the Hive.
//!
//! The coordinator keeps Abigail's setup conversation in-process and starts
//! family entity-daemons on demand. Each child receives its own scoped Hive
//! credential and a separate credential for its Runtime window.

use anyhow::Context;
use hive_core::LOCAL_AUTH_ENV;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Locate the `entity-daemon` executable next to the current (hive-daemon) exe.
/// In both dev (`target/debug`) and a packaged install the two binaries are
/// siblings, so this resolves correctly without any configured path.
fn entity_daemon_binary() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("ABIGAIL_ENTITY_DAEMON_PATH")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty() && path.is_file())
    {
        return Ok(path);
    }

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

/// Poll the authenticated runtime status endpoint until it returns success or the timeout elapses.
async fn wait_for_health(url: &str, token: &str, timeout: Duration) -> bool {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("local HTTP client");
    let health = format!("{}/v1/session/status", url);
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if let Ok(resp) = client
            .get(&health)
            .bearer_auth(token)
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
/// zombie (Unix) or an orphan (we spawn with `kill_on_drop(true)`).
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
    auth_token: &str,
    runtime_token: &str,
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
        .env(LOCAL_AUTH_ENV, auth_token)
        .env("ABIGAIL_ENTITY_AUTH_TOKEN", runtime_token)
        // Route this child's persistence through the Hive. The shared store is
        // held under an exclusive file lock by the Hive process, so a child
        // that opened it directly would die with a lock error — which is
        // exactly what used to happen the moment a second entity-daemon
        // started alongside the immortal Hive helper.
        .env(abigail_persistence::PERSISTENCE_URL_ENV, hive_url)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        // Detach stdio entirely. The entity daemon logs to a file via
        // `abigail_diag`, its port is passed explicitly, and health is polled
        // over HTTP — nothing reads its stdout. Inheriting our handles is
        // actively harmful: when a GUI shell pipes our stdout to parse the
        // listen line, an inheriting child holds that pipe open forever, and
        // when we run with no console the child inherits invalid handles.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let child = command
        .spawn()
        .with_context(|| format!("spawning entity-daemon at {}", bin.display()))?;

    if wait_for_health(&local_url, runtime_token, Duration::from_secs(45)).await {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("abigail-hive-daemon-{}-{}", label, nanos))
    }

    #[test]
    fn entity_daemon_binary_prefers_packaged_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("env");
        fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) {
            "entity-daemon.exe"
        } else {
            "entity-daemon"
        };
        let path = dir.join(name);
        fs::write(&path, b"daemon").unwrap();

        std::env::set_var("ABIGAIL_ENTITY_DAEMON_PATH", &path);
        let resolved = entity_daemon_binary().unwrap();
        std::env::remove_var("ABIGAIL_ENTITY_DAEMON_PATH");

        assert_eq!(resolved, path);
        let _ = fs::remove_dir_all(dir);
    }
}

/// A running family entity-daemon tracked by the supervisor.
struct RunningEntity {
    local_url: String,
    child: tokio::process::Child,
    runtime_token: String,
    hive_token: String,
}

/// Supervises on-demand family entity-daemons (start, reuse, stop). The
/// setup assistant stays in-process and is unaffected by Entity window lifecycle.
pub struct HiveSupervisor {
    hive_url: String,
    data_root: PathBuf,
    auth: crate::local_auth::LocalAuth,
    running: Mutex<HashMap<String, RunningEntity>>,
    launch: tokio::sync::Mutex<()>,
}

impl HiveSupervisor {
    pub fn new(
        hive_url: String,
        data_root: PathBuf,
        auth: crate::local_auth::LocalAuth,
    ) -> Arc<Self> {
        Arc::new(Self {
            hive_url,
            data_root,
            auth,
            running: Mutex::new(HashMap::new()),
            launch: tokio::sync::Mutex::new(()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunningEntity>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Ensure an entity-daemon is running for `entity_id`, returning its URL.
    /// Idempotent: reuses a live daemon if one is already tracked, otherwise
    /// spawns a fresh one. A tracked-but-dead daemon is replaced.
    pub async fn ensure_entity_running(&self, entity_id: &str) -> anyhow::Result<String> {
        let _launch = self.launch.lock().await;
        let existing = {
            let mut map = self.lock();
            match map.get_mut(entity_id) {
                Some(entry) => match entry.child.try_wait() {
                    Ok(None) => Some((entry.local_url.clone(), entry.runtime_token.clone())), // still alive
                    _ => {
                        if let Some(stale) = map.remove(entity_id) {
                            self.auth.revoke(&stale.hive_token);
                        } // exited — replace below
                        None
                    }
                },
                None => None,
            }
        };

        if let Some((url, token)) = existing {
            if wait_for_health(&url, &token, Duration::from_secs(3)).await {
                return Ok(url);
            }
            // Tracked but unhealthy — kill the old daemon, then respawn.
            if let Some(stale) = self.lock().remove(entity_id) {
                self.auth.revoke(&stale.hive_token);
                kill_and_reap(stale.child);
            }
        }

        let hive_token = self.auth.grant_entity(entity_id);
        let runtime_token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let (child, url) = match spawn_entity_daemon(
            entity_id,
            &self.hive_url,
            &self.data_root,
            &hive_token,
            &runtime_token,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                self.auth.revoke(&hive_token);
                return Err(error);
            }
        };
        self.lock().insert(
            entity_id.to_string(),
            RunningEntity {
                local_url: url.clone(),
                child,
                runtime_token,
                hive_token,
            },
        );
        tracing::info!("Entity {} running at {}", entity_id, url);
        Ok(url)
    }

    /// Stop and forget the entity-daemon for `entity_id`, if running.
    pub fn runtime_token(&self, entity_id: &str) -> Option<String> {
        self.lock().get(entity_id).map(|e| e.runtime_token.clone())
    }

    pub async fn stop_entity(&self, entity_id: &str) {
        let _launch = self.launch.lock().await;
        if let Some(entry) = self.lock().remove(entity_id) {
            tracing::info!("Stopped entity {} at {}", entity_id, entry.local_url);
            self.auth.revoke(&entry.hive_token);
            kill_and_reap(entry.child);
        }
    }
}
