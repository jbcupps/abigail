#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use daemon_client::HiveDaemonClient;
use hive_core::{
    CreateForgeApprovalJobRequest, HiveStatus, RuntimeSessionLease, SkillAssignmentsResponse,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct HiveConnectionInfo {
    hive_url: String,
}

fn hive_url() -> String {
    std::env::var("ABIGAIL_HIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:3141".to_string())
}

#[tauri::command]
async fn get_hive_status() -> Result<HiveStatus, String> {
    let client = HiveDaemonClient::new(&hive_url());
    let entities = client.list_entities().await.map_err(|e| e.to_string())?;
    Ok(HiveStatus {
        master_key_loaded: true,
        entity_count: entities.len(),
        entities,
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
        .post(format!("{}/v1/entities/{}/forge-approvals", base_url, entity_id))
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

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "abigail_hive_app=info".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_hive_connection_info,
            get_hive_status,
            create_entity,
            issue_runtime_session,
            list_assignments,
            approve_forge_job
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Abigail Hive app");
}
