//! Rich cognitive types — supplementary structs for LLM provider metadata.
//! Migrated from abigail-skills capability/llm.rs.

/// Information about an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelInfo>,
}

/// Information about a specific model.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_size: Option<u32>,
}

/// Token usage statistics from an LLM completion.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
