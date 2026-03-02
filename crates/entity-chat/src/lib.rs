//! Shared chat pipeline for entity-daemon and Tauri app.
//!
//! This crate owns the single chat engine used by both the CLI (entity-daemon)
//! and the GUI (Tauri desktop app). Changes here automatically affect both
//! consumers, so testing `cargo test -p entity-chat` validates the shared engine.

use abigail_capabilities::cognitive::{Message, StreamEvent, ToolCall, ToolDefinition};
use abigail_router::IdEgoRouter;
use abigail_skills::manifest::SkillId;
use abigail_skills::skill::ToolParams;
use abigail_skills::{SkillExecutor, SkillRegistry};
use entity_core::{SessionMessage, ToolCallRecord};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// RuntimeContext — per-request metadata injected into the system prompt
// ---------------------------------------------------------------------------

/// Carries per-request runtime metadata so the entity knows what provider,
/// model, and routing mode are active. This prevents identity confusion
/// (e.g. "what model are you?") and makes provider switches visible.
#[derive(Debug, Clone, Default)]
pub struct RuntimeContext {
    pub provider_name: Option<String>,
    pub model_id: Option<String>,
    pub routing_mode: Option<String>,
    pub tier: Option<String>,
    pub complexity_score: Option<u8>,
    pub entity_name: Option<String>,
    pub entity_id: Option<String>,
    pub has_local_llm: bool,
    pub last_provider_change_at: Option<String>,
}

impl RuntimeContext {
    /// Render a concise `## Runtime Context` markdown section for the system prompt.
    /// Returns an empty string when the context is entirely empty.
    pub fn format_for_prompt(&self) -> String {
        let mut lines = Vec::new();

        if let Some(ref name) = self.entity_name {
            lines.push(format!("- Entity name: {}", name));
        }
        if let Some(ref provider) = self.provider_name {
            lines.push(format!("- Active provider: {}", provider));
        }
        if let Some(ref model) = self.model_id {
            lines.push(format!("- Model: {}", model));
        }
        if let Some(ref mode) = self.routing_mode {
            lines.push(format!("- Routing mode: {}", mode));
        }
        if let Some(ref tier) = self.tier {
            lines.push(format!("- Quality tier: {}", tier));
        }
        if self.has_local_llm {
            lines.push("- Local LLM: available (failsafe)".to_string());
        }
        if let Some(ref ts) = self.last_provider_change_at {
            lines.push(format!("- Provider last changed: {}", ts));
        }

        if lines.is_empty() {
            return String::new();
        }

        let mut section = String::from("\n\n## Runtime Context\n\n");
        section.push_str("You are executing within the following runtime environment:\n\n");
        section.push_str(&lines.join("\n"));
        section.push_str("\n\n");
        section.push_str(
            "When asked what model or provider you are using, report ONLY the \
             provider and model shown above. Do not guess or repeat stale information \
             from previous turns. If the information above is absent, say you are \
             unsure and suggest the user check the routing details panel.\n\
             \n\
             You are always the Entity — the single conversational agent. If a \
             local fallback path was used, describe it as an internal execution \
             path, not as a separate identity. Never refer to yourself as \"Id\" \
             or claim to be a different system than the Entity.",
        );
        section.push('\n');
        section
    }
}

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
// Memory context (ContextBudget + build_memory_context)
// ---------------------------------------------------------------------------

/// Budget controlling how much memory context is injected into prompts.
pub struct ContextBudget {
    pub recent_turns_limit: usize,
    pub memory_search_limit: usize,
    pub max_total_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            recent_turns_limit: 5,
            memory_search_limit: 3,
            max_total_chars: 8000,
        }
    }
}

