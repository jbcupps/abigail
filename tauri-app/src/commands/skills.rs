use crate::state::AppState;
use abigail_core::McpServerDefinition;
use abigail_skills::protocol::mcp::{HttpMcpClient, McpTool};
use abigail_skills::{SkillId, SkillManifest, ToolDescriptor, ToolOutput, ToolParams};
use std::collections::HashMap;
use tauri::State;

#[tauri::command]
pub fn list_skills(state: State<AppState>) -> Result<Vec<SkillManifest>, String> {
    state.registry.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_discovered_skills(state: State<AppState>) -> Result<Vec<SkillManifest>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(abigail_skills::SkillRegistry::discover(&paths))
}

#[tauri::command]
pub fn list_tools(state: State<AppState>, skill_id: String) -> Result<Vec<ToolDescriptor>, String> {
    let id = SkillId(skill_id);
    let (skill, _) = state.registry.get_skill(&id).map_err(|e| e.to_string())?;
    Ok(skill.tools())
}

#[tauri::command]
pub async fn execute_tool(
    state: State<'_, AppState>,
    skill_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<ToolOutput, String> {
    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        if !config.approved_skill_ids.is_empty() && !config.approved_skill_ids.contains(&skill_id) {
            return Err(format!("Skill {} is not approved for execution.", skill_id));
        }
    }
    let id = SkillId(skill_id);
    let tool_params = ToolParams { values: params };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_mcp_servers(state: State<AppState>) -> Result<Vec<McpServerDefinition>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.mcp_servers.clone())
}

#[tauri::command]
pub async fn mcp_list_tools(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<McpTool>, String> {
    let url = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let server = config
            .mcp_servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
        server.command_or_url.clone()
    };
    let client = HttpMcpClient::new(url);
    client.initialize().await.map_err(|e| e.to_string())?;
    client.list_tools_impl().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_approved_skills(state: State<AppState>) -> Result<Vec<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.approved_skill_ids.clone())
}

#[tauri::command]
pub fn approve_skill(state: State<AppState>, skill_id: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if !config.approved_skill_ids.contains(&skill_id) {
        config.approved_skill_ids.push(skill_id.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
