//! Tauri chat command — uses the shared `entity-chat` engine.
//!
//! The chat pipeline (sanitization, tool definitions, tool-use loop) lives in
//! the `entity-chat` crate so that CLI and GUI share a single engine. This
//! module only contains Tauri-specific wrappers: API key auto-detection from
//! user messages, the `#[tauri::command]` handler, and system diagnostics.

use crate::state::AppState;
use entity_core::{ChatResponse, SessionMessage};
use tauri::State;

// ---------------------------------------------------------------------------
// API key auto-detection (Tauri-specific)
// ---------------------------------------------------------------------------

/// Check if a message contains recognizable API keys and store them.
pub async fn auto_detect_and_store_key_internal(
    state: &AppState,
    message: &str,
) -> Vec<(String, String)> {
    let patterns = [
        (r"sk-ant-[a-zA-Z0-9_-]{20,}", "anthropic"),
        (r"sk-[a-zA-Z0-9]{20,}", "openai"),
        (r"xai-[a-zA-Z0-9_-]{20,}", "xai"),
        (r"pplx-[a-zA-Z0-9_-]{20,}", "perplexity"),
        (r"AIza[a-zA-Z0-9_-]{35}", "google"),
        (r"tvly-[a-zA-Z0-9_-]{20,}", "tavily"),
    ];

    let mut detected = Vec::new();

    for (pattern, provider) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(message) {
                let key = mat.as_str().to_string();
                tracing::info!(
                    "Detected possible {} key in message (length: {})",
                    provider,
                    key.len()
                );
                detected.push((provider.to_string(), key));
            }
        }
    }

    if !detected.is_empty() {
        {
            if let Ok(mut vault) = state.secrets.lock() {
                for (provider, key) in &detected {
                    vault.set_secret(provider, key);
                    match provider.as_str() {
                        "openai" => {
                            vault.set_secret("codex-cli", key);
                        }
                        "anthropic" => {
                            vault.set_secret("claude-cli", key);
                        }
                        "google" => {
                            vault.set_secret("gemini-cli", key);
                        }
                        "xai" => {
                            vault.set_secret("grok-cli", key);
                        }
                        _ => {}
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
