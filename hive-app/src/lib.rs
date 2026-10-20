#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use abigail_core::profile::{self, ProcessIdentity, ProfileContext};
use daemon_client::HiveDaemonClient;
use hive_core::{
    ApiEnvelope, CreateForgeApprovalJobRequest, EntityOpenResponse, HiveStatus,
    RuntimeSessionLease, SkillAssignmentsResponse, UpdateEntityConfigRequest,
    UpdateEntityConfigResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;

/// Tracks which entities currently have a Runtime window open, so a second
/// "Open" click focuses the idea of one window per entity rather than spawning
/// duplicates. Shared with the per-window watcher task via an `Arc`.
#[derive(Default)]
struct OpenEntities(Arc<Mutex<HashSet<String>>>);

const ENTITY_RUNTIME_APP_PATH_ENV: &str = "ABIGAIL_ENTITY_RUNTIME_APP_PATH";
const ENTITY_DAEMON_PATH_ENV: &str = "ABIGAIL_ENTITY_DAEMON_PATH";
const INTERNAL_BIN_DIR_ENV: &str = "ABIGAIL_INTERNAL_BIN_DIR";
static CONNECTION: OnceLock<HiveConnectionInfo> = OnceLock::new();
static PACKAGED_BINARIES: OnceLock<PackagedBinaries> = OnceLock::new();
static STARTUP_ERROR: OnceLock<String> = OnceLock::new();
// A failed drain must not turn into a force-kill or an automatic retry.
// Keep the actual process handle for the lifetime of the visible error window.
static FAILED_HIVE: OnceLock<Mutex<std::process::Child>> = OnceLock::new();

struct PackagedBinaries {
    hive: PathBuf,
    entity: PathBuf,
    runtime: PathBuf,
}

fn preview() -> Result<Option<Arc<ProfileContext>>, String> {
    profile::current().map_err(|e| e.to_string())
}

fn loopback_url(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Invalid daemon URL")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url
            .port()
            .is_some_and(|port| port > 0 && !(43141..=43150).contains(&port))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Preview requires an explicit loopback daemon origin".into());
    }
    let canonical = url.origin().ascii_serialization();
    if value.trim_end_matches('/') != canonical {
        return Err("Preview daemon origin must use canonical loopback notation".into());
    }
    Ok(canonical)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("Cannot initialize local daemon HTTP client")
}

fn identity_headers(
    request: reqwest::RequestBuilder,
    identity: Option<&ProcessIdentity>,
) -> reqwest::RequestBuilder {
    match identity {
        Some(identity) => request
            .header("x-abigail-profile-id", &identity.profile_id)
            .header("x-abigail-process-instance-id", &identity.instance_id),
        None => request,
    }
}

