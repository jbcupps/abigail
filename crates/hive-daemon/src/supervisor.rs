//! Process supervisor for entity-daemons spawned by the Hive.
//!
//! The Hive control plane spawns entity-daemons as child processes: the immortal
//! "Abigail Hive" identity as the persistent family helper (kept alive for the
//! lifetime of the control plane), and family entity-daemons on demand when a
//! family member opens an Entity. Spawned daemons are given a cleaned
//! environment so a dev or agent shell can't leak `CLAUDECODE` into them, which
//! would otherwise make the claude-cli provider refuse to run inside a nested
//! Claude Code session.

use abigail_core::profile::{ProcessIdentity, ProfileContext};
use anyhow::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(45);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(55);
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
    entity_daemon_binary_for_profile(false)
}

pub(crate) fn entity_daemon_binary_for_profile(preview: bool) -> anyhow::Result<PathBuf> {
    if preview {
        let override_path = std::env::var_os("ABIGAIL_ENTITY_DAEMON_PATH")
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!("Preview requires an explicit packaged Entity daemon")
            })?;
        let executable = std::env::current_exe()?.canonicalize()?;
        return validate_packaged_entity_binary(
            executable
                .parent()
                .context("Hive executable has no parent")?,
            &override_path,
        );
    }
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

fn validate_packaged_entity_binary(directory: &Path, candidate: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        candidate.is_absolute() && candidate.is_file(),
        "Preview Entity daemon path must be an existing absolute file"
    );
    let canonical = candidate.canonicalize()?;
    let expected_name = if cfg!(windows) {
        "entity-daemon.exe"
    } else {
        "entity-daemon"
    };
    anyhow::ensure!(
        canonical.parent() == Some(directory)
            && canonical.file_name() == Some(std::ffi::OsStr::new(expected_name)),
        "Preview Entity daemon must be the packaged sibling of the Hive daemon"
    );
    Ok(canonical)
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
    #[serde(default)]
    process_identity: Option<ProcessIdentity>,
}

fn validate_ready_identity(
    actual: &ReadyEntity,
    entity_id: &str,
    expected: Option<&ProcessIdentity>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        actual.entity_id == entity_id,
        "Entity readiness status belongs to a different Entity"
    );
    if let Some(expected) = expected {
        anyhow::ensure!(
            actual.process_identity.as_ref() == Some(expected),
            "Entity readiness status belongs to a different preview process"
        );
    }
    Ok(())
}

async fn probe_entity_ready(
    client: &reqwest::Client,
    url: &str,
    entity_id: &str,
    expected: Option<&ProcessIdentity>,
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
    validate_ready_identity(&actual, entity_id, expected)?;
    Ok(true)
}

async fn wait_for_entity_ready(
    url: &str,
    entity_id: &str,
    expected: Option<&ProcessIdentity>,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?;
    loop {
        if probe_entity_ready(&client, url, entity_id, expected).await? {
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

type ReapTicket = tokio::sync::oneshot::Receiver<Result<(), String>>;

#[derive(Default)]
struct CleanupState {
    launches: usize,
    reapers: usize,
    first_error: Option<String>,
    /// A failed wait does not authorize dropping the only owned process handle.
    unreaped: Vec<tokio::process::Child>,
}

#[derive(Default)]
struct CleanupTracker {
    state: Mutex<CleanupState>,
    changed: tokio::sync::Notify,
}

impl CleanupTracker {
    fn lock(&self) -> std::sync::MutexGuard<'_, CleanupState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn begin_launch(self: &Arc<Self>) -> LaunchRegistration {
        self.lock().launches += 1;
        LaunchRegistration(self.clone())
    }

    fn spawn_cleanup(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
        work: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
    ) -> ReapTicket {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.lock().reapers += 1;
        let cleanup = self.clone();
        runtime.spawn(async move {
            let result = work.await;
            {
                let mut state = cleanup.lock();
                state.reapers -= 1;
                if let Err(error) = &result {
                    state.first_error.get_or_insert_with(|| error.clone());
                }
            }
            // Completion is published only after the child wait has completed.
            cleanup.changed.notify_waiters();
            let _ = send.send(result);
        });
        receive
    }

    fn kill_and_reap(self: &Arc<Self>, mut child: tokio::process::Child) -> ReapTicket {
        let _ = child.start_kill();
        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(runtime) => runtime,
            Err(_) => {
                let error = "An owned Entity could not be reaped without a runtime".to_string();
                let mut state = self.lock();
                state.first_error.get_or_insert_with(|| error.clone());
                state.unreaped.push(child);
                let (send, receive) = tokio::sync::oneshot::channel();
                let _ = send.send(Err(error));
                return receive;
            }
        };
        let cleanup = self.clone();
        self.spawn_cleanup(&runtime, async move {
            match child.wait().await {
                Ok(_) => Ok(()),
                Err(error) => {
                    cleanup.lock().unreaped.push(child);
                    Err(format!(
                        "An owned Entity wait could not be confirmed: {error}"
                    ))
                }
            }
        })
    }

    async fn wait_until_reaped(&self) -> anyhow::Result<()> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // Register before inspecting counts so a concurrent completion
            // cannot be lost between the inspection and this await.
            changed.as_mut().enable();
            {
                let state = self.lock();
                if let Some(error) = &state.first_error {
                    anyhow::bail!("{error}");
                }
                if state.launches == 0 && state.reapers == 0 && state.unreaped.is_empty() {
                    return Ok(());
                }
            }
            changed.await;
        }
    }
}

