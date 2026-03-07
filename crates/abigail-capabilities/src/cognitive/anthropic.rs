//! Anthropic Claude LLM provider implementation.
//!
//! Uses the Anthropic Messages API directly via reqwest (no SDK dependency).
//! Handles Anthropic-specific message format: system prompt is a top-level
//! parameter, not a message in the array.

use crate::cognitive::provider::{
    sanitize_tool_name, CompletionRequest, CompletionResponse, LlmProvider, StreamEvent, ToolCall,
    ToolDefinition,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 4096;

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> anyhow::Result<Self> {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;
        Ok(Self {
            api_key,
            model,
            client,
        })
    }
}

// ── Anthropic API request/response types ────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

/// Anthropic content can be a simple string or an array of content blocks.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ResponseContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicError {
    error: AnthropicErrorDetail,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

// ── Streaming SSE event types ────────────────────────────────────────

/// Top-level SSE event from Anthropic's streaming API.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SseEvent {
    #[serde(rename = "message_start")]
    MessageStart {},
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: SseContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: SseDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {},
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: AnthropicErrorDetail },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SseContentBlock {
    #[serde(rename = "text")]
    Text {
        #[allow(dead_code)]
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum SseDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

// ── Conversion helpers ──────────────────────────────────────────────

/// Convert our Message list into Anthropic format.
/// Extracts system messages to top-level field; converts tool messages
/// to user-role content blocks with tool_result type.
fn convert_messages(
    messages: &[crate::cognitive::provider::Message],
) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system_text: Option<String> = None;
    let mut anthropic_msgs: Vec<AnthropicMessage> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" => {
                // Anthropic wants system as a top-level param, not in messages
                match &mut system_text {
                    Some(existing) => {
                        existing.push_str("\n\n");
                        existing.push_str(&msg.content);
                    }
                    None => system_text = Some(msg.content.clone()),
                }
            }
            "assistant" => {
                // Check if this assistant message has tool_calls
                if let Some(ref tool_calls) = msg.tool_calls {
                    let mut blocks = Vec::new();
                    if !msg.content.is_empty() {
                        blocks.push(ContentBlock::Text {
                            text: msg.content.clone(),
                        });
                    }
                    for tc in tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&tc.arguments).unwrap_or_default();
                        blocks.push(ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: sanitize_tool_name(&tc.name),
                            input,
                        });
                    }
                    anthropic_msgs.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Blocks(blocks),
                    });
                } else {
                    anthropic_msgs.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
            }
            "tool" => {
                // Anthropic: tool results are sent as user messages with tool_result content blocks
                let tool_use_id = msg.tool_call_id.clone().unwrap_or_default();
                let block = ContentBlock::ToolResult {
                    tool_use_id,
                    content: msg.content.clone(),
                };
                // If the last message is already a user message with blocks, append to it
                if let Some(last) = anthropic_msgs.last_mut() {
                    if last.role == "user" {
                        if let AnthropicContent::Blocks(ref mut blocks) = last.content {
                            blocks.push(block);
                            continue;
                        }
                    }
                }
                anthropic_msgs.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![block]),
                });
            }
            _ => {
                // "user" and anything else → user message
                anthropic_msgs.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Text(msg.content.clone()),
                });
            }
        }
    }

    // Anthropic requires messages to start with a user message and alternate roles.
    // Merge consecutive same-role messages if needed.
    let merged = merge_consecutive_roles(anthropic_msgs);

    (system_text, merged)
}

/// Merge consecutive messages with the same role into a single message with blocks.
fn merge_consecutive_roles(msgs: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    let mut result: Vec<AnthropicMessage> = Vec::new();
    for msg in msgs {
        if let Some(last) = result.last_mut() {
            if last.role == msg.role {
                // Merge into blocks
                let mut blocks = to_blocks(&last.content);
                blocks.extend(to_blocks(&msg.content));
                last.content = AnthropicContent::Blocks(blocks);
                continue;
            }
        }
        result.push(msg);
    }
    result
}

