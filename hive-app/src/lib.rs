#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod genesis_voice;

use abigail_core::channel;
use daemon_client::HiveDaemonClient;
use hive_core::{
    ApiEnvelope, CreateForgeApprovalJobRequest, EntityOpenResponse, HiveStatus,
    SkillAssignmentsResponse, UpdateEntityConfigRequest, UpdateEntityConfigResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;

/// The Hive daemon this shell adopted at startup. Only a daemon that proved it
/// holds this install's control token is ever sent that token.
static HIVE: OnceLock<HiveDaemonClient> = OnceLock::new();

const HIVE_STARTING: &str = "Abigail Hive is still starting. Try again in a moment.";

fn hive() -> Result<&'static HiveDaemonClient, String> {
    HIVE.get().ok_or_else(|| HIVE_STARTING.to_string())
}

/// Tracks which entities currently have a Runtime window open, so a second
/// "Open" click focuses the idea of one window per entity rather than spawning
/// duplicates. Shared with the per-window watcher task via an `Arc`.
#[derive(Default)]
struct OpenEntities(Arc<Mutex<HashSet<String>>>);

const ENTITY_RUNTIME_APP_PATH_ENV: &str = "ABIGAIL_ENTITY_RUNTIME_APP_PATH";
const ENTITY_DAEMON_PATH_ENV: &str = "ABIGAIL_ENTITY_DAEMON_PATH";
const INTERNAL_BIN_DIR_ENV: &str = "ABIGAIL_INTERNAL_BIN_DIR";

fn platform_binary_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", stem)
    } else {
        stem.to_string()
    }
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

fn resolve_binary_from_dirs(
    env_override: Option<&str>,
    binary_name: &str,
    dirs: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(path) = env_override.and_then(nonempty_env_path) {
        if path.is_file() {
            return Some(path);
        }
    }

    for dir in dirs {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    dirs.first().map(|dir| dir.join(binary_name))
}

fn internal_binary_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = current_exe_dir() {
        dirs.push(dir.clone());
        dirs.push(dir.join("resources"));
    }
    if let Some(dir) = nonempty_env_path(INTERNAL_BIN_DIR_ENV) {
        if !dirs.iter().any(|existing| existing == &dir) {
            dirs.push(dir.clone());
        }
        let nested = dir.join("resources");
        if !dirs.iter().any(|existing| existing == &nested) {
            dirs.push(nested);
        }
    }
    dirs
}

fn resolve_internal_binary(env_override: Option<&str>, stem: &str) -> Option<PathBuf> {
    resolve_binary_from_dirs(
        env_override,
        &platform_binary_name(stem),
        &internal_binary_dirs(),
    )
}

/// Locate the Entity Runtime app executable from an explicit packaged resource
/// path, the Tauri resource directory, or next to this Hive executable.
fn entity_app_binary() -> Option<PathBuf> {
    resolve_internal_binary(
        Some(ENTITY_RUNTIME_APP_PATH_ENV),
        "abigail-entity-runtime-app",
    )
}

/// Stop tracking an entity as having an open window.
fn forget(tracking: &Arc<Mutex<HashSet<String>>>, entity_id: &str) {
    if let Ok(mut set) = tracking.lock() {
        set.remove(entity_id);
    }
}

#[derive(Debug, Serialize)]
struct HiveConnectionInfo {
    hive_url: String,
}

