//! Tauri chat command — unified with entity-daemon chat flow.
//!
//! This module mirrors the exact same pipeline as `entity-daemon/src/chat_pipeline.rs`
//! so that testing the CLI validates the GUI and vice versa.

use crate::state::AppState;
use abigail_capabilities::cognitive::{CompletionResponse, Message, ToolCall, ToolDefinition};
use abigail_router::IdEgoRouter;
use abigail_skills::manifest::SkillId;
use abigail_skills::skill::ToolParams;
use abigail_skills::{SkillExecutor, SkillRegistry};
use entity_core::{ChatResponse, ToolCallRecord};
use serde::Deserialize;
use tauri::State;

#[derive(Debug, Clone, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Shared helpers (identical to entity-daemon/src/chat_pipeline.rs)
// ---------------------------------------------------------------------------

const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_MESSAGE_CHARS: usize = 4_000;

fn sanitize_session_history(history: Option<Vec<SessionMessage>>) -> Vec<Message> {
    history
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let trimmed = m.content.trim();
            if trimmed.is_empty() {
                return None;
            }
            let content = if trimmed.chars().count() > MAX_MESSAGE_CHARS {
                trimmed.chars().take(MAX_MESSAGE_CHARS).collect::<String>()
            } else {
                trimmed.to_string()
            };
            Some(Message::new(&m.role, &content))
        })
        .rev()
        .take(MAX_HISTORY_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn build_contextual_messages(
    system_prompt: &str,
    session_messages: Option<Vec<SessionMessage>>,
    latest_user_message: &str,
) -> Vec<Message> {
    let mut messages = vec![Message::new("system", system_prompt)];
    let mut history = sanitize_session_history(session_messages);

    if let Some(last) = history.last() {
        if last.role == "user" && last.content == latest_user_message.trim() {
            history.pop();
        }
    }

    messages.extend(history);
    messages.push(Message::new("user", latest_user_message));
    messages
}

// ---------------------------------------------------------------------------
// Tool definitions (mirrors entity-daemon/src/chat_pipeline.rs)
// ---------------------------------------------------------------------------

fn build_tool_definitions(registry: &SkillRegistry) -> Vec<ToolDefinition> {
    let mut defs = Vec::new();
    if let Ok(manifests) = registry.list() {
        for manifest in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&manifest.id) {
                for t in skill.tools() {
                    defs.push(ToolDefinition {
                        name: format!("{}::{}", manifest.id.0, t.name),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    });
                }
            }
        }
    }
    defs
}

fn split_qualified_tool_name(qualified: &str) -> Option<(String, String)> {
    let idx = qualified.find("::")?;
    let skill_id = qualified[..idx].to_string();
    let tool_name = qualified[idx + 2..].to_string();
    if skill_id.is_empty() || tool_name.is_empty() {
        return None;
    }
    Some((skill_id, tool_name))
}

// ---------------------------------------------------------------------------
// Tool-use loop (mirrors entity-daemon/src/chat_pipeline.rs)
// ---------------------------------------------------------------------------

const MAX_TOOL_ROUNDS: usize = 8;

struct ToolUseResult {
    content: String,
    tool_calls_made: Vec<ToolCallRecord>,
}

