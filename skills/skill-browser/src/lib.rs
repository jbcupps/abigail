use abigail_capabilities::cognitive::ToolDefinition;
use abigail_capabilities::sensory::browser::{BrowserCapability, BrowserCapabilityConfig};
use abigail_capabilities::sensory::url_security::UrlSecurityPolicy;
use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, NetworkPermission,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;

pub struct BrowserSkill {
    manifest: SkillManifest,
    capability: BrowserCapability,
}

impl BrowserSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse browser skill.toml")
    }

    pub fn new(manifest: SkillManifest) -> Self {
        Self::new_with_local_network(manifest, false)
    }

    pub fn new_with_local_network(manifest: SkillManifest, allow_local_network: bool) -> Self {
        let security_policy = if allow_local_network {
            UrlSecurityPolicy {
                block_private_ips: false,
                ..UrlSecurityPolicy::default()
            }
        } else {
            UrlSecurityPolicy::default()
        };

        Self {
            manifest,
            capability: BrowserCapability::new_with_security_policy(
                BrowserCapabilityConfig::default(),
                security_policy,
            ),
        }
    }

    fn descriptor_from_definition(def: ToolDefinition) -> ToolDescriptor {
        ToolDescriptor {
            name: def.name,
            description: def.description,
            parameters: def.parameters,
            returns: serde_json::json!({
                "type": ["object", "string"]
            }),
            cost_estimate: CostEstimate {
                latency_ms: 3_000,
                network_bound: true,
                token_cost: None,
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Full)],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    fn is_error_result(result: &str) -> bool {
        let lowered = result.trim().to_ascii_lowercase();
        lowered.starts_with("missing required parameter")
            || lowered.starts_with("unknown browser tool")
            || lowered.starts_with("url rejected:")
            || lowered.starts_with("browser not initialized")
            || lowered.starts_with("no active page")
            || lowered.contains(" failed:")
            || lowered.contains(" navigation failed")
    }
}

#[async_trait]
impl Skill for BrowserSkill {
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
        self.capability
            .tool_definitions()
            .into_iter()
            .map(Self::descriptor_from_definition)
            .collect()
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        if !self.tools().iter().any(|tool| tool.name == tool_name) {
            return Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            )));
        }

        let args = serde_json::Value::Object(params.values.into_iter().collect());
        let raw = self.capability.execute_tool(tool_name, &args).await;

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
            return Ok(ToolOutput::success(json));
        }

        if Self::is_error_result(&raw) {
            return Err(SkillError::ToolFailed(raw));
        }

        Ok(ToolOutput::success(raw))
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![CapabilityDescriptor {
            capability_type: "browser_automation".to_string(),
            version: "1.0".to_string(),
        }]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parses() {
        let manifest = BrowserSkill::default_manifest();
        assert_eq!(manifest.id.0, "com.abigail.skills.browser");
    }

    #[test]
    fn test_tools_include_navigation_and_content() {
        let skill = BrowserSkill::new(BrowserSkill::default_manifest());
        let tools = skill.tools();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert!(names.contains(&"browser_navigate"));
        assert!(names.contains(&"browser_get_content"));
        assert!(names.contains(&"browser_close"));
    }
}
