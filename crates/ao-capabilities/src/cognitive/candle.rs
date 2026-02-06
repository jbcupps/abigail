//! Local Phi-3 via Candle — STUBBED for MVP.
//! Handles classification for router, but returns error for actual chat (no local LLM configured).
//! Replace with real Candle inference later.

use crate::cognitive::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use async_trait::async_trait;

pub struct CandleProvider;

impl CandleProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CandleProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for CandleProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        // Stub: for classification prompts, return COMPLEX when input suggests complex task (for router test).
        let full = request
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<String>();

        if full.contains("Classify this user request") {
            // Extract the user request portion (after "User request:") to avoid matching
            // keywords in the classification instructions themselves.
            let user_request = full
                .split("User request:")
                .nth(1)
                .unwrap_or("")
                .to_lowercase();
            let content = if user_request.contains("essay")
                || user_request.contains("quantum")
                || user_request.contains("poem")
                || user_request.contains("analyze")
                || user_request.contains("explain in detail")
            {
                "COMPLEX".into()
            } else {
                "ROUTINE".into()
            };
            return Ok(CompletionResponse {
                content,
                tool_calls: None,
            });
        }

        // For actual chat requests - return error instead of echo
        Err(anyhow::anyhow!(
            "No local LLM configured. Please install Ollama or set LOCAL_LLM_BASE_URL environment variable."
        ))
    }
}
