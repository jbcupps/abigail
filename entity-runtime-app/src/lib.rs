#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use abigail_core::profile::{self, ProcessIdentity, ProfileContext};
use daemon_client::EntityClient;
use entity_core::{
    ChatRequest, ChatResponse, EntityOutboxStatus, RuntimeSessionStatusResponse,
    SkillApplyAcknowledgementList,
};
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Emitter;

#[derive(Debug, Clone, Serialize)]
struct RuntimeConnectionInfo {
    runtime_url: String,
    hive_url: Option<String>,
    process_identity: Option<ProcessIdentity>,
    hive_process_identity: Option<ProcessIdentity>,
    #[serde(skip)]
    entity_id: Option<String>,
}

static CONNECTION: OnceLock<RuntimeConnectionInfo> = OnceLock::new();

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

async fn verify_status(
    url: &str,
    expected: &ProcessIdentity,
    entity_id: Option<&str>,
) -> Result<(), String> {
    let context = profile::current()
        .map_err(|e| e.to_string())?
        .ok_or("Preview context is missing")?;
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
        .json::<entity_core::ApiEnvelope<serde_json::Value>>()
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

#[tauri::command]
async fn get_runtime_connection_info(
    runtime_url: Option<String>,
    hive_url: Option<String>,
) -> Result<RuntimeConnectionInfo, String> {
    let info = CONNECTION.get().cloned().unwrap_or(RuntimeConnectionInfo {
        runtime_url: std::env::var("ABIGAIL_ENTITY_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:43142".into()),
        hive_url: std::env::var("ABIGAIL_HIVE_URL").ok(),
        process_identity: None,
        hive_process_identity: None,
        entity_id: None,
    });
    if runtime_url.is_some_and(|url| url.trim_end_matches('/') != info.runtime_url)
        || hive_url.is_some_and(|url| Some(url.trim_end_matches('/')) != info.hive_url.as_deref())
    {
        return Err("Requested endpoint differs from this Runtime's verified connection".into());
    }
    if profile::current().map_err(|e| e.to_string())?.is_some() && info.process_identity.is_none() {
        return Err("Preview Runtime connection has not been established".into());
    }
    if let Some(identity) = &info.hive_process_identity {
        verify_status(
            info.hive_url.as_deref().ok_or("Missing Hive endpoint")?,
            identity,
            None,
        )
        .await?;
    }
    if let Some(identity) = &info.process_identity {
        verify_status(&info.runtime_url, identity, info.entity_id.as_deref()).await?;
    }
    Ok(info)
}

async fn entity_client() -> Result<EntityClient, String> {
    let info = get_runtime_connection_info(None, None).await?;
    let client = EntityClient::new(&info.runtime_url);
    match info.process_identity {
        Some(identity) => client
            .with_process_identity(&identity)
            .map_err(|e| e.to_string()),
        None => Ok(client),
    }
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .map_err(|_| format!("Preview Runtime requires {name}"))
        .and_then(|value| {
            if value.trim().is_empty() {
                Err(format!("Preview Runtime requires nonempty {name}"))
            } else {
                Ok(value)
            }
        })
}

fn initialize_runtime_profile() -> Result<Option<Arc<ProfileContext>>, String> {
    let requested = profile::compiled_preview()
        || std::env::var("ABIGAIL_PROFILE").ok().as_deref() == Some("preview");
    if !requested {
        return profile::initialize_process(None, None).map_err(|e| e.to_string());
    }
    // Children may not create missing parent profiles or infer endpoints.
    let profile_id = required_env("ABIGAIL_PROFILE_ID")?;
    profile::validate_uuid(&profile_id).map_err(|e| e.to_string())?;
    let instance = required_env("ABIGAIL_PROCESS_INSTANCE_ID")?;
    let hive_instance = required_env("ABIGAIL_HIVE_INSTANCE_ID")?;
    let entity_id = required_env("ABIGAIL_ENTITY_ID")?;
    for id in [&instance, &hive_instance, &entity_id] {
        profile::validate_uuid(id).map_err(|e| e.to_string())?;
    }
    let runtime_url = loopback_url(&required_env("ABIGAIL_ENTITY_URL")?)?;
    let hive_url = loopback_url(&required_env("ABIGAIL_HIVE_URL")?)?;
    let context = profile::initialize_process(None, None)
        .map_err(|e| e.to_string())?
        .ok_or("Preview Runtime profile is missing")?;
    let identity = context
        .process_identity(&instance, &hive_instance)
        .map_err(|e| e.to_string())?;
    let hive_identity = context
        .process_identity(&hive_instance, &hive_instance)
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::block_on(async {
        verify_status(&hive_url, &hive_identity, None).await?;
        verify_status(&runtime_url, &identity, Some(&entity_id)).await
    })?;
    CONNECTION
        .set(RuntimeConnectionInfo {
            runtime_url,
            hive_url: Some(hive_url),
            process_identity: Some(identity),
            hive_process_identity: Some(hive_identity),
            entity_id: Some(entity_id),
        })
        .map_err(|_| "Runtime connection is already initialized")?;
    Ok(Some(context))
}

#[tauri::command]
async fn get_runtime_status() -> Result<serde_json::Value, String> {
    entity_client()
        .await?
        .status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_session_status() -> Result<RuntimeSessionStatusResponse, String> {
    entity_client()
        .await?
        .session_status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_outbox_status() -> Result<EntityOutboxStatus, String> {
    entity_client()
        .await?
        .outbox_status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_chat(message: String, session_id: Option<String>) -> Result<ChatResponse, String> {
    entity_client()
        .await?
        .chat(&ChatRequest {
            message,
            target: None,
            session_messages: None,
            session_id,
            model_override: None,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_chat_stream(
    app: tauri::AppHandle,
    message: String,
    session_id: Option<String>,
) -> Result<(), String> {
    let client = entity_client().await?;
    let request = ChatRequest {
        message: message.clone(),
        target: None,
        session_messages: None,
        session_id,
        model_override: None,
    };
    let _ = app.emit(
        "runtime-chat-envelope",
        serde_json::json!({ "type": "Request", "message": message }),
    );
    let mut rx = client
        .chat_stream(&request)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(event) = rx.recv().await {
        match event {
            daemon_client::ChatStreamEvent::Token(token) => {
                let _ = app.emit(
                    "runtime-chat-envelope",
                    serde_json::json!({ "type": "Token", "token": token }),
                );
            }
            daemon_client::ChatStreamEvent::Done(resp) => {
                let _ = app.emit(
                    "runtime-chat-envelope",
                    serde_json::json!({
                        "type": "Done",
                        "reply": &resp.reply,
                        "provider": &resp.provider,
                        "tier": &resp.tier,
                        "model_used": &resp.model_used,
                        "complexity_score": &resp.complexity_score,
                    }),
                );
            }
            daemon_client::ChatStreamEvent::Error(error) => {
                let _ = app.emit(
                    "runtime-chat-envelope",
                    serde_json::json!({ "type": "Error", "error": error }),
                );
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn cancel_chat_stream() -> Result<bool, String> {
    entity_client()
        .await?
        .cancel_chat_stream()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_skill_acks() -> Result<SkillApplyAcknowledgementList, String> {
    entity_client()
        .await?
        .list_skill_apply_acknowledgements()
        .await
        .map_err(|e| e.to_string())
}

pub fn run() {
    let profile = initialize_runtime_profile().expect("Runtime profile preflight failed");
    let subscriber = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "abigail_entity_runtime_app=info".into()),
    );
    let mut context = tauri::generate_context!();
    let preview_windows = if let Some(profile) = &profile {
        let entity_id = CONNECTION
            .get()
            .and_then(|info| info.entity_id.as_deref())
            .expect("Validated Entity identity");
        let path = profile
            .logs_dir()
            .join(format!("runtime-app-{entity_id}.log"));
        profile
            .validate_path(&path)
            .expect("Invalid preview log path");
        std::fs::create_dir_all(profile.logs_dir()).expect("Cannot create preview logs");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("Cannot open preview log");
        subscriber
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .init();
        let windows = context.config().app.windows.clone();
        for window in &mut context.config_mut().app.windows {
            window.create = false;
        }
        windows
    } else {
        subscriber.init();
        Vec::new()
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            if let Some(profile) = &profile {
                let entity_id = CONNECTION
                    .get()
                    .and_then(|info| info.entity_id.as_deref())
                    .expect("Validated Entity identity");
                let data = profile
                    .root()
                    .join("webview")
                    .join("entities")
                    .join(entity_id);
                profile.validate_path(&data)?;
                for window in &preview_windows {
                    tauri::WebviewWindowBuilder::from_config(app, window)?
                        .data_directory(data.clone())
                        .build()?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_connection_info,
            get_runtime_status,
            get_session_status,
            get_outbox_status,
            send_chat,
            send_chat_stream,
            cancel_chat_stream,
            list_skill_acks
        ])
        .run(context)
        .expect("failed to run Abigail Entity Runtime app");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_runtime_origins_refuse_legacy_and_foreign_endpoints() {
        for value in [
            "http://127.0.0.1:43142",
            "http://127.0.0.1:0",
            "http://localhost:51001",
            "http://127.1:51001",
            "http://127.0.0.1:051001",
            "http://127.0.0.1:51001/path",
            "http://127.0.0.1:51001/#fragment",
            "https://127.0.0.1:51001",
        ] {
            assert!(loopback_url(value).is_err(), "{value}");
        }
        assert_eq!(
            loopback_url("http://127.0.0.1:51001").unwrap(),
            "http://127.0.0.1:51001"
        );
    }

    #[test]
    fn status_checks_both_entity_id_and_all_process_identity_fields() {
        let identity = ProcessIdentity {
            channel: "beta".into(),
            profile_id: profile::new_instance_id(),
            canonical_data_root: "synthetic-profile".into(),
            instance_id: profile::new_instance_id(),
            hive_instance_id: profile::new_instance_id(),
        };
        let data = serde_json::json!({ "entity_id": "one", "process_identity": identity });
        assert!(validate_status_payload(&data, &identity, Some("one")).is_ok());
        assert!(validate_status_payload(&data, &identity, Some("two")).is_err());
        for field in [
            "instance_id",
            "hive_instance_id",
            "profile_id",
            "channel",
            "canonical_data_root",
        ] {
            let mut changed = data.clone();
            changed["process_identity"][field] = serde_json::json!("different");
            assert!(
                validate_status_payload(&changed, &identity, Some("one")).is_err(),
                "{field}"
            );
        }
        assert!(validate_status_payload(&serde_json::json!({}), &identity, None).is_err());
    }
}
