use crate::state::AppState;
use abigail_capabilities::sensory::file_ingestion::ingest_file;
use std::path::Path;
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
    let agent_id = agent_id
        .as_deref()
        .ok_or("No active agent loaded")?;
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
    let agent_id = agent_id
        .as_deref()
        .ok_or("No active agent loaded")?;
    let docs_dir = state.identity_manager.create_documents_folder(agent_id)?;
    let dest = docs_dir.join(&filename);
    std::fs::copy(&source_path, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}