async fn run_tool_use_loop(
    router: &IdEgoRouter,
    executor: &SkillExecutor,
    mut messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
) -> anyhow::Result<ToolUseResult> {
    let mut all_records = Vec::new();

    for round in 0..MAX_TOOL_ROUNDS {
        tracing::debug!("Tool-use loop round {}", round);

        let response: CompletionResponse = router
            .route_with_tools(messages.clone(), tools.clone())
            .await?;

        // If no tool_calls, we're done — return the text reply.
        let tool_calls = match response.tool_calls {
            Some(ref tcs) if !tcs.is_empty() => tcs.clone(),
            _ => {
                return Ok(ToolUseResult {
                    content: response.content,
                    tool_calls_made: all_records,
                });
            }
        };

        // Append the assistant message (with tool_calls metadata) to history.
        messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
            tool_call_id: None,
            tool_calls: Some(tool_calls.clone()),
        });

        // Execute each tool call and append results.
        for tc in &tool_calls {
            let (output_json, record) = execute_single_tool_call(executor, tc).await;
            all_records.push(record);
            messages.push(Message::tool_result(&tc.id, output_json));
        }
    }

    // Safety: if we exhausted rounds, return what we have.
    tracing::warn!(
        "Tool-use loop exhausted {} rounds, returning partial result",
        MAX_TOOL_ROUNDS
    );
    Ok(ToolUseResult {
        content: "I attempted several tool calls but hit the maximum number of rounds. Here's what I have so far.".to_string(),
        tool_calls_made: all_records,
    })
}

async fn execute_single_tool_call(
    executor: &SkillExecutor,
    tc: &ToolCall,
) -> (String, ToolCallRecord) {
    let Some((skill_id_str, tool_name)) = split_qualified_tool_name(&tc.name) else {
        let err_msg = format!("Invalid tool name format: {}", tc.name);
        tracing::warn!("{}", err_msg);
        return (
            serde_json::json!({"error": err_msg}).to_string(),
            ToolCallRecord {
                skill_id: tc.name.clone(),
                tool_name: tc.name.clone(),
                success: false,
            },
        );
    };

    // Parse the arguments JSON into ToolParams.
    let params = match serde_json::from_str::<serde_json::Value>(&tc.arguments) {
        Ok(serde_json::Value::Object(obj)) => {
            let mut tp = ToolParams::new();
            for (k, v) in obj {
                tp.values.insert(k, v);
            }
            tp
        }
        _ => ToolParams::new(),
    };

    tracing::info!("Executing tool: {}::{}", skill_id_str, tool_name);

    let skill_id = SkillId(skill_id_str.clone());
    match executor.execute(&skill_id, &tool_name, params).await {
        Ok(output) => {
            let result_json = serde_json::json!({
                "success": output.success,
                "data": output.data,
            })
            .to_string();
            (
                result_json,
                ToolCallRecord {
                    skill_id: skill_id_str,
                    tool_name,
                    success: output.success,
                },
            )
        }
        Err(e) => {
            let err_json = serde_json::json!({
                "error": e.to_string(),
            })
            .to_string();
            tracing::warn!("Tool execution failed: {}", e);
            (
                err_json,
                ToolCallRecord {
                    skill_id: skill_id_str,
                    tool_name,
                    success: false,
                },
            )
        }
    }
}

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
        let _ = crate::rebuild_router_with_superego(state).await;
    }

    detected
}

// ---------------------------------------------------------------------------
// POST /chat — unified Tauri command (mirrors entity-daemon POST /v1/chat)
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

    // 2. Build system prompt + router snapshot
    let (router, base_system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, prompt)
    };

    // 3. Build contextual messages with sanitization + deduplication
    let messages = build_contextual_messages(&base_system_prompt, session_messages, &message);

    // 4. Build tool definitions from registered skills
    let tools = build_tool_definitions(&state.registry);

    // 5. Route — use tool-use loop if tools are available, plain route otherwise
    let target_mode = target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target_mode == "ID" {
        // No tools or explicit Id-only: simple route
        let res = if target_mode == "ID" {
            router.id_only(messages).await
        } else {
            router.route(messages).await
        };
        res.map(|r| ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
        })
    } else {
        // Tools available: run the agentic tool-use loop
        run_tool_use_loop(&router, &state.executor, messages, tools).await
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

            // 6. Return JSON-serialized ChatResponse (same DTO as entity-daemon)
            let response = ChatResponse {
                reply: tool_result.content,
                provider: Some(provider),
                tool_calls_made: tool_result.tool_calls_made,
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
    report.push_str(&format!("- Superego Configured: {}\n", s.has_superego));

    Ok(report)
}