fn configured_hive_url() -> Option<String> {
    std::env::var("ABIGAIL_HIVE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn hive_url() -> String {
    match HIVE.get() {
        Some(client) => client.base_url().to_string(),
        None => configured_hive_url().unwrap_or_else(|| channel::current().default_hive_url()),
    }
}

/// Matches the daemon's own default so both find the same control token.
fn data_root() -> PathBuf {
    nonempty_env_path("ABIGAIL_DATA_DIR")
        .unwrap_or_else(|| abigail_core::AppConfig::default_paths().data_dir)
}

#[tauri::command]
async fn get_hive_status() -> Result<HiveStatus, String> {
    let entities = hive()?.list_entities().await.map_err(|e| e.to_string())?;
    Ok(HiveStatus {
        master_key_loaded: true,
        entity_count: entities.len(),
        entities,
        ..Default::default()
    })
}

#[tauri::command]
fn get_hive_connection_info() -> HiveConnectionInfo {
    HiveConnectionInfo {
        hive_url: hive_url(),
    }
}

/// The webview's credential for direct Hive requests, available only after
/// the daemon at [`hive_url`] has been adopted.
#[tauri::command]
fn get_hive_control_token() -> Result<String, String> {
    hive()?;
    daemon_client::read_control_token(&data_root()).ok_or_else(|| HIVE_STARTING.to_string())
}

#[tauri::command]
async fn create_entity(name: String) -> Result<String, String> {
    hive()?
        .create_entity(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_provider_config(entity_id: String) -> Result<hive_core::ProviderConfig, String> {
    hive()?
        .get_provider_config(&entity_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_entity_provider_config(
    entity_id: String,
    active_provider_preference: Option<String>,
    ego_model: Option<String>,
    local_llm_base_url: Option<String>,
    routing_mode: Option<String>,
    cli_permission_mode: Option<String>,
) -> Result<UpdateEntityConfigResponse, String> {
    hive()?
        .update_entity_config(
            &entity_id,
            &UpdateEntityConfigRequest {
                active_provider_preference,
                ego_model,
                local_llm_base_url,
                routing_mode,
                cli_permission_mode,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn store_secret(key: String, value: String) -> Result<String, String> {
    hive()?
        .store_secret(&key, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("Secret '{}' stored", key))
}

#[tauri::command]
async fn list_secrets() -> Result<Vec<String>, String> {
    hive()?.list_secrets().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn discover_provider_models(
    provider: String,
    api_key: String,
) -> Result<hive_core::ProviderModelsResponse, String> {
    hive()?
        .discover_provider_models(&provider, &api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_assignments(entity_id: String) -> Result<SkillAssignmentsResponse, String> {
    hive()?
        .get_skill_assignments(&entity_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn approve_forge_job(
    entity_id: String,
    skill_id: String,
    code_path: String,
    markdown_path: String,
) -> Result<hive_core::ForgeApprovalJob, String> {
    let hive = hive()?;
    let response: hive_core::ApiEnvelope<hive_core::ForgeApprovalJob> = hive
        .http()
        .post(format!(
            "{}/v1/entities/{}/forge-approvals",
            hive.base_url(),
            entity_id
        ))
        .json(&CreateForgeApprovalJobRequest {
            skill_id,
            code_path,
            markdown_path,
            correlation_id: None,
        })
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if response.ok {
        response
            .data
            .ok_or_else(|| "Missing forge approval response data".to_string())
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "Unknown forge approval error".to_string()))
    }
}

async fn hive_get<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let hive = hive()?;
    let response: hive_core::ApiEnvelope<T> = hive
        .http()
        .get(format!("{}{}", hive.base_url(), path))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if response.ok {
        response
            .data
            .ok_or_else(|| "Missing response data".to_string())
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "Unknown hive error".to_string()))
    }
}

async fn hive_post<T: serde::de::DeserializeOwned, B: Serialize>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    let hive = hive()?;
    let response: hive_core::ApiEnvelope<T> = hive
        .http()
        .post(format!("{}{}", hive.base_url(), path))
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if response.ok {
        response
            .data
            .ok_or_else(|| "Missing response data".to_string())
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "Unknown hive error".to_string()))
    }
}

#[tauri::command]
async fn get_birth_scenarios() -> Result<hive_core::ForgeScenariosResponse, String> {
    hive_get("/v1/birth/scenarios").await
}

#[tauri::command]
async fn perform_birth(
    entity_id: String,
    path: String,
    choices: Vec<(String, String)>,
    purpose: Option<String>,
) -> Result<hive_core::BirthRiteResponse, String> {
    hive_post(
        &format!("/v1/entities/{}/birth", entity_id),
        &hive_core::BirthRiteRequest {
            path,
            choices,
            purpose,
        },
    )
    .await
}

#[tauri::command]
async fn get_birth_document(entity_id: String) -> Result<hive_core::EntityBirthDocument, String> {
    hive_get(&format!("/v1/entities/{}/birth", entity_id)).await
}

/// Open an Entity: ensure its daemon is running, then launch the Entity Runtime
/// app pointed at it. When that window closes, tell the Hive to stop the daemon.
#[tauri::command]
async fn open_entity(
    entity_id: String,
    open: tauri::State<'_, OpenEntities>,
) -> Result<(), String> {
    let bin = entity_app_binary().ok_or("Entity Runtime app binary not found")?;
    let http = hive()?.http().clone();
    // One window per entity — skip if already open.
    {
        let mut set = open.0.lock().map_err(|e| e.to_string())?;
        if set.contains(&entity_id) {
            return Ok(());
        }
        set.insert(entity_id.clone());
    }
    let tracking = open.0.clone();

    let hive = hive_url();
    // 1. Ensure the entity's daemon is running and learn its URL.
    let resp: Result<ApiEnvelope<EntityOpenResponse>, String> = async {
        http.post(format!("{}/v1/entities/{}/open", hive, entity_id))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    let local_url = match resp {
        Ok(env) if env.ok => match env.data {
            Some(d) => d.local_url,
            None => {
                forget(&tracking, &entity_id);
                return Err("Hive returned no runtime URL".to_string());
            }
        },
        Ok(env) => {
            forget(&tracking, &entity_id);
            return Err(env
                .error
                .unwrap_or_else(|| "Failed to open entity".to_string()));
        }
        Err(e) => {
            forget(&tracking, &entity_id);
            return Err(e);
        }
    };

    // 2. Launch the Entity Runtime app pointed at that daemon.
    // Don't let the runtime app inherit our (possibly invalid, console-less)
    // stdio handles — it has its own file logging and nothing reads its output.
    let child = tokio::process::Command::new(&bin)
        .env("ABIGAIL_ENTITY_URL", &local_url)
        .env("ABIGAIL_HIVE_URL", &hive)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch Entity Runtime app: {}", e));
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            forget(&tracking, &entity_id);
            let _ = http
                .post(format!("{}/v1/entities/{}/close", hive, entity_id))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
            return Err(e);
        }
    };

    // 3. When the window closes (process exits), stop the daemon.
    let entity_id_for_task = entity_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        let _ = http
            .post(format!("{}/v1/entities/{}/close", hive, entity_id_for_task))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        forget(&tracking, &entity_id_for_task);
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Production launch — ensure the Hive daemon is running.
//
// In a packaged install the Hive app is the user's only entry point: double-
// clicking it must bring up the whole stack with no terminal. On launch it
// adopts the daemon recorded in a runtime descriptor, or spawns a fresh one
// (on the first free port) and records it. In dev the split launcher sets
// `ABIGAIL_HIVE_URL` and starts the daemon itself. Either way a daemon is only
// adopted once it proves it holds this install's control token: a healthy
// `/health` can be a stale descriptor, a reused port, or another channel.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct HiveRuntimeDescriptor {
    hive_url: String,
    pid: u32,
    started_at_epoch: u64,
}

fn runtime_dir() -> std::path::PathBuf {
    if let Some(data_dir) = nonempty_env_path("ABIGAIL_DATA_DIR") {
        return data_dir.join("runtime");
    }
    channel::current().runtime_dir()
}

fn descriptor_path() -> std::path::PathBuf {
    runtime_dir().join("hive.json")
}

/// The same directory the daemons use for their own file logging, so the
/// shell-captured spawn log lands next to it.
fn logs_dir() -> std::path::PathBuf {
    channel::current().log_dir()
}

fn read_descriptor() -> Option<HiveRuntimeDescriptor> {
    serde_json::from_slice(&std::fs::read(descriptor_path()).ok()?).ok()
}

fn write_descriptor(url: &str, pid: u32) {
    if std::fs::create_dir_all(runtime_dir()).is_err() {
        return;
    }
    let started_at_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(json) = serde_json::to_vec_pretty(&HiveRuntimeDescriptor {
        hive_url: url.to_string(),
        pid,
        started_at_epoch,
    }) {
        let _ = std::fs::write(descriptor_path(), json);
    }
}

fn adopt(url: &str) -> Option<HiveDaemonClient> {
    let token = daemon_client::read_control_token(&data_root())?;
    let channel = channel::current().name();
    tauri::async_runtime::block_on(HiveDaemonClient::adopt(url, &token, channel))
        .ok()
        .flatten()
}

/// The daemon writes its token before it starts listening, but it may still
/// be starting when the shell launches.
fn adopt_within(url: &str, timeout: std::time::Duration) -> Option<HiveDaemonClient> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(client) = adopt(url) {
            return Some(client);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

fn hive_daemon_binary() -> Option<PathBuf> {
    resolve_internal_binary(None, "hive-daemon")
}

fn pick_hive_port() -> Option<u16> {
    channel::current()
        .hive_ports()
        .find(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

/// Spawn a process with a cleaned environment (a dev/agent shell can leak
/// `CLAUDECODE`, which breaks the claude-cli provider) and no console window.
fn spawn_clean(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    command
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()
}

fn spawn_hive_daemon() -> Result<(HiveDaemonClient, u32), String> {
    let bin = hive_daemon_binary().ok_or("hive-daemon binary not found")?;
    let port = pick_hive_port().ok_or("no free port for the Hive daemon")?;
    let url = format!("http://127.0.0.1:{}", port);
    let mut command = std::process::Command::new(&bin);
    command.arg("--port").arg(port.to_string());
    if let Some(data_dir) = nonempty_env_path("ABIGAIL_DATA_DIR") {
        command.arg("--data-dir").arg(data_dir);
    }

    // Redirect the child's stdout/stderr to a file instead of inheriting ours.
    // We're a `windows_subsystem = "windows"` shell with no console, so default
    // inherited handles are invalid and a console-subsystem child can die in the
    // loader before `main()` runs — before it even gets a chance to set up its
    // own `diag` file logging. This catches that window too.
    let spawn_log_path = logs_dir().join("hive-daemon.spawn.log");
    if std::fs::create_dir_all(logs_dir()).is_ok() {
        if let Ok(out) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spawn_log_path)
        {
            if let Ok(err) = out.try_clone() {
                command.stdout(out).stderr(err);
            }
        }
    }

    let mut child = spawn_clean(&mut command).map_err(|e| e.to_string())?;
    let pid = child.id();

    // Confirm it actually came up (e.g. it lost the port race) before we record
    // it in the descriptor and point the UI at it. The normal case binds in well
    // under a second; the bound only matters on failure.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "Hive daemon exited with {} (see {})",
                status,
                spawn_log_path.display()
            ));
        }
        if let Some(client) = adopt(&url) {
            return Ok((client, pid));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "Hive daemon did not become ready at {} (see {})",
        url,
        spawn_log_path.display()
    ))
}

fn ensure_hive_daemon() -> Result<HiveDaemonClient, String> {
    // Dev: the split launcher already started the daemon and set this.
    if let Some(url) = configured_hive_url() {
        return adopt_within(&url, std::time::Duration::from_secs(20)).ok_or_else(|| {
            format!(
                "The Hive daemon at {} did not prove it holds the control token in {}",
                url,
                data_root().display()
            )
        });
    }

    if let Some(desc) = read_descriptor() {
        if let Some(client) = adopt(&desc.hive_url) {
            tracing::info!("Adopted running Hive daemon at {}", desc.hive_url);
            return Ok(client);
        }
        // A recorded PID can be reused by Windows after the daemon exits.
        // Pick a free port without killing an unrelated process by stale PID.
    }

    let (client, pid) = spawn_hive_daemon()?;
    write_descriptor(client.base_url(), pid);
    tracing::info!("Started Hive daemon at {} (pid {})", client.base_url(), pid);
    Ok(client)
}

fn configure_packaged_resource_paths<R: tauri::Runtime>(app: &tauri::App<R>) {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return;
    };

    let nested_resource_dir = resource_dir.join("resources");
    let internal_bin_dir = if nested_resource_dir.is_dir() {
        nested_resource_dir.clone()
    } else {
        resource_dir.clone()
    };
    std::env::set_var(INTERNAL_BIN_DIR_ENV, &internal_bin_dir);

    let dirs = vec![resource_dir, nested_resource_dir];
    if let Some(path) = resolve_binary_from_dirs(
        None,
        &platform_binary_name("abigail-entity-runtime-app"),
        &dirs,
    ) {
        if path.is_file() {
            std::env::set_var(ENTITY_RUNTIME_APP_PATH_ENV, path);
        }
    }
    if let Some(path) =
        resolve_binary_from_dirs(None, &platform_binary_name("entity-daemon"), &dirs)
    {
        if path.is_file() {
            std::env::set_var(ENTITY_DAEMON_PATH_ENV, path);
        }
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "abigail_hive_app=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(OpenEntities::default())
        .setup(|app| {
            configure_packaged_resource_paths(app);
            match ensure_hive_daemon() {
                Ok(client) => {
                    let _ = HIVE.set(client);
                }
                Err(e) => tracing::error!("Failed to start Hive daemon: {}", e),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_hive_control_token,
            genesis_voice::transcribe_genesis_speech,
            genesis_voice::cancel_genesis_speech,
            get_hive_connection_info,
            open_entity,
            get_hive_status,
            create_entity,
            get_provider_config,
            update_entity_provider_config,
            store_secret,
            list_secrets,
            discover_provider_models,
            list_assignments,
            approve_forge_job,
            get_birth_scenarios,
            perform_birth,
            get_birth_document
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Abigail Hive app");
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
        std::env::temp_dir().join(format!("abigail-{}-{}", label, nanos))
    }

    #[test]
    fn control_token_is_withheld_until_a_daemon_is_adopted() {
        let _guard = ENV_LOCK.lock().unwrap();
        let data_dir = temp_dir("unadopted");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(
            data_dir.join(hive_core::access::CONTROL_TOKEN_FILE),
            "ab".repeat(32),
        )
        .unwrap();
        std::env::set_var("ABIGAIL_DATA_DIR", &data_dir);
        let result = get_hive_control_token();
        std::env::remove_var("ABIGAIL_DATA_DIR");

        assert_eq!(result, Err(HIVE_STARTING.to_string()));
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn resolver_prefers_valid_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let env_dir = temp_dir("env");
        let sibling_dir = temp_dir("sibling");
        fs::create_dir_all(&env_dir).unwrap();
        fs::create_dir_all(&sibling_dir).unwrap();
        let name = platform_binary_name("entity-daemon");
        let env_path = env_dir.join(&name);
        fs::write(&env_path, b"env").unwrap();
        fs::write(sibling_dir.join(&name), b"sibling").unwrap();

        std::env::set_var(ENTITY_DAEMON_PATH_ENV, &env_path);
        let resolved = resolve_binary_from_dirs(
            Some(ENTITY_DAEMON_PATH_ENV),
            &name,
            std::slice::from_ref(&sibling_dir),
        );
        std::env::remove_var(ENTITY_DAEMON_PATH_ENV);

        assert_eq!(resolved, Some(env_path));
        let _ = fs::remove_dir_all(env_dir);
        let _ = fs::remove_dir_all(sibling_dir);
    }

    #[test]
    fn resolver_uses_resource_dir_when_sibling_is_absent() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENTITY_RUNTIME_APP_PATH_ENV);
        let sibling_dir = temp_dir("missing-sibling");
        let resource_dir = temp_dir("resource");
        fs::create_dir_all(&sibling_dir).unwrap();
        fs::create_dir_all(&resource_dir).unwrap();
        let name = platform_binary_name("abigail-entity-runtime-app");
        let resource_path = resource_dir.join(&name);
        fs::write(&resource_path, b"resource").unwrap();

        let resolved = resolve_binary_from_dirs(
            Some(ENTITY_RUNTIME_APP_PATH_ENV),
            &name,
            &[sibling_dir, resource_dir.clone()],
        );

        assert_eq!(resolved, Some(resource_path));
        let _ = fs::remove_dir_all(resource_dir);
    }

    #[test]
    fn resolver_uses_nested_resources_dir_for_installer_layout() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(INTERNAL_BIN_DIR_ENV);
        let install_dir = temp_dir("installer-layout");
        let resources_dir = install_dir.join("resources");
        fs::create_dir_all(&resources_dir).unwrap();
        let name = platform_binary_name("hive-daemon");
        let nested_path = resources_dir.join(&name);
        fs::write(&nested_path, b"installer resource").unwrap();

        std::env::set_var(INTERNAL_BIN_DIR_ENV, &install_dir);
        let resolved = resolve_internal_binary(None, "hive-daemon");
        std::env::remove_var(INTERNAL_BIN_DIR_ENV);

        assert_eq!(resolved, Some(nested_path));
        let _ = fs::remove_dir_all(install_dir);
    }

    #[test]
    fn resolver_falls_back_to_first_candidate_for_clear_spawn_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENTITY_DAEMON_PATH_ENV);
        let sibling_dir = temp_dir("fallback");
        fs::create_dir_all(&sibling_dir).unwrap();
        let name = platform_binary_name("entity-daemon");

        let resolved = resolve_binary_from_dirs(
            Some(ENTITY_DAEMON_PATH_ENV),
            &name,
            std::slice::from_ref(&sibling_dir),
        );

        assert_eq!(resolved, Some(sibling_dir.join(name)));
        let _ = fs::remove_dir_all(sibling_dir);
    }
}
