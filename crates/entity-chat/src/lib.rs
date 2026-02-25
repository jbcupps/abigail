//! Shared chat pipeline for entity-daemon and Tauri app.
//!
//! This crate owns the single chat engine used by both the CLI (entity-daemon)
//! and the GUI (Tauri desktop app). Changes here automatically affect both
//! consumers, so testing `cargo test -p entity-chat` validates the shared engine.

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

/// Augment the base system prompt with tool-awareness and skill-specific
/// instructions matched from the `InstructionRegistry`.
///
/// Returns a new prompt string: `base + tool_list + matched_instructions`.
pub fn augment_system_prompt(
    base: &str,
    registry: &SkillRegistry,
    instruction_registry: &abigail_skills::InstructionRegistry,
    user_message: &str,
) -> String {
    let mut prompt = base.to_string();

    if let Ok(manifests) = registry.list() {
        let mut tool_lines = Vec::new();
        for m in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&m.id) {
                for t in skill.tools() {
                    tool_lines.push(format!("- `{}::{}`: {}", m.id.0, t.name, t.description));
                }
            }
        }
        if !tool_lines.is_empty() {
            prompt.push_str("\n\n## Available Tools\n\n");
            prompt.push_str(&tool_lines.join("\n"));
        }
    }

    let skill_section = instruction_registry.format_for_prompt(user_message);
    if !skill_section.is_empty() {
        prompt.push_str(&skill_section);
    }

    prompt
}

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
                    let qualified = format!("{}::{}", manifest.id.0, t.name);
                    // Validate: OpenAI requires parameters to have "type":"object"
                    if t.parameters.get("type").and_then(|v| v.as_str()) != Some("object") {
                        tracing::warn!(
                            "Skipping tool '{}': parameters missing \"type\":\"object\" — would cause API errors",
                            qualified
                        );
                        continue;
                    }
                    defs.push(ToolDefinition {
                        name: qualified,
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
    /// Model quality tier used: "fast", "standard", or "pro".
    pub tier: Option<String>,
    /// Actual model ID used for this request.
    pub model_used: Option<String>,
    /// Complexity score (5–95) that determined tier selection.
    pub complexity_score: Option<u8>,
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

    // Compute tier metadata from the user's original message (last user msg).
    let user_msg = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let (tier, model_used, complexity_score) = router.tier_metadata_for_message(user_msg);

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
                    tier: tier.clone(),
                    model_used: model_used.clone(),
                    complexity_score,
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
        tier,
        model_used,
        complexity_score,
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
    use abigail_skills::channel::TriggerDescriptor;
    use abigail_skills::manifest::SkillManifest;
    use abigail_skills::skill::{
        CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillHealth, SkillResult,
        ToolDescriptor, ToolOutput,
    };
    use std::collections::HashMap;
    use std::sync::Arc;

    // ── Test helpers ─────────────────────────────────────────────────

    fn test_manifest(id: &str) -> SkillManifest {
        SkillManifest {
            id: SkillId(id.to_string()),
            name: id.to_string(),
            version: "1.0".to_string(),
            description: "Test skill".to_string(),
            license: None,
            category: "Test".to_string(),
            keywords: vec![],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![],
            secrets: vec![],
            config_defaults: HashMap::new(),
        }
    }

    struct StubSkill {
        manifest: SkillManifest,
        tool_descriptors: Vec<ToolDescriptor>,
    }

    #[async_trait::async_trait]
    impl Skill for StubSkill {
        fn manifest(&self) -> &SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: SkillConfig) -> SkillResult<()> {
            Ok(())
        }
        async fn shutdown(&mut self) -> SkillResult<()> {
            Ok(())
        }
        fn health(&self) -> SkillHealth {
            SkillHealth {
                status: HealthStatus::Healthy,
                message: None,
                last_check: chrono::Utc::now(),
                metrics: HashMap::new(),
            }
        }
        fn tools(&self) -> Vec<ToolDescriptor> {
            self.tool_descriptors.clone()
        }
        async fn execute_tool(
            &self,
            tool_name: &str,
            params: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<ToolOutput> {
            let echo = params
                .values
                .get("input")
                .cloned()
                .unwrap_or(serde_json::json!("none"));
            Ok(ToolOutput::success(
                serde_json::json!({ "tool": tool_name, "echo": echo }),
            ))
        }
        fn capabilities(&self) -> Vec<abigail_skills::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<TriggerDescriptor> {
            vec![]
        }
    }

    fn valid_tool(name: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: format!("Test tool {}", name),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "input": { "type": "string" } },
                "required": []
            }),
            returns: serde_json::json!({}),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    fn malformed_tool(name: &str) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: "Malformed params".to_string(),
            parameters: serde_json::json!({ "properties": {} }),
            returns: serde_json::json!({}),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    // ── split_qualified_tool_name ────────────────────────────────────

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
    fn test_split_qualified_tool_name_multiple_separators() {
        let result = split_qualified_tool_name("a.b::c::d");
        let (skill, tool) = result.unwrap();
        assert_eq!(skill, "a.b");
        assert_eq!(tool, "c::d");
    }

    // ── build_tool_definitions ───────────────────────────────────────

    #[test]
    fn test_build_tool_definitions_empty_registry() {
        let registry = SkillRegistry::new();
        let defs = build_tool_definitions(&registry);
        assert!(defs.is_empty());
    }

    #[test]
    fn test_build_tool_definitions_single_skill_single_tool() {
        let registry = SkillRegistry::new();
        let skill = StubSkill {
            manifest: test_manifest("test.echo"),
            tool_descriptors: vec![valid_tool("echo")],
        };
        registry
            .register(SkillId("test.echo".to_string()), Arc::new(skill))
            .unwrap();
        let defs = build_tool_definitions(&registry);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "test.echo::echo");
        assert_eq!(defs[0].description, "Test tool echo");
        assert_eq!(defs[0].parameters["type"], "object");
    }

    #[test]
    fn test_build_tool_definitions_multi_skill_multi_tool() {
        let registry = SkillRegistry::new();

        let skill_a = StubSkill {
            manifest: test_manifest("alpha"),
            tool_descriptors: vec![valid_tool("one"), valid_tool("two")],
        };
        let skill_b = StubSkill {
            manifest: test_manifest("beta"),
            tool_descriptors: vec![valid_tool("three")],
        };
        registry
            .register(SkillId("alpha".to_string()), Arc::new(skill_a))
            .unwrap();
        registry
            .register(SkillId("beta".to_string()), Arc::new(skill_b))
            .unwrap();

        let defs = build_tool_definitions(&registry);
        assert_eq!(defs.len(), 3);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"alpha::one"));
        assert!(names.contains(&"alpha::two"));
        assert!(names.contains(&"beta::three"));
    }

    #[test]
    fn test_build_tool_definitions_skips_malformed_params() {
        let registry = SkillRegistry::new();
        let skill = StubSkill {
            manifest: test_manifest("test.mixed"),
            tool_descriptors: vec![valid_tool("good"), malformed_tool("bad")],
        };
        registry
            .register(SkillId("test.mixed".to_string()), Arc::new(skill))
            .unwrap();
        let defs = build_tool_definitions(&registry);
        assert_eq!(defs.len(), 1, "malformed tool should be skipped");
        assert_eq!(defs[0].name, "test.mixed::good");
    }

    #[test]
    fn test_build_tool_definitions_all_malformed_yields_empty() {
        let registry = SkillRegistry::new();
        let skill = StubSkill {
            manifest: test_manifest("test.broken"),
            tool_descriptors: vec![malformed_tool("bad1"), malformed_tool("bad2")],
        };
        registry
            .register(SkillId("test.broken".to_string()), Arc::new(skill))
            .unwrap();
        let defs = build_tool_definitions(&registry);
        assert!(defs.is_empty(), "all-malformed skill should yield no defs");
    }

    // ── execute_single_tool_call (via public ToolCallRecord) ────────

    #[tokio::test]
    async fn test_execute_single_tool_call_success() {
        let registry = Arc::new(SkillRegistry::new());
        let skill = StubSkill {
            manifest: test_manifest("test.echo"),
            tool_descriptors: vec![valid_tool("echo")],
        };
        registry
            .register(SkillId("test.echo".to_string()), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);

        let tc = ToolCall {
            id: "call_1".into(),
            name: "test.echo::echo".into(),
            arguments: r#"{"input":"hello"}"#.into(),
        };
        let (json, record) = execute_single_tool_call(&executor, &tc).await;
        assert!(record.success);
        assert_eq!(record.skill_id, "test.echo");
        assert_eq!(record.tool_name, "echo");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
    }

    #[tokio::test]
    async fn test_execute_single_tool_call_invalid_name() {
        let registry = Arc::new(SkillRegistry::new());
        let executor = SkillExecutor::new(registry);

        let tc = ToolCall {
            id: "call_bad".into(),
            name: "no_separator".into(),
            arguments: "{}".into(),
        };
        let (json, record) = execute_single_tool_call(&executor, &tc).await;
        assert!(!record.success);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["error"]
            .as_str()
            .unwrap()
            .contains("Invalid tool name"));
    }

    #[tokio::test]
    async fn test_execute_single_tool_call_malformed_arguments() {
        let registry = Arc::new(SkillRegistry::new());
        let skill = StubSkill {
            manifest: test_manifest("test.echo"),
            tool_descriptors: vec![valid_tool("echo")],
        };
        registry
            .register(SkillId("test.echo".to_string()), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);

        let tc = ToolCall {
            id: "call_malformed".into(),
            name: "test.echo::echo".into(),
            arguments: "not valid json!!!".into(),
        };
        let (json, record) = execute_single_tool_call(&executor, &tc).await;
        assert!(
            record.success,
            "malformed args should default to empty params, not fail"
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["success"], true);
    }

    #[tokio::test]
    async fn test_execute_single_tool_call_nonexistent_skill() {
        let registry = Arc::new(SkillRegistry::new());
        let executor = SkillExecutor::new(registry);

        let tc = ToolCall {
            id: "call_missing".into(),
            name: "ghost.skill::tool".into(),
            arguments: "{}".into(),
        };
        let (json, record) = execute_single_tool_call(&executor, &tc).await;
        assert!(!record.success);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["error"].is_string());
    }

    // ── sanitize_session_history ─────────────────────────────────────

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
        assert_eq!(result[0].content, "msg 6");
    }

    // ── build_contextual_messages ────────────────────────────────────

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
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[3].role, "user");
        assert_eq!(msgs[3].content, "how are you");
    }

    #[test]
    fn test_build_contextual_no_history() {
        let msgs = build_contextual_messages("system prompt", None, "hi");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[0].content, "system prompt");
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "hi");
    }

    // ── augment_system_prompt ──────────────────────────────────────

    #[test]
    fn test_augment_prompt_adds_tool_section() {
        let registry = SkillRegistry::new();
        let skill = StubSkill {
            manifest: test_manifest("test.echo"),
            tool_descriptors: vec![valid_tool("echo")],
        };
        registry
            .register(SkillId("test.echo".to_string()), Arc::new(skill))
            .unwrap();

        let instr_reg = abigail_skills::InstructionRegistry::empty();
        let result = augment_system_prompt("Base prompt.", &registry, &instr_reg, "hello");
        assert!(result.starts_with("Base prompt."));
        assert!(result.contains("## Available Tools"));
        assert!(result.contains("test.echo::echo"));
    }

    #[test]
    fn test_augment_prompt_no_tools_no_section() {
        let registry = SkillRegistry::new();
        let instr_reg = abigail_skills::InstructionRegistry::empty();
        let result = augment_system_prompt("Base.", &registry, &instr_reg, "hi");
        assert_eq!(result, "Base.");
    }

    // ── ToolUseResult struct ─────────────────────────────────────────

    #[test]
    fn test_tool_use_result_fields() {
        let result = ToolUseResult {
            content: "done".into(),
            tool_calls_made: vec![ToolCallRecord {
                skill_id: "a".into(),
                tool_name: "b".into(),
                success: true,
            }],
            tier: Some("fast".into()),
            model_used: Some("gpt-4.1-mini".into()),
            complexity_score: Some(25),
        };
        assert_eq!(result.content, "done");
        assert_eq!(result.tool_calls_made.len(), 1);
        assert_eq!(result.tier.as_deref(), Some("fast"));
        assert_eq!(result.model_used.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(result.complexity_score, Some(25));
    }
}
