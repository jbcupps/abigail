use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ToolCall, ToolDefinition,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 4096;

pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::new();
        Self { api_key, client }
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
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize)]
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
    content: Vec<ResponseBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseBlock {
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
                            name: tc.name.clone(),
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

fn convert_tools(tools: &[ToolDefinition]) -> Vec<AnthropicTool> {
    tools
        .iter()
        .map(|td| AnthropicTool {
            name: td.name.clone(),
            description: td.description.clone(),
            input_schema: td.parameters.clone(),
        })
        .collect()
}

// ── LlmProvider impl ───────────────────────────────────────────────

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let (system, messages) = convert_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| convert_tools(t));

        let body = AnthropicRequest {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: MAX_TOKENS,
            system,
            messages,
            tools,
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
            // Try to extract error message from JSON
            if let Ok(err) = serde_json::from_str::<AnthropicError>(&body_text) {
                return Err(anyhow::anyhow!(
                    "Anthropic API error ({}): {}",
                    status,
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

        // Extract text content and tool calls from response blocks
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in &api_response.content {
            match block {
                ResponseBlock::Text { text } => text_parts.push(text.clone()),
                ResponseBlock::ToolUse { id, name, input } => {
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

        Ok(CompletionResponse {
            content,
            tool_calls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::Message;

    #[test]
    fn test_convert_messages_extracts_system() {
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
    fn test_convert_messages_tool_result() {
        let messages = vec![
            Message::new("user", "Use this tool"),
            Message {
                role: "assistant".to_string(),
                content: "".to_string(),
                tool_call_id: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc_1".to_string(),
                    name: "search".to_string(),
                    arguments: r#"{"q":"test"}"#.to_string(),
                }]),
            },
            Message::tool_result("tc_1", "result here"),
        ];
        let (_system, msgs) = convert_messages(&messages);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, "user"); // tool_result becomes user message
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDefinition {
            name: "search".to_string(),
            description: "Search the web".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let result = convert_tools(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "search");
    }

    #[tokio::test]
    async fn test_anthropic_provider_integration() {
        // Only run with a real key
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