/// Build a context window from the memory store instead of replaying
/// the full session_messages array.
///
/// Returns a Vec<Message> with recent turns from the current session
/// plus keyword-matched memories from older sessions.
pub fn build_memory_context(
    store: &abigail_memory::MemoryStore,
    session_id: &str,
    user_message: &str,
    budget: &ContextBudget,
) -> Vec<Message> {
    let mut context = Vec::new();
    let mut total_chars = 0usize;

    // Layer 1: Recent turns from the current session.
    if let Ok(recent) = store.recent_turns(session_id, budget.recent_turns_limit) {
        for turn in &recent {
            let len = turn.content.len();
            if total_chars + len > budget.max_total_chars {
                break;
            }
            context.push(Message::new(&turn.role, &turn.content));
            total_chars += len;
        }
    }

    // Layer 2: Keyword search across all memories.
    if total_chars < budget.max_total_chars {
        if let Ok(memories) = store.search_memories(user_message, budget.memory_search_limit) {
            let mut mem_lines = Vec::new();
            for m in &memories {
                let line = format!("[{}] {}", m.created_at.format("%Y-%m-%d"), m.content);
                if total_chars + line.len() > budget.max_total_chars {
                    break;
                }
                total_chars += line.len();
                mem_lines.push(line);
            }
            if !mem_lines.is_empty() {
                let section = format!("## Recalled Memories\n\n{}", mem_lines.join("\n"));
                context.insert(0, Message::new("system", &section));
            }
        }
    }

    context
}

// ---------------------------------------------------------------------------
// Tool definitions: SkillRegistry → ToolDefinition[]
// ---------------------------------------------------------------------------

