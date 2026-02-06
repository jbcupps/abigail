//! LiteLLM / OpenAI-compatible HTTP provider for local LLM servers.
//!
//! Connects to a local LLM server (LiteLLM, Ollama, LM Studio, etc.) via
//! the OpenAI-compatible chat completions API.

use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, StreamEvent, ToolCall,
};
use ao_core::validate_local_llm_url;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// HTTP provider for local LLM servers with OpenAI-compatible API.
pub struct LocalHttpProvider {
    base_url: String,
    client: reqwest::Client,
    model: String,
}

/// OpenAI-compatible chat completion request body.
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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
    tool_calls: Option<Vec<ChatToolCall>>,
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
struct ChatToolCall {
    id: String,
    r#type: String,
    function: ChatFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
}

/// OpenAI-compatible chat completion response.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// Response from /v1/models endpoint.
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
    tool_calls: Option<Vec<ChatToolCall>>,
}

/// OpenAI-compatible streaming chunk.
#[derive(Debug, Deserialize)]
struct ChatStreamChunk {
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

impl LocalHttpProvider {
    /// Create a new provider pointing to a local LLM server.
    ///
    /// # Arguments
    /// * `base_url` - Base URL of the server, e.g. "http://localhost:1234"
    /// * `model` - Model name to use, e.g. "local-model" or "gpt-3.5-turbo" (depends on server)
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.into(),
            client,
            model: model.into(),
        }
    }

    /// Create a provider with default model name "local-model".
    /// Note: Use `with_url_auto_model()` for LM Studio which requires actual model names.
    pub fn with_url(base_url: impl Into<String>) -> Self {
        Self::new(base_url, "local-model")
    }

    /// Create a provider that auto-detects the model name from /v1/models.
    /// Falls back to "local-model" if detection fails.
    pub async fn with_url_auto_model(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        let model = Self::detect_model(&base).await.unwrap_or_else(|e| {
            tracing::warn!("Model auto-detection failed: {}. Using 'local-model'", e);
            "local-model".to_string()
        });
        tracing::info!("Auto-detected model: {}", model);
        Self::new(base, model)
    }

    /// Query /v1/models and return the first available model ID.
    async fn detect_model(base_url: &str) -> anyhow::Result<String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(3))
            .build()?;

        let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Models endpoint returned {}",
                response.status()
            ));
        }

        let models: ModelsResponse = response.json().await?;
        models
            .data
            .first()
            .map(|m| m.id.clone())
            .ok_or_else(|| anyhow::anyhow!("No models available"))
    }

    /// Perform a heartbeat check to verify the LLM server is reachable.
    /// Sends a minimal completion request and checks for a valid response.
    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        validate_local_llm_url(&self.base_url).map_err(|e| anyhow::anyhow!("{}", e))?;
        let _request = CompletionRequest::simple(vec![Message::new("user", "ping")]);

        // Use a shorter timeout for heartbeat
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()?;

        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".into(),
                content: "ping".into(),
                tool_call_id: None,
                tool_calls: None,
            }],
            max_tokens: Some(10), // Minimal response for heartbeat
            tools: None,
            stream: false,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let response = client
            .post(&url)
            .json(&chat_request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LLM heartbeat failed: connection error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "LLM heartbeat failed: HTTP {} - {}",
                status,
                body
            ));
        }

        let _: ChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("LLM heartbeat failed: invalid response: {}", e))?;

        tracing::info!("LLM heartbeat OK: {} is reachable", self.base_url);
        Ok(())
    }

    /// Get the base URL of the server.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build the messages list from a CompletionRequest.
    fn build_messages(request: &CompletionRequest) -> Vec<ChatMessage> {
        request
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| ChatToolCall {
                            id: tc.id.clone(),
                            r#type: "function".to_string(),
                            function: ChatFunctionCall {
                                name: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                            },
                        })
                        .collect()
                }),
            })
            .collect()
    }

    /// Build the tools list from a CompletionRequest.
    fn build_tools(request: &CompletionRequest) -> Option<Vec<ChatTool>> {
        request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| ChatTool {
                    r#type: "function".to_string(),
                    function: ChatFunction {
                        name: td.name.clone(),
                        description: Some(td.description.clone()),
                        parameters: Some(td.parameters.clone()),
                    },
                })
                .collect()
        })
    }
}

#[async_trait]
impl LlmProvider for LocalHttpProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        validate_local_llm_url(&self.base_url).map_err(|e| anyhow::anyhow!("{}", e))?;

        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: Self::build_messages(request),
            max_tokens: None,
            tools: Self::build_tools(request),
            stream: false,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(&url)
            .json(&chat_request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LLM request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "LLM request failed: HTTP {} - {}",
                status,
                body
            ));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("LLM response parse failed: {}", e))?;

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
        validate_local_llm_url(&self.base_url).map_err(|e| anyhow::anyhow!("{}", e))?;

        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages: Self::build_messages(request),
            max_tokens: None,
            tools: Self::build_tools(request),
            stream: true,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );

        let response = self
            .client
            .post(&url)
            .json(&chat_request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LLM request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "LLM request failed: HTTP {} - {}",
                status,
                body
            ));
        }

        let mut full_text = String::new();
        // Track tool calls being built incrementally
        let mut tool_call_map: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new(); // index -> (id, name, arguments)

        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = byte_stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines
            while let Some(pos) = buffer.find("\n\n") {
                let event_block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                for line in event_block.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }

                        if let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(data) {
                            if let Some(choice) = chunk.choices.first() {
                                // Handle text content delta
                                if let Some(ref text) = choice.delta.content {
                                    full_text.push_str(text);
                                    let _ = tx.send(StreamEvent::Token(text.clone())).await;
                                }

                                // Handle tool call deltas
                                if let Some(ref tc_deltas) = choice.delta.tool_calls {
                                    for tc_delta in tc_deltas {
                                        let entry =
                                            tool_call_map.entry(tc_delta.index).or_insert_with(
                                                || (String::new(), String::new(), String::new()),
                                            );
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
        }

        // Assemble tool calls
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
            content: full_text,
            tool_calls,
        };
        let _ = tx.send(StreamEvent::Done(response.clone())).await;
        Ok(response)
    }
}

/// Perform a heartbeat check using the Candle stub (always succeeds).
/// Used when no local LLM URL is configured.
pub async fn stub_heartbeat() -> anyhow::Result<()> {
    tracing::info!("LLM heartbeat OK: using in-process stub (no external server)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stub_heartbeat() {
        // Stub heartbeat should always succeed
        stub_heartbeat().await.unwrap();
    }

    #[tokio::test]
    async fn test_local_http_provider_unreachable() {
        // Test that heartbeat fails gracefully when server is not running
        let provider = LocalHttpProvider::with_url("http://localhost:59999");
        let result = provider.heartbeat().await;
        assert!(result.is_err());
    }
}
