//! Local Phi-3 via Candle — STUBBED for MVP.
//! Handles classification for router. Returns a helpful configuration message for
//! actual chat requests (no local LLM configured). Replace with real Candle inference later.

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
        tracing::warn!(
            "CandleProvider::complete (STUB) called with {} messages — no real LLM backing this provider",
            request.messages.len(),
        );
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

        Err(anyhow::anyhow!("No language model is connected. Add a cloud model or connect a running local model in Abigail Hive."))
    }
}
