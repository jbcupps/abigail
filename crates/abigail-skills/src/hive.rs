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
                description: "Birth a new Entity with the given name.".to_string(),
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
                description: "Switch the active consciousness to a different Entity.".to_string(),
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
                    .map_err(SkillError::ToolFailed)?;
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
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(serde_json::json!(id)))
            }
            "switch_entity" => {
                let id: String = params
                    .get("id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'id'".to_string()))?;
                self.ops
                    .load_agent(&id)
                    .await
                    .map_err(SkillError::ToolFailed)?;
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
                    .map_err(SkillError::ToolFailed)?;
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
                    .map_err(SkillError::ToolFailed)?;
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
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(serde_json::json!(true)))
            }
            "list_secrets" => {
                let names = self
                    .ops
                    .get_skill_secret_names()
                    .await
                    .map_err(SkillError::ToolFailed)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::{ExecutionContext, Skill, SkillError, ToolParams};
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockHiveOps {
        agents: Mutex<Vec<HiveAgentInfo>>,
        active_agent_id: Mutex<Option<String>>,
        config: Mutex<HashMap<String, serde_json::Value>>,
        secrets: Mutex<HashMap<String, String>>,
    }

    #[async_trait]
    impl HiveOperations for MockHiveOps {
        async fn list_agents(&self) -> Result<Vec<HiveAgentInfo>, String> {
            Ok(self.agents.lock().unwrap().clone())
        }

        async fn load_agent(&self, agent_id: &str) -> Result<(), String> {
            let exists = self
                .agents
                .lock()
                .unwrap()
                .iter()
                .any(|a| a.id == agent_id);
            if !exists {
                return Err(format!("unknown agent: {}", agent_id));
            }
            *self.active_agent_id.lock().unwrap() = Some(agent_id.to_string());
            Ok(())
        }

        async fn create_agent(&self, name: &str) -> Result<String, String> {
            let id = format!("agent-{}", name.to_lowercase().replace(' ', "-"));
            self.agents.lock().unwrap().push(HiveAgentInfo {
                id: id.clone(),
                name: name.to_string(),
            });
            *self.active_agent_id.lock().unwrap() = Some(id.clone());
            Ok(id)
        }

        async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
            Ok(self.active_agent_id.lock().unwrap().clone())
        }

        async fn get_config_value(&self, key: &str) -> Result<serde_json::Value, String> {
            self.config
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(|| format!("missing config key: {}", key))
        }

        async fn set_config_value(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
            self.config.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }

        async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String> {
            self.secrets
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        async fn get_skill_secret_names(&self) -> Result<Vec<String>, String> {
            let mut names = self
                .secrets
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort();
            Ok(names)
        }
    }

    fn test_context() -> ExecutionContext {
        ExecutionContext {
            request_id: "loop2-test-request".to_string(),
            user_id: Some("mentor".to_string()),
        }
    }

    #[tokio::test]
    async fn hive_management_skill_entity_and_config_tools_work() {
        let ops = Arc::new(MockHiveOps::default());
        let skill = HiveManagementSkill::new(ops.clone());
        let ctx = test_context();

        let create_out = skill
            .execute_tool(
                "create_entity",
                ToolParams::new().with("name", "Nova"),
                &ctx,
            )
            .await
            .unwrap();
        let created_id: String = serde_json::from_value(create_out.data.unwrap()).unwrap();
        assert_eq!(created_id, "agent-nova");

        let list_out = skill
            .execute_tool("list_entities", ToolParams::new(), &ctx)
            .await
            .unwrap();
        let entities: Vec<HiveAgentInfo> = serde_json::from_value(list_out.data.unwrap()).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].id, "agent-nova");

        let switch_out = skill
            .execute_tool(
                "switch_entity",
                ToolParams::new().with("id", "agent-nova"),
                &ctx,
            )
            .await
            .unwrap();
        assert!(switch_out.success);

        let set_out = skill
            .execute_tool(
                "set_config",
                ToolParams::new()
                    .with("key", "primary_color")
                    .with("value", "#00ffaa"),
                &ctx,
            )
            .await
            .unwrap();
        assert!(set_out.success);

        let get_out = skill
            .execute_tool(
                "get_config",
                ToolParams::new().with("key", "primary_color"),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(get_out.data.unwrap(), serde_json::json!("#00ffaa"));
    }

    #[tokio::test]
    async fn hive_management_skill_secret_tools_work() {
        let ops = Arc::new(MockHiveOps::default());
        let skill = HiveManagementSkill::new(ops.clone());
        let ctx = test_context();

        let store_a = skill
            .execute_tool(
                "store_secret",
                ToolParams::new()
                    .with("key", "OPENAI_API_KEY")
                    .with("value", "sk-test-1"),
                &ctx,
            )
            .await
            .unwrap();
        assert!(store_a.success);

        let store_b = skill
            .execute_tool(
                "store_secret",
                ToolParams::new()
                    .with("key", "ANTHROPIC_API_KEY")
                    .with("value", "sk-ant-test-2"),
                &ctx,
            )
            .await
            .unwrap();
        assert!(store_b.success);

        let list = skill
            .execute_tool("list_secrets", ToolParams::new(), &ctx)
            .await
            .unwrap();
        let names: Vec<String> = serde_json::from_value(list.data.unwrap()).unwrap();
        assert_eq!(names, vec!["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]);
    }

    #[tokio::test]
    async fn hive_management_skill_unknown_tool_errors() {
        let ops = Arc::new(MockHiveOps::default());
        let skill = HiveManagementSkill::new(ops);
        let err = skill
            .execute_tool("not_a_real_tool", ToolParams::new(), &test_context())
            .await
            .unwrap_err();

        match err {
            SkillError::ToolFailed(message) => {
                assert!(message.contains("Unknown tool"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }
}