struct LaunchRegistration(Arc<CleanupTracker>);

impl Drop for LaunchRegistration {
    fn drop(&mut self) {
        self.0.lock().launches -= 1;
        self.0.changed.notify_waiters();
    }
}

async fn await_reaped(ticket: ReapTicket) -> anyhow::Result<()> {
    ticket
        .await
        .context("Owned Entity cleanup did not report completion")?
        .map_err(anyhow::Error::msg)
}

/// Cancellation drops this guard just like an ordinary startup error. Neither
/// an untracked child nor an unused bootstrap capability may survive it.
struct StartupGuard {
    child: Option<tokio::process::Child>,
    stdout_drain: Option<tokio::task::JoinHandle<()>>,
    entity_id: String,
    bootstrap: String,
    phase: &'static str,
    launch: LaunchRegistration,
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
            self.launch.0.kill_and_reap(child);
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
    anyhow::ensure!(
        abigail_core::profile::current()?.is_none(),
        "Preview spawning requires an explicit supervisor ownership context"
    );
    let (child, url, _) = spawn_entity_with_profile(
        entity_id,
        hive_url,
        data_root,
        None,
        None,
        &Arc::new(CleanupTracker::default()),
    )
    .await?;
    Ok((child, url))
}

async fn spawn_entity_with_profile(
    entity_id: &str,
    hive_url: &str,
    data_root: &Path,
    profile: Option<&ProfileContext>,
    hive_identity: Option<&ProcessIdentity>,
    cleanup: &Arc<CleanupTracker>,
) -> anyhow::Result<(tokio::process::Child, String, Option<ProcessIdentity>)> {
    let entity_id = uuid::Uuid::parse_str(entity_id)
        .context("Entity runtime requires a valid Entity UUID")?
        .to_string();
    let process_identity = match (profile, hive_identity) {
        (Some(profile), Some(hive_identity)) => {
            profile.validate_identity(hive_identity)?;
            Some(profile.process_identity(
                &uuid::Uuid::new_v4().to_string(),
                &hive_identity.instance_id,
            )?)
        }
        (None, None) => None,
        _ => anyhow::bail!("Incomplete preview process ownership context"),
    };
    let bin = entity_daemon_binary_for_profile(profile.is_some())?;
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    let mut startup = StartupGuard {
        child: None,
        stdout_drain: None,
        bootstrap: issue_runtime_bootstrap(&entity_id),
        entity_id: entity_id.clone(),
        phase: "spawn",
        launch: cleanup.begin_launch(),
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
    if let (Some(profile), Some(identity)) = (profile, process_identity.as_ref()) {
        command
            .arg("--profile")
            .arg("preview")
            .env("ABIGAIL_PROFILE", "preview")
            .env("ABIGAIL_DATA_DIR", profile.root())
            .env("ABIGAIL_PROFILE_ID", profile.profile_id())
            .env("ABIGAIL_PROCESS_INSTANCE_ID", &identity.instance_id)
            .env("ABIGAIL_HIVE_INSTANCE_ID", &identity.hive_instance_id)
            .env("ABIGAIL_HIVE_URL", hive_url);
    }

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
    observe_startup(child, deadline, wait_for_entity_ready(&local_url, &entity_id, process_identity.as_ref()))
        .await
        .with_context(|| {
            format!("Entity {entity_id} startup failed during health and identity at {local_url} (pid {process_id:?}); see Entity startup logs")
        })?;
    Ok((startup.finish(), local_url, process_identity))
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
            launch: Arc::new(CleanupTracker::default()).begin_launch(),
        }
    }

    async fn assert_cleanup_finished(cleanup: &CleanupTracker) {
        tokio::time::timeout(Duration::from_secs(5), cleanup.wait_until_reaped())
            .await
            .expect("owned toy child cleanup must finish before the test runtime exits")
            .expect("owned toy child wait must confirm successful reaping");
        let state = cleanup.lock();
        assert_eq!(state.launches, 0);
        assert_eq!(state.reapers, 0);
        assert!(state.unreaped.is_empty());
        assert!(state.first_error.is_none());
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
            launch: Arc::new(CleanupTracker::default()).begin_launch(),
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
            launch: Arc::new(CleanupTracker::default()).begin_launch(),
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
        let cleanup = guard.launch.0.clone();
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
        assert_cleanup_finished(&cleanup).await;
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
        let cleanup = guard.launch.0.clone();
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
        drop(guard);
        assert_cleanup_finished(&cleanup).await;
    }

    #[tokio::test]
    async fn readiness_rejects_another_entity_even_when_health_succeeds() {
        let health_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let status_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/health",
                axum::routing::get({
                    let requests = health_requests.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        async { "ok" }
                    }
                }),
            )
            .route(
                "/v1/status",
                axum::routing::get({
                    let requests = status_requests.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        async {
                            axum::Json(
                                serde_json::json!({"ok": true, "data": {"entity_id": "other-entity"}}),
                            )
                        }
                    }
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result = probe_entity_ready(
            &reqwest::Client::builder().no_proxy().build().unwrap(),
            &url,
            "expected-entity",
            None,
        )
        .await;
        server.abort();
        assert_eq!(
            health_requests.load(Ordering::SeqCst),
            1,
            "The synthetic loopback health endpoint was not reached: {result:?}"
        );
        assert_eq!(
            status_requests.load(Ordering::SeqCst),
            1,
            "The synthetic identity response must be read before rejection: {result:?}"
        );
        assert!(result.unwrap_err().to_string().contains("different Entity"));
    }

    #[tokio::test]
    async fn readiness_does_not_follow_a_redirect_to_another_server() {
        let health_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let redirected_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let status_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/health",
                axum::routing::get({
                    let requests = health_requests.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        async { axum::response::Redirect::temporary("/foreign-health") }
                    }
                }),
            )
            .route(
                "/foreign-health",
                axum::routing::get({
                    let requests = redirected_requests.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        async { "ok" }
                    }
                }),
            )
            .route(
                "/v1/status",
                axum::routing::get({
                    let requests = status_requests.clone();
                    move || {
                        requests.fetch_add(1, Ordering::SeqCst);
                        async {
                            axum::Json(
                                serde_json::json!({"ok": true, "data": {"entity_id": "expected-entity"}}),
                            )
                        }
                    }
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
        let result = probe_entity_ready(&client, &url, "expected-entity", None).await;
        server.abort();
        assert_eq!(
            health_requests.load(Ordering::SeqCst),
            1,
            "The synthetic redirect endpoint was not reached: {result:?}"
        );
        assert_eq!(
            redirected_requests.load(Ordering::SeqCst),
            0,
            "The client must not follow the fixture's redirect"
        );
        assert_eq!(
            status_requests.load(Ordering::SeqCst),
            0,
            "A rejected health redirect must stop readiness before the status request"
        );
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
    process_identity: Option<ProcessIdentity>,
}

/// Owns every Entity child, including the persistent helper. Individual Entity
/// operations are serialized by UUID; shutdown excludes all operations and
/// confirms child termination before permitting the Hive to exit.
pub struct HiveSupervisor {
    hive_url: String,
    data_root: PathBuf,
    profile: Option<Arc<ProfileContext>>,
    process_identity: Option<ProcessIdentity>,
    running: Mutex<HashMap<String, RunningEntity>>,
    lifecycle: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    operations: tokio::sync::RwLock<()>,
    shutdown_started: AtomicBool,
    shutdown_changed: tokio::sync::Notify,
    cleanup: Arc<CleanupTracker>,
}

impl HiveSupervisor {
    pub fn new(hive_url: String, data_root: PathBuf) -> Arc<Self> {
        Self::with_profile(hive_url, data_root, None, None)
    }

    pub fn with_profile(
        hive_url: String,
        data_root: PathBuf,
        profile: Option<Arc<ProfileContext>>,
        process_identity: Option<ProcessIdentity>,
    ) -> Arc<Self> {
        Arc::new(Self {
            hive_url,
            data_root,
            profile,
            process_identity,
            running: Mutex::new(HashMap::new()),
            lifecycle: Mutex::new(HashMap::new()),
            operations: tokio::sync::RwLock::new(()),
            shutdown_started: AtomicBool::new(false),
            shutdown_changed: tokio::sync::Notify::new(),
            cleanup: Arc::new(CleanupTracker::default()),
        })
    }

    fn lifecycle_lock(&self, entity_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.lifecycle
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(entity_id.to_string())
            .or_default()
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunningEntity>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown_started.load(Ordering::Acquire)
    }

    async fn operation(&self) -> anyhow::Result<tokio::sync::RwLockReadGuard<'_, ()>> {
        anyhow::ensure!(!self.is_shutdown(), "Hive supervisor is shutting down");
        let operation = self.operations.read().await;
        anyhow::ensure!(!self.is_shutdown(), "Hive supervisor is shutting down");
        Ok(operation)
    }

    /// Latch closure before waiting for in-flight starts. The write guard keeps
    /// the final map drain atomic with respect to insertion, while the cleanup
    /// tracker also covers children from cancelled or failed starts.
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.shutdown_started.store(true, Ordering::Release);
        self.shutdown_changed.notify_waiters();
        tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            let _operations = self.operations.write().await;
            let children: Vec<_> = self.lock().drain().map(|(_, entry)| entry.child).collect();
            for child in children {
                // Independent cleanup tasks retain the only handles if this
                // request is cancelled or its deadline expires.
                self.cleanup.kill_and_reap(child);
            }
            self.cleanup.wait_until_reaped().await
        })
        .await
        .context("Owned Entity shutdown deadline exceeded")?
    }

    async fn stop_owned_child(&self, child: tokio::process::Child) -> anyhow::Result<()> {
        tokio::time::timeout(
            SHUTDOWN_TIMEOUT,
            await_reaped(self.cleanup.kill_and_reap(child)),
        )
        .await
        .context("Owned Entity stop deadline exceeded")?
    }

    /// Ensure an entity-daemon is running for `entity_id`, returning its URL.
    /// Idempotent: reuses a live daemon if one is already tracked, otherwise
    /// spawns a fresh one. A tracked-but-dead daemon is replaced.
    pub async fn ensure_entity_running(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<hive_core::EntityOpenResponse> {
        self.ensure_entity_running_with_readiness(entity_id, true)
            .await
    }

    async fn ensure_entity_running_with_readiness(
        &self,
        entity_id: &str,
        verify_existing: bool,
    ) -> anyhow::Result<hive_core::EntityOpenResponse> {
        let _operation = self.operation().await?;
        let entity_id = uuid::Uuid::parse_str(entity_id)
            .context("Entity runtime requires a valid Entity UUID")?
            .to_string();
        let lifecycle = self.lifecycle_lock(&entity_id);
        let _entity_operation = lifecycle.lock().await;
        anyhow::ensure!(!self.is_shutdown(), "Hive supervisor is shutting down");
        let mut uncertain_child = None;
        let existing = {
            let mut map = self.lock();
            match map.get_mut(&entity_id) {
                Some(entry) => match entry.child.try_wait() {
                    Ok(None) => Some((entry.local_url.clone(), entry.process_identity.clone())),
                    Ok(Some(_)) => {
                        map.remove(&entity_id); // exited — replace below
                        None
                    }
                    Err(_) => {
                        uncertain_child = map.remove(&entity_id).map(|entry| entry.child);
                        None
                    }
                },
                None => None,
            }
        };
        if let Some(child) = uncertain_child {
            self.stop_owned_child(child).await?;
        }

        if let Some((url, identity)) = existing {
            if !verify_existing
                || matches!(
                    tokio::time::timeout(
                        Duration::from_secs(3),
                        wait_for_entity_ready(&url, &entity_id, identity.as_ref())
                    )
                    .await,
                    Ok(Ok(()))
                )
            {
                return Ok(hive_core::EntityOpenResponse {
                    entity_id,
                    local_url: url,
                    process_identity: identity,
                });
            }
            // Tracked but unhealthy — kill the old daemon, then respawn.
            let stale = self.lock().remove(&entity_id);
            if let Some(stale) = stale {
                self.stop_owned_child(stale.child).await?;
            }
        }

        // A cancelled earlier start/close may have handed its owned child to
        // an independent reaper. Never create a replacement while that outcome
        // is still unknown, even when the request that initiated it is gone.
        tokio::time::timeout(SHUTDOWN_TIMEOUT, self.cleanup.wait_until_reaped())
            .await
            .context("Previous owned Entity cleanup is still unconfirmed")??;
        anyhow::ensure!(!self.is_shutdown(), "Hive supervisor is shutting down");
        let (child, url, identity) = spawn_entity_with_profile(
            &entity_id,
            &self.hive_url,
            &self.data_root,
            self.profile.as_deref(),
            self.process_identity.as_ref(),
            &self.cleanup,
        )
        .await?;
        self.lock().insert(
            entity_id.to_string(),
            RunningEntity {
                local_url: url.clone(),
                child,
                process_identity: identity.clone(),
            },
        );
        tracing::info!("Entity {} running at {}", entity_id, url);
        Ok(hive_core::EntityOpenResponse {
            entity_id,
            local_url: url,
            process_identity: identity,
        })
    }

    /// Stop and forget the entity-daemon for `entity_id`, if running.
    pub async fn stop_entity(
        &self,
        entity_id: &str,
        expected_instance: Option<&str>,
        revoke_sessions: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let _operation = self.operation().await?;
        let entity_id = uuid::Uuid::parse_str(entity_id)
            .context("Entity runtime requires a valid Entity UUID")?
            .to_string();
        let lifecycle = self.lifecycle_lock(&entity_id);
        let _entity_operation = lifecycle.lock().await;
        let entry = {
            let mut running = self.lock();
            if self.profile.is_some() {
                let expected = expected_instance.ok_or_else(|| {
                    anyhow::anyhow!("Preview close requires the expected Entity instance")
                })?;
                if let Some(entry) = running.get(&entity_id) {
                    validate_close_instance(entry.process_identity.as_ref(), Some(expected))?;
                }
            }
            running.remove(&entity_id)
        };
        if let Some(entry) = entry {
            self.stop_owned_child(entry.child).await?;
            tracing::info!("Stopped entity {} at {}", entity_id, entry.local_url);
        }
        // Keep revocation inside the same operation as the owned-child stop:
        // a replacement must not register a session until this close finishes.
        revoke_sessions()?;
        Ok(())
    }
}

