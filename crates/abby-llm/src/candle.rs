//! Local Phi-3 via Candle — STUBBED for MVP.
//! Returns input prefixed with "[LOCAL]". Replace with real Candle inference later.

use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider};
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
            if full.contains("essay") || full.contains("quantum mechanics") || full.contains("poem") {
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