/// Augment the base system prompt with tool-awareness, skill-specific
/// instructions matched from the `InstructionRegistry`, and runtime context.
///
/// Returns a new prompt string: `base + runtime_context + tool_list + matched_instructions`.
///
/// Skill instructions are filtered against actually-registered skill IDs to
/// prevent "phantom tool" hallucinations (where the LLM sees instructions for
/// skills that aren't loaded).
pub fn augment_system_prompt(
    base: &str,
    registry: &SkillRegistry,
    instruction_registry: &abigail_skills::InstructionRegistry,
    user_message: &str,
    runtime_ctx: &RuntimeContext,
) -> String {
    let mut prompt = base.to_string();

    // Inject runtime context so the entity knows its own provider/model
    let ctx_section = runtime_ctx.format_for_prompt();
    if !ctx_section.is_empty() {
        prompt.push_str(&ctx_section);
    }

    // Collect registered skill IDs for instruction filtering
    let mut registered_ids = HashSet::new();

    if let Ok(manifests) = registry.list() {
        let mut tool_lines = Vec::new();
        for m in &manifests {
            registered_ids.insert(m.id.0.clone());
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

    // Only inject instructions for skills that are actually registered
    let skill_section =
        instruction_registry.format_for_prompt_filtered(user_message, &registered_ids);
    if !skill_section.is_empty() {
        prompt.push_str(&skill_section);
    }

    prompt
}

/// Build a CLI-optimized system prompt for CliOrchestrator mode.
///
/// Uses a heavily compressed inline prompt (~1.5 KB) with:
/// - Extracted personality essence from soul.md
/// - Condensed ethics block
/// - Compact tool list (grouped by skill, names only)
/// - Budgeted instruction injection (max 1 instruction, 2048 bytes)
///
/// Full constitutional docs + all matched skill instructions are written to
/// a temp file so the CLI LLM can lazily read them when needed.
pub fn build_cli_system_prompt(
    docs_dir: &std::path::Path,
    agent_name: &Option<String>,
    registry: &SkillRegistry,
    instruction_registry: &abigail_skills::InstructionRegistry,
    user_message: &str,
) -> String {
    let mut prompt =
        abigail_core::system_prompt::build_cli_system_prompt_compressed(docs_dir, agent_name);

    // Build compact grouped tool list and collect registered IDs
    let mut registered_ids = HashSet::new();
    let mut skill_tools: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    if let Ok(manifests) = registry.list() {
        for m in &manifests {
            registered_ids.insert(m.id.0.clone());
            if let Ok((skill, _)) = registry.get_skill(&m.id) {
                let tool_names: Vec<String> =
                    skill.tools().iter().map(|t| t.name.clone()).collect();
                if !tool_names.is_empty() {
                    // Use short skill name (last segment after dots)
                    let short_name = m.id.0.rsplit('.').next().unwrap_or(&m.id.0).to_string();
                    skill_tools
                        .entry(short_name)
                        .or_default()
                        .extend(tool_names);
                }
            }
        }
    }

    if !skill_tools.is_empty() {
        prompt.push_str("\n\n## Entity Tools\n");
        let mut entries: Vec<_> = skill_tools.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (skill_name, tools) in &entries {
            prompt.push_str(&format!("{}: {}\n", skill_name, tools.join(", ")));
        }
    }

    // Budgeted instruction injection: max 1 instruction, 2048 bytes for CLI mode
    let skill_section = instruction_registry.format_for_prompt_budgeted(
        user_message,
        &registered_ids,
        1,    // max_instructions
        2048, // max_bytes
    );
    if !skill_section.is_empty() {
        prompt.push_str(&skill_section);
    }

    // Write full constitutional docs + all instructions to temp file for lazy loading
    let spillover = build_cli_spillover_file(
        docs_dir,
        instruction_registry,
        user_message,
        &registered_ids,
    );
    if let Some(path) = spillover {
        prompt.push_str(&format!(
            "\n\nDetailed entity instructions are at: {}. Read this file if you need guidance on ethics, tool usage, or skill-specific behavior.\n",
            path
        ));
    }

    tracing::info!("CLI system prompt: {} bytes (compressed)", prompt.len());

    prompt
}

/// Write the full constitutional docs and all matched skill instructions to
/// a temp file for lazy loading by the CLI LLM.
///
/// Returns the file path if successful, or `None` on failure.
fn build_cli_spillover_file(
    docs_dir: &std::path::Path,
    instruction_registry: &abigail_skills::InstructionRegistry,
    user_message: &str,
    registered_ids: &HashSet<String>,
) -> Option<String> {
    let mut doc = abigail_core::system_prompt::build_cli_spillover_document(docs_dir);

    // Append all matched instructions (unbudgeted — this is the full reference)
    let all_instructions =
        instruction_registry.format_for_prompt_filtered(user_message, registered_ids);
    if !all_instructions.is_empty() {
        doc.push_str(&all_instructions);
    }

    let path = std::env::temp_dir().join("abigail_entity_context.md");
    match std::fs::write(&path, &doc) {
        Ok(()) => {
            tracing::debug!(
                "Wrote CLI spillover file: {} ({} bytes)",
                path.display(),
                doc.len()
            );
            Some(path.to_string_lossy().to_string())
        }
        Err(e) => {
            tracing::warn!("Failed to write CLI spillover file: {}", e);
            None
        }
    }
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
    /// Authoritative execution trace from the final LLM call that produced
    /// the text response (captures fallback chain and timing).
    /// All attribution (tier, model, provider, complexity) is derived from this.
    pub execution_trace: Option<entity_core::ExecutionTrace>,
}

impl ToolUseResult {
    /// Derive the tier from the authoritative execution trace.
    pub fn tier(&self) -> Option<&str> {
        self.execution_trace.as_ref().and_then(|t| t.final_tier())
    }

    /// Derive the model from the authoritative execution trace.
    pub fn model_used(&self) -> Option<&str> {
        self.execution_trace.as_ref().and_then(|t| t.final_model())
    }

    /// Derive the complexity score from the authoritative execution trace.
    pub fn complexity_score(&self) -> Option<u8> {
        self.execution_trace
            .as_ref()
            .and_then(|t| t.complexity_score)
    }
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
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
) -> anyhow::Result<ToolUseResult> {
    run_tool_use_loop_with_model_override(router, executor, messages, tools, None).await
}

/// Same as [`run_tool_use_loop`] but allows forcing a model override for all
/// LLM calls in the loop.
pub async fn run_tool_use_loop_with_model_override(
    router: &IdEgoRouter,
    executor: &SkillExecutor,
    mut messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    model_override: Option<String>,
) -> anyhow::Result<ToolUseResult> {
    let mut all_records = Vec::new();
    let mut last_trace: Option<entity_core::ExecutionTrace> = None;

    for round in 0..MAX_TOOL_ROUNDS {
        tracing::debug!("Tool-use loop round {}", round);

        let (response, trace) = router
            .route_with_tools_traced_override(
                messages.clone(),
                tools.clone(),
                model_override.clone(),
            )
            .await?;
        last_trace = Some(trace);

        let tool_calls = match response.tool_calls {
            Some(ref tcs) if !tcs.is_empty() => tcs.clone(),
            _ => {
                return Ok(ToolUseResult {
                    content: response.content,
                    tool_calls_made: all_records,
                    execution_trace: last_trace,
                });
            }
        };

        messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
            tool_call_id: None,
            tool_calls: Some(tool_calls.clone()),
        });

        for tc in &tool_calls {
            let (output_json, record) = execute_single_tool_call(executor, tc).await;
            all_records.push(record);
            messages.push(Message::tool_result(&tc.id, output_json));
        }
    }

    tracing::warn!(
        "Tool-use loop exhausted {} rounds, returning partial result",
        MAX_TOOL_ROUNDS
    );
    Ok(ToolUseResult {
        content: "I attempted several tool calls but hit the maximum number of rounds. Here's what I have so far.".to_string(),
        tool_calls_made: all_records,
        execution_trace: last_trace,
    })
}

