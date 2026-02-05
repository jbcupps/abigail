//! Skill execution engine.

use std::sync::Arc;
use uuid::Uuid;

use crate::manifest::{NetworkPermission, Permission, SkillId};
use crate::registry::SkillRegistry;
use crate::sandbox::{AuditAction, AuditActionKind, ResourceLimits, SkillSandbox};
use crate::skill::{ExecutionContext, SkillError, SkillResult, ToolParams, ToolOutput};

pub struct SkillExecutor {
    pub registry: Arc<SkillRegistry>,
}

impl SkillExecutor {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }

    /// Build the appropriate audit action for a tool based on its required_permissions (e.g. network domain).
    fn audit_action_for_tool(
        _tool_name: &str,
        required_permissions: &[Permission],
    ) -> Option<AuditAction> {
        for p in required_permissions {
            if let Permission::Network(np) = p {
                let domain = match np {
                    NetworkPermission::Full => "any".to_string(),
                    NetworkPermission::LocalOnly => "localhost".to_string(),
                    NetworkPermission::Domains(domains) => {
                        domains.first().cloned().unwrap_or_else(|| "unknown".into())
                    }
                };
                return Some(AuditAction {
                    kind: AuditActionKind::NetworkRequest { domain },
                });
            }
        }
        None
    }

    pub async fn execute(
        &self,
        skill_id: &SkillId,
        tool_name: &str,
        params: ToolParams,
    ) -> SkillResult<ToolOutput> {
        let (skill, manifest) = self.registry.get_skill(skill_id)?;

        let tool = skill
            .tools()
            .into_iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| SkillError::ToolFailed(format!("Unknown tool: {}", tool_name)))?;

        let mut sandbox = SkillSandbox::new(
            manifest.id.clone(),
            manifest.permissions.clone(),
            ResourceLimits::default(),
        );
        if let Some(action) = Self::audit_action_for_tool(tool_name, &tool.required_permissions) {
            if !sandbox.check_permission(&action) {
                return Err(SkillError::PermissionDenied(format!(
                    "Tool {} requires permission that is not granted for this skill",
                    tool_name
                )));
            }
        }

        let context = ExecutionContext {
            request_id: Uuid::new_v4().to_string(),
            user_id: None,
        };

        skill
            .execute_tool(tool_name, params, &context)
            .await
    }
}