fn validate_close_instance(
    actual: Option<&ProcessIdentity>,
    expected: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        actual.is_some_and(|actual| Some(actual.instance_id.as_str()) == expected),
        "Preview close belongs to a different Entity process"
    );
    Ok(())
}

/// Keep the immortal Hive helper running for the lifetime of the Hive daemon. If
/// it exits, restart it (with exponential backoff on repeated spawn failures).
/// Runs out-of-band so the control plane serves `/health` immediately. The
/// helper's live URL is published into `helper_url` so `/v1/status` and the Hive
/// app's helper chat can reach it.
pub fn supervise_hive_helper(
    supervisor: Arc<HiveSupervisor>,
    hive_entity_id: String,
    helper: Arc<Mutex<Option<hive_core::HelperInfo>>>,
) {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            let shutdown_changed = supervisor.shutdown_changed.notified();
            tokio::pin!(shutdown_changed);
            shutdown_changed.as_mut().enable();
            if supervisor.is_shutdown() {
                break;
            }
            // Preserve the helper's existing wait-for-exit behavior: a slow
            // HTTP response must not terminate a still-owned live helper.
            // Initial/replacement starts always perform full readiness checks.
            let delay = match supervisor
                .ensure_entity_running_with_readiness(&hive_entity_id, false)
                .await
            {
                Ok(connection) => {
                    if supervisor.is_shutdown() {
                        break;
                    }
                    backoff = Duration::from_secs(1);
                    if let Ok(mut guard) = helper.lock() {
                        *guard = Some(hive_core::HelperInfo {
                            running: true,
                            local_url: Some(connection.local_url),
                            process_identity: connection.process_identity,
                        });
                    }
                    Duration::from_secs(1)
                }
                Err(e) => {
                    if let Ok(mut guard) = helper.lock() {
                        *guard = None;
                    }
                    if supervisor.is_shutdown() {
                        break;
                    }
                    tracing::error!(
                        "Failed to start Hive helper: {:#}; retrying in {:?}",
                        e,
                        backoff
                    );
                    let delay = backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    delay
                }
            };
            tokio::select! {
                _ = shutdown_changed => break,
                _ = tokio::time::sleep(delay) => {}
            }
        }
        if let Ok(mut guard) = helper.lock() {
            *guard = None;
        }
    });
}

