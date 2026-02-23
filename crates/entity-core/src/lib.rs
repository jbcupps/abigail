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
