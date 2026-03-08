use crate::state::AppState;
use abigail_capabilities::sensory::file_ingestion::ingest_file;
use std::path::{Path, PathBuf};
use tauri::State;

#[tauri::command]
pub fn upload_chat_attachment(file_path: String) -> Result<serde_json::Value, String> {
    let path = Path::new(&file_path);
    let result = ingest_file(path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "content": result.content,
        "content_type": result.content_type,
        "filename": result.filename,
        "size_bytes": result.size_bytes,
        "truncated": result.truncated,
    }))
}

#[tauri::command]
pub fn get_entity_documents_path(state: State<AppState>) -> Result<String, String> {
    let agent_id = state.active_agent_id.read().map_err(|e| e.to_string())?;
    let agent_id = agent_id.as_deref().ok_or("No active agent loaded")?;
    let docs_path = state.identity_manager.create_documents_folder(agent_id)?;
    Ok(docs_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn save_to_entity_docs(
    state: State<AppState>,
    source_path: String,
    filename: String,
) -> Result<String, String> {
    let agent_id = state.active_agent_id.read().map_err(|e| e.to_string())?;
    let agent_id = agent_id.as_deref().ok_or("No active agent loaded")?;
    let docs_dir = state.identity_manager.create_documents_folder(agent_id)?;
    let dest = docs_dir.join(&filename);
    std::fs::copy(&source_path, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserSessionInfo {
    pub entity_id: Option<String>,
    pub profile_dir: String,
    pub active_in_process: bool,
    pub last_used_at_utc: String,
    pub last_action: Option<String>,
    pub current_url: Option<String>,
    pub page_title: Option<String>,
    pub cookie_count: Option<usize>,
}

#[tauri::command]
pub async fn list_browser_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<BrowserSessionInfo>, String> {
    let data_root = state.identity_manager.data_root().to_path_buf();
    let mut sessions = skill_browser::discover_browser_sessions(&data_root)?;
    let browser_skill_id = skill_browser::BrowserSkill::default_manifest().id.clone();
    if let Ok((skill, _)) = state.registry.get_skill(&browser_skill_id) {
        if let Some(any) = skill.get_capability("browser_session_control") {
            if let Some(browser_skill) = any.downcast_ref::<skill_browser::BrowserSkill>() {
                if let Some(current) = browser_skill.current_session_record().await {
                    if let Some(existing) = sessions
                        .iter_mut()
                        .find(|session| session.profile_dir == current.profile_dir)
                    {
                        *existing = current;
                    } else {
                        sessions.push(current);
                    }
                }
            }
        }
    }
    sessions.sort_by(|left, right| right.last_used_at_utc.cmp(&left.last_used_at_utc));
    Ok(sessions
        .into_iter()
        .map(|session| BrowserSessionInfo {
            entity_id: session.entity_id,
            profile_dir: session.profile_dir,
            active_in_process: session.active_in_process,
            last_used_at_utc: session.last_used_at_utc,
            last_action: session.last_action,
            current_url: session.current_url,
            page_title: session.page_title,
            cookie_count: session.cookie_count,
        })
        .collect())
}

#[tauri::command]
pub async fn clear_browser_session(
    state: State<'_, AppState>,
    profile_dir: Option<String>,
    entity_id: Option<String>,
) -> Result<(), String> {
    let target_profile = if let Some(profile_dir) = profile_dir {
        PathBuf::from(profile_dir)
    } else if let Some(entity_id) = entity_id {
        state
            .identity_manager
            .identities_dir()
            .join(entity_id)
            .join("browser_profile")
    } else {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.data_dir.join("browser_profile")
    };

    let browser_skill_id = skill_browser::BrowserSkill::default_manifest().id.clone();
    if let Ok((skill, _)) = state.registry.get_skill(&browser_skill_id) {
        if let Some(any) = skill.get_capability("browser_session_control") {
            if let Some(browser_skill) = any.downcast_ref::<skill_browser::BrowserSkill>() {
                if browser_skill.profile_dir() == target_profile.as_path() {
                    return browser_skill.clear_session().await;
                }
            }
        }
    }

    skill_browser::clear_browser_profile_dir(&target_profile)
}
