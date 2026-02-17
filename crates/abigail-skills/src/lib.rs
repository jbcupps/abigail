//! Abigail Skills — plugin and tool execution layer.

pub mod channel;
pub mod dynamic;
pub mod executor;
pub mod manifest;
pub mod prelude;
pub mod protocol;
pub mod registry;
pub mod runtime;
pub mod sandbox;
pub mod skill;
pub mod transport;
pub mod watcher;

/// Backward-compatible alias: `capability` now lives in `protocol`.
pub use protocol as capability;

pub use channel::*;
pub use dynamic::{DynamicApiSkill, DynamicSkillConfig, DynamicToolConfig};
pub use executor::SkillExecutor;
pub use manifest::*;
pub use prelude::*;
pub use protocol::*;
pub use registry::{MissingSkillSecret, RegisteredSkill, SkillRegistry};
pub use sandbox::*;
pub use skill::*;
pub use watcher::{SkillFileEvent, SkillsWatcher};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SkillId;
    use crate::manifest::SkillManifest;
    use crate::skill::{SkillConfig, SkillHealth, ToolDescriptor, ToolOutput, ToolParams};
    use std::collections::HashMap;
    use std::sync::Arc;

    struct NoOpSkill {
        manifest: SkillManifest,
    }

    #[async_trait::async_trait]
    impl Skill for NoOpSkill {
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
            vec![ToolDescriptor {
                name: "noop".to_string(),
                description: "No-op tool".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            }]
        }

        async fn execute_tool(
            &self,
            tool_name: &str,
            _params: ToolParams,
            _context: &ExecutionContext,
        ) -> SkillResult<ToolOutput> {
            if tool_name == "noop" {
                Ok(ToolOutput::success(serde_json::json!({"ok": true})))
            } else {
                Err(SkillError::ToolFailed(format!("Unknown: {}", tool_name)))
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

    #[tokio::test]
    async fn test_register_and_execute_tool() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.noop".to_string());
        let manifest = SkillManifest {
            id: skill_id.clone(),
            name: "NoOp".to_string(),
            version: "1.0".to_string(),
            description: "Test".to_string(),
            license: None,
            category: "Test".to_string(),
            keywords: vec![],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![],
            secrets: vec![],
            config_defaults: HashMap::new(),
        };
        let skill = NoOpSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let list = registry.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id.0, "test.noop");

        let executor = SkillExecutor::new(registry);
        let out = executor
            .execute(&skill_id, "noop", ToolParams::new())
            .await
            .unwrap();
        assert!(out.success);
        assert!(out.data.is_some());
    }
}
