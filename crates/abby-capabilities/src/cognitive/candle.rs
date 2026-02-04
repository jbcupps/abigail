//! Local Phi-3 via Candle — STUBBED for MVP.
//! Returns input prefixed with "[LOCAL]". Replace with real Candle inference later.

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
        let last = request
            .messages
            .last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        // Stub: for classification prompts, return COMPLEX when input suggests complex task (for router test).
        let full = request.messages.iter().map(|m| m.content.as_str()).collect::<String>();
        let content = if full.contains("Classify this user request") {
            // Extract the user request portion (after "User request:") to avoid matching
            // keywords in the classification instructions themselves.
            let user_request = full.split("User request:").nth(1).unwrap_or("").to_lowercase();
            if user_request.contains("essay") || user_request.contains("quantum") || user_request.contains("poem") || user_request.contains("analyze") || user_request.contains("explain in detail") {
                "COMPLEX".into()
            } else {
                "ROUTINE".into()
            }
        } else {
            format!("[LOCAL] {}", last)
        };
        Ok(CompletionResponse { content })
    }
}
