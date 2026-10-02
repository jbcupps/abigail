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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_ANNOUNCEMENT_BYTES: usize = 256;
const MAX_STATUS_BYTES: usize = 64 * 1024;

/// One-use launch capabilities are held only in the supervising Hive process.
/// A child receives its capability through its environment, never an HTTP API.
fn bootstrap_registry() -> &'static Mutex<HashMap<String, String>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn issue_runtime_bootstrap(entity_id: &str) -> String {
    let capability = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    bootstrap_registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(entity_id.to_string(), capability.clone());
    capability
}

pub(crate) fn consume_runtime_bootstrap(entity_id: &str, capability: &str) -> bool {
    let mut registry = bootstrap_registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if registry
        .get(entity_id)
        .is_some_and(|expected| expected == capability)
    {
        registry.remove(entity_id);
        true
    } else {
        false
    }
}

fn revoke_runtime_bootstrap(entity_id: &str, capability: &str) {
    let _ = consume_runtime_bootstrap(entity_id, capability);
}

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

/// Only the Entity's dedicated, canonical announcement can choose its URL.
/// Reject other log lines and URL spellings without echoing their contents.
fn parse_listen_announcement(line: &[u8]) -> anyhow::Result<String> {
    const PREFIX: &str = "Entity daemon listening on http://127.0.0.1:";
    let line = std::str::from_utf8(line)
        .map_err(|_| anyhow::anyhow!("Invalid Entity listen announcement"))?;
    let digits = line
        .strip_prefix(PREFIX)
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| anyhow::anyhow!("Invalid Entity listen announcement"))?;
    let port = digits
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0 && port.to_string() == digits)
        .ok_or_else(|| anyhow::anyhow!("Invalid Entity listen announcement"))?;
    Ok(format!("http://127.0.0.1:{port}"))
}

async fn read_listen_announcement<R: AsyncRead + Unpin>(reader: &mut R) -> anyhow::Result<String> {
    let mut line = Vec::with_capacity(MAX_ANNOUNCEMENT_BYTES);
    loop {
        let byte = reader.read_u8().await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                anyhow::anyhow!("Entity stdout closed before its listen announcement")
            } else {
                anyhow::anyhow!("Could not read Entity listen announcement")
            }
        })?;
        if byte == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return parse_listen_announcement(&line);
        }
        if line.len() == MAX_ANNOUNCEMENT_BYTES {
            anyhow::bail!("Entity listen announcement exceeded its size limit");
        }
        line.push(byte);
    }
}

#[derive(serde::Deserialize)]
struct ReadyEnvelope {
    ok: bool,
    data: Option<ReadyEntity>,
}

#[derive(serde::Deserialize)]
struct ReadyEntity {
    entity_id: String,
}

