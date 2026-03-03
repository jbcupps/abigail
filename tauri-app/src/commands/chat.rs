//! Tauri chat commands — thin adapters over the desktop ChatCoordinator.
//!
//! In `InProcess` mode, commands use the in-process `ChatCoordinator`.
//! In `Daemon` mode, commands delegate to entity-daemon over HTTP/SSE.

use crate::chat_coordinator::{ChatCommandRequest, ChatCoordinator};
use crate::state::AppState;
use abigail_core::RuntimeMode;
use entity_core::{ChatRequest, SessionMessage};
use tauri::{Emitter, State};

#[tauri::command]
pub async fn cancel_chat_stream(state: State<'_, AppState>) -> Result<bool, String> {
    let mode = {
        state.config.read().map_err(|e| e.to_string())?.runtime_mode
    };
    if mode == RuntimeMode::Daemon {
        let entity_url = {
            state.config.read().map_err(|e| e.to_string())?.entity_daemon_url.clone()
        };
        let client = daemon_client::EntityClient::new(&entity_url);
        return client.cancel_chat_stream().await.map_err(|e| e.to_string());
    }

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
    let mode = {
        state.config.read().map_err(|e| e.to_string())?.runtime_mode
    };

    if mode == RuntimeMode::Daemon {
        let entity_url = {
            state.config.read().map_err(|e| e.to_string())?.entity_daemon_url.clone()
        };
        let client = daemon_client::EntityClient::new(&entity_url);
        let request = ChatRequest {
            message: message.clone(),
            target: None,
            session_messages: session_messages.clone(),
            session_id: session_id.clone(),
        };

        // Emit request envelope so the frontend knows a chat started
        let _ = app.emit(
            "chat-internal-envelope",
            serde_json::json!({ "type": "Request", "message": message }),
        );

        let mut rx = client.chat_stream(&request).await.map_err(|e| e.to_string())?;
        while let Some(event) = rx.recv().await {
            match event {
                daemon_client::ChatStreamEvent::Token(t) => {
                    let _ = app.emit(
                        "chat-internal-envelope",
                        serde_json::json!({ "type": "Token", "token": t }),
                    );
                }
                daemon_client::ChatStreamEvent::Done(resp) => {
                    let _ = app.emit(
                        "chat-internal-envelope",
                        serde_json::json!({
                            "type": "Done",
                            "reply": &resp.reply,
                            "provider": &resp.provider,
                            "tier": &resp.tier,
                            "model_used": &resp.model_used,
                            "complexity_score": &resp.complexity_score,
                        }),
                    );
                }
                daemon_client::ChatStreamEvent::Error(e) => {
                    let _ = app.emit(
                        "chat-internal-envelope",
                        serde_json::json!({ "type": "Error", "error": e }),
                    );
                }
            }
        }
        return Ok(());
    }

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
pub fn get_assembled_prompt(state: State<AppState>) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let router = state.router.read().map_err(|e| e.to_string())?;
    let status = router.status();

    let prompt = if status.mode == abigail_core::RoutingMode::CliOrchestrator {
        entity_chat::build_cli_system_prompt(
            &config.docs_dir,
            &config.agent_name,
            &state.registry,
            &state.instruction_registry,
            "",
        )
    } else {
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: None,
            routing_mode: Some(format!("{:?}", status.mode)),
            tier: None,
            complexity_score: None,
            entity_name: config.agent_name.clone(),
            entity_id: None,
            has_local_llm: status.has_local_http,
            last_provider_change_at: config.last_provider_change_at.clone(),
        };
        entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            "",
            &runtime_ctx,
            entity_chat::PromptMode::Full,
        )
    };

    Ok(prompt)
}

#[tauri::command]
pub fn get_topic_stats(_state: State<AppState>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "topics": [
            { "stream": "abigail", "topic": "conversation-turns", "description": "Chat turn persistence" },
            { "stream": "abigail", "topic": "job-events", "description": "Job lifecycle events" },
            { "stream": "abigail", "topic": "skill-events", "description": "Skill hot-reload events" },
            { "stream": "entity", "topic": "conscience-check", "description": "Ethical check requests" },
            { "stream": "entity", "topic": "ethical-signals", "description": "Ethical evaluation results" },
        ]
    }))
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
