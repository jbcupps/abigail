#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use daemon_client::HiveDaemonClient;
use hive_core::{
    ApiEnvelope, CreateForgeApprovalJobRequest, EntityOpenResponse, HiveStatus,
    RuntimeSessionLease, SkillAssignmentsResponse, UpdateEntityConfigRequest,
    UpdateEntityConfigResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;

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
        dirs.push(dir);
    }
    if let Some(dir) = nonempty_env_path(INTERNAL_BIN_DIR_ENV) {
        if !dirs.iter().any(|existing| existing == &dir) {
            dirs.push(dir);
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

fn hive_url() -> String {
    std::env::var("ABIGAIL_HIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:43141".to_string())
}

#[tauri::command]
async fn get_hive_status() -> Result<HiveStatus, String> {
    let client = HiveDaemonClient::new(&hive_url());
    let entities = client.list_entities().await.map_err(|e| e.to_string())?;
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

#[tauri::command]
async fn create_entity(name: String) -> Result<String, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client.create_entity(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn issue_runtime_session(entity_id: String) -> Result<RuntimeSessionLease, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client
        .issue_runtime_session(&entity_id, Some(format!("entity-runtime-{}", entity_id)))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_provider_config(entity_id: String) -> Result<hive_core::ProviderConfig, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client
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
    let client = HiveDaemonClient::new(&hive_url());
    client
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
    let client = HiveDaemonClient::new(&hive_url());
    client
        .store_secret(&key, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("Secret '{}' stored", key))
}

#[tauri::command]
async fn list_secrets() -> Result<Vec<String>, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client.list_secrets().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn discover_provider_models(
    provider: String,
    api_key: String,
) -> Result<hive_core::ProviderModelsResponse, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client
        .discover_provider_models(&provider, &api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_assignments(entity_id: String) -> Result<SkillAssignmentsResponse, String> {
    let client = HiveDaemonClient::new(&hive_url());
    client
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
    let client = reqwest::Client::new();
    let base_url = hive_url();
    let response: hive_core::ApiEnvelope<hive_core::ForgeApprovalJob> = client
        .post(format!(
            "{}/v1/entities/{}/forge-approvals",
            base_url, entity_id
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
    let response: hive_core::ApiEnvelope<T> = reqwest::Client::new()
        .get(format!("{}{}", hive_url(), path))
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
    let response: hive_core::ApiEnvelope<T> = reqwest::Client::new()
        .post(format!("{}{}", hive_url(), path))
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
) -> Result<hive_core::BirthRiteResponse, String> {
    hive_post(
        &format!("/v1/entities/{}/birth", entity_id),
        &hive_core::BirthRiteRequest { path, choices },
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
        reqwest::Client::new()
            .post(format!("{}/v1/entities/{}/open", hive, entity_id))
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
    let bin = entity_app_binary().ok_or("Entity Runtime app binary not found")?;
    let child = tokio::process::Command::new(&bin)
        .env("ABIGAIL_ENTITY_URL", &local_url)
        .env("ABIGAIL_HIVE_URL", &hive)
        .spawn()
        .map_err(|e| format!("Failed to launch Entity Runtime app: {}", e));
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            forget(&tracking, &entity_id);
            return Err(e);
        }
    };

    // 3. When the window closes (process exits), stop the daemon.
    let entity_id_for_task = entity_id.clone();
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        forget(&tracking, &entity_id_for_task);
        let _ = reqwest::Client::new()
            .post(format!("{}/v1/entities/{}/close", hive, entity_id_for_task))
            .send()
            .await;
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Production launch — ensure the Hive daemon is running.
//
// In a packaged install the Hive app is the user's only entry point: double-
// clicking it must bring up the whole stack with no terminal. On launch it
// adopts a healthy daemon recorded in a runtime descriptor, or spawns a fresh
// one (on the first free port) and records it. In dev the split launcher sets
// `ABIGAIL_HIVE_URL` and starts the daemon itself, so this is skipped entirely.
// ---------------------------------------------------------------------------

const HIVE_PORT_START: u16 = 43141;
const HIVE_PORT_END: u16 = 43150;

#[derive(Serialize, Deserialize)]
struct HiveRuntimeDescriptor {
    hive_url: String,
    pid: u32,
    started_at_epoch: u64,
}

fn runtime_dir() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Abigail").join("runtime")
}

fn descriptor_path() -> std::path::PathBuf {
    runtime_dir().join("hive.json")
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

fn is_daemon_healthy(url: &str) -> bool {
    let url = url.to_string();
    tauri::async_runtime::block_on(async move {
        reqwest::Client::new()
            .get(format!("{}/health", url))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    })
}

fn hive_daemon_binary() -> Option<PathBuf> {
    resolve_internal_binary(None, "hive-daemon")
}

fn pick_hive_port() -> Option<u16> {
    (HIVE_PORT_START..=HIVE_PORT_END)
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

fn spawn_hive_daemon() -> Result<(String, u32), String> {
    let bin = hive_daemon_binary().ok_or("hive-daemon binary not found")?;
    let port = pick_hive_port().ok_or("no free port for the Hive daemon")?;
    let url = format!("http://127.0.0.1:{}", port);
    let mut command = std::process::Command::new(&bin);
    command.arg("--port").arg(port.to_string());
    let child = spawn_clean(&mut command).map_err(|e| e.to_string())?;
    let pid = child.id();

    // Confirm it actually came up (e.g. it lost the port race) before we record
    // it in the descriptor and point the UI at it. The normal case binds in well
    // under a second; the bound only matters on failure.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if is_daemon_healthy(&url) {
            return Ok((url, pid));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    Err(format!("Hive daemon did not become healthy at {}", url))
}

fn reap_stale(pid: u32) {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/F", "/T"]);
        let _ = spawn_clean(&mut cmd).map(|mut c| c.wait());
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

fn ensure_hive_daemon() {
    // Dev: the split launcher already started the daemon and set this.
    if std::env::var("ABIGAIL_HIVE_URL")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }

    // Adopt a healthy recorded daemon if one exists.
    if let Some(desc) = read_descriptor() {
        if is_daemon_healthy(&desc.hive_url) {
            std::env::set_var("ABIGAIL_HIVE_URL", &desc.hive_url);
            tracing::info!("Adopted running Hive daemon at {}", desc.hive_url);
            return;
        }
        // Recorded but unhealthy — reap the stale process before respawning.
        reap_stale(desc.pid);
    }

    match spawn_hive_daemon() {
        Ok((url, pid)) => {
            std::env::set_var("ABIGAIL_HIVE_URL", &url);
            write_descriptor(&url, pid);
            tracing::info!("Started Hive daemon at {} (pid {})", url, pid);
        }
        Err(e) => tracing::error!("Failed to start Hive daemon: {}", e),
    }
}

fn configure_packaged_resource_paths<R: tauri::Runtime>(app: &tauri::App<R>) {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return;
    };

    std::env::set_var(INTERNAL_BIN_DIR_ENV, &resource_dir);

    let dirs = vec![resource_dir];
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
            ensure_hive_daemon();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_hive_connection_info,
            open_entity,
            get_hive_status,
            create_entity,
            issue_runtime_session,
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
        let resolved =
            resolve_binary_from_dirs(Some(ENTITY_DAEMON_PATH_ENV), &name, &[sibling_dir.clone()]);
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
    fn resolver_falls_back_to_first_candidate_for_clear_spawn_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(ENTITY_DAEMON_PATH_ENV);
        let sibling_dir = temp_dir("fallback");
        fs::create_dir_all(&sibling_dir).unwrap();
        let name = platform_binary_name("entity-daemon");

        let resolved =
            resolve_binary_from_dirs(Some(ENTITY_DAEMON_PATH_ENV), &name, &[sibling_dir.clone()]);

        assert_eq!(resolved, Some(sibling_dir.join(name)));
        let _ = fs::remove_dir_all(sibling_dir);
    }
}
