//! Tauri chat commands — uses the shared `entity-chat` engine.
//!
//! The chat pipeline (sanitization, tool definitions, tool-use loop) lives in
//! the `entity-chat` crate so that CLI and GUI share a single engine. This
//! module only contains Tauri-specific wrappers: API key auto-detection from
//! user messages, the `#[tauri::command]` handlers, and system diagnostics.

use crate::state::AppState;
use abigail_capabilities::cognitive::StreamEvent;
use abigail_core::key_detection::{detect_api_keys, CLI_ALIASES};
use entity_core::{ChatResponse, SessionMessage};
use tauri::{Emitter, State};

// ---------------------------------------------------------------------------
// API key auto-detection (Tauri-specific side-effect wrapper)
// ---------------------------------------------------------------------------

/// Check if a message contains recognizable API keys, store them, and rebuild the router.
pub async fn auto_detect_and_store_key_internal(
    state: &AppState,
    message: &str,
) -> Vec<(String, String)> {
    let detected = detect_api_keys(message);

    if !detected.is_empty() {
        {
            if let Ok(mut vault) = state.secrets.lock() {
                for (provider, key) in &detected {
                    vault.set_secret(provider, key);
                    for (src, alias) in CLI_ALIASES {
                        if provider == src {
                            vault.set_secret(alias, key);
                        }
                    }
                }
                let _ = vault.save();
            }
        }
        let _ = crate::rebuild_router(state).await;
    }

    detected
}

// ---------------------------------------------------------------------------
// Recent provider change check
// ---------------------------------------------------------------------------

