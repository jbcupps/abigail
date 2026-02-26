//! Tauri chat commands — uses the shared `entity-chat` engine.
//!
//! The chat pipeline (sanitization, tool definitions, tool-use loop) lives in
//! the `entity-chat` crate so that CLI and GUI share a single engine. This
//! module only contains Tauri-specific wrappers: API key auto-detection from
//! user messages, the `#[tauri::command]` handlers, and system diagnostics.

use crate::state::AppState;
use abigail_capabilities::cognitive::StreamEvent;
use entity_core::{ChatResponse, SessionMessage};
use tauri::{Emitter, State};

// ---------------------------------------------------------------------------
// API key auto-detection (Tauri-specific)
// ---------------------------------------------------------------------------

/// Regex patterns for API key detection. Each entry: (pattern, provider_name).
pub const KEY_PATTERNS: &[(&str, &str)] = &[
    (r"sk-ant-[a-zA-Z0-9_-]{20,}", "anthropic"),
    (r"sk-[a-zA-Z0-9]{20,}", "openai"),
    (r"xai-[a-zA-Z0-9_-]{20,}", "xai"),
    (r"pplx-[a-zA-Z0-9_-]{20,}", "perplexity"),
    (r"AIza[a-zA-Z0-9_-]{35}", "google"),
    (r"tvly-[a-zA-Z0-9_-]{20,}", "tavily"),
];

/// Alias mapping: when a key is detected for a provider, also store it under these names.
const CLI_ALIASES: &[(&str, &str)] = &[
    ("openai", "codex-cli"),
    ("anthropic", "claude-cli"),
    ("google", "gemini-cli"),
    ("xai", "grok-cli"),
];

/// Pure detection function: scans a message for API key patterns.
/// Returns a vec of (provider_name, key_string) tuples.
/// No side effects — can be tested independently.
pub fn detect_api_keys(message: &str) -> Vec<(String, String)> {
    let mut detected = Vec::new();
    for (pattern, provider) in KEY_PATTERNS {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(message) {
                detected.push((provider.to_string(), mat.as_str().to_string()));
            }
        }
    }
    detected
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_openai_key() {
        let msg = "Here is my key: sk-abcdefghijklmnopqrstuvwxyz";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "openai");
    }

    #[test]
    fn detects_anthropic_key() {
        let msg = "Use sk-ant-abc123def456ghi789jklmno";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "anthropic");
    }

    #[test]
    fn detects_google_key() {
        let msg = "AIzaSyAbCdEfGhIjKlMnOpQrStUvWxYz12345678901";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "google");
    }

    #[test]
    fn detects_xai_key() {
        let msg = "xai-abcdefghijklmnopqrstuvwxyz";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, "xai");
    }

    #[test]
    fn no_false_positives_on_normal_message() {
        let msg = "Hello, how are you? I want to build a project with React and Rust.";
        let keys = detect_api_keys(msg);
        assert!(keys.is_empty());
    }

    #[test]
    fn detects_multiple_keys() {
        let msg =
            "OpenAI: sk-abcdefghijklmnopqrstuvwxyz and Anthropic: sk-ant-abc123def456ghi789jklmno";
        let keys = detect_api_keys(msg);
        assert_eq!(keys.len(), 2);
    }
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

    // 2. Build system prompt + router snapshot, augmented with tool awareness, skill instructions, and runtime context
    let (router, system_prompt, tier, model_used, complexity_score) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        let (t, m, c) = router.tier_metadata_for_message(&message);
        let status = router.status();

        // Build runtime context for self-awareness
        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: m.clone(),
            routing_mode: Some(format!("{:?}", status.mode)),
            tier: t.clone(),
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

        let augmented = entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            &message,
            &runtime_ctx,
        );
        (router, augmented, t, m, c)
    };

    // 3. Build contextual messages with sanitization + deduplication (shared engine)
    let messages =
        entity_chat::build_contextual_messages(&system_prompt, session_messages, &message);

    // 4. Build tool definitions from registered skills (shared engine)
    let tools = entity_chat::build_tool_definitions(&state.registry);

    // 5. Route — use tool-use loop if tools are available, plain route otherwise
    let target_mode = target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target_mode == "ID" {
        let traced = if target_mode == "ID" {
            router.id_only_traced(messages).await
        } else {
            router.route_traced(messages).await
        };
        traced.map(|(r, trace)| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            tier: tier.clone(),
            model_used: model_used.clone(),
            complexity_score,
            execution_trace: Some(trace),
        })
    } else {
        entity_chat::run_tool_use_loop(&router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let provider = entity_chat::provider_label(&router);

            let response = ChatResponse {
                reply: tool_result.content,
                provider: Some(provider),
                tool_calls_made: tool_result.tool_calls_made,
                tier: tool_result.tier,
                model_used: tool_result.model_used,
                complexity_score: tool_result.complexity_score,
                execution_trace: tool_result.execution_trace,
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
) -> Result<(), String> {
    auto_detect_and_store_key_internal(&state, &message).await;

    let (router, system_prompt, tier, model_used, complexity_score) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        let (t, m, c) = router.tier_metadata_for_message(&message);
        let status = router.status();

        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: m.clone(),
            routing_mode: Some(format!("{:?}", status.mode)),
            tier: t.clone(),
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

        let augmented = entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            &message,
            &runtime_ctx,
        );
        (router, augmented, t, m, c)
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
            let provider = entity_chat::provider_label(&router);
            let response = ChatResponse {
                reply: pipeline.content,
                provider: Some(provider),
                tool_calls_made: pipeline.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: pipeline.execution_trace,
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