#[cfg(test)]
mod preview_identity_tests {
    use super::*;

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            channel: "beta".into(),
            profile_id: uuid::Uuid::new_v4().to_string(),
            canonical_data_root: std::env::temp_dir().join("synthetic-preview"),
            instance_id: uuid::Uuid::new_v4().to_string(),
            hive_instance_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    #[test]
    fn readiness_requires_profile_root_channel_and_both_live_instances() {
        let expected = identity();
        let ready = ReadyEntity {
            entity_id: "synthetic-entity".into(),
            process_identity: Some(expected.clone()),
        };
        validate_ready_identity(&ready, "synthetic-entity", Some(&expected)).unwrap();
        for field in ["channel", "profile", "root", "instance", "hive"] {
            let mut other = expected.clone();
            match field {
                "channel" => other.channel = "stable".into(),
                "profile" => other.profile_id = uuid::Uuid::new_v4().to_string(),
                "root" => other.canonical_data_root = other.canonical_data_root.join("another"),
                "instance" => other.instance_id = uuid::Uuid::new_v4().to_string(),
                "hive" => other.hive_instance_id = uuid::Uuid::new_v4().to_string(),
                _ => unreachable!(),
            }
            let actual = ReadyEntity {
                entity_id: "synthetic-entity".into(),
                process_identity: Some(other),
            };
            assert!(
                validate_ready_identity(&actual, "synthetic-entity", Some(&expected)).is_err(),
                "{field}"
            );
        }
        let absent = ReadyEntity {
            entity_id: "synthetic-entity".into(),
            process_identity: None,
        };
        assert!(validate_ready_identity(&absent, "synthetic-entity", Some(&expected)).is_err());
        assert!(validate_ready_identity(&absent, "synthetic-entity", None).is_ok());
    }

