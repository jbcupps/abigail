//! Generic OpenAI-compatible HTTP provider for cloud LLM APIs.
//!
//! Works with any API that follows the OpenAI chat completions format:
//! - Perplexity (sonar, sonar-pro, sonar-reasoning-pro)
//! - xAI Grok (grok-4-1-fast-reasoning, grok-4-0709)
//! - Google Gemini (gemini-2.5-flash, gemini-2.5-pro)
//! - Any other OpenAI-compatible endpoint

use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, StreamEvent, ToolCall,
    ToolDefinition,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Known OpenAI-compatible providers with their endpoints and default models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibleProvider {
    Perplexity,
    Xai,
    Google,
    Custom,
}

impl fmt::Display for CompatibleProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Perplexity => write!(f, "perplexity"),
            Self::Xai => write!(f, "xai"),
            Self::Google => write!(f, "google"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

impl CompatibleProvider {
    /// Default base URL for this provider.
    pub fn base_url(&self) -> &str {
        match self {
            Self::Perplexity => "https://api.perplexity.ai",
            Self::Xai => "https://api.x.ai/v1",
            Self::Google => "https://generativelanguage.googleapis.com/v1beta/openai",
            Self::Custom => "",
        }
    }

    /// Default model for this provider (Feb 2026 — Standard tier).
    pub fn default_model(&self) -> &str {
        match self {
            Self::Perplexity => "sonar-pro",
            Self::Xai => "grok-4-1-fast-reasoning",
            Self::Google => "gemini-2.5-flash",
            Self::Custom => "default",
        }
    }

    /// Parse a provider name string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "perplexity" | "pplx" | "sonar" => Some(Self::Perplexity),
            "xai" | "grok" | "x" => Some(Self::Xai),
            "google" | "gemini" => Some(Self::Google),
            _ => None,
        }
    }
}

/// A generic LLM provider for any OpenAI-compatible API.
pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: String,
    model: String,
    provider_type: CompatibleProvider,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    /// Create a provider for a known compatible API.
    pub fn new(provider: CompatibleProvider, api_key: String) -> anyhow::Result<Self> {
        Self::with_config(
            provider,
            provider.base_url().to_string(),
            api_key,
            provider.default_model().to_string(),
        )
    }

    /// Create a provider with custom configuration.
    pub fn with_config(
        provider: CompatibleProvider,
        base_url: String,
        api_key: String,
        model: String,
    ) -> anyhow::Result<Self> {
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            anyhow::bail!("API key is empty");
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            base_url,
            api_key,
            model,
            provider_type: provider,
            client,
        })
    }

    /// Get the provider type.
    pub fn provider_type(&self) -> CompatibleProvider {
        self.provider_type
    }

    /// Build the chat completions URL.
    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Build messages for the request.
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

    /// Build tool definitions for the request.
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

// ── API Types ────────────────────────────────────────────────────

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

// SSE streaming types
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

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let model = request.model_override.as_deref().unwrap_or(&self.model);
        tracing::info!(
            "OpenAiCompatible[{}]::complete model={}, messages={}, url={}",
            self.provider_type,
            model,
            request.messages.len(),
            self.completions_url(),
        );
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: model.to_string(),
            messages,
            max_tokens: Some(4096),
            tools,
            stream: false,
        };

        let mut req = self
            .client
            .post(self.completions_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        if self.provider_type == CompatibleProvider::Google {
            req = req.header("x-goog-api-key", self.api_key.clone());
        }

        let response = req.json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("{} API error ({}): {}", self.provider_type, status, text);
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
        tracing::info!(
            "OpenAiCompatible[{}]::stream model={}, messages={}, url={}",
            self.provider_type,
            model,
            request.messages.len(),
            self.completions_url(),
        );
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: model.to_string(),
            messages,
            max_tokens: Some(4096),
            tools,
            stream: true,
        };

        let mut req = self
            .client
            .post(self.completions_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        if self.provider_type == CompatibleProvider::Google {
            req = req.header("x-goog-api-key", self.api_key.clone());
        }

        let response = req.json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("{} API error ({}): {}", self.provider_type, status, text);
        }

        let mut full_content = String::new();
        let mut tool_call_map: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();
        let mut byte_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut receiver_closed = false;

        'stream_loop: while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE lines
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
                    if let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(ref content) = choice.delta.content {
                                full_content.push_str(content);
                                if tx.send(StreamEvent::Token(content.clone())).await.is_err() {
                                    tracing::warn!(
                                        "OpenAI-compatible stream receiver closed while sending token; terminating stream early"
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
            tracing::warn!(
                "OpenAI-compatible stream receiver closed while sending completion event"
            );
        } else if receiver_closed {
            tracing::debug!("OpenAI-compatible stream ended after receiver disconnect");
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::Message;

    #[test]
    fn test_compatible_provider_from_name() {
        assert_eq!(
            CompatibleProvider::from_name("perplexity"),
            Some(CompatibleProvider::Perplexity)
        );
        assert_eq!(
            CompatibleProvider::from_name("xai"),
            Some(CompatibleProvider::Xai)
        );
        assert_eq!(
            CompatibleProvider::from_name("grok"),
            Some(CompatibleProvider::Xai)
        );
        assert_eq!(
            CompatibleProvider::from_name("google"),
            Some(CompatibleProvider::Google)
        );
        assert_eq!(
            CompatibleProvider::from_name("gemini"),
            Some(CompatibleProvider::Google)
        );
        assert_eq!(CompatibleProvider::from_name("unknown"), None);
    }

    #[test]
    fn test_provider_base_urls() {
        assert!(CompatibleProvider::Perplexity
            .base_url()
            .contains("perplexity"));
        assert!(CompatibleProvider::Xai.base_url().contains("x.ai"));
        assert!(CompatibleProvider::Google.base_url().contains("google"));
    }

    #[test]
    fn test_provider_default_models() {
        assert_eq!(CompatibleProvider::Perplexity.default_model(), "sonar-pro");
        assert_eq!(
            CompatibleProvider::Xai.default_model(),
            "grok-4-1-fast-reasoning"
        );
        assert_eq!(
            CompatibleProvider::Google.default_model(),
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn test_build_messages() {
        let messages = vec![
            Message::new("system", "You are helpful."),
            Message::new("user", "Hello"),
        ];
        let built = OpenAiCompatibleProvider::build_messages(&messages);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0].role, "system");
        assert_eq!(built[1].role, "user");
    }

    #[test]
    fn test_completions_url() {
        let provider =
            OpenAiCompatibleProvider::new(CompatibleProvider::Perplexity, "test-key".to_string())
                .unwrap();
        assert_eq!(
            provider.completions_url(),
            "https://api.perplexity.ai/chat/completions"
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(CompatibleProvider::Perplexity.to_string(), "perplexity");
        assert_eq!(CompatibleProvider::Xai.to_string(), "xai");
        assert_eq!(CompatibleProvider::Google.to_string(), "google");
    }

    #[test]
    fn test_rejects_empty_api_key() {
        let provider =
            OpenAiCompatibleProvider::new(CompatibleProvider::Perplexity, "   ".to_string());
        assert!(provider.is_err());
    }
}
