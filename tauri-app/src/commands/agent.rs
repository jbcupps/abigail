use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

#[tauri::command]
pub fn list_subagents(state: State<AppState>) -> Result<Vec<SubagentInfo>, String> {
    let mgr = state
        .subagent_manager
        .read()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(mgr
        .list()
        .iter()
        .map(|d| SubagentInfo {
            id: d.id.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            capabilities: d.capabilities.clone(),
        })
        .collect())
}

#[tauri::command]
pub async fn delegate_to_subagent(
    state: State<'_, AppState>,
    id: String,
    message: String,
) -> Result<String, String> {
    let def = {
        let mgr = state.subagent_manager.read().map_err(|e| e.to_string())?;
        mgr
            .list()
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| format!("Subagent '{}' not found", id))?
    };

    let messages = vec![abigail_capabilities::cognitive::Message::new("user", &message)];
    let tools = entity_chat::build_tool_definitions(&state.registry);

    let mgr = state
        .subagent_manager
        .read()
        .map_err(|e| e.to_string())?
        .clone();
    let response = mgr
        .delegate(&def.id, messages, tools)
        .await
        .map_err(|e| e.to_string())?;

    Ok(response.content)
}

#[tauri::command]
pub fn get_governor_status(_state: State<AppState>) -> Result<serde_json::Value, String> {
    // Phase 2 logic - stub for now
    Ok(serde_json::json!({ "status": "active" }))
}

#[tauri::command]
pub fn get_constraint_store(_state: State<AppState>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "constraints": [] }))
}

#[tauri::command]
pub fn clear_constraints(_state: State<AppState>) -> Result<(), String> {
    Ok(())
}
