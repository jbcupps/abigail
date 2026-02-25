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

    // 2. Build system prompt + router snapshot, augmented with tool awareness and skill instructions
    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let augmented = entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            &message,
        );
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, augmented)
    };

    // 3. Build contextual messages with sanitization + deduplication (shared engine)
    let messages =
        entity_chat::build_contextual_messages(&system_prompt, session_messages, &message);

    // 4. Build tool definitions from registered skills (shared engine)
    let tools = entity_chat::build_tool_definitions(&state.registry);

    // 5. Compute tier metadata from the user's message
    let (tier, model_used, complexity_score) = router.tier_metadata_for_message(&message);

    // 6. Route — use tool-use loop if tools are available, plain route otherwise
    let target_mode = target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target_mode == "ID" {
        let res = if target_mode == "ID" {
            router.id_only(messages).await
        } else {
            router.route(messages).await
        };
        res.map(|r| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            tier: tier.clone(),
            model_used: model_used.clone(),
            complexity_score,
        })
    } else {
        entity_chat::run_tool_use_loop(&router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let provider = if router.has_ego() {
                router
                    .ego_provider_name()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "id".to_string())
            } else {
                "id".to_string()
            };

            // 7. Return JSON-serialized ChatResponse (same DTO as entity-daemon)
            let response = ChatResponse {
                reply: tool_result.content,
                provider: Some(provider),
                tool_calls_made: tool_result.tool_calls_made,
                tier: tool_result.tier,
                model_used: tool_result.model_used,
                complexity_score: tool_result.complexity_score,
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

    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let augmented = entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            &message,
        );
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, augmented)
    };

    let mut messages =
        entity_chat::build_contextual_messages(&system_prompt, session_messages, &message);
    let tools = entity_chat::build_tool_definitions(&state.registry);
    let (tier, model_used, complexity_score) = router.tier_metadata_for_message(&message);

    let target_mode = target.as_deref().unwrap_or("AUTO");

    // If tools are available (and not forced to Id), run the non-streaming
    // tool-use loop for intermediate rounds first.
    let mut tool_calls_made = Vec::new();
    if !tools.is_empty() && target_mode != "ID" {
        match entity_chat::run_tool_use_loop_rounds_only(
            &router,
            &state.executor,
            &mut messages,
            &tools,
        )
        .await
        {
            Ok(intermediate) => {
                tool_calls_made = intermediate.tool_calls_made;
                if let Some(final_text) = intermediate.final_text {
                    // Loop completed with a text response (no streaming needed).
                    let provider = provider_label(&router);
                    let response = ChatResponse {
                        reply: final_text,
                        provider: Some(provider),
                        tool_calls_made,
                        tier: tier.clone(),
                        model_used: model_used.clone(),
                        complexity_score,
                    };
                    let _ = app.emit("chat-done", &response);
                    return Ok(());
                }
                // Otherwise, messages have been updated with tool results and
                // we fall through to stream the final LLM response.
            }
            Err(e) => {
                let _ = app.emit("chat-error", e.to_string());
                return Ok(());
            }
        }
    }

    // Stream the final response (or the only response if no tools).
    let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
    let app_clone = app.clone();
    let tier_clone = tier.clone();
    let model_clone = model_used.clone();

    let stream_task = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::Token(token) => {
                    let _ = app_clone.emit("chat-token", &token);
                }
                StreamEvent::Done(_resp) => {
                    // Metadata is sent separately via chat-done below.
                }
            }
        }
        (tier_clone, model_clone)
    });

    let stream_result = if target_mode == "ID" {
        router.id_stream(messages, tx.clone()).await
    } else if tools.is_empty() {
        router.route_stream(messages, tx.clone()).await
    } else {
        router
            .route_stream_with_tools(messages, tools, tx.clone())
            .await
    };

    drop(tx); // Signal receiver that no more events are coming.
    let _ = stream_task.await;

    match stream_result {
        Ok(final_response) => {
            let provider = provider_label(&router);
            let response = ChatResponse {
                reply: final_response.content,
                provider: Some(provider),
                tool_calls_made,
                tier,
                model_used,
                complexity_score,
            };
            let _ = app.emit("chat-done", &response);
        }
        Err(e) => {
            let _ = app.emit("chat-error", e.to_string());
        }
    }

    Ok(())
}

fn provider_label(router: &abigail_router::IdEgoRouter) -> String {
    if router.has_ego() {
        router
            .ego_provider_name()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "id".to_string())
    } else {
        "id".to_string()
    }
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
