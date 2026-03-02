//! Tauri chat commands — thin adapters over the desktop ChatCoordinator.

use crate::chat_coordinator::{ChatCommandRequest, ChatCoordinator};
use crate::state::AppState;
use entity_core::SessionMessage;
use tauri::State;

#[tauri::command]
pub async fn cancel_chat_stream(state: State<'_, AppState>) -> Result<bool, String> {
    let mut active = state.active_chat_cancel.lock().await;
    if let Some(token) = active.take() {
        token.cancel();
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
pub async fn chat_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    message: String,
    session_messages: Option<Vec<SessionMessage>>,
    session_id: Option<String>,
) -> Result<(), String> {
    let coordinator = ChatCoordinator::new(&state);
    coordinator
        .execute_chat_stream(
            app,
            ChatCommandRequest {
                message,
                target: None,
                session_messages,
                session_id,
            },
        )
        .await
}

#[tauri::command]
pub fn get_system_diagnostics(state: State<AppState>) -> Result<String, String> {
    let mut report = String::from("# Abigail System Diagnostics\n\n");
    let router = state.router.read().map_err(|e| e.to_string())?;
    let s = router.status();

    report.push_str("## Router\n");
    report.push_str(&format!(
        "- Id: {}\n",
        if s.has_local_http {
            "local_http"
        } else {
            "candle_stub"
        }
    ));
    report.push_str(&format!("- Ego Configured: {}\n", s.has_ego));
    if let Some(ref p) = s.ego_provider {
        report.push_str(&format!("- Ego Provider: {}\n", p));
    }

    Ok(report)
}
