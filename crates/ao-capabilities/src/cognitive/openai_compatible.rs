//! Generic OpenAI-compatible HTTP provider for cloud LLM APIs.
//!
//! Works with any API that follows the OpenAI chat completions format:
//! - Perplexity (sonar, sonar-pro)
//! - xAI Grok (grok-2, grok-3)
//! - Google Gemini (gemini-2.0-flash, gemini-2.5-pro)
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

    /// Default model for this provider.
    pub fn default_model(&self) -> &str {
        match self {
            Self::Perplexity => "sonar",
            Self::Xai => "grok-2-latest",
            Self::Google => "gemini-2.0-flash",
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
    pub fn new(provider: CompatibleProvider, api_key: String) -> Self {
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
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url,
            api_key,
            model,
            provider_type: provider,
            client,
        }
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
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(4096),
            tools,
            stream: false,
        };

        let response = self
            .client
            .post(self.completions_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "{} API error ({}): {}",
                self.provider_type,
                status,
                text
            );
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
        let messages = Self::build_messages(&request.messages);
        let tools = request.tools.as_ref().map(|t| Self::build_tools(t));

        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: Some(4096),
            tools,
            stream: true,
        };

        let response = self
            .client
            .post(self.completions_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "{} API error ({}): {}",
                self.provider_type,
                status,
                text
            );
        }

        let mut full_content = String::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            let text = String::from_utf8_lossy(&bytes);

            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || !line.starts_with("data: ") {
                    continue;
                }
                let json_str = &line[6..];
                if json_str == "[DONE]" {
                    break;
                }
                if let Ok(chunk) = serde_json::from_str::<ChatStreamChunk>(json_str) {
                    if let Some(choice) = chunk.choices.first() {
                        if let Some(ref content) = choice.delta.content {
                            full_content.push_str(content);
                            let _ = tx.send(StreamEvent::Token(content.clone())).await;
                        }
                    }
                }
            }
        }

        let response = CompletionResponse {
            content: full_content,
            tool_calls: None,
        };
        let _ = tx.send(StreamEvent::Done(response.clone())).await;
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
        assert!(CompatibleProvider::Perplexity.base_url().contains("perplexity"));
        assert!(CompatibleProvider::Xai.base_url().contains("x.ai"));
        assert!(CompatibleProvider::Google.base_url().contains("google"));
    }

    #[test]
    fn test_provider_default_models() {
        assert_eq!(CompatibleProvider::Perplexity.default_model(), "sonar");
        assert!(CompatibleProvider::Xai.default_model().contains("grok"));
        assert!(CompatibleProvider::Google.default_model().contains("gemini"));
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
        let provider = OpenAiCompatibleProvider::new(
            CompatibleProvider::Perplexity,
            "test-key".to_string(),
        );
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
}
