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
    /// Compatibility field — prefer `execution_trace` for authoritative attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Tool calls executed during this chat turn (empty if none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls_made: Vec<ToolCallRecord>,
    /// Model quality tier used: "fast", "standard", or "pro".
    /// Compatibility field — prefer `execution_trace` for authoritative attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Actual model ID used for this request (e.g. "gpt-4.1", "claude-sonnet-4-6").
    /// Compatibility field — prefer `execution_trace` for authoritative attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_used: Option<String>,
    /// Complexity score (5–95) that determined the tier selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity_score: Option<u8>,
    /// Authoritative per-turn execution trace. Single source of truth for
    /// which provider/model actually generated this response, including
    /// fallback chain and timing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_trace: Option<ExecutionTrace>,
}

/// Record of a single tool call made during a chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub skill_id: String,
    pub tool_name: String,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Execution Trace — authoritative per-turn telemetry
// ---------------------------------------------------------------------------

/// Outcome of a single provider call attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepResult {
    Success,
    Error,
}

/// One hop in the execution chain (primary attempt or fallback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    /// Human-readable provider label (e.g. "openai", "anthropic", "id", "candle_stub").
    pub provider_label: String,
    /// Model ID sent in the request (may be None for local/stub providers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_requested: Option<String>,
    /// Model ID reported by the provider in the response (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_reported: Option<String>,
    pub result: StepResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    pub started_at_utc: String,
    pub ended_at_utc: String,
}

/// Why a particular tier/model was selected for this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    /// Complexity score mapped to a tier via thresholds.
    Complexity,
    /// Force override pinned a specific tier.
    PinnedTier,
    /// Force override pinned a specific model.
    PinnedModel,
    /// Setup/credential intent auto-escalated to Pro.
    SetupIntent,
    /// Ego-primary mode (no tier logic).
    EgoPrimary,
    /// Council mode (multi-provider deliberation).
    Council,
    /// Fallback after primary provider failed.
    Fallback,
}

impl std::fmt::Display for SelectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionReason::Complexity => write!(f, "complexity"),
            SelectionReason::PinnedTier => write!(f, "pinned_tier"),
            SelectionReason::PinnedModel => write!(f, "pinned_model"),
            SelectionReason::SetupIntent => write!(f, "setup_intent"),
            SelectionReason::EgoPrimary => write!(f, "ego_primary"),
            SelectionReason::Council => write!(f, "council"),
            SelectionReason::Fallback => write!(f, "fallback"),
        }
    }
}

/// Full execution trace for a single chat turn. This is the single source of
/// truth for attribution — UI and prompt self-awareness should derive facts
/// from this struct, not from legacy compatibility fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    /// Unique identifier for this turn.
    pub turn_id: String,
    /// UTC timestamp when routing began.
    pub timestamp_utc: String,
    /// Routing mode active for this turn (e.g. "tier_based", "ego_primary").
    pub routing_mode: String,
    /// Provider the router was configured to prefer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_provider: Option<String>,
    /// Model the tier/config system resolved before execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_model: Option<String>,
    /// Tier the router intended before execution (e.g. "fast", "standard", "pro").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_tier: Option<String>,
    /// Complexity score (5-95) used for tier selection (None if not tier-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity_score: Option<u8>,
    /// Why this tier/model was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_reason: Option<SelectionReason>,
    /// Which target the fast-path classifier selected ("id" or "ego").
    pub target_selected: String,
    /// Ordered list of provider call attempts (primary + any fallbacks).
    pub steps: Vec<ExecutionStep>,
    /// Index into `steps` of the attempt that produced the final response.
    pub final_step_index: usize,
    /// True when the response came from a fallback, not the primary target.
    pub fallback_occurred: bool,
}

impl ExecutionTrace {
    /// Create a new trace with routing intent populated and an empty step list.
    pub fn new(
        routing_mode: &str,
        configured_provider: Option<String>,
        configured_model: Option<String>,
        target_selected: &str,
    ) -> Self {
        Self {
            turn_id: uuid::Uuid::new_v4().to_string(),
            timestamp_utc: chrono::Utc::now().to_rfc3339(),
            routing_mode: routing_mode.to_string(),
            configured_provider,
            configured_model,
            configured_tier: None,
            complexity_score: None,
            selection_reason: None,
            target_selected: target_selected.to_string(),
            steps: Vec::new(),
            final_step_index: 0,
            fallback_occurred: false,
        }
    }

    /// Record a successful provider call.
    pub fn record_success(
        &mut self,
        provider_label: &str,
        model_requested: Option<String>,
        started_at: chrono::DateTime<chrono::Utc>,
    ) {
        let idx = self.steps.len();
        self.steps.push(ExecutionStep {
            provider_label: provider_label.to_string(),
            model_requested,
            model_reported: None,
            result: StepResult::Success,
            error_summary: None,
            started_at_utc: started_at.to_rfc3339(),
            ended_at_utc: chrono::Utc::now().to_rfc3339(),
        });
        self.final_step_index = idx;
        self.fallback_occurred = idx > 0;
    }

    /// Record a failed provider call (before fallback).
    pub fn record_error(
        &mut self,
        provider_label: &str,
        model_requested: Option<String>,
        error: &str,
        started_at: chrono::DateTime<chrono::Utc>,
    ) {
        self.steps.push(ExecutionStep {
            provider_label: provider_label.to_string(),
            model_requested,
            model_reported: None,
            result: StepResult::Error,
            error_summary: Some(error.to_string()),
            started_at_utc: started_at.to_rfc3339(),
            ended_at_utc: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// The provider label of whichever step produced the final response.
    pub fn final_provider(&self) -> Option<&str> {
        self.steps
            .get(self.final_step_index)
            .map(|s| s.provider_label.as_str())
    }

    /// The model requested in the final successful step.
    pub fn final_model(&self) -> Option<&str> {
        self.steps
            .get(self.final_step_index)
            .and_then(|s| s.model_requested.as_deref())
    }

    /// The tier that was actually used — derived from configured_tier unless
    /// fallback occurred (in which case there's no meaningful tier).
    pub fn final_tier(&self) -> Option<&str> {
        if self.fallback_occurred {
            None
        } else {
            self.configured_tier.as_deref()
        }
    }

    /// Derive the selection reason as a display string.
    pub fn selection_reason_str(&self) -> &str {
        self.selection_reason
            .as_ref()
            .map(|r| match r {
                SelectionReason::Complexity => "complexity",
                SelectionReason::PinnedTier => "pinned_tier",
                SelectionReason::PinnedModel => "pinned_model",
                SelectionReason::SetupIntent => "setup_intent",
                SelectionReason::EgoPrimary => "ego_primary",
                SelectionReason::Council => "council",
                SelectionReason::Fallback => "fallback",
            })
            .unwrap_or("unknown")
    }
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