async fn verify_status(
    url: &str,
    expected: &ProcessIdentity,
    entity_id: Option<&str>,
) -> Result<(), String> {
    let context = preview()?.ok_or("Preview profile context is missing")?;
    context
        .validate_identity(expected)
        .map_err(|e| e.to_string())?;
    let origin = loopback_url(url)?;
    let response = http_client()
        .get(format!("{origin}/v1/status"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<ApiEnvelope<serde_json::Value>>()
        .await
        .map_err(|e| e.to_string())?;
    let data = response
        .data
        .filter(|_| response.ok && response.error.is_none())
        .ok_or("Daemon status was unsuccessful")?;
    validate_status_payload(&data, expected, entity_id)
}

fn validate_status_payload(
    data: &serde_json::Value,
    expected: &ProcessIdentity,
    entity_id: Option<&str>,
) -> Result<(), String> {
    let actual: ProcessIdentity = serde_json::from_value(data["process_identity"].clone())
        .map_err(|_| "Daemon did not return a preview process identity")?;
    if actual != *expected || entity_id.is_some_and(|id| data["entity_id"].as_str() != Some(id)) {
        return Err("Daemon profile or live process identity changed; reopen the app".into());
    }
    Ok(())
}

async fn verified_connection(requested_url: Option<&str>) -> Result<HiveConnectionInfo, String> {
    if let Some(error) = STARTUP_ERROR.get() {
        return Err(error.clone());
    }
    let connection = CONNECTION.get().cloned().unwrap_or(HiveConnectionInfo {
        hive_url: hive_url(),
        process_identity: None,
    });
    if requested_url.is_some_and(|url| url.trim_end_matches('/') != connection.hive_url) {
        return Err("Saved Hive endpoint differs from this app's verified connection".into());
    }
    if preview()?.is_some() && connection.process_identity.is_none() {
        return Err("Preview Hive connection has not been established".into());
    }
    if let Some(identity) = &connection.process_identity {
        verify_status(&connection.hive_url, identity, None).await?;
    }
    Ok(connection)
}

async fn hive_client() -> Result<HiveDaemonClient, String> {
    let info = verified_connection(None).await?;
    let client = HiveDaemonClient::new(&info.hive_url);
    match info.process_identity {
        Some(identity) => client
            .with_process_identity(&identity)
            .map_err(|e| e.to_string()),
        None => Ok(client),
    }
}

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
    if preview()
        .expect("Profile preflight must precede binary resolution")
        .is_some()
    {
        return PACKAGED_BINARIES.get().map(|paths| paths.runtime.clone());
    }
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

#[derive(Debug, Clone, Serialize)]
struct HiveConnectionInfo {
    hive_url: String,
    process_identity: Option<ProcessIdentity>,
}

fn hive_url() -> String {
    CONNECTION
        .get()
        .map(|info| info.hive_url.clone())
        .unwrap_or_else(|| {
            std::env::var("ABIGAIL_HIVE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:43141".to_string())
        })
}

#[tauri::command]
async fn get_hive_status() -> Result<HiveStatus, String> {
    hive_get("/v1/status").await
}

#[tauri::command]
async fn get_hive_connection_info(hive_url: Option<String>) -> Result<HiveConnectionInfo, String> {
    verified_connection(hive_url.as_deref()).await
}

#[tauri::command]
async fn create_entity(name: String) -> Result<String, String> {
    let client = hive_client().await?;
    client.create_entity(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn issue_runtime_session(entity_id: String) -> Result<RuntimeSessionLease, String> {
    let client = hive_client().await?;
    client
        .issue_runtime_session(&entity_id, Some(format!("entity-runtime-{}", entity_id)))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_provider_config(entity_id: String) -> Result<hive_core::ProviderConfig, String> {
    let client = hive_client().await?;
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
    let client = hive_client().await?;
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
    let client = hive_client().await?;
    client
        .store_secret(&key, &value)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("Secret '{}' stored", key))
}

#[tauri::command]
async fn list_secrets() -> Result<Vec<String>, String> {
    let client = hive_client().await?;
    client.list_secrets().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn discover_provider_models(
    provider: String,
    api_key: String,
) -> Result<hive_core::ProviderModelsResponse, String> {
    let client = hive_client().await?;
    client
        .discover_provider_models(&provider, &api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_assignments(entity_id: String) -> Result<SkillAssignmentsResponse, String> {
    let client = hive_client().await?;
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
    hive_post(
        &format!("/v1/entities/{}/forge-approvals", entity_id),
        &CreateForgeApprovalJobRequest {
            skill_id,
            code_path,
            markdown_path,
            correlation_id: None,
        },
    )
    .await
}

async fn hive_get<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let info = verified_connection(None).await?;
    let response: hive_core::ApiEnvelope<T> = identity_headers(
        http_client().get(format!("{}{}", info.hive_url, path)),
        info.process_identity.as_ref(),
    )
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
    let info = verified_connection(None).await?;
    let response: hive_core::ApiEnvelope<T> = identity_headers(
        http_client().post(format!("{}{}", info.hive_url, path)),
        info.process_identity.as_ref(),
    )
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

#[derive(Debug, Clone, Serialize)]
struct EntityConnectionInfo {
    runtime_url: String,
    hive_url: String,
    process_identity: Option<ProcessIdentity>,
    hive_process_identity: Option<ProcessIdentity>,
    entity_id: String,
}

static ENTITY_CONNECTIONS: OnceLock<
    Mutex<std::collections::HashMap<String, EntityConnectionInfo>>,
> = OnceLock::new();

#[tauri::command]
async fn open_entity_connection(
    entity_id: String,
    hive_url: Option<String>,
) -> Result<EntityConnectionInfo, String> {
    let hive = verified_connection(hive_url.as_deref()).await?;
    if preview()?.is_some() {
        profile::validate_uuid(&entity_id).map_err(|e| e.to_string())?;
    }
    let response = identity_headers(
        http_client().post(format!("{}/v1/entities/{}/open", hive.hive_url, entity_id)),
        hive.process_identity.as_ref(),
    )
    .timeout(std::time::Duration::from_secs(55))
    .send()
    .await
    .map_err(|e| e.to_string())?
    .error_for_status()
    .map_err(|e| e.to_string())?
    .json::<ApiEnvelope<EntityOpenResponse>>()
    .await
    .map_err(|e| e.to_string())?;
    let opened = response
        .data
        .filter(|_| response.ok && response.error.is_none())
        .ok_or_else(|| {
            response
                .error
                .unwrap_or("Hive returned no runtime connection".into())
        })?;
    if let Some(context) = preview()? {
        let expected = opened
            .process_identity
            .as_ref()
            .ok_or("Hive returned no preview Entity identity")?;
        context
            .validate_identity(expected)
            .map_err(|e| e.to_string())?;
        if Some(expected.hive_instance_id.as_str())
            != hive
                .process_identity
                .as_ref()
                .map(|id| id.instance_id.as_str())
        {
            return Err("Entity belongs to a different Hive instance".into());
        }
        verify_status(&opened.local_url, expected, Some(&entity_id)).await?;
    }
    let info = EntityConnectionInfo {
        runtime_url: opened.local_url.trim_end_matches('/').to_string(),
        hive_url: hive.hive_url,
        process_identity: opened.process_identity,
        hive_process_identity: hive.process_identity,
        entity_id,
    };
    let mut connections = ENTITY_CONNECTIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some(existing) = connections.get(&info.runtime_url) {
        validate_connection_reuse(existing, &info)?;
    }
    connections.insert(info.runtime_url.clone(), info.clone());
    Ok(info)
}

fn validate_connection_reuse(
    existing: &EntityConnectionInfo,
    new: &EntityConnectionInfo,
) -> Result<(), String> {
    if existing.entity_id != new.entity_id
        || existing.process_identity != new.process_identity
        || existing.hive_process_identity != new.hive_process_identity
    {
        return Err(
            "An Entity endpoint was reused by another process; reopen Hive before continuing"
                .into(),
        );
    }
    Ok(())
}

#[tauri::command]
async fn get_entity_connection_info(runtime_url: String) -> Result<EntityConnectionInfo, String> {
    let info = ENTITY_CONNECTIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?
        .get(runtime_url.trim_end_matches('/'))
        .cloned()
        .ok_or("This Entity connection was not opened by this app")?;
    if let Some(identity) = &info.hive_process_identity {
        verify_status(&info.hive_url, identity, None).await?;
    }
    if let Some(identity) = &info.process_identity {
        verify_status(&info.runtime_url, identity, Some(&info.entity_id)).await?;
    }
    Ok(info)
}

#[tauri::command]
async fn get_runtime_connection_info(
    runtime_url: Option<String>,
    hive_url: Option<String>,
) -> Result<EntityConnectionInfo, String> {
    let info =
        get_entity_connection_info(runtime_url.ok_or("An opened Entity URL is required")?).await?;
    if hive_url.is_some_and(|url| url.trim_end_matches('/') != info.hive_url) {
        return Err("Hive endpoint differs from this Entity's verified connection".into());
    }
    Ok(info)
}

async fn close_owned_entity(entity_id: &str, info: &EntityConnectionInfo) -> Result<(), String> {
    if let Some(identity) = &info.hive_process_identity {
        verify_status(&info.hive_url, identity, None).await?;
    }
    let request = identity_headers(
        http_client().post(format!("{}/v1/entities/{}/close", info.hive_url, entity_id)),
        info.hive_process_identity.as_ref(),
    );
    let request = match &info.process_identity {
        Some(identity) => request
            .json(&serde_json::json!({ "expected_entity_instance_id": identity.instance_id })),
        None => request,
    };
    let response = request
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<ApiEnvelope<String>>()
        .await
        .map_err(|e| e.to_string())?;
    validate_close_response(response)
}

fn validate_close_response(response: ApiEnvelope<String>) -> Result<(), String> {
    if response.ok
        && response.error.is_none()
        && response.data.as_ref().is_some_and(|data| !data.is_empty())
    {
        Ok(())
    } else {
        Err(response
            .error
            .unwrap_or_else(|| "Hive did not confirm Entity cleanup".into()))
    }
}

/// Open one native window per Entity. Closure is fenced to the exact daemon
/// instances opened here, so an old window cannot terminate a replacement.
#[tauri::command]
async fn open_entity(
    entity_id: String,
    open: tauri::State<'_, OpenEntities>,
) -> Result<(), String> {
    let bin = entity_app_binary().ok_or("Entity Runtime app binary not found")?;
    {
        let mut set = open.0.lock().map_err(|e| e.to_string())?;
        if !set.insert(entity_id.clone()) {
            return Ok(());
        }
    }
    let tracking = open.0.clone();
    let info = match open_entity_connection(entity_id.clone(), None).await {
        Ok(info) => info,
        Err(error) => {
            forget(&tracking, &entity_id);
            return Err(error);
        }
    };
    let mut command = tokio::process::Command::new(&bin);
    command
        .env("ABIGAIL_ENTITY_URL", &info.runtime_url)
        .env("ABIGAIL_HIVE_URL", &info.hive_url)
        .env("ABIGAIL_ENTITY_ID", &entity_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(identity) = &info.process_identity {
        command
            .env("ABIGAIL_PROFILE", "preview")
            .env("ABIGAIL_DATA_DIR", &identity.canonical_data_root)
            .env("ABIGAIL_PROFILE_ID", &identity.profile_id)
            .env("ABIGAIL_HIVE_INSTANCE_ID", &identity.hive_instance_id)
            .env("ABIGAIL_PROCESS_INSTANCE_ID", &identity.instance_id);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            forget(&tracking, &entity_id);
            let _ = close_owned_entity(&entity_id, &info).await;
            return Err(format!("Failed to launch Entity Runtime app: {error}"));
        }
    };
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        if let Err(error) = close_owned_entity(&entity_id, &info).await {
            tracing::warn!("Entity close was refused: {}", error);
        }
        forget(&tracking, &entity_id);
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
    #[serde(default)]
    process_identity: Option<ProcessIdentity>,
}

fn runtime_dir() -> std::path::PathBuf {
    if let Some(context) = preview().expect("Profile preflight must precede runtime paths") {
        return context.runtime_dir();
    }
    if let Some(data_dir) = nonempty_env_path("ABIGAIL_DATA_DIR") {
        return data_dir.join("runtime");
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Abigail").join("runtime")
}

fn descriptor_path() -> std::path::PathBuf {
    runtime_dir().join("hive.json")
}

/// Mirrors `hive_daemon::diag::log_dir()` — same base the daemon uses for its
/// own file logging, so the shell-captured spawn log lands next to it.
fn logs_dir() -> std::path::PathBuf {
    if let Some(context) = preview().expect("Profile preflight must precede logging") {
        return context.logs_dir();
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Abigail").join("logs")
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
        process_identity: None,
    }) {
        let _ = std::fs::write(descriptor_path(), json);
    }
}

fn is_daemon_healthy(url: &str) -> bool {
    let url = url.to_string();
    tauri::async_runtime::block_on(async move {
        http_client()
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

fn pick_preview_port() -> Result<u16, String> {
    for _ in 0..16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        if !(43141..=43150).contains(&port) {
            return Ok(port);
        }
    }
    Err("Could not select a preview port outside the normal application range".into())
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
        if is_daemon_healthy(&url) {
            return Ok((url, pid));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "Hive daemon did not become healthy at {} (see {})",
        url,
        spawn_log_path.display()
    ))
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
        // A recorded PID can be reused by Windows after the daemon exits.
        // Pick a free port without killing an unrelated process by stale PID.
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

fn validate_preview_descriptor(
    context: &ProfileContext,
    desc: &HiveRuntimeDescriptor,
) -> Result<(), String> {
    loopback_url(&desc.hive_url)?;
    let identity = desc
        .process_identity
        .as_ref()
        .ok_or("Preview descriptor has no process identity")?;
    context
        .validate_identity(identity)
        .map_err(|e| e.to_string())?;
    if identity.instance_id != identity.hive_instance_id {
        return Err("Preview descriptor does not identify a Hive process".into());
    }
    Ok(())
}

fn shutdown_acknowledged(response: &ApiEnvelope<serde_json::Value>) -> bool {
    response.ok
        && response.error.is_none()
        && response.data.as_ref().is_some_and(|data| {
            data["shutdown"].as_bool() == Some(true)
                && data["owned_entities_reaped"].as_bool() == Some(true)
        })
}

#[derive(Debug, PartialEq, Eq)]
enum StartupCleanup {
    ConfirmedExit,
    RetainHandle,
}

fn startup_cleanup(drained: bool, child_exited: bool) -> StartupCleanup {
    if drained && child_exited {
        StartupCleanup::ConfirmedExit
    } else {
        StartupCleanup::RetainHandle
    }
}

fn failed_preview_start(
    mut child: std::process::Child,
    url: &str,
    identity: &ProcessIdentity,
    reason: String,
) -> String {
    let drained = tauri::async_runtime::block_on(async {
        verify_status(url, identity, None).await?;
        let response = identity_headers(
            http_client().post(format!("{url}/v1/shutdown")),
            Some(identity),
        )
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<ApiEnvelope<serde_json::Value>>()
        .await
        .map_err(|e| e.to_string())?;
        if !shutdown_acknowledged(&response) {
            return Err("Hive did not confirm owned Entity cleanup".to_string());
        }
        Ok::<(), String>(())
    })
    .is_ok();
    if drained {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if startup_cleanup(drained, matches!(child.try_wait(), Ok(Some(_))))
                == StartupCleanup::ConfirmedExit
            {
                return format!(
                    "{reason}. Hive confirmed cleanup and exited; reopen the app to retry."
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = FAILED_HIVE.set(Mutex::new(child));
    if drained {
        format!("{reason}. Entity cleanup was confirmed, but Hive exit is unconfirmed. The app retains its process handle and will not restart automatically.")
    } else {
        format!("{reason}. Hive and Entity cleanup are unconfirmed. The app retains its process handle and will not force-stop or restart it.")
    }
}

fn ensure_preview_hive(context: &ProfileContext) -> Result<(), String> {
    let descriptor = context.runtime_dir().join("hive.json");
    context
        .validate_path(&descriptor)
        .map_err(|e| e.to_string())?;
    match std::fs::read(&descriptor) {
        Ok(bytes) => {
            let desc: HiveRuntimeDescriptor =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid preview Hive descriptor")?;
            validate_preview_descriptor(context, &desc)?;
            if is_daemon_healthy(&desc.hive_url) {
                let identity = desc.process_identity.expect("Validated descriptor");
                tauri::async_runtime::block_on(verify_status(&desc.hive_url, &identity, None))?;
                return CONNECTION
                    .set(HiveConnectionInfo {
                        hive_url: desc.hive_url,
                        process_identity: Some(identity),
                    })
                    .map_err(|_| "Hive connection already initialized".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let binaries = PACKAGED_BINARIES
        .get()
        .ok_or("Preview packaged binaries are missing")?;
    // The port is provisional until the exact expected instance answers. A
    // process winning the bind race can never be adopted based on health alone.
    let port = pick_preview_port()?;
    let url = format!("http://127.0.0.1:{port}");
    let instance = profile::new_instance_id();
    let identity = context
        .process_identity(&instance, &instance)
        .map_err(|e| e.to_string())?;
    let log_path = context.logs_dir().join("hive-daemon.spawn.log");
    context
        .validate_path(&log_path)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(context.logs_dir()).map_err(|e| e.to_string())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(&binaries.hive);
    command
        .arg("--profile")
        .arg("preview")
        .arg("--data-dir")
        .arg(context.root())
        .arg("--port")
        .arg(port.to_string())
        .env("ABIGAIL_PROFILE", "preview")
        .env("ABIGAIL_DATA_DIR", context.root())
        .env("ABIGAIL_PROFILE_ID", context.profile_id())
        .env("ABIGAIL_HIVE_INSTANCE_ID", &instance)
        .env("ABIGAIL_PROCESS_INSTANCE_ID", &instance)
        .env(ENTITY_DAEMON_PATH_ENV, &binaries.entity)
        .env_remove("ABIGAIL_HIVE_URL")
        .env_remove("ABIGAIL_ENTITY_URL")
        .stdin(std::process::Stdio::null())
        .stderr(log.try_clone().map_err(|e| e.to_string())?)
        .stdout(log);
    let mut child = spawn_clean(&mut command).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => {
                let _ = FAILED_HIVE.set(Mutex::new(child));
                return Err(format!("Preview Hive exited during startup: {status}. Entity cleanup is unconfirmed; no automatic restart."));
            }
            Ok(None) => {}
            Err(error) => {
                return Err(failed_preview_start(
                    child,
                    &url,
                    &identity,
                    error.to_string(),
                ))
            }
        }
        if tauri::async_runtime::block_on(verify_status(&url, &identity, None)).is_ok() {
            let desc = HiveRuntimeDescriptor {
                hive_url: url.clone(),
                pid: child.id(),
                started_at_epoch: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                process_identity: Some(identity.clone()),
            };
            let bytes = match serde_json::to_vec_pretty(&desc) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Err(failed_preview_start(
                        child,
                        &url,
                        &identity,
                        error.to_string(),
                    ))
                }
            };
            if let Err(error) = abigail_core::secure_fs::write_bytes_atomic(&descriptor, &bytes) {
                return Err(failed_preview_start(
                    child,
                    &url,
                    &identity,
                    error.to_string(),
                ));
            }
            if CONNECTION
                .set(HiveConnectionInfo {
                    hive_url: url.clone(),
                    process_identity: Some(identity.clone()),
                })
                .is_err()
            {
                return Err(failed_preview_start(
                    child,
                    &url,
                    &identity,
                    "Hive connection already initialized".into(),
                ));
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err(failed_preview_start(
        child,
        &url,
        &identity,
        "Preview Hive did not confirm its expected process identity".into(),
    ))
}

fn validate_preview_binary_override(
    name: &str,
    value: Option<&std::ffi::OsStr>,
    expected: &Path,
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let supplied = Path::new(value);
    if !supplied.is_absolute()
        || supplied
            .canonicalize()
            .map_err(|_| format!("Preview launcher cannot verify inherited {name}"))?
            != expected
    {
        return Err(format!(
            "Preview launcher refuses inherited {name} that differs from its packaged path"
        ));
    }
    Ok(())
}

fn configure_preview_binaries<R: tauri::Runtime>(app: &tauri::App<R>) -> Result<(), String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    let dirs = [resource_dir.join("resources"), resource_dir];
    let find = |stem| -> Result<PathBuf, String> {
        dirs.iter()
            .map(|dir| dir.join(platform_binary_name(stem)))
            .find(|path| path.is_file())
            .ok_or_else(|| format!("Required packaged preview binary is missing: {stem}"))
            .and_then(|path| path.canonicalize().map_err(|e| e.to_string()))
    };
    let binaries = PackagedBinaries {
        hive: find("hive-daemon")?,
        entity: find("entity-daemon")?,
        runtime: find("abigail-entity-runtime-app")?,
    };
    for (name, expected) in [
        (ENTITY_DAEMON_PATH_ENV, binaries.entity.as_path()),
        (ENTITY_RUNTIME_APP_PATH_ENV, binaries.runtime.as_path()),
        (
            INTERNAL_BIN_DIR_ENV,
            binaries
                .hive
                .parent()
                .ok_or("Packaged Hive has no parent directory")?,
        ),
    ] {
        validate_preview_binary_override(name, std::env::var_os(name).as_deref(), expected)?;
    }
    PACKAGED_BINARIES
        .set(binaries)
        .map_err(|_| "Packaged binaries were already configured".into())
}

fn initialize_launcher_profile() -> Result<Option<Arc<ProfileContext>>, String> {
    if profile::compiled_preview() {
        if std::env::var_os("ABIGAIL_PROFILE").is_none() {
            std::env::set_var("ABIGAIL_PROFILE", "preview");
        }
        if std::env::var_os("ABIGAIL_DATA_DIR").is_none() {
            std::env::set_var(
                "ABIGAIL_DATA_DIR",
                profile::default_preview_root().map_err(|e| e.to_string())?,
            );
        }
    }
    let requested = profile::compiled_preview()
        || std::env::var("ABIGAIL_PROFILE").ok().as_deref() == Some("preview");
    if requested && std::env::var_os("ABIGAIL_HIVE_URL").is_some() {
        return Err("Preview Hive launcher refuses an inherited ABIGAIL_HIVE_URL".into());
    }
    profile::initialize_process(None, None).map_err(|e| e.to_string())
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
    // No logging, resource setup, or WebView may precede profile validation.
    let profile = initialize_launcher_profile().expect("Hive profile preflight failed");
    let subscriber = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "abigail_hive_app=info".into()),
    );
    if let Some(context) = &profile {
        let path = context.logs_dir().join("hive-app.log");
        context
            .validate_path(&path)
            .expect("Invalid preview log path");
        std::fs::create_dir_all(context.logs_dir()).expect("Cannot create preview logs");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("Cannot open preview log");
        subscriber
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .init();
    } else {
        subscriber.init();
    }
    let mut context = tauri::generate_context!();
    let preview_windows = if profile.is_some() {
        let windows = context.config().app.windows.clone();
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
        windows
    } else {
        Vec::new()
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(OpenEntities::default())
        .setup(move |app| {
            if let Some(context) = &profile {
                if let Err(error) =
                    configure_preview_binaries(app).and_then(|_| ensure_preview_hive(context))
                {
                    tracing::error!("Preview startup refused: {}", error);
                    let _ = STARTUP_ERROR.set(error);
                }
                let data = context.root().join("webview").join("hive");
                context.validate_path(&data)?;
                for window in &preview_windows {
                    tauri::WebviewWindowBuilder::from_config(app, window)?
                        .data_directory(data.clone())
                        .build()?;
                }
            } else {
                configure_packaged_resource_paths(app);
                ensure_hive_daemon();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_hive_connection_info,
            open_entity_connection,
            get_entity_connection_info,
            get_runtime_connection_info,
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
        .run(context)
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
    fn preview_binary_overrides_must_match_the_packaged_path() {
        let root = temp_dir("preview-binary-overrides");
        fs::create_dir_all(&root).unwrap();
        let expected = root.join(platform_binary_name("entity-daemon"));
        let conflicting = root.join(platform_binary_name("other-entity-daemon"));
        fs::write(&expected, b"synthetic-preview-binary").unwrap();
        fs::write(&conflicting, b"synthetic-other-binary").unwrap();
        let canonical = expected.canonicalize().unwrap();
        assert!(validate_preview_binary_override("SYNTHETIC_PATH", None, &canonical).is_ok());
        assert!(validate_preview_binary_override(
            "SYNTHETIC_PATH",
            Some(expected.as_os_str()),
            &canonical,
        )
        .is_ok());
        for supplied in [Path::new(""), Path::new("relative"), conflicting.as_path()] {
            assert!(validate_preview_binary_override(
                "SYNTHETIC_PATH",
                Some(supplied.as_os_str()),
                &canonical,
            )
            .is_err());
        }
        assert_eq!(fs::read(&conflicting).unwrap(), b"synthetic-other-binary");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn entity_cleanup_requires_a_successful_envelope() {
        assert!(validate_close_response(ApiEnvelope::success("Entity stopped".into())).is_ok());
        assert_eq!(
            validate_close_response(ApiEnvelope::error("Stale Entity instance")),
            Err("Stale Entity instance".into()),
        );
        for response in [
            ApiEnvelope {
                ok: true,
                data: Some("Entity stopped".into()),
                error: Some("Cleanup failed".into()),
            },
            ApiEnvelope {
                ok: true,
                data: None,
                error: None,
            },
            ApiEnvelope::success(String::new()),
        ] {
            assert!(validate_close_response(response).is_err());
        }
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

        let resolved =
            resolve_binary_from_dirs(Some(ENTITY_DAEMON_PATH_ENV), &name, &[sibling_dir.clone()]);

        assert_eq!(resolved, Some(sibling_dir.join(name)));
        let _ = fs::remove_dir_all(sibling_dir);
    }

    #[test]
    fn preview_origin_refuses_normal_ports_remote_hosts_and_redirect_paths() {
        for value in [
            "http://127.0.0.1:43141",
            "http://127.0.0.1:43142",
            "http://127.0.0.1:0",
            "https://127.0.0.1:51000",
            "http://localhost:51000",
            "http://127.1:51000",
            "http://127.0.0.1:051000",
            "http://example.test:51000",
            "http://user@127.0.0.1:51000",
            "http://127.0.0.1:51000/v1",
            "http://127.0.0.1:51000/?x=1",
        ] {
            assert!(loopback_url(value).is_err(), "{value}");
        }
        assert_eq!(
            loopback_url("http://127.0.0.1:51000/").unwrap(),
            "http://127.0.0.1:51000"
        );
    }

    #[test]
    fn preview_descriptor_is_bound_to_profile_root_channel_and_hive_instance() {
        let root = temp_dir("preview-descriptor");
        let context = ProfileContext::preflight(&root, &[]).unwrap();
        let instance = profile::new_instance_id();
        let identity = context.process_identity(&instance, &instance).unwrap();
        let mut descriptor = HiveRuntimeDescriptor {
            hive_url: "http://127.0.0.1:51000".into(),
            pid: 1,
            started_at_epoch: 0,
            process_identity: Some(identity.clone()),
        };
        assert!(validate_preview_descriptor(&context, &descriptor).is_ok());
        for field in ["profile", "root", "channel", "hive_instance"] {
            let mut foreign = identity.clone();
            match field {
                "profile" => foreign.profile_id = profile::new_instance_id(),
                "root" => foreign.canonical_data_root = root.join("another"),
                "channel" => foreign.channel = "stable".into(),
                _ => foreign.hive_instance_id = profile::new_instance_id(),
            }
            descriptor.process_identity = Some(foreign);
            assert!(
                validate_preview_descriptor(&context, &descriptor).is_err(),
                "{field}"
            );
        }
        descriptor.process_identity = None;
        assert!(validate_preview_descriptor(&context, &descriptor).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn status_requires_exact_expected_live_instance_and_entity() {
        let identity = ProcessIdentity {
            channel: "beta".into(),
            profile_id: profile::new_instance_id(),
            canonical_data_root: PathBuf::from("synthetic-profile"),
            instance_id: profile::new_instance_id(),
            hive_instance_id: profile::new_instance_id(),
        };
        let data = serde_json::json!({ "process_identity": identity, "entity_id": "expected" });
        assert!(validate_status_payload(&data, &identity, Some("expected")).is_ok());
        assert!(validate_status_payload(&data, &identity, Some("another")).is_err());
        let mut replacement = identity.clone();
        replacement.instance_id = profile::new_instance_id();
        assert!(validate_status_payload(&data, &replacement, Some("expected")).is_err());
        assert!(validate_status_payload(&serde_json::json!({}), &identity, None).is_err());
    }

    #[test]
    fn a_reused_entity_port_cannot_repin_an_existing_window() {
        let identity = ProcessIdentity {
            channel: "beta".into(),
            profile_id: profile::new_instance_id(),
            canonical_data_root: PathBuf::from("synthetic-profile"),
            instance_id: profile::new_instance_id(),
            hive_instance_id: profile::new_instance_id(),
        };
        let existing = EntityConnectionInfo {
            runtime_url: "http://127.0.0.1:51001".into(),
            hive_url: "http://127.0.0.1:51000".into(),
            process_identity: Some(identity),
            hive_process_identity: None,
            entity_id: "one".into(),
        };
        assert!(validate_connection_reuse(&existing, &existing).is_ok());
        let mut replacement = existing.clone();
        replacement.process_identity.as_mut().unwrap().instance_id = profile::new_instance_id();
        assert!(validate_connection_reuse(&existing, &replacement).is_err());
        replacement = existing.clone();
        replacement.entity_id = "two".into();
        assert!(validate_connection_reuse(&existing, &replacement).is_err());
    }

    #[test]
    fn startup_failure_requires_explicit_drain_and_exit_before_releasing_ownership() {
        let envelope = |data| ApiEnvelope::<serde_json::Value> {
            ok: true,
            data: Some(data),
            error: None,
        };
        assert!(shutdown_acknowledged(&envelope(serde_json::json!({
            "shutdown": true, "owned_entities_reaped": true,
        }))));
        let mut contradictory = envelope(serde_json::json!({
            "shutdown": true, "owned_entities_reaped": true,
        }));
        contradictory.error = Some("cleanup failed".into());
        assert!(!shutdown_acknowledged(&contradictory));
        for partial in [
            serde_json::json!({}),
            serde_json::json!({ "shutdown": true }),
            serde_json::json!({ "shutdown": true, "owned_entities_reaped": false }),
        ] {
            assert!(!shutdown_acknowledged(&envelope(partial)));
        }
        assert_eq!(startup_cleanup(true, true), StartupCleanup::ConfirmedExit);
        assert_eq!(startup_cleanup(false, true), StartupCleanup::RetainHandle);
        assert_eq!(startup_cleanup(true, false), StartupCleanup::RetainHandle);
        assert_eq!(startup_cleanup(false, false), StartupCleanup::RetainHandle);
    }
}