// ---------------------------------------------------------------------------
// Streaming-compatible tool-use: run tool rounds, stop before final text
// ---------------------------------------------------------------------------

/// Intermediate result from `run_tool_use_loop_rounds_only`.
pub struct IntermediateToolResult {
    /// Tool calls executed so far.
    pub tool_calls_made: Vec<ToolCallRecord>,
    /// If the LLM produced a final text response during the tool rounds
    /// (i.e. it stopped calling tools), this contains that text. When `None`,
    /// the caller should stream one more LLM call using the updated `messages`.
    pub final_text: Option<String>,
    /// Execution trace from the final LLM call in the tool-use rounds.
    pub execution_trace: Option<entity_core::ExecutionTrace>,
}

/// Run tool-use rounds non-streaming, but stop *before* the final text
/// response so the caller can stream it. Mutates `messages` in place so
/// the caller can pass the updated conversation to a streaming route.
///
/// Returns `IntermediateToolResult`:
/// - `final_text = Some(...)` means the loop completed and no streaming is needed.
/// - `final_text = None` means tool calls were executed and `messages` now
///   contains the full conversation; the caller should stream the next completion.
pub async fn run_tool_use_loop_rounds_only(
    router: &IdEgoRouter,
    executor: &SkillExecutor,
    messages: &mut Vec<Message>,
    tools: &[ToolDefinition],
) -> anyhow::Result<IntermediateToolResult> {
    let mut all_records = Vec::new();
    let mut did_tool_calls = false;
    let mut last_trace: Option<entity_core::ExecutionTrace> = None;

    for round in 0..MAX_TOOL_ROUNDS {
        tracing::debug!("Tool-use loop (rounds-only) round {}", round);

        let (response, trace) = router
            .route_with_tools_traced(messages.clone(), tools.to_vec())
            .await?;
        last_trace = Some(trace);

        let tool_calls = match response.tool_calls {
            Some(ref tcs) if !tcs.is_empty() => tcs.clone(),
            _ => {
                return Ok(IntermediateToolResult {
                    tool_calls_made: all_records,
                    final_text: Some(response.content),
                    execution_trace: last_trace,
                });
            }
        };

        did_tool_calls = true;

        messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
            tool_call_id: None,
            tool_calls: Some(tool_calls.clone()),
        });

        for tc in &tool_calls {
            let (output_json, record) = execute_single_tool_call(executor, tc).await;
            all_records.push(record);
            messages.push(Message::tool_result(&tc.id, output_json));
        }
    }

    let _ = did_tool_calls;
    Ok(IntermediateToolResult {
        tool_calls_made: all_records,
        final_text: None,
        execution_trace: last_trace,
    })
}

// ---------------------------------------------------------------------------
// Shared streaming chat pipeline
// ---------------------------------------------------------------------------

/// Result of [`stream_chat_pipeline`]. The caller assembles a `ChatResponse`
/// by combining this with pre-computed tier metadata and provider info.
pub struct StreamPipelineResult {
    pub content: String,
    pub tool_calls_made: Vec<ToolCallRecord>,
    /// Authoritative execution trace for the final streaming LLM call.
    pub execution_trace: Option<entity_core::ExecutionTrace>,
}

/// Run the full streaming chat pipeline: tool-use rounds (non-streaming) then
/// stream the final LLM response through `tx`.
///
/// When tool rounds produce a final text response directly (LLM didn't invoke
/// tools), the function returns immediately without sending any `StreamEvent`s
/// — the caller should emit the result as a single "done" event. When streaming
/// does occur, `StreamEvent::Token` values are sent through `tx` as they arrive.
pub async fn stream_chat_pipeline(
    router: &IdEgoRouter,
    executor: &SkillExecutor,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    _target_mode: &str, // Reserved; chat always uses Ego path
    tx: tokio::sync::mpsc::Sender<StreamEvent>,
) -> anyhow::Result<StreamPipelineResult> {
    let mut messages = messages;
    let mut tool_calls_made = Vec::new();

    // Chat never uses Id; always Ego path. Tool-use loop when tools present.
    if !tools.is_empty() {
        let intermediate =
            run_tool_use_loop_rounds_only(router, executor, &mut messages, &tools).await?;
        tool_calls_made = intermediate.tool_calls_made;
        if let Some(final_text) = intermediate.final_text {
            drop(tx);
            return Ok(StreamPipelineResult {
                content: final_text,
                tool_calls_made,
                execution_trace: intermediate.execution_trace,
            });
        }
    }

    let (final_response, trace) = if tools.is_empty() {
        router.route_stream_traced(messages, tx.clone()).await?
    } else {
        router
            .route_stream_with_tools_traced(messages, tools, tx.clone())
            .await?
    };

    drop(tx);

    Ok(StreamPipelineResult {
        content: final_response.content,
        tool_calls_made,
        execution_trace: Some(trace),
    })
}

