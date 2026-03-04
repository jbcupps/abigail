//! Core Skill trait and shared types.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;

use crate::manifest::{CapabilityDescriptor, SkillManifest};
use crate::sandbox::ResourceLimits;

pub type SkillResult<T> = Result<T, SkillError>;

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("Skill not found: {0}")]
    NotFound(crate::manifest::SkillId),
    #[error("Initialization failed: {0}")]
    InitFailed(String),
    #[error("Tool execution failed: {0}")]
    ToolFailed(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Missing secret: {0}")]
    MissingSecret(String),
    #[error("Confirmation required: {0}")]
    ConfirmationRequired(String),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    pub values: HashMap<String, serde_json::Value>,
    pub secrets: HashMap<String, String>,
    pub limits: ResourceLimits,
    pub permissions: Vec<crate::manifest::Permission>,
    /// Optional stream broker so the skill can publish events (e.g. email_received).
    #[serde(skip, default)]
    pub stream_broker: Option<std::sync::Arc<dyn abigail_streaming::StreamBroker>>,
}

impl std::fmt::Debug for SkillConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillConfig")
            .field("values", &self.values)
            .field("secrets", &format!("[{} keys]", self.secrets.len()))
            .field("limits", &self.limits)
            .field("permissions", &self.permissions)
            .field("stream_broker", &self.stream_broker.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHealth {
    pub status: HealthStatus,
    pub message: Option<String>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub metrics: HashMap<String, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub returns: serde_json::Value,
    pub cost_estimate: CostEstimate,
    pub required_permissions: Vec<crate::manifest::Permission>,
    pub autonomous: bool,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostEstimate {
    pub latency_ms: u64,
    pub network_bound: bool,
    pub token_cost: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolParams {
    pub values: HashMap<String, serde_json::Value>,
}

impl ToolParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with<T: Serialize>(mut self, key: &str, value: T) -> Self {
        self.values
            .insert(key.to_string(), serde_json::to_value(value).unwrap());
        self
    }

    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
        self.values.get(key).and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| {
                    tracing::warn!(
                        "ToolParams::get(\"{}\") deserialization failed: {} (value type: {})",
                        key,
                        e,
                        match v {
                            serde_json::Value::Null => "null",
                            serde_json::Value::Bool(_) => "bool",
                            serde_json::Value::Number(_) => "number",
                            serde_json::Value::String(_) => "string",
                            serde_json::Value::Array(_) => "array",
                            serde_json::Value::Object(_) => "object",
                        }
                    );
                    e
                })
                .ok()
        })
    }

    /// Leniently extract a string value, converting non-string JSON types:
    /// - `String` → returned as-is
    /// - `Number` / `Bool` → `.to_string()`
    /// - `Object` / `Array` → JSON-serialized string
    /// - `Null` → `None`
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Null => None,
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            other => Some(other.to_string()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub metadata: ToolMetadata,
}

impl ToolOutput {
    pub fn success<T: Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: Some(serde_json::to_value(data).unwrap()),
            error: None,
            metadata: ToolMetadata::default(),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
            metadata: ToolMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub latency_ms: Option<u64>,
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub request_id: String,
    pub user_id: Option<String>,
}

/// Core skill trait — all skills must implement this.
#[async_trait]
pub trait Skill: Send + Sync {
    fn manifest(&self) -> &SkillManifest;

    async fn initialize(&mut self, config: SkillConfig) -> SkillResult<()>;

    async fn shutdown(&mut self) -> SkillResult<()>;

    fn health(&self) -> SkillHealth;

    fn tools(&self) -> Vec<ToolDescriptor>;

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        context: &ExecutionContext,
    ) -> SkillResult<ToolOutput>;

    fn capabilities(&self) -> Vec<CapabilityDescriptor>;

    fn get_capability(&self, cap_type: &str) -> Option<&dyn Any>;

    fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor>;
}