fn to_blocks(content: &AnthropicContent) -> Vec<ContentBlock> {
    match content {
        AnthropicContent::Text(t) => vec![ContentBlock::Text { text: t.clone() }],
        AnthropicContent::Blocks(b) => {
            // We need to clone the blocks; since ContentBlock doesn't derive Clone,
            // re-serialize/deserialize. This is only for the rare merge case.
            let json = serde_json::to_value(b).unwrap_or_default();
            serde_json::from_value(json).unwrap_or_default()
        }
    }
}

/// Convert tool definitions and return the API-safe tools plus a
/// reverse map from sanitized name back to the original qualified name.
fn convert_tools(
    tools: &[ToolDefinition],
) -> (
    Vec<AnthropicTool>,
    std::collections::HashMap<String, String>,
) {
    let mut api_tools = Vec::new();
    let mut name_map = std::collections::HashMap::new();
    for td in tools {
        let safe_name = sanitize_tool_name(&td.name);
        name_map.insert(safe_name.clone(), td.name.clone());
        api_tools.push(AnthropicTool {
            name: safe_name,
            description: td.description.clone(),
            input_schema: td.parameters.clone(),
        });
    }
    (api_tools, name_map)
}

// ── LlmProvider impl ───────────────────────────────────────────────

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let model = request.model_override.as_deref().unwrap_or(&self.model);
        tracing::info!(
            "Anthropic::complete model={}, messages={}, tools={}",
            model,
            request.messages.len(),
            request.tools.as_ref().map_or(0, |t| t.len()),
        );
        let (system, messages) = convert_messages(&request.messages);
        let (tools, name_map) = match request.tools.as_ref() {
            Some(t) => {
                let (api_tools, map) = convert_tools(t);
                (Some(api_tools), map)
            }
            None => (None, std::collections::HashMap::new()),
        };

        let body = AnthropicRequest {
            model: model.to_string(),
            max_tokens: MAX_TOKENS,
            system,
            messages,
            tools,
            stream: false,
        };

        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<AnthropicError>(&body_text) {
                return Err(anyhow::anyhow!(
                    "Anthropic API error ({}): {} - {}",
                    status,
                    err.error.error_type,
                    err.error.message
                ));
            }
            return Err(anyhow::anyhow!(
                "Anthropic API error ({}): {}",
                status,
                body_text
            ));
        }

        let api_response: AnthropicResponse = response.json().await?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in &api_response.content {
            match block {
                ResponseContentBlock::Text { text } => text_parts.push(text.clone()),
                ResponseContentBlock::ToolUse { id, name, input } => {
                    let original_name = name_map
                        .get(name.as_str())
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        name: original_name,
                        arguments: input.to_string(),
                    });
                }
            }
        }

        let content = text_parts.join("");
        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        if let Some(ref reason) = api_response.stop_reason {
            tracing::debug!("Anthropic stop_reason: {}", reason);
        }

        Ok(CompletionResponse {
            content,
            tool_calls,
        })
    }

    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        let model = request.model_override.as_deref().unwrap_or(&self.model);
        tracing::info!(
            "Anthropic::stream model={}, messages={}, tools={}",
            model,
            request.messages.len(),
            request.tools.as_ref().map_or(0, |t| t.len()),
        );
        let (system, messages) = convert_messages(&request.messages);
        let (tools, name_map) = match request.tools.as_ref() {
            Some(t) => {
                let (api_tools, map) = convert_tools(t);
                (Some(api_tools), map)
            }
            None => (None, std::collections::HashMap::new()),
        };

        let api_request = AnthropicRequest {
            model: model.to_string(),
            max_tokens: MAX_TOKENS,
            system,
            messages,
            tools,
            stream: true,
        };

        let response = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&api_request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<AnthropicError>(&body) {
                return Err(anyhow::anyhow!(
                    "Anthropic API error ({}): {} - {}",
                    status,
                    err.error.error_type,
                    err.error.message
                ));
            }
            return Err(anyhow::anyhow!(
                "Anthropic API error ({}): {}",
                status,
                body
            ));
        }

        // Parse SSE stream
        let mut full_text = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        // Track in-progress tool calls by content block index
        let mut tool_ids: std::collections::HashMap<usize, (String, String)> =
            std::collections::HashMap::new();
        let mut tool_args: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();

        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines from buffer
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<SseEvent>(data) {
                        Ok(event) => match event {
                            SseEvent::ContentBlockStart {
                                index,
                                content_block: SseContentBlock::ToolUse { id, name },
                            } => {
                                let original_name =
                                    name_map.get(name.as_str()).cloned().unwrap_or(name);
                                tool_ids.insert(index, (id, original_name));
                                tool_args.insert(index, String::new());
                            }
                            SseEvent::ContentBlockDelta { index, delta } => match delta {
                                SseDelta::TextDelta { text } => {
                                    full_text.push_str(&text);
                                    let _ = tx.send(StreamEvent::Token(text)).await;
                                }
                                SseDelta::InputJsonDelta { partial_json } => {
                                    if let Some(args) = tool_args.get_mut(&index) {
                                        args.push_str(&partial_json);
                                    }
                                }
                            },
                            SseEvent::ContentBlockStop { index } => {
                                if let Some((id, name)) = tool_ids.remove(&index) {
                                    let arguments = tool_args.remove(&index).unwrap_or_default();
                                    tool_calls.push(ToolCall {
                                        id,
                                        name,
                                        arguments,
                                    });
                                }
                            }
                            SseEvent::Error { error } => {
                                return Err(anyhow::anyhow!(
                                    "Anthropic stream error: {} - {}",
                                    error.error_type,
                                    error.message
                                ));
                            }
                            _ => {} // MessageStart, MessageDelta, MessageStop, Ping
                        },
                        Err(e) => {
                            tracing::debug!("Skipping unparseable SSE event ({}): {}", e, data);
                        }
                    }
                }
            }
        }

        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        let response = CompletionResponse {
            content: full_text,
            tool_calls,
        };

        let _ = tx.send(StreamEvent::Done(response.clone())).await;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::{CompletionRequest, Message, ToolDefinition};

    #[test]
    fn test_system_prompt_extraction() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("user", "Hello"),
        ];
        let (system, msgs) = convert_messages(&messages);
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn test_multiple_system_messages_concatenated() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("system", "Be concise."),
            Message::new("user", "Hello"),
        ];
        let (system, msgs) = convert_messages(&messages);
        assert_eq!(system, Some("You are helpful.\n\nBe concise.".to_string()));
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_tool_result_messages() {
        let messages = vec![
            Message::new("user", "Search for cats"),
            Message {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "web_search".to_string(),
                    arguments: r#"{"query":"cats"}"#.to_string(),
                }]),
            },
            Message::tool_result("call_1", "Found 10 results about cats"),
        ];
        let (_, msgs) = convert_messages(&messages);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "user"); // tool result wrapped in user message
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let (result, map) = convert_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "search");
        assert_eq!(map["search"], "search");
    }

    #[test]
    fn test_sanitize_tool_name_qualified() {
        let name = "builtin.hive_management::store_secret";
        let safe = sanitize_tool_name(name);
        assert_eq!(safe, "builtin_hive_management__store_secret");
    }

    #[test]
    fn test_convert_tools_round_trip() {
        let tools = vec![ToolDefinition {
            name: "dynamic.example-tool::lookup_records".to_string(),
            description: "Look up records".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let (result, map) = convert_tools(&tools);
        assert_eq!(result[0].name, "dynamic_example-tool__lookup_records");
        assert_eq!(
            map["dynamic_example-tool__lookup_records"],
            "dynamic.example-tool::lookup_records"
        );
    }

    #[tokio::test]
    async fn test_anthropic_provider_with_real_key() {
        // Only runs when ANTHROPIC_API_KEY is set
        let key = match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => return,
        };
        let provider = AnthropicProvider::new(key).unwrap();
        let request =
            CompletionRequest::simple(vec![Message::new("user", "Say hello in one word.")]);
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
