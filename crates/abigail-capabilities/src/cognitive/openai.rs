use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, StreamEvent, ToolCall,
};
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestToolMessage, ChatCompletionRequestUserMessage, ChatCompletionTool,
    ChatCompletionToolType, CreateChatCompletionRequest, FunctionCall, FunctionObject,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct OpenAiProvider {
    client: async_openai::Client<OpenAIConfig>,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<String>) -> Self {
        Self::with_model(api_key, "gpt-4o-mini".to_string())
    }

    pub fn with_model(api_key: Option<String>, model: String) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .filter(|k| !k.trim().is_empty())
            .unwrap_or_else(|| {
                tracing::warn!("OpenAI API key is empty or missing; requests will fail");
                String::new()
            });
        let config = OpenAIConfig::new().with_api_key(&key);
        let client = async_openai::Client::with_config(config);
        Self {
            client,
            api_key: key,
            model,
        }
    }
}

/// Map our Message role string to the correct async_openai variant.
fn map_message(m: &crate::cognitive::provider::Message) -> ChatCompletionRequestMessage {
    match m.role.as_str() {
        "system" => ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
            content: m.content.clone(),
            ..Default::default()
        }),
        "assistant" => {
            // If the assistant message carried tool_calls, map them too.
            let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| ChatCompletionMessageToolCall {
                        id: tc.id.clone(),
                        r#type: ChatCompletionToolType::Function,
                        function: FunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        },
                    })
                    .collect()
            });
            ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                content: Some(m.content.clone()),
                tool_calls,
                ..Default::default()
            })
        }
        "tool" => ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
            content: m.content.clone(),
            tool_call_id: m.tool_call_id.clone().unwrap_or_default(),
            ..Default::default()
        }),
        _ => ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            content: m.content.clone().into(),
            ..Default::default()
        }),
    }
}

// ── Streaming types (raw reqwest SSE) ────────────────────────────────

#[derive(Debug, Serialize)]
struct StreamChatRequest {
    model: String,
    messages: Vec<StreamChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<StreamChatTool>>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct StreamChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<StreamChatToolCallObj>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StreamChatTool {
    r#type: String,
    function: StreamChatFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct StreamChatFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamChatToolCallObj {
    id: String,
    r#type: String,
    function: StreamChatFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamChatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let messages: Vec<ChatCompletionRequestMessage> =
            request.messages.iter().map(map_message).collect();

        // Map tool definitions if provided.
        let tools: Option<Vec<ChatCompletionTool>> = request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| ChatCompletionTool {
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionObject {
                        name: td.name.clone(),
                        description: Some(td.description.clone()),
                        parameters: Some(td.parameters.clone()),
                    },
                })
                .collect()
        });

        let req = CreateChatCompletionRequest {
            model: self.model.clone(),
            messages,
            tools,
            ..Default::default()
        };

        let response = self.client.chat().create(req).await?;
        let choice = response.choices.first();

        let content = choice
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        // Extract tool calls from the response.
        let tool_calls = choice
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

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
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        let messages: Vec<StreamChatMessage> = request
            .messages
            .iter()
            .map(|m| StreamChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| StreamChatToolCallObj {
                            id: tc.id.clone(),
                            r#type: "function".to_string(),
                            function: StreamChatFunctionCall {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect()
                }),
            })
            .collect();

        let tools: Option<Vec<StreamChatTool>> = request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| StreamChatTool {
                    r#type: "function".to_string(),
                    function: StreamChatFunction {
                        name: td.name.clone(),
                        description: Some(td.description.clone()),
                        parameters: Some(td.parameters.clone()),
                    },
                })
                .collect()
        });

        let body = StreamChatRequest {
            model: self.model.clone(),
            messages,
            tools,
            stream: true,
        };

        let response = http_client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, text);
        }

        let mut full_content = String::new();
        let mut tool_call_map: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

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
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(ref text) = choice.delta.content {
                                full_content.push_str(text);
                                let _ = tx.send(StreamEvent::Token(text.clone())).await;
                            }
                            if let Some(ref tc_deltas) = choice.delta.tool_calls {
                                for tc_delta in tc_deltas {
                                    let entry =
                                        tool_call_map.entry(tc_delta.index).or_insert_with(|| {
                                            (String::new(), String::new(), String::new())
                                        });
                                    if let Some(ref id) = tc_delta.id {
                                        entry.0 = id.clone();
                                    }
                                    if let Some(ref func) = tc_delta.function {
                                        if let Some(ref name) = func.name {
                                            entry.1 = name.clone();
                                        }
                                        if let Some(ref args) = func.arguments {
                                            entry.2.push_str(args);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let tool_calls: Vec<ToolCall> = tool_call_map
            .into_values()
            .map(|(id, name, arguments)| ToolCall {
                id,
                name,
                arguments,
            })
            .collect();
        let tool_calls = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        };

        let response = CompletionResponse {
            content: full_content,
            tool_calls,
        };
        let _ = tx.send(StreamEvent::Done(response.clone())).await;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::{CompletionRequest, Message};

    #[tokio::test]
    async fn test_openai_provider() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            return;
        }
        let key = std::env::var("OPENAI_API_KEY").unwrap();
        let provider = OpenAiProvider::new(Some(key));
        let request =
            CompletionRequest::simple(vec![Message::new("user", "Say hello in one word.")]);
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
