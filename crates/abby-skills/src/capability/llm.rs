//! LLM provider capability trait.

use async_trait::async_trait;

use crate::SkillResult;

#[derive(Debug, Clone)]
pub struct LlmProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_size: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub messages: Vec<LlmMessage>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait]
pub trait LlmProviderCapability: Send + Sync {
    fn info(&self) -> LlmProviderInfo;
    async fn complete(&self, request: LlmRequest) -> SkillResult<LlmResponse>;
    async fn embed(&self, texts: Vec<String>) -> SkillResult<Vec<Vec<f32>>>;
    fn models(&self) -> Vec<ModelInfo>;
}
