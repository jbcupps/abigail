use crate::state::AppState;
use tauri::State;

fn not_implemented(name: &str) -> String {
    format!(
        "Agentic command '{}' is not wired to AgenticEngine yet. Surface is intentionally gated.",
        name
    )
}

#[tauri::command]
pub fn start_agentic_run(
    _state: State<'_, AppState>,
    _goal: String,
    _max_turns: u32,
    _require_confirmation: bool,
) -> Result<String, String> {
    Err(not_implemented("start_agentic_run"))
}

#[tauri::command]
pub fn get_agentic_run_status(
    _state: State<'_, AppState>,
    _task_id: String,
) -> Result<serde_json::Value, String> {
    Err(not_implemented("get_agentic_run_status"))
}

#[tauri::command]
pub fn respond_to_mentor_ask(
    _state: State<'_, AppState>,
    _task_id: String,
    _response: String,
) -> Result<(), String> {
    Err(not_implemented("respond_to_mentor_ask"))
}

#[tauri::command]
pub fn confirm_tool_execution(
    _state: State<'_, AppState>,
    _task_id: String,
    _approved: bool,
) -> Result<(), String> {
    Err(not_implemented("confirm_tool_execution"))
}

#[tauri::command]
pub fn cancel_agentic_run(_state: State<'_, AppState>, _task_id: String) -> Result<(), String> {
    Err(not_implemented("cancel_agentic_run"))
}

#[tauri::command]
pub fn list_agentic_runs(_state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    Err(not_implemented("list_agentic_runs"))
}