    #[test]
    fn stale_close_cannot_select_a_replacement_instance() {
        let expected = identity();
        assert!(validate_close_instance(Some(&expected), Some(&expected.instance_id)).is_ok());
        assert!(
            validate_close_instance(Some(&expected), Some(&uuid::Uuid::new_v4().to_string()))
                .is_err()
        );
        assert!(validate_close_instance(Some(&expected), None).is_err());
        assert!(validate_close_instance(None, Some(&expected.instance_id)).is_err());
    }

    #[tokio::test]
    async fn lifecycle_serializes_the_same_entity_without_blocking_another_entity() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let first = supervisor.lifecycle_lock("first");
        let same = supervisor.lifecycle_lock("first");
        let other = supervisor.lifecycle_lock("other");
        let operation = first.lock().await;
        assert!(same.try_lock().is_err());
        assert!(other.try_lock().is_ok());
        drop(operation);
        assert!(same.try_lock().is_ok());
    }

    #[tokio::test]
    async fn close_keeps_session_revocation_inside_the_entity_lifecycle_lock() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let entity_id = uuid::Uuid::new_v4().to_string();
        let lifecycle = supervisor.lifecycle_lock(&entity_id);
        let revoked = std::sync::atomic::AtomicBool::new(false);
        supervisor
            .stop_entity(&entity_id, None, || {
                assert!(
                    lifecycle.try_lock().is_err(),
                    "A replacement cannot start before session revocation"
                );
                revoked.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap();
        assert!(revoked.load(std::sync::atomic::Ordering::SeqCst));
        assert!(lifecycle.try_lock().is_ok());
    }

    async fn wait_for_shutdown_latch(supervisor: &HiveSupervisor) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !supervisor.is_shutdown() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown task must latch before waiting for existing work");
    }

    #[tokio::test]
    async fn shutdown_blocks_new_opens_before_an_existing_operation_drains() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let operation = supervisor.operation().await.unwrap();
        let closing = supervisor.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        wait_for_shutdown_latch(&supervisor).await;
        assert!(
            !shutdown.is_finished(),
            "pending operation must delay acknowledgement"
        );
        assert!(supervisor
            .ensure_entity_running(&uuid::Uuid::new_v4().to_string())
            .await
            .is_err());
        drop(operation);
        shutdown.await.unwrap().unwrap();
        assert!(supervisor.operation().await.is_err());
    }

    #[tokio::test]
    async fn shutdown_waits_for_cancelled_launch_and_owned_cleanup_completion() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let launch = supervisor.cleanup.begin_launch();
        let (finish, finished) = tokio::sync::oneshot::channel();
        let ticket =
            supervisor
                .cleanup
                .spawn_cleanup(&tokio::runtime::Handle::current(), async move {
                    finished
                        .await
                        .map_err(|_| "synthetic reaper cancelled".to_string())?;
                    Ok(())
                });
        let closing = supervisor.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        wait_for_shutdown_latch(&supervisor).await;
        assert!(!shutdown.is_finished());
        drop(launch);
        tokio::task::yield_now().await;
        assert!(
            !shutdown.is_finished(),
            "launch completion does not prove reaping"
        );
        finish.send(()).unwrap();
        await_reaped(ticket).await.unwrap();
        shutdown.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shutdown_preserves_cleanup_failure_and_never_reopens() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let ticket = supervisor
            .cleanup
            .spawn_cleanup(&tokio::runtime::Handle::current(), async {
                Err("synthetic wait could not be confirmed".to_string())
            });
        assert!(await_reaped(ticket).await.is_err());
        let error = supervisor.shutdown().await.unwrap_err();
        assert!(error.to_string().contains("synthetic wait"));
        assert!(
            supervisor.shutdown().await.is_err(),
            "retry must not invent success"
        );
        assert!(supervisor
            .ensure_entity_running(&uuid::Uuid::new_v4().to_string())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn cancelled_shutdown_retains_cleanup_and_allows_only_a_confirmed_retry() {
        let supervisor = HiveSupervisor::new("http://127.0.0.1:32123".into(), std::env::temp_dir());
        let (finish, finished) = tokio::sync::oneshot::channel();
        let ticket =
            supervisor
                .cleanup
                .spawn_cleanup(&tokio::runtime::Handle::current(), async move {
                    finished
                        .await
                        .map_err(|_| "synthetic reaper cancelled".to_string())?;
                    Ok(())
                });
        let closing = supervisor.clone();
        let shutdown = tokio::spawn(async move { closing.shutdown().await });
        wait_for_shutdown_latch(&supervisor).await;
        shutdown.abort();
        assert!(shutdown.await.unwrap_err().is_cancelled());
        assert_eq!(supervisor.cleanup.lock().reapers, 1);
        assert!(supervisor.operation().await.is_err());
        finish.send(()).unwrap();
        await_reaped(ticket).await.unwrap();
        supervisor.shutdown().await.unwrap();
    }

    #[test]
    fn packaged_entity_binary_rejects_other_directories_and_names() {
        let root =
            std::env::temp_dir().join(format!("abigail-preview-binaries-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("other")).unwrap();
        let root = root.canonicalize().unwrap();
        let name = if cfg!(windows) {
            "entity-daemon.exe"
        } else {
            "entity-daemon"
        };
        let expected = root.join(name);
        let outside = root.join("other").join(name);
        let wrong_name = root.join("wrong-daemon");
        for path in [&expected, &outside, &wrong_name] {
            std::fs::write(path, b"synthetic; not executable").unwrap();
        }
        assert_eq!(
            validate_packaged_entity_binary(&root, &expected).unwrap(),
            expected
        );
        assert!(validate_packaged_entity_binary(&root, &outside).is_err());
        assert!(validate_packaged_entity_binary(&root, &wrong_name).is_err());
        assert!(validate_packaged_entity_binary(&root, Path::new(name)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
