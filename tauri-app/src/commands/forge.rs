use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn get_forge_scenarios(_state: State<AppState>) -> Result<serde_json::Value, String> {
    // Stub for now
    Ok(serde_json::json!([]))
}

#[tauri::command]
pub fn crystallize_forge(_state: State<AppState>) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn genesis_chat(_state: State<'_, AppState>, _message: String) -> Result<String, String> {
    Ok("Soul discovered.".to_string())
}

#[tauri::command]
pub fn get_active_provider(state: State<AppState>) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.active_provider_preference.clone())
}
