use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn start_agentic_run(
    _state: State<'_, AppState>,
    _goal: String,
    _max_turns: u32,
    _require_confirmation: bool,
) -> Result<String, String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok("run_123".to_string())
}

#[tauri::command]
pub fn get_agentic_run_status(
    _state: State<'_, AppState>,
    _task_id: String,
) -> Result<serde_json::Value, String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok(serde_json::json!({
        "status": "running",
        "current_turn": 1,
        "goal": "Test goal"
    }))
}

#[tauri::command]
pub fn respond_to_mentor_ask(
    _state: State<'_, AppState>,
    _task_id: String,
    _response: String,
) -> Result<(), String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok(())
}

#[tauri::command]
pub fn confirm_tool_execution(
    _state: State<'_, AppState>,
    _task_id: String,
    _approved: bool,
) -> Result<(), String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok(())
}

#[tauri::command]
pub fn cancel_agentic_run(_state: State<'_, AppState>, _task_id: String) -> Result<(), String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok(())
}

#[tauri::command]
pub fn list_agentic_runs(_state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    // TODO: wire to AgenticEngine when AppState is updated
    Ok(serde_json::json!([]))
}
