//! Anthropic Claude LLM provider implementation.
//!
//! Uses the Anthropic Messages API directly via reqwest (no SDK dependency).
//! Handles Anthropic-specific message format: system prompt is a top-level
//! parameter, not a message in the array.

use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, StreamEvent, ToolCall,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, DEFAULT_MODEL.to_string())
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Self {
            api_key,
            model,
            client,
        }
    }
}

// ── Anthropic API request/response types ─────────────────────────────

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

// ── Message mapping ─────────────────────────────────────────────────

/// Convert AO messages to Anthropic format.
/// Anthropic requires:
/// - System prompt as a top-level field (not in messages array)
/// - Messages alternate user/assistant (tool results go inside user messages)
/// - tool_calls from assistant become content blocks with type "tool_use"
/// - tool results become content blocks with type "tool_result" inside user messages
fn map_messages(
    ao_messages: &[crate::cognitive::provider::Message],
) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system_prompt = None;
    let mut messages: Vec<AnthropicMessage> = Vec::new();

    for msg in ao_messages {
        match msg.role.as_str() {
            "system" => {
                // Anthropic: system prompt is a top-level field, not a message.
                // If multiple system messages, concatenate them.
                match &mut system_prompt {
                    Some(existing) => {
                        *existing = format!("{}\n\n{}", existing, msg.content);
                    }
                    None => {
                        system_prompt = Some(msg.content.clone());
                    }
                }
            }
            "assistant" => {
                // If the assistant message has tool_calls, represent them as content blocks
                if let Some(ref tool_calls) = msg.tool_calls {
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    if !msg.content.is_empty() {
                        blocks.push(ContentBlock::Text {
                            text: msg.content.clone(),
                        });
                    }
                    for tc in tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                        blocks.push(ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input,
                        });
                    }
                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Blocks(blocks),
                    });
                } else {
                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Text(msg.content.clone()),
                    });
                }
            }
            "tool" => {
                // Anthropic requires tool results inside a "user" message with tool_result blocks.
                let tool_call_id = msg.tool_call_id.clone().unwrap_or_default();
                let block = ContentBlock::ToolResult {
                    tool_use_id: tool_call_id,
                    content: msg.content.clone(),
                };
                // If the last message is already a user message with blocks, append to it.
                // Otherwise create a new user message.
                if let Some(last) = messages.last_mut() {
                    if last.role == "user" {
                        if let AnthropicContent::Blocks(ref mut blocks) = last.content {
                            blocks.push(block);
                            continue;
                        }
                    }
                }
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![block]),
                });
            }
            _ => {
                // "user" and anything else → user message
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Text(msg.content.clone()),
                });
            }
        }
    }

    (system_prompt, messages)
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let (system, messages) = map_messages(&request.messages);

        // Map tool definitions to Anthropic format
        let tools: Option<Vec<AnthropicTool>> = request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| AnthropicTool {
                    name: td.name.clone(),
                    description: td.description.clone(),
                    input_schema: td.parameters.clone(),
                })
                .collect()
        });

        let api_request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
            system,
            messages,
            tools,
            stream: false,
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
            return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, body));
        }

        let api_response: AnthropicResponse = response.json().await?;

        // Extract text content and tool calls from response blocks
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for block in &api_response.content {
            match block {
                ResponseContentBlock::Text { text } => {
                    text_parts.push(text.clone());
                }
                ResponseContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
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

        // Log stop reason for debugging
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
        let (system, messages) = map_messages(&request.messages);

        let tools: Option<Vec<AnthropicTool>> = request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| AnthropicTool {
                    name: td.name.clone(),
                    description: td.description.clone(),
                    input_schema: td.parameters.clone(),
                })
                .collect()
        });

        let api_request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
            system,
            messages,
            tools,
            stream: true,
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
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
            return Err(anyhow::anyhow!("Anthropic API error ({}): {}", status, body));
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
            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                // Parse SSE event lines
                let mut data_line = None;
                for line in event_block.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        data_line = Some(data.to_string());
                    }
                }

                if let Some(data) = data_line {
                    if data == "[DONE]" {
                        break;
                    }

                    match serde_json::from_str::<SseEvent>(&data) {
                        Ok(event) => match event {
                            SseEvent::ContentBlockStart {
                                index,
                                content_block: SseContentBlock::ToolUse { id, name },
                            } => {
                                tool_ids.insert(index, (id, name));
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
                                    let arguments =
                                        tool_args.remove(&index).unwrap_or_default();
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
                            tracing::debug!("Skipping unparseable SSE event: {}", e);
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
        let (system, mapped) = map_messages(&messages);
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].role, "user");
    }

    #[test]
    fn test_multiple_system_messages_concatenated() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("system", "Be concise."),
            Message::new("user", "Hello"),
        ];
        let (system, mapped) = map_messages(&messages);
        assert_eq!(
            system,
            Some("You are helpful.\n\nBe concise.".to_string())
        );
        assert_eq!(mapped.len(), 1);
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
        let (_, mapped) = map_messages(&messages);
        assert_eq!(mapped.len(), 3);
        assert_eq!(mapped[0].role, "user");
        assert_eq!(mapped[1].role, "assistant");
        assert_eq!(mapped[2].role, "user"); // tool result wrapped in user message
    }

    #[test]
    fn test_tool_definition_mapping() {
        let tools = vec![ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        }];

        let mapped: Vec<AnthropicTool> = tools
            .iter()
            .map(|td| AnthropicTool {
                name: td.name.clone(),
                description: td.description.clone(),
                input_schema: td.parameters.clone(),
            })
            .collect();

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].name, "web_search");
    }

    #[tokio::test]
    async fn test_anthropic_provider_with_real_key() {
        // Only runs when ANTHROPIC_API_KEY is set
        let key = match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => return,
        };
        let provider = AnthropicProvider::new(key);
        let request =
            CompletionRequest::simple(vec![Message::new("user", "Say hello in one word.")]);
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
