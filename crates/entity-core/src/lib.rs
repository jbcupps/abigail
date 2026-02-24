//! Entity API contracts — pure DTO types shared between entity-daemon and CLI clients.

use serde::{Deserialize, Serialize};

// Re-export the shared envelope from hive-core.
pub use hive_core::ApiEnvelope;

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

/// Chat request sent to the Entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    /// Optional target: "EGO", "ID", or None (router decides).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Optional prior messages for multi-turn context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_messages: Option<Vec<SessionMessage>>,
}

/// A single message in a chat session (for multi-turn context).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
}

/// Chat response from the Entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub reply: String,
    /// Which provider handled the request ("id", "ego", provider name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Tool calls executed during this chat turn (empty if none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls_made: Vec<ToolCallRecord>,
    /// Model quality tier used: "fast", "standard", or "pro".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Actual model ID used for this request (e.g. "gpt-4.1", "claude-sonnet-4-6").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    /// Complexity score (5–95) that determined the tier selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity_score: Option<u8>,
}

/// Record of a single tool call made during a chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub skill_id: String,
    pub tool_name: String,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Entity status
// ---------------------------------------------------------------------------

/// Entity runtime status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityStatus {
    pub entity_id: String,
    pub name: Option<String>,
    pub birth_complete: bool,
    pub has_ego: bool,
    pub ego_provider: Option<String>,
    pub routing_mode: String,
    pub skills_count: usize,
}

// ---------------------------------------------------------------------------
// Skills / Tools
// ---------------------------------------------------------------------------

/// Summary info about a loaded skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub tools: Vec<ToolInfo>,
}

/// Summary info about a tool within a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub autonomous: bool,
}

/// Request to execute a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecRequest {
    pub skill_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecResponse {
    pub success: bool,
    pub output: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// Request to search memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default = "default_memory_limit")]
    pub limit: usize,
}

fn default_memory_limit() -> usize {
    10
}

/// Request to insert a memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInsertRequest {
    pub content: String,
    /// Weight tier: "ephemeral", "distilled", or "crystallized".
    #[serde(default = "default_memory_weight")]
    pub weight: String,
}

fn default_memory_weight() -> String {
    "ephemeral".to_string()
}

/// A single memory entry returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub weight: String,
    pub created_at: String,
}

/// Memory store statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub memory_count: u64,
    pub has_birth: bool,
}

// ---------------------------------------------------------------------------
// Chat memory hook (for future Hive / Superego integration)
// ---------------------------------------------------------------------------

/// Hook invoked when the entity persists a chat memory.
///
/// The only extension point for future Hive-side policy (e.g. Superego) is at
/// chat memory: when a memory is written, this hook is called. The Hive can
/// later implement this to audit, replicate, or apply alignment checks.
#[allow(dead_code)]
pub trait ChatMemoryHook: Send + Sync {
    /// Called after a memory has been persisted. Default is a no-op.
    fn on_memory_persisted(
        &self,
        id: &str,
        content: &str,
        weight: &str,
        created_at: &str,
    ) -> Result<(), String> {
        let _ = (id, content, weight, created_at);
        Ok(())
    }
}
