use crate::agentic_runtime::RunAttribution;
use crate::state::AppState;
use abigail_router::RunConfig;
use tauri::State;

fn validate_run_inputs(goal: &str, max_turns: u32) -> Result<(), String> {
    if goal.trim().is_empty() {
        return Err("goal cannot be empty".to_string());
    }
    if max_turns == 0 {
        return Err("max_turns must be at least 1".to_string());
    }
    if max_turns > 100 {
        return Err("max_turns cannot exceed 100".to_string());
    }
    Ok(())
}

fn resolve_agentic_provider(
    state: &State<'_, AppState>,
) -> Result<std::sync::Arc<dyn abigail_capabilities::cognitive::provider::LlmProvider>, String> {
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    router
        .best_available_provider()
        .ok_or_else(|| "No available provider for agentic run. Configure an Ego provider or local HTTP Id provider.".to_string())
}

#[tauri::command]
pub async fn start_agentic_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    goal: String,
    max_turns: u32,
    require_confirmation: bool,
) -> Result<String, String> {
    validate_run_inputs(&goal, max_turns)?;

    let provider = resolve_agentic_provider(&state)?;
    let tools = entity_chat::build_tool_definitions(&state.registry);

    state
        .agentic_runtime
        .start_run(
            provider,
            tools,
            state.executor.clone(),
            RunConfig {
                goal: goal.trim().to_string(),
                max_turns,
                require_confirmation,
                system_context: None,
            },
            RunAttribution::gui(),
            Some(app),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_entity_initiated_agentic_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    goal: String,
    max_turns: u32,
    require_confirmation: bool,
    entity_id: Option<String>,
    session_id: Option<String>,
    correlation_id: Option<String>,
) -> Result<String, String> {
    validate_run_inputs(&goal, max_turns)?;

    let provider = resolve_agentic_provider(&state)?;
    let tools = entity_chat::build_tool_definitions(&state.registry);

    state
        .agentic_runtime
        .start_run(
            provider,
            tools,
            state.executor.clone(),
            RunConfig {
                goal: goal.trim().to_string(),
                max_turns,
                require_confirmation,
                system_context: None,
            },
            RunAttribution::entity(entity_id, session_id, correlation_id),
            Some(app),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agentic_run_status(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<serde_json::Value, String> {
    let snapshot = state
        .agentic_runtime
        .get_run_status(&task_id)
        .await
        .map_err(|e| e.to_string())?;

    serde_json::to_value(snapshot).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn respond_to_mentor_ask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    response: String,
) -> Result<(), String> {
    let response = response.trim();
    if response.is_empty() {
        return Err("response cannot be empty".to_string());
    }

    state
        .agentic_runtime
        .respond_to_mentor(&task_id, response.to_string(), Some(&app))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn respond_agentic_mentor(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    response: String,
) -> Result<(), String> {
    respond_to_mentor_ask(app, state, task_id, response).await
}

#[tauri::command]
pub async fn confirm_tool_execution(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    approved: bool,
) -> Result<(), String> {
    state
        .agentic_runtime
        .confirm_action(&task_id, approved, Some(&app))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn confirm_agentic_action(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    approved: bool,
) -> Result<(), String> {
    confirm_tool_execution(app, state, task_id, approved).await
}

#[tauri::command]
pub async fn cancel_agentic_run(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state
        .agentic_runtime
        .cancel_run(&task_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_agentic_runs(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let runs = state.agentic_runtime.list_runs().await;
    serde_json::to_value(runs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agentic_runtime_status(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let status = state.agentic_runtime.status().await;
    serde_json::to_value(status).map_err(|e| e.to_string())
}
