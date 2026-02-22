use crate::state::AppState;
use abigail_core::config::SignedSkillAllowlistEntry;
use abigail_core::McpServerDefinition;
use abigail_skills::protocol::mcp::{HttpMcpClient, McpTool};
use abigail_skills::{
    FileSystemPermission, Permission, SkillId, SkillManifest, ToolDescriptor, ToolOutput,
    ToolParams,
};
use std::collections::HashMap;
use tauri::State;

fn is_signed_allowlisted(config: &abigail_core::AppConfig, skill_id: &str) -> bool {
    config
        .signed_skill_allowlist
        .iter()
        .any(|entry| entry.active && entry.skill_id == skill_id)
}

fn is_dangerous_tool(td: &ToolDescriptor) -> bool {
    let name = td.name.to_lowercase();
    let destructive_name = [
        "delete", "remove", "drop", "wipe", "truncate", "reset", "kill",
    ]
    .iter()
    .any(|k| name.contains(k));
    let destructive_permission = td.required_permissions.iter().any(|perm| {
        matches!(
            perm,
            Permission::FileSystem(FileSystemPermission::Write(_))
                | Permission::FileSystem(FileSystemPermission::Full)
        )
    });
    td.requires_confirmation || destructive_name || destructive_permission
}

fn resolve_mcp_server_url(state: &State<'_, AppState>, server_id: &str) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let server = config
        .mcp_servers
        .iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
    if server.transport != "http" {
        return Err("Only HTTP transport is supported for MCP list_tools".to_string());
    }
    Ok(server.command_or_url.clone())
}

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
pub fn list_missing_skill_secrets(
    state: State<AppState>,
) -> Result<Vec<abigail_skills::MissingSkillSecret>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(state.registry.list_all_missing_secrets(&paths))
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
        if !is_signed_allowlisted(&config, &skill_id)
            && !config.approved_skill_ids.is_empty()
            && !config.approved_skill_ids.contains(&skill_id)
        {
            return Err(format!("Skill {} is not approved for execution.", skill_id));
        }
    }
    let id = SkillId(skill_id);
    if let Ok((skill, _)) = state.registry.get_skill(&id) {
        if let Some(td) = skill.tools().into_iter().find(|t| t.name == tool_name) {
            let mentor_confirmed = params
                .get("mentor_confirmed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_dangerous_tool(&td) && !mentor_confirmed {
                return Err(
                    "This tool requires explicit mentor confirmation. Re-run with `mentor_confirmed: true`."
                        .to_string(),
                );
            }
        }
    }
    let mentor_confirmed = params
        .get("mentor_confirmed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let l2_mode = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.superego_l2_mode
    };
    let tool_params = ToolParams { values: params };
    state
        .executor
        .execute_with_policy(&id, &tool_name, tool_params, l2_mode, mentor_confirmed)
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
    let url = resolve_mcp_server_url(&state, &server_id)?;
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

#[tauri::command]
pub fn list_signed_skill_allowlist(
    state: State<AppState>,
) -> Result<Vec<SignedSkillAllowlistEntry>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.signed_skill_allowlist.clone())
}

#[tauri::command]
pub fn upsert_signed_skill_allowlist_entry(
    state: State<AppState>,
    skill_id: String,
    signer: String,
    signature: String,
    source: String,
) -> Result<(), String> {
    if signer.trim().is_empty() || signature.trim().is_empty() || source.trim().is_empty() {
        return Err("signer, signature, and source are required.".to_string());
    }
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if let Some(entry) = config
        .signed_skill_allowlist
        .iter_mut()
        .find(|e| e.skill_id == skill_id)
    {
        entry.signer = signer;
        entry.signature = signature;
        entry.source = source;
        entry.active = true;
    } else {
        config
            .signed_skill_allowlist
            .push(SignedSkillAllowlistEntry {
                skill_id,
                signer,
                signature,
                source,
                added_at: chrono::Utc::now().to_rfc3339(),
                active: true,
            });
    }
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn revoke_signed_skill_allowlist_entry(
    state: State<AppState>,
    skill_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if let Some(entry) = config
        .signed_skill_allowlist
        .iter_mut()
        .find(|e| e.skill_id == skill_id)
    {
        entry.active = false;
        if let Some(reason) = reason {
            if !reason.trim().is_empty() {
                entry.source = format!("{} (revoked: {})", entry.source, reason.trim());
            }
        }
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err(format!(
        "No signed allowlist entry found for skill {}",
        skill_id
    ))
}