/// Returns true if the given ISO 8601 timestamp is within the last 10 minutes.
/// Used to only surface provider switches that are contextually relevant.
fn is_recent_provider_change(ts: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| {
            let age = chrono::Utc::now().signed_duration_since(dt);
            age.num_minutes() < 10
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// POST /chat — Tauri command using shared entity-chat engine
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn chat(
    state: State<'_, AppState>,
    message: String,
    target: Option<String>,
    session_messages: Option<Vec<SessionMessage>>,
) -> Result<String, String> {
    // 1. Auto-detect and store API keys (Tauri-specific pre-hook, GUI only)
    auto_detect_and_store_key_internal(&state, &message).await;

    // 2. Build system prompt + router snapshot with runtime context
    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        let status = router.status();

        let augmented = if status.mode == abigail_core::RoutingMode::CliOrchestrator {
            entity_chat::build_cli_system_prompt(
                &config.docs_dir,
                &config.agent_name,
                &state.registry,
                &state.instruction_registry,
                &message,
            )
        } else {
            let base = abigail_core::system_prompt::build_system_prompt(
                &config.docs_dir,
                &config.agent_name,
            );
            let (t, m, c) = router.tier_metadata_for_message(&message);
            let runtime_ctx = entity_chat::RuntimeContext {
                provider_name: status.ego_provider.clone(),
                model_id: m,
                routing_mode: Some(format!("{:?}", status.mode)),
                tier: t,
                complexity_score: c,
                entity_name: config.agent_name.clone(),
                entity_id: None,
                has_local_llm: status.has_local_http,
                last_provider_change_at: config
                    .last_provider_change_at
                    .as_ref()
                    .filter(|ts| is_recent_provider_change(ts))
                    .cloned(),
            };
            entity_chat::augment_system_prompt(
                &base,
                &state.registry,
                &state.instruction_registry,
                &message,
                &runtime_ctx,
            )
        };
        (router, augmented)
    };

    // 3. Build contextual messages with sanitization + deduplication (shared engine)
    let messages =
        entity_chat::build_contextual_messages(&system_prompt, session_messages, &message);

    // 4. Build tool definitions from registered skills (shared engine)
    let tools = entity_chat::build_tool_definitions(&state.registry);

    // 5. Route — use tool-use loop if tools are available, plain route otherwise.
    // Chat never uses Id; Id is for background tasks only. Treat ID as AUTO (Ego when available).
    let result = if tools.is_empty() {
        let traced = router.route_traced(messages).await;
        traced.map(|(r, trace)| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            execution_trace: Some(trace),
        })
    } else {
        entity_chat::run_tool_use_loop(&router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let tier = tool_result.tier().map(|s| s.to_string());
            let model_used = tool_result.model_used().map(|s| s.to_string());
            let complexity_score = tool_result.complexity_score();
            let provider = tool_result
                .execution_trace
                .as_ref()
                .and_then(|t| t.final_provider())
                .map(|s| s.to_string())
                .or_else(|| Some(entity_chat::provider_label(&router)));

            let response = ChatResponse {
                reply: tool_result.content,
                provider,
                tool_calls_made: tool_result.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: tool_result.execution_trace,
                session_id: None,
            };
            serde_json::to_string(&response).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// POST /chat_stream — streaming variant using Tauri events
// ---------------------------------------------------------------------------

/// Streaming chat command. Runs the tool-use loop non-streaming for
/// intermediate rounds, then streams the final text response via Tauri events:
///   - `chat-token`  — each text delta as it arrives
///   - `chat-done`   — final ChatResponse JSON with tier/model metadata
///   - `chat-error`  — error string if the pipeline fails
#[tauri::command]
pub async fn chat_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    message: String,
    target: Option<String>,
    session_messages: Option<Vec<SessionMessage>>,
    session_id: Option<String>,
) -> Result<(), String> {
    let _ = session_id; // Passed from frontend for future session-scoped memory/logging
    auto_detect_and_store_key_internal(&state, &message).await;

    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        let status = router.status();

        let augmented = if status.mode == abigail_core::RoutingMode::CliOrchestrator {
            entity_chat::build_cli_system_prompt(
                &config.docs_dir,
                &config.agent_name,
                &state.registry,
                &state.instruction_registry,
                &message,
            )
        } else {
            let base = abigail_core::system_prompt::build_system_prompt(
                &config.docs_dir,
                &config.agent_name,
            );
            let (t, m, c) = router.tier_metadata_for_message(&message);
            let runtime_ctx = entity_chat::RuntimeContext {
                provider_name: status.ego_provider.clone(),
                model_id: m,
                routing_mode: Some(format!("{:?}", status.mode)),
                tier: t,
                complexity_score: c,
                entity_name: config.agent_name.clone(),
                entity_id: None,
                has_local_llm: status.has_local_http,
                last_provider_change_at: config
                    .last_provider_change_at
                    .as_ref()
                    .filter(|ts| is_recent_provider_change(ts))
                    .cloned(),
            };
            entity_chat::augment_system_prompt(
                &base,
                &state.registry,
                &state.instruction_registry,
                &message,
                &runtime_ctx,
            )
        };
        (router, augmented)
    };

    let messages =
        entity_chat::build_contextual_messages(&system_prompt, session_messages, &message);
    let tools = entity_chat::build_tool_definitions(&state.registry);
    let target_mode = target.as_deref().unwrap_or("AUTO");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
    let app_clone = app.clone();
    let stream_task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let StreamEvent::Token(token) = event {
                let _ = app_clone.emit("chat-token", &token);
            }
        }
    });

    let result = entity_chat::stream_chat_pipeline(
        &router,
        &state.executor,
        messages,
        tools,
        target_mode,
        tx,
    )
    .await;

    let _ = stream_task.await;

    match result {
        Ok(pipeline) => {
            let trace_ref = pipeline.execution_trace.as_ref();
            let tier = trace_ref
                .and_then(|t| t.final_tier())
                .map(|s| s.to_string());
            let model_used = trace_ref
                .and_then(|t| t.final_model())
                .map(|s| s.to_string());
            let complexity_score = trace_ref.and_then(|t| t.complexity_score);
            let provider = trace_ref
                .and_then(|t| t.final_provider())
                .map(|s| s.to_string())
                .or_else(|| Some(entity_chat::provider_label(&router)));

            let response = ChatResponse {
                reply: pipeline.content,
                provider,
                tool_calls_made: pipeline.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: pipeline.execution_trace,
                session_id: None,
            };
            let _ = app.emit("chat-done", &response);
        }
        Err(e) => {
            let _ = app.emit("chat-error", e.to_string());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// GET /system_diagnostics — Tauri-specific
// ---------------------------------------------------------------------------

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