async fn probe_entity_ready(
    client: &reqwest::Client,
    url: &str,
    entity_id: &str,
) -> anyhow::Result<bool> {
    let health = match client.get(format!("{url}/health")).send().await {
        Ok(response) => response,
        Err(_) => return Ok(false),
    };
    if !health.status().is_success() {
        return Ok(false);
    }
    let mut status = match client.get(format!("{url}/v1/status")).send().await {
        Ok(response) => response,
        Err(_) => return Ok(false),
    };
    if !status.status().is_success() {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = status
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Could not read Entity readiness status"))?
    {
        if bytes.len() + chunk.len() > MAX_STATUS_BYTES {
            anyhow::bail!("Entity readiness status exceeded its size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    let envelope: ReadyEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid Entity readiness status"))?;
    let actual = envelope
        .data
        .filter(|_| envelope.ok)
        .ok_or_else(|| anyhow::anyhow!("Invalid Entity readiness status"))?;
    if actual.entity_id != entity_id {
        anyhow::bail!("Entity readiness status belongs to a different Entity");
    }
    Ok(true)
}

async fn wait_for_entity_ready(url: &str, entity_id: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?;
    loop {
        if probe_entity_ready(&client, url, entity_id).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Readiness must never conceal an already exited child or reset its deadline.
async fn observe_startup<T>(
    child: &mut tokio::process::Child,
    deadline: tokio::time::Instant,
    operation: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::select! {
        biased;
        status = child.wait() => {
            let status = status.context("Could not observe Entity startup process")?;
            anyhow::bail!("Entity process exited before readiness (exit code {:?})", status.code());
        }
        result = tokio::time::timeout_at(deadline, operation) => {
            let ready = result.map_err(|_| anyhow::anyhow!("Entity startup deadline exceeded"))??;
            if let Some(status) = child.try_wait().context("Could not observe Entity startup process")? {
                anyhow::bail!("Entity process exited before readiness (exit code {:?})", status.code());
            }
            Ok(ready)
        }
    }
}

/// Kill a child process and reap it in the background so it never lingers as a
/// zombie (Unix) or an orphan (we spawn with `kill_on_drop(false)`).
fn kill_and_reap(mut child: tokio::process::Child) {
    let _ = child.start_kill();
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let _ = child.wait().await;
        });
    }
}

/// Cancellation drops this guard just like an ordinary startup error. Neither
/// an untracked child nor an unused bootstrap capability may survive it.
struct StartupGuard {
    child: Option<tokio::process::Child>,
    stdout_drain: Option<tokio::task::JoinHandle<()>>,
    entity_id: String,
    bootstrap: String,
    phase: &'static str,
}

impl StartupGuard {
    fn finish(mut self) -> tokio::process::Child {
        // The bounded drain owns only this child's pipe and ends at EOF. After
        // readiness it follows the managed child's lifetime, including reuse.
        self.stdout_drain.take();
        self.child.take().expect("startup guard owns its child")
    }
}

impl Drop for StartupGuard {
    fn drop(&mut self) {
        revoke_runtime_bootstrap(&self.entity_id, &self.bootstrap);
        if let Some(child) = self.child.take() {
            tracing::warn!(
                entity_id = %self.entity_id,
                process_id = child.id(),
                phase = self.phase,
                "Stopping incomplete Entity startup"
            );
            kill_and_reap(child);
            if let Some(drain) = self.stdout_drain.take() {
                drain.abort();
            }
        }
    }
}

/// Spawn an `entity-daemon` for `entity_id` on a free local port, returning its
/// child handle and base URL once it reports healthy.
pub async fn spawn_entity_daemon(
    entity_id: &str,
    hive_url: &str,
    data_root: &Path,
) -> anyhow::Result<(tokio::process::Child, String)> {
    let entity_id = uuid::Uuid::parse_str(entity_id)
        .context("Entity runtime requires a valid Entity UUID")?
        .to_string();
    let bin = entity_daemon_binary()?;
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    let mut startup = StartupGuard {
        child: None,
        stdout_drain: None,
        bootstrap: issue_runtime_bootstrap(&entity_id),
        entity_id: entity_id.clone(),
        phase: "spawn",
    };

    let mut command = tokio::process::Command::new(&bin);
    command
        .arg("--entity-id")
        .arg(&entity_id)
        .arg("--hive-url")
        .arg(hive_url)
        .arg("--port")
        .arg("0")
        .arg("--data-dir")
        .arg(data_root)
        .env("ABIGAIL_RUNTIME_BOOTSTRAP", &startup.bootstrap)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        // Do not inherit GUI/Hive handles. Only a dedicated pipe carries the
        // Entity's bound address; normal diagnostics still go to its log file.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(false);

    startup.child =
        Some(command.spawn().with_context(|| {
            format!("Spawning Entity {entity_id} runtime at {}", bin.display())
        })?);
    let child = startup
        .child
        .as_mut()
        .expect("startup guard owns its child");
    let process_id = child.id();
    let mut stdout = child
        .stdout
        .take()
        .context("Entity stdout pipe unavailable")?;
    startup.phase = "listen announcement";
    let local_url = observe_startup(child, deadline, read_listen_announcement(&mut stdout))
        .await
        .with_context(|| {
            format!("Entity {entity_id} startup failed during listen announcement (pid {process_id:?}); see Entity startup logs")
        })?;
    startup.stdout_drain = Some(tokio::spawn(async move {
        let mut buffer = [0u8; 1024];
        while matches!(stdout.read(&mut buffer).await, Ok(count) if count > 0) {}
    }));
    startup.phase = "health and identity";
    observe_startup(child, deadline, wait_for_entity_ready(&local_url, &entity_id))
        .await
        .with_context(|| {
            format!("Entity {entity_id} startup failed during health and identity at {local_url} (pid {process_id:?}); see Entity startup logs")
        })?;
    Ok((startup.finish(), local_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn long_lived_child() -> tokio::process::Child {
        #[cfg(windows)]
        let mut command = {
            let root = std::env::var_os("SystemRoot").expect("Windows system directory");
            let mut command = tokio::process::Command::new(
                PathBuf::from(&root).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            );
            command
                .env_clear()
                .env("SystemRoot", root)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Start-Sleep -Seconds 60",
                ])
                .creation_flags(0x08000000);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.env_clear().args(["-c", "exec sleep 60"]);
            command
        };
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(false)
            .spawn()
            .unwrap()
    }

    fn guard_for_child(child: tokio::process::Child) -> StartupGuard {
        let entity_id = uuid::Uuid::new_v4().to_string();
        StartupGuard {
            child: Some(child),
            stdout_drain: None,
            bootstrap: issue_runtime_bootstrap(&entity_id),
            entity_id,
            phase: "listen announcement",
        }
    }

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

    #[test]
    fn listen_announcement_accepts_only_canonical_entity_loopback_url() {
        assert_eq!(
            parse_listen_announcement(b"Entity daemon listening on http://127.0.0.1:43142")
                .unwrap(),
            "http://127.0.0.1:43142"
        );
        for invalid in [
            "Hive daemon listening on http://127.0.0.1:43142",
            "Entity daemon listening on http://localhost:43142",
            "Entity daemon listening on http://127.0.0.2:43142",
            "Entity daemon listening on https://127.0.0.1:43142",
            "Entity daemon listening on HTTP://127.0.0.1:43142",
            "Entity daemon listening on http://127.0.0.1:0",
            "Entity daemon listening on http://127.0.0.1:65536",
            "Entity daemon listening on http://127.0.0.1:043142",
            "Entity daemon listening on http://user@127.0.0.1:43142",
            "Entity daemon listening on http://127.0.0.1:43142/",
            "Entity daemon listening on http://127.0.0.1:43142?token=private",
            "Entity daemon listening on http://127.0.0.1:43142#fragment",
            " Entity daemon listening on http://127.0.0.1:43142",
            "Entity daemon listening on http://127.0.0.1:43142 ",
        ] {
            let error = parse_listen_announcement(invalid.as_bytes()).unwrap_err();
            assert_eq!(error.to_string(), "Invalid Entity listen announcement");
            assert!(!error.to_string().contains("private"));
        }
    }

    #[tokio::test]
    async fn listen_reader_bounds_unterminated_lines_and_rejects_early_eof() {
        let mut valid = b"Entity daemon listening on http://127.0.0.1:43142\r\n".as_slice();
        assert_eq!(
            read_listen_announcement(&mut valid).await.unwrap(),
            "http://127.0.0.1:43142"
        );
        let too_long = vec![b'x'; MAX_ANNOUNCEMENT_BYTES + 1];
        let mut reader = too_long.as_slice();
        let error = read_listen_announcement(&mut reader).await.unwrap_err();
        assert!(error.to_string().contains("size limit"));
        let mut reader = b"Entity daemon listening on http://127.0.0.1:43142".as_slice();
        let error = read_listen_announcement(&mut reader).await.unwrap_err();
        assert!(error.to_string().contains("stdout closed"));
    }

    #[test]
    fn dropping_incomplete_startup_revokes_only_its_bootstrap_capability() {
        let entity_id = uuid::Uuid::new_v4().to_string();
        let bootstrap = issue_runtime_bootstrap(&entity_id);
        let guard = StartupGuard {
            child: None,
            stdout_drain: None,
            entity_id: entity_id.clone(),
            bootstrap: bootstrap.clone(),
            phase: "listen announcement",
        };
        drop(guard);
        assert!(!consume_runtime_bootstrap(&entity_id, &bootstrap));

        let first = issue_runtime_bootstrap(&entity_id);
        let replacement = issue_runtime_bootstrap(&entity_id);
        drop(StartupGuard {
            child: None,
            stdout_drain: None,
            entity_id: entity_id.clone(),
            bootstrap: first,
            phase: "spawn",
        });
        assert!(consume_runtime_bootstrap(&entity_id, &replacement));
    }

    #[tokio::test]
    async fn observe_startup_reports_early_process_exit_without_waiting_for_health() {
        #[cfg(windows)]
        let mut command = {
            let root = std::env::var_os("SystemRoot").expect("Windows system directory");
            let mut command = tokio::process::Command::new(
                PathBuf::from(&root).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            );
            command
                .env_clear()
                .env("SystemRoot", root)
                .args(["-NoProfile", "-NonInteractive", "-Command", "exit 23"])
                .creation_flags(0x08000000);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.env_clear().args(["-c", "exit 23"]);
            command
        };
        let mut child = command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let error = observe_startup(
            &mut child,
            tokio::time::Instant::now() + Duration::from_secs(15),
            std::future::pending::<anyhow::Result<()>>(),
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("exit code Some(23)"),
            "Unexpected owned fixture outcome: {error}"
        );
        assert_eq!(child.wait().await.unwrap().code(), Some(23));
    }

    #[tokio::test]
    async fn cancelling_startup_kills_owned_child_and_revokes_bootstrap() {
        let child = long_lived_child();
        #[cfg(windows)]
        let process_handle = {
            use std::os::windows::io::BorrowedHandle;
            // Duplicate the owned fixture's handle before cleanup closes its
            // original; unlike a PID query this cannot observe a reused PID.
            unsafe { BorrowedHandle::borrow_raw(child.raw_handle().unwrap()) }
                .try_clone_to_owned()
                .unwrap()
        };
        #[cfg(not(windows))]
        let process_id = child.id().unwrap();
        let guard = guard_for_child(child);
        let entity_id = guard.entity_id.clone();
        let bootstrap = guard.bootstrap.clone();
        let (entered, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut guard = guard;
            let _ = entered.send(());
            observe_startup(
                guard.child.as_mut().unwrap(),
                tokio::time::Instant::now() + STARTUP_TIMEOUT,
                std::future::pending::<anyhow::Result<()>>(),
            )
            .await
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!consume_runtime_bootstrap(&entity_id, &bootstrap));
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
            }
            let status = tokio::task::spawn_blocking(move || {
                // The duplicated handle refers only to our test process.
                unsafe { WaitForSingleObject(process_handle.as_raw_handle(), 5000) }
            })
            .await
            .unwrap();
            assert_eq!(status, 0, "owned child must terminate after cancellation");
        }
        #[cfg(not(windows))]
        {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let exists = tokio::process::Command::new("/bin/kill")
                        .env_clear()
                        .args(["-0", &process_id.to_string()])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await
                        .unwrap()
                        .success();
                    if !exists {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("owned child must be killed and reaped after cancellation");
        }
    }

    #[tokio::test]
    async fn health_phase_does_not_reset_the_announcement_deadline() {
        let mut guard = guard_for_child(long_lived_child());
        let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
        observe_startup(guard.child.as_mut().unwrap(), deadline, async { Ok(()) })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            observe_startup(
                guard.child.as_mut().unwrap(),
                deadline,
                std::future::pending::<anyhow::Result<()>>(),
            ),
        )
        .await
        .expect("the expired shared deadline must fail immediately")
        .unwrap_err();
        assert!(error.to_string().contains("deadline exceeded"));
    }

    #[tokio::test]
    async fn readiness_rejects_another_entity_even_when_health_succeeds() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route("/health", axum::routing::get(|| async { "ok" }))
            .route(
                "/v1/status",
                axum::routing::get(|| async {
                    axum::Json(
                        serde_json::json!({"ok": true, "data": {"entity_id": "other-entity"}}),
                    )
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result = probe_entity_ready(
            &reqwest::Client::builder().no_proxy().build().unwrap(),
            &url,
            "expected-entity",
        )
        .await;
        server.abort();
        assert!(result.unwrap_err().to_string().contains("different Entity"));
    }

    #[tokio::test]
    async fn readiness_does_not_follow_a_redirect_to_another_server() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/health",
                axum::routing::get(|| async {
                    axum::response::Redirect::temporary("/foreign-health")
                }),
            )
            .route("/foreign-health", axum::routing::get(|| async { "ok" }))
            .route(
                "/v1/status",
                axum::routing::get(|| async {
                    axum::Json(
                        serde_json::json!({"ok": true, "data": {"entity_id": "expected-entity"}}),
                    )
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let result = probe_entity_ready(&client, &url, "expected-entity").await;
        server.abort();
        assert!(
            !result.unwrap(),
            "redirects cannot establish child readiness"
        );
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
            if matches!(
                tokio::time::timeout(
                    Duration::from_secs(3),
                    wait_for_entity_ready(&url, entity_id)
                )
                .await,
                Ok(Ok(()))
            ) {
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
