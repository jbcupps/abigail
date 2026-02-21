use crate::channel::TriggerDescriptor;
use crate::manifest::CapabilityDescriptor;
use crate::manifest::{SkillId, SkillManifest};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiveAgentInfo {
    pub id: String,
    pub name: String,
}

#[async_trait]
pub trait HiveOperations: Send + Sync {
    async fn list_agents(&self) -> Result<Vec<HiveAgentInfo>, String>;
    async fn load_agent(&self, agent_id: &str) -> Result<(), String>;
    async fn create_agent(&self, name: &str) -> Result<String, String>;
    async fn get_active_agent_id(&self) -> Result<Option<String>, String>;

    // Config operations (filtered to exclude Superego)
    async fn get_config_value(&self, key: &str) -> Result<serde_json::Value, String>;
    async fn set_config_value(&self, key: &str, value: serde_json::Value) -> Result<(), String>;

    // Secret management (for Skills Vault)
    async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String>;
    async fn get_skill_secret_names(&self) -> Result<Vec<String>, String>;
}

pub struct HiveManagementSkill {
    manifest: SkillManifest,
    ops: Arc<dyn HiveOperations>,
}

impl HiveManagementSkill {
    pub fn new(ops: Arc<dyn HiveOperations>) -> Self {
        let manifest = SkillManifest {
            id: SkillId("builtin.hive_management".to_string()),
            name: "Hive Management".to_string(),
            version: "0.1.0".to_string(),
            description: "Manage Sovereign Entities, agents, and Hive configuration.".to_string(),
            license: Some("MIT".to_string()),
            category: "System".to_string(),
            keywords: vec![
                "hive".to_string(),
                "identity".to_string(),
                "config".to_string(),
            ],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![],
            secrets: vec![],
            config_defaults: HashMap::new(),
        };

        Self { manifest, ops }
    }
}

#[async_trait]
impl Skill for HiveManagementSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        SkillHealth {
            status: HealthStatus::Healthy,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "list_entities".to_string(),
                description: "List all Sovereign Entities (agents) registered in this Hive.".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" }
                        }
                    }
                }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "create_entity".to_string(),
                description: "Birth a new Sovereign Entity with the given name.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "The name of the new entity" }
                    },
                    "required": ["name"]
                }),
                returns: serde_json::json!({ "type": "string", "description": "The ID of the new entity" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "switch_entity".to_string(),
                description: "Switch the active consciousness to a different Sovereign Entity.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the entity to switch to" }
                    },
                    "required": ["id"]
                }),
                returns: serde_json::json!({ "type": "boolean" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "get_config".to_string(),
                description: "Read a non-sensitive configuration value from the Hive.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The configuration key (e.g., 'primary_color', 'agent_name')" }
                    },
                    "required": ["key"]
                }),
                returns: serde_json::json!({ "type": "any" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "set_config".to_string(),
                description: "Update a non-sensitive configuration value in the Hive.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The configuration key" },
                        "value": { "type": "any", "description": "The new value" }
                    },
                    "required": ["key", "value"]
                }),
                returns: serde_json::json!({ "type": "boolean" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "store_secret".to_string(),
                description: "Store an operational secret (e.g. API key, password) in the Skills Vault. The value is write-only and cannot be read back by the Entity directly.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The secret identifier (e.g., 'proton_mail_password')" },
                        "value": { "type": "string", "description": "The secret value to store" }
                    },
                    "required": ["key", "value"]
                }),
                returns: serde_json::json!({ "type": "boolean" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "list_secrets".to_string(),
                description: "List the names of all secrets currently stored in the Skills Vault.".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({ "type": "array", "items": { "type": "string" } }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "list_entities" => {
                let agents = self
                    .ops
                    .list_agents()
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::to_value(agents).unwrap()))
            }
            "create_entity" => {
                let name: String = params
                    .get("name")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'name'".to_string()))?;
                let id = self
                    .ops
                    .create_agent(&name)
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::json!(id)))
            }
            "switch_entity" => {
                let id: String = params
                    .get("id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'id'".to_string()))?;
                self.ops
                    .load_agent(&id)
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::json!(true)))
            }
            "get_config" => {
                let key: String = params
                    .get("key")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'key'".to_string()))?;
                let val = self
                    .ops
                    .get_config_value(&key)
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(val))
            }
            "set_config" => {
                let key: String = params
                    .get("key")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'key'".to_string()))?;
                let value = params
                    .values
                    .get("value")
                    .cloned()
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'value'".to_string()))?;
                self.ops
                    .set_config_value(&key, value)
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::json!(true)))
            }
            "store_secret" => {
                let key: String = params
                    .get("key")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'key'".to_string()))?;
                let value: String = params
                    .get("value")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'value'".to_string()))?;
                self.ops
                    .set_skill_secret(&key, &value)
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::json!(true)))
            }
            "list_secrets" => {
                let names = self
                    .ops
                    .get_skill_secret_names()
                    .await
                    .map_err(|e| SkillError::ToolFailed(e))?;
                Ok(ToolOutput::success(serde_json::to_value(names).unwrap()))
            }
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}
