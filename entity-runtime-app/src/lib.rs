#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use daemon_client::EntityClient;
use entity_core::{
    ChatRequest, ChatResponse, EntityOutboxStatus, RuntimeSessionStatusResponse,
    SkillApplyAcknowledgementList,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct RuntimeConnectionInfo {
    runtime_url: String,
}

fn entity_url() -> String {
    std::env::var("ABIGAIL_ENTITY_URL").unwrap_or_else(|_| "http://127.0.0.1:3142".to_string())
}

#[tauri::command]
fn get_runtime_connection_info() -> RuntimeConnectionInfo {
    RuntimeConnectionInfo {
        runtime_url: entity_url(),
    }
}

#[tauri::command]
async fn get_runtime_status() -> Result<serde_json::Value, String> {
    EntityClient::new(&entity_url())
        .status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_session_status() -> Result<RuntimeSessionStatusResponse, String> {
    EntityClient::new(&entity_url())
        .session_status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_outbox_status() -> Result<EntityOutboxStatus, String> {
    EntityClient::new(&entity_url())
        .outbox_status()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn send_chat(message: String, session_id: Option<String>) -> Result<ChatResponse, String> {
    EntityClient::new(&entity_url())
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
async fn list_skill_acks() -> Result<SkillApplyAcknowledgementList, String> {
    EntityClient::new(&entity_url())
        .list_skill_apply_acknowledgements()
        .await
        .map_err(|e| e.to_string())
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "abigail_entity_runtime_app=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_runtime_connection_info,
            get_runtime_status,
            get_session_status,
            get_outbox_status,
            send_chat,
            list_skill_acks
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Abigail Entity Runtime app");
}