/// Human-readable label for the active provider ("openai", "anthropic", etc.
/// or "local" when no Ego is configured). Id is a background subsystem and
/// should never appear as a conversational actor label.
pub fn provider_label(router: &IdEgoRouter) -> String {
    if router.has_ego() {
        router
            .ego_provider_name()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "local".to_string())
    } else {
        "local".to_string()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

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
            tracing::debug!(
                "Parsed tool params for {}::{}: keys={:?}",
                skill_id_str,
                tool_name,
                tp.values.keys().collect::<Vec<_>>()
            );
            tp
        }
        Ok(other) => {
            tracing::warn!(
                "Tool {}::{} arguments parsed as non-object JSON (type: {}), using empty params. Raw: {}",
                skill_id_str,
                tool_name,
                match &other {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Array(_) => "array",
                    _ => "unknown",
                },
                &tc.arguments.chars().take(200).collect::<String>()
            );
            ToolParams::new()
        }
        Err(e) => {
            tracing::warn!(
                "Tool {}::{} arguments JSON parse failed: {}. Raw (first 200 chars): {}",
                skill_id_str,
                tool_name,
                e,
                &tc.arguments.chars().take(200).collect::<String>()
            );
            ToolParams::new()
        }
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
        let result = augment_system_prompt(
            "Base prompt.",
            &registry,
            &instr_reg,
            "hello",
            &RuntimeContext::default(),
        );
        assert!(result.starts_with("Base prompt."));
        assert!(result.contains("## Available Tools"));
        assert!(result.contains("test.echo::echo"));
    }

    #[test]
    fn test_augment_prompt_no_tools_no_section() {
        let registry = SkillRegistry::new();
        let instr_reg = abigail_skills::InstructionRegistry::empty();
        let result = augment_system_prompt(
            "Base.",
            &registry,
            &instr_reg,
            "hi",
            &RuntimeContext::default(),
        );
        assert_eq!(result, "Base.");
    }

    #[test]
    fn test_augment_prompt_includes_runtime_context() {
        let registry = SkillRegistry::new();
        let instr_reg = abigail_skills::InstructionRegistry::empty();
        let ctx = RuntimeContext {
            provider_name: Some("anthropic".to_string()),
            model_id: Some("claude-sonnet-4-6".to_string()),
            entity_name: Some("Adam".to_string()),
            ..Default::default()
        };
        let result = augment_system_prompt("Base.", &registry, &instr_reg, "hi", &ctx);
        assert!(result.contains("## Runtime Context"));
        assert!(result.contains("anthropic"));
        assert!(result.contains("claude-sonnet-4-6"));
        assert!(result.contains("Adam"));
    }

    // ── RuntimeContext ────────────────────────────────────────────

    #[test]
    fn test_runtime_context_empty() {
        let ctx = RuntimeContext::default();
        assert!(ctx.format_for_prompt().is_empty());
    }

    #[test]
    fn test_runtime_context_partial() {
        let ctx = RuntimeContext {
            provider_name: Some("openai".to_string()),
            ..Default::default()
        };
        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("## Runtime Context"));
        assert!(prompt.contains("openai"));
        assert!(!prompt.contains("Model:"));
    }

    #[test]
    fn test_runtime_context_full() {
        let ctx = RuntimeContext {
            provider_name: Some("anthropic".to_string()),
            model_id: Some("claude-opus-4-6".to_string()),
            routing_mode: Some("tier_based".to_string()),
            tier: Some("pro".to_string()),
            complexity_score: Some(85),
            entity_name: Some("Adam".to_string()),
            entity_id: Some("abc-123".to_string()),
            has_local_llm: true,
            last_provider_change_at: Some("2026-02-26T10:00:00Z".to_string()),
        };
        let prompt = ctx.format_for_prompt();
        assert!(prompt.contains("anthropic"));
        assert!(prompt.contains("claude-opus-4-6"));
        assert!(prompt.contains("tier_based"));
        assert!(prompt.contains("pro"));
        assert!(prompt.contains("Adam"));
        assert!(prompt.contains("Local LLM: available"));
        assert!(prompt.contains("2026-02-26T10:00:00Z"));
    }

    // ── ToolUseResult struct ─────────────────────────────────────────

    #[test]
    fn test_tool_use_result_fields() {
        let mut trace = entity_core::ExecutionTrace::new(
            "tierbased",
            Some("openai".into()),
            Some("gpt-4.1-mini".into()),
            "ego",
        );
        trace.configured_tier = Some("fast".into());
        trace.complexity_score = Some(25);
        trace.record_success("openai", Some("gpt-4.1-mini".into()), chrono::Utc::now());

        let result = ToolUseResult {
            content: "done".into(),
            tool_calls_made: vec![ToolCallRecord {
                skill_id: "a".into(),
                tool_name: "b".into(),
                success: true,
            }],
            execution_trace: Some(trace),
        };
        assert_eq!(result.content, "done");
        assert_eq!(result.tool_calls_made.len(), 1);
        assert_eq!(result.tier(), Some("fast"));
        assert_eq!(result.model_used(), Some("gpt-4.1-mini"));
        assert_eq!(result.complexity_score(), Some(25));
        assert!(result.execution_trace.is_some());
    }

    #[test]
    fn test_tool_use_result_with_trace() {
        let mut trace = entity_core::ExecutionTrace::new(
            "tierbased",
            Some("openai".into()),
            Some("gpt-4.1-mini".into()),
            "ego",
        );
        trace.configured_tier = Some("fast".into());
        trace.complexity_score = Some(20);
        let t0 = chrono::Utc::now();
        trace.record_success("openai", Some("gpt-4.1-mini".into()), t0);

        let result = ToolUseResult {
            content: "done".into(),
            tool_calls_made: vec![],
            execution_trace: Some(trace),
        };
        let trace_ref = result.execution_trace.as_ref().unwrap();
        assert!(!trace_ref.fallback_occurred);
        assert_eq!(trace_ref.final_provider(), Some("openai"));
        assert_eq!(trace_ref.final_model(), Some("gpt-4.1-mini"));
        assert_eq!(result.tier(), Some("fast"));
        assert_eq!(result.model_used(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn test_stream_pipeline_result_has_trace_field() {
        let result = StreamPipelineResult {
            content: "streamed".into(),
            tool_calls_made: vec![],
            execution_trace: None,
        };
        assert!(result.execution_trace.is_none());
    }

    #[test]
    fn test_runtime_context_includes_self_awareness_instruction() {
        let ctx = RuntimeContext {
            provider_name: Some("openai".to_string()),
            model_id: Some("gpt-4.1".to_string()),
            routing_mode: None,
            tier: None,
            complexity_score: None,
            entity_name: None,
            entity_id: None,
            has_local_llm: false,
            last_provider_change_at: None,
        };
        let prompt = ctx.format_for_prompt();
        assert!(
            prompt.contains("report ONLY the provider and model shown above"),
            "Prompt should contain self-awareness instruction"
        );
        assert!(
            prompt.contains("You are always the Entity"),
            "Prompt should contain entity-first identity rule"
        );
        assert!(
            prompt.contains("Never refer to yourself as \"Id\""),
            "Prompt should forbid self-identifying as Id"
        );
    }

    #[test]
    fn test_provider_label_never_returns_id() {
        use abigail_router::{IdEgoRouter, RoutingMode};
        let router = IdEgoRouter::new(None, None, None, None, RoutingMode::TierBased);
        let label = provider_label(&router);
        assert_eq!(label, "local", "provider_label must never return 'id'");
    }

    #[test]
    fn test_runtime_context_empty_no_instruction() {
        let ctx = RuntimeContext {
            provider_name: None,
            model_id: None,
            routing_mode: None,
            tier: None,
            complexity_score: None,
            entity_name: None,
            entity_id: None,
            has_local_llm: false,
            last_provider_change_at: None,
        };
        let prompt = ctx.format_for_prompt();
        assert!(
            prompt.is_empty(),
            "Empty context should produce empty prompt"
        );
    }
}
