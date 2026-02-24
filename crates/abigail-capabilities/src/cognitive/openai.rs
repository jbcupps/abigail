use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, StreamEvent, ToolCall,
    ToolDefinition,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<String>) -> anyhow::Result<Self> {
        Self::with_model(api_key, "gpt-4.1".to_string())
    }

    pub fn with_model(api_key: Option<String>, model: String) -> anyhow::Result<Self> {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .ok_or_else(|| anyhow::anyhow!("OpenAI API key is empty or missing"))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        Ok(Self {
            client,
            api_key: key,
            model,
        })
    }

    fn build_messages(messages: &[Message]) -> Vec<ChatMessage> {
        messages
            .iter()
            .map(|m| {
                let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| ChatToolCallObj {
                            id: tc.id.clone(),
                            r#type: "function".to_string(),
                            function: ChatFunctionCall {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect()
                });
                ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    tool_call_id: m.tool_call_id.clone(),
                    tool_calls,
                }
            })
            .collect()
    }

    fn build_tools(tools: &[ToolDefinition]) -> Vec<ChatTool> {
        tools
            .iter()
            .map(|td| ChatTool {
                r#type: "function".to_string(),
                function: ChatFunction {
                    name: td.name.clone(),
                    description: Some(td.description.clone()),
                    parameters: Some(td.parameters.clone()),
                },
            })
            .collect()
    }
}

// ── API request/response types ───────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCallObj>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatTool {
    r#type: String,
    function: ChatFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatFunction {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatToolCallObj {
    id: String,
    r#type: String,
    function: ChatFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatToolCallObj>>,
}

// ── SSE streaming types ──────────────────────────────────────────

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
        let model = request.model_override.as_deref().unwrap_or(&self.model);
        tracing::info!("OpenAiProvider::complete model={}", model);
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: model.to_string(),
            messages,
            tools,
            stream: false,
        };

        let response = self
            .client
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

        let chat_response: ChatResponse = response.json().await?;
        let choice = chat_response.choices.first();

        let content = choice
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

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
        let model = request.model_override.as_deref().unwrap_or(&self.model);
        tracing::info!("OpenAiProvider::stream model={}", model);
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: model.to_string(),
            messages,
            tools,
            stream: true,
        };

        let response = self
            .client
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
        let mut receiver_closed = false;

        'stream_loop: while let Some(chunk) = byte_stream.next().await {
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
                                if tx.send(StreamEvent::Token(text.clone())).await.is_err() {
                                    tracing::warn!(
                                        "OpenAI stream receiver closed while sending token; terminating stream early"
                                    );
                                    receiver_closed = true;
                                    break 'stream_loop;
                                }
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
        if tx.send(StreamEvent::Done(response.clone())).await.is_err() {
            tracing::warn!("OpenAI stream receiver closed while sending completion event");
        } else if receiver_closed {
            tracing::debug!("OpenAI stream ended after receiver disconnect");
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::{CompletionRequest, Message};

    #[test]
    fn test_openai_provider_rejects_empty_key() {
        let provider = OpenAiProvider::new(Some("   ".to_string()));
        assert!(provider.is_err());
    }

    #[tokio::test]
    async fn test_openai_provider() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            return;
        }
        let key = std::env::var("OPENAI_API_KEY").unwrap();
        let provider = OpenAiProvider::new(Some(key)).unwrap();
        let request =
            CompletionRequest::simple(vec![Message::new("user", "Say hello in one word.")]);
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
