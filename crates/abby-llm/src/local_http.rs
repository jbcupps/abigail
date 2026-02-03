//! LiteLLM / OpenAI-compatible HTTP provider for local LLM servers.
//!
//! Connects to a local LLM server (LiteLLM, Ollama, LM Studio, etc.) via
//! the OpenAI-compatible chat completions API.

use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider, Message};
use async_trait::async_trait;
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
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// OpenAI-compatible chat completion response.
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
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
    pub fn with_url(base_url: impl Into<String>) -> Self {
        Self::new(base_url, "local-model")
    }

    /// Perform a heartbeat check to verify the LLM server is reachable.
    /// Sends a minimal completion request and checks for a valid response.
    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        let request = CompletionRequest {
            messages: vec![Message {
                role: "user".into(),
                content: "ping".into(),
            }],
        };

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
            }],
            max_tokens: Some(10), // Minimal response for heartbeat
        };

        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));

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
}

#[async_trait]
impl LlmProvider for LocalHttpProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let chat_request = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: None,
        };

        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));

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
            return Err(anyhow::anyhow!("LLM request failed: HTTP {} - {}", status, body));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("LLM response parse failed: {}", e))?;

        let content = chat_response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(CompletionResponse { content })
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
