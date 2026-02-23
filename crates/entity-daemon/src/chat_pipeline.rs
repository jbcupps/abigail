//! Chat pipeline utilities for entity-daemon.
//!
//! Ported from `tauri-app/src/commands/chat.rs` to bring entity-daemon chat
//! to functional parity: sanitization, system prompt, tool awareness, dedup.
//! Phase 2a adds the tool-use loop: convert skill tools to LLM-native
//! ToolDefinitions, call `route_with_tools`, execute returned tool calls,
//! feed results back, and iterate until the LLM produces a final text reply.

use abigail_capabilities::cognitive::{CompletionResponse, Message, ToolCall, ToolDefinition};
use abigail_router::IdEgoRouter;
use abigail_skills::manifest::SkillId;
use abigail_skills::skill::ToolParams;
use abigail_skills::{SkillExecutor, SkillRegistry};
use entity_core::{SessionMessage, ToolCallRecord};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_HISTORY_MESSAGES: usize = 24;
const MAX_MESSAGE_CHARS: usize = 4_000;

// ---------------------------------------------------------------------------
// Sanitize session history
// ---------------------------------------------------------------------------

/// Filter invalid roles, trim content, cap at 24 messages / 4000 chars each.
pub fn sanitize_session_history(history: Option<Vec<SessionMessage>>) -> Vec<Message> {
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

// ---------------------------------------------------------------------------
// Build contextual messages
// ---------------------------------------------------------------------------

/// Assemble `[system_prompt, ...sanitized_history, user_message]` with
/// last-message deduplication (if the final history message is identical to
/// the new user message, drop it to avoid repeating).
pub fn build_contextual_messages(
    system_prompt: &str,
    session_messages: Option<Vec<SessionMessage>>,
    latest_user_message: &str,
) -> Vec<Message> {
    let mut messages = vec![Message::new("system", system_prompt)];
    let mut history = sanitize_session_history(session_messages);

    // Deduplicate: if the last history message is the same as what the user
    // just sent, drop it so we don't feed the LLM a duplicate.
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
// Tool definitions: SkillRegistry → ToolDefinition[]
// ---------------------------------------------------------------------------

/// Convert all registered skill tools into LLM-native `ToolDefinition`s.
///
/// Tool names are qualified as `{skill_id}::{tool_name}` so the LLM knows
/// which skill to invoke and we can split them back apart in the loop.
pub fn build_tool_definitions(registry: &SkillRegistry) -> Vec<ToolDefinition> {
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

/// Split a qualified tool name `skill_id::tool_name` back into its parts.
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
// Tool-use loop
// ---------------------------------------------------------------------------

/// Maximum number of tool-use round-trips before forcing a text response.
const MAX_TOOL_ROUNDS: usize = 8;

/// Outcome of the tool-use loop.
pub struct ToolUseResult {
    /// The final text reply from the LLM.
    pub content: String,
    /// All tool calls executed during the loop.
    pub tool_calls_made: Vec<ToolCallRecord>,
}

/// Run the full tool-use loop:
/// 1. Send messages + tool definitions to the LLM via `route_with_tools`.
/// 2. If the LLM returns `tool_calls`, execute each one via `SkillExecutor`.
/// 3. Append the assistant's tool-call message and each tool result to the
///    conversation, then re-prompt.
/// 4. Repeat until the LLM returns a plain text response (no tool_calls)
///    or we hit `MAX_TOOL_ROUNDS`.
pub async fn run_tool_use_loop(
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

/// Execute a single tool call, returning the JSON result string and a record.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_qualified_tool_name_valid() {
        let (skill, tool) =
            split_qualified_tool_name("com.abigail.skills.hive::create_entity").unwrap();
        assert_eq!(skill, "com.abigail.skills.hive");
        assert_eq!(tool, "create_entity");
    }

    #[test]
    fn test_split_qualified_tool_name_invalid() {
        assert!(split_qualified_tool_name("no_separator").is_none());
        assert!(split_qualified_tool_name("::tool").is_none());
        assert!(split_qualified_tool_name("skill::").is_none());
    }

    #[test]
    fn test_build_tool_definitions_empty_registry() {
        let registry = SkillRegistry::new();
        let defs = build_tool_definitions(&registry);
        assert!(defs.is_empty());
    }

    #[test]
    fn test_sanitize_empty_history() {
        let result = sanitize_session_history(None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_sanitize_filters_invalid_roles() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            SessionMessage {
                role: "system".into(),
                content: "should be filtered".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "world".into(),
            },
        ];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, "user");
        assert_eq!(result[1].role, "assistant");
    }

    #[test]
    fn test_sanitize_filters_empty_content() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "   ".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "ok".into(),
            },
        ];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "ok");
    }

    #[test]
    fn test_sanitize_caps_message_length() {
        let long_content = "a".repeat(5000);
        let history = vec![SessionMessage {
            role: "user".into(),
            content: long_content,
        }];
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.len(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn test_sanitize_caps_history_count() {
        let history: Vec<SessionMessage> = (0..30)
            .map(|i| SessionMessage {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                content: format!("msg {}", i),
            })
            .collect();
        let result = sanitize_session_history(Some(history));
        assert_eq!(result.len(), MAX_HISTORY_MESSAGES);
        // Should keep the most recent 24 (indices 6..30)
        assert_eq!(result[0].content, "msg 6");
    }

    #[test]
    fn test_build_contextual_deduplicates_last() {
        let history = vec![
            SessionMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            SessionMessage {
                role: "assistant".into(),
                content: "hi".into(),
            },
            SessionMessage {
                role: "user".into(),
                content: "how are you".into(),
            },
        ];
        let msgs = build_contextual_messages("sys", Some(history), "how are you");
        // system + user("hello") + assistant("hi") + user("how are you")
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[3].role, "user");
        assert_eq!(msgs[3].content, "how are you");
    }
}
