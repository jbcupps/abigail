//! Skill execution engine.
//!
//! Enforces per-call timeouts and global concurrency limits from `ResourceLimits`. File/network
//! I/O must go through capability layers that call `SkillSandbox::check_permission`.

use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use tokio::sync::Semaphore;

use crate::manifest::{FileSystemPermission, NetworkPermission, Permission, SkillId};
use crate::registry::SkillRegistry;
use crate::sandbox::{AuditAction, AuditActionKind, ResourceLimits, SkillSandbox};
use crate::skill::{ExecutionContext, SkillError, SkillResult, ToolOutput, ToolParams};

pub struct SkillExecutor {
    pub registry: Arc<SkillRegistry>,
    /// Limits concurrent tool executions across all skills (from ResourceLimits::max_concurrency).
    concurrency_limiter: Arc<Semaphore>,
    /// Default timeout for a single tool call (from ResourceLimits::max_cpu_ms).
    default_timeout_ms: u64,
}

impl SkillExecutor {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self::with_limits(registry, ResourceLimits::default())
    }

    /// Build executor with custom resource limits (e.g. for tests with short timeouts).
    pub fn with_limits(registry: Arc<SkillRegistry>, limits: ResourceLimits) -> Self {
        Self {
            registry,
            concurrency_limiter: Arc::new(Semaphore::new(limits.max_concurrency as usize)),
            default_timeout_ms: limits.max_cpu_ms,
        }
    }

    /// Build audit actions for a tool based on its required_permissions.
    fn audit_actions_for_tool(
        _tool_name: &str,
        required_permissions: &[Permission],
    ) -> Vec<AuditAction> {
        let mut actions = Vec::new();
        for p in required_permissions {
            match p {
                Permission::Network(np) => {
                    let domain = match np {
                        NetworkPermission::Full => "any".to_string(),
                        NetworkPermission::LocalOnly => "localhost".to_string(),
                        NetworkPermission::Domains(domains) => {
                            domains.first().cloned().unwrap_or_else(|| "unknown".into())
                        }
                    };
                    actions.push(AuditAction {
                        kind: AuditActionKind::NetworkRequest { domain },
                    });
                }
                Permission::FileSystem(fsp) => match fsp {
                    FileSystemPermission::Read(paths) => {
                        let path = paths.first().cloned().unwrap_or_else(|| "unknown".into());
                        actions.push(AuditAction {
                            kind: AuditActionKind::FileRead { path },
                        });
                    }
                    FileSystemPermission::Write(paths) => {
                        let path = paths.first().cloned().unwrap_or_else(|| "unknown".into());
                        actions.push(AuditAction {
                            kind: AuditActionKind::FileWrite { path },
                        });
                    }
                    FileSystemPermission::Full => {
                        actions.push(AuditAction {
                            kind: AuditActionKind::FileRead {
                                path: "/".to_string(),
                            },
                        });
                    }
                },
                _ => {}
            }
        }
        actions
    }

    pub async fn execute(
        &self,
        skill_id: &SkillId,
        tool_name: &str,
        params: ToolParams,
    ) -> SkillResult<ToolOutput> {
        let request_id = Uuid::new_v4().to_string();
        tracing::info!(
            skill_id = %skill_id,
            tool_name = tool_name,
            request_id = %request_id,
            "Executing tool"
        );

        let start = Instant::now();
        let (skill, manifest) = self.registry.get_skill(skill_id)?;

        let tool = skill
            .tools()
            .into_iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| SkillError::ToolFailed(format!("Unknown tool: {}", tool_name)))?;

        let limits = ResourceLimits::default();
        let mut sandbox =
            SkillSandbox::new(manifest.id.clone(), manifest.permissions.clone(), limits);
        let actions = Self::audit_actions_for_tool(tool_name, &tool.required_permissions);
        for action in &actions {
            if !sandbox.check_permission(action) {
                tracing::warn!(
                    skill_id = %skill_id,
                    tool_name = tool_name,
                    action = ?action.kind,
                    "Permission denied"
                );
                return Err(SkillError::PermissionDenied(format!(
                    "Tool {} requires permission that is not granted for this skill",
                    tool_name
                )));
            }
        }

        let _permit = self
            .concurrency_limiter
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| SkillError::ToolFailed("concurrency limiter closed".into()))?;

        let context = ExecutionContext {
            request_id,
            user_id: None,
        };

        let timeout_ms = self.default_timeout_ms;
        let fut = skill.execute_tool(tool_name, params, &context);
        match tokio::time::timeout(Duration::from_millis(timeout_ms), fut).await {
            Ok(Ok(mut out)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                out.metadata.latency_ms = Some(duration_ms);
                tracing::info!(
                    skill_id = %skill_id,
                    tool_name = tool_name,
                    duration_ms = duration_ms,
                    "Tool completed successfully"
                );
                Ok(out)
            }
            Ok(Err(e)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                tracing::error!(
                    skill_id = %skill_id,
                    tool_name = tool_name,
                    duration_ms = duration_ms,
                    error = %e,
                    "Tool execution failed"
                );
                Err(e)
            }
            Err(_) => {
                tracing::error!(
                    skill_id = %skill_id,
                    tool_name = tool_name,
                    timeout_ms = timeout_ms,
                    "Tool exceeded timeout"
                );
                Err(SkillError::ToolFailed(format!(
                    "Tool {} exceeded timeout ({} ms)",
                    tool_name, timeout_ms
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{NetworkPermission, Permission};
    use crate::skill::{HealthStatus, SkillHealth, ToolDescriptor};
    use std::collections::HashMap;

    /// Skill that sleeps longer than the test timeout so executor returns timeout error.
    struct SleepSkill {
        manifest: crate::manifest::SkillManifest,
        sleep_ms: u64,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for SleepSkill {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "sleep".to_string(),
                description: "Sleep".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
            Ok(crate::skill::ToolOutput::success(serde_json::json!({})))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    /// Skill that declares network permission but manifest has no network permission (sandbox denies).
    struct NetworkToolNoPermissionSkill {
        manifest: crate::manifest::SkillManifest,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for NetworkToolNoPermissionSkill {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "fetch".to_string(),
                description: "Fetch".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![Permission::Network(NetworkPermission::Full)],
                autonomous: false,
                requires_confirmation: true,
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            Ok(crate::skill::ToolOutput::success(serde_json::json!({})))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    #[tokio::test]
    async fn executor_enforces_timeout() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.sleep".to_string());
        let manifest = crate::manifest::SkillManifest {
            id: skill_id.clone(),
            name: "Sleep".to_string(),
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
        let skill = SleepSkill {
            manifest,
            sleep_ms: 2000, // well above timeout so CI reliably hits timeout
        };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let limits = ResourceLimits {
            max_cpu_ms: 100, // short so test completes quickly; 2s sleep >> 100ms
            max_concurrency: 2,
            ..ResourceLimits::default()
        };
        let executor = SkillExecutor::with_limits(registry, limits);
        let result = executor
            .execute(&skill_id, "sleep", ToolParams::new())
            .await;
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("exceeded timeout"),
            "expected timeout error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn executor_denies_network_when_not_granted() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.fetch".to_string());
        let manifest = crate::manifest::SkillManifest {
            id: skill_id.clone(),
            name: "Fetch".to_string(),
            version: "1.0".to_string(),
            description: "Test".to_string(),
            license: None,
            category: "Test".to_string(),
            keywords: vec![],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![], // no network permission
            secrets: vec![],
            config_defaults: HashMap::new(),
        };
        let skill = NetworkToolNoPermissionSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(&skill_id, "fetch", ToolParams::new())
            .await;
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("permission") && msg.to_lowercase().contains("denied"),
            "expected permission denied, got: {}",
            err
        );
    }

    // ── New coverage tests ─────────────────────────────────────────

    /// Helper to build a test manifest with given permissions.
    fn test_manifest(id: &str, permissions: Vec<Permission>) -> crate::manifest::SkillManifest {
        crate::manifest::SkillManifest {
            id: SkillId(id.to_string()),
            name: id.to_string(),
            version: "1.0".to_string(),
            description: "Test".to_string(),
            license: None,
            category: "Test".to_string(),
            keywords: vec![],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions,
            secrets: vec![],
            config_defaults: HashMap::new(),
        }
    }

    /// Skill that echoes params back as success output.
    struct EchoSkill {
        manifest: crate::manifest::SkillManifest,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for EchoSkill {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "echo".to_string(),
                description: "Echo params".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            }]
        }
        async fn execute_tool(
            &self,
            _tool_name: &str,
            params: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            Ok(crate::skill::ToolOutput::success(
                serde_json::to_value(&params.values).unwrap_or_default(),
            ))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    /// Skill that always returns a ToolFailed error.
    struct FailingSkill {
        manifest: crate::manifest::SkillManifest,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for FailingSkill {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "fail".to_string(),
                description: "Always fails".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            Err(SkillError::ToolFailed("intentional failure".to_string()))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    /// Skill with a Network(Full) tool and Network(Full) permission granted.
    struct NetworkSkillGranted {
        manifest: crate::manifest::SkillManifest,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for NetworkSkillGranted {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "fetch".to_string(),
                description: "Fetch URL".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![Permission::Network(NetworkPermission::Full)],
                autonomous: false,
                requires_confirmation: false,
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            Ok(crate::skill::ToolOutput::success(
                serde_json::json!({"fetched": true}),
            ))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    #[tokio::test]
    async fn execute_success() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.echo".to_string());
        let manifest = test_manifest("test.echo", vec![]);
        let skill = EchoSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor.execute(&skill_id, "echo", ToolParams::new()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn skill_not_found() {
        let registry = Arc::new(SkillRegistry::new());
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(
                &SkillId("nonexistent".to_string()),
                "tool",
                ToolParams::new(),
            )
            .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            msg.contains("not found") || msg.contains("unknown"),
            "expected not found error, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn tool_not_found() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.echo".to_string());
        let manifest = test_manifest("test.echo", vec![]);
        let skill = EchoSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(&skill_id, "nonexistent_tool", ToolParams::new())
            .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Unknown tool"),
            "expected unknown tool error, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn network_permission_granted() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.net".to_string());
        let manifest = test_manifest(
            "test.net",
            vec![Permission::Network(NetworkPermission::Full)],
        );
        let skill = NetworkSkillGranted { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(&skill_id, "fetch", ToolParams::new())
            .await;
        assert!(result.is_ok(), "network permission should be granted");
    }

    #[tokio::test]
    async fn domain_permission_mismatch() {
        // Skill has LocalOnly permission, but tool requires Full network
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.local".to_string());
        let manifest = test_manifest(
            "test.local",
            vec![Permission::Network(NetworkPermission::LocalOnly)],
        );
        // Reuse NetworkToolNoPermissionSkill but with LocalOnly permission in manifest
        // Tool requires Full → sandbox should deny because "any" != "localhost"
        let skill = NetworkSkillGranted { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(&skill_id, "fetch", ToolParams::new())
            .await;
        assert!(
            result.is_err(),
            "LocalOnly should not satisfy Full network permission"
        );
    }

    #[tokio::test]
    async fn concurrency_limit_respected() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.sleep".to_string());
        let manifest = test_manifest("test.sleep", vec![]);
        let skill = SleepSkill {
            manifest,
            sleep_ms: 100,
        };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let limits = ResourceLimits {
            max_concurrency: 1,
            max_cpu_ms: 5000,
            ..ResourceLimits::default()
        };
        let executor = Arc::new(SkillExecutor::with_limits(registry, limits));

        // Launch 3 concurrent tasks — with concurrency=1, they should serialize
        let start = std::time::Instant::now();
        let mut handles = vec![];
        for _ in 0..3 {
            let exec = executor.clone();
            let sid = skill_id.clone();
            handles.push(tokio::spawn(async move {
                exec.execute(&sid, "sleep", ToolParams::new()).await
            }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }
        let elapsed = start.elapsed();
        // With concurrency=1 and 100ms sleep, 3 tasks should take >= 300ms
        assert!(
            elapsed.as_millis() >= 250,
            "expected serialized execution (>=250ms), got {}ms",
            elapsed.as_millis()
        );
    }

    #[tokio::test]
    async fn tool_returns_error() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.fail".to_string());
        let manifest = test_manifest("test.fail", vec![]);
        let skill = FailingSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor.execute(&skill_id, "fail", ToolParams::new()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("intentional failure"));
    }

    #[tokio::test]
    async fn default_limits_work() {
        let registry = Arc::new(SkillRegistry::new());
        // new(registry) should not panic
        let _executor = SkillExecutor::new(registry);
    }

    #[tokio::test]
    async fn execute_with_params() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.echo".to_string());
        let manifest = test_manifest("test.echo", vec![]);
        let skill = EchoSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let params = ToolParams::new().with("key", "value");
        let result = executor.execute(&skill_id, "echo", params).await.unwrap();
        assert!(result.success);
        // The echo skill returns the values as output data
        assert_eq!(result.data.as_ref().unwrap()["key"], "value");
    }

    #[tokio::test]
    async fn filesystem_permission_passthrough() {
        // When no FS audit action is generated (tool has no FS required_permissions),
        // execution should pass through without sandbox check
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.echo2".to_string());
        let manifest = test_manifest("test.echo2", vec![]);
        let skill = EchoSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor.execute(&skill_id, "echo", ToolParams::new()).await;
        assert!(result.is_ok(), "no FS audit action → should pass through");
    }

    #[tokio::test]
    async fn latency_ms_populated_on_success() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.echo_latency".to_string());
        let manifest = test_manifest("test.echo_latency", vec![]);
        let skill = EchoSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor
            .execute(&skill_id, "echo", ToolParams::new())
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.metadata.latency_ms.is_some(),
            "latency_ms should be set on successful execution"
        );
    }

    /// Skill whose tool declares FileSystem::Read permission requirement.
    struct FsReadToolSkill {
        manifest: crate::manifest::SkillManifest,
    }

    #[async_trait::async_trait]
    impl crate::skill::Skill for FsReadToolSkill {
        fn manifest(&self) -> &crate::manifest::SkillManifest {
            &self.manifest
        }
        async fn initialize(&mut self, _: crate::skill::SkillConfig) -> SkillResult<()> {
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
                name: "read".to_string(),
                description: "Read a file".to_string(),
                parameters: serde_json::json!({}),
                returns: serde_json::json!({}),
                cost_estimate: crate::skill::CostEstimate::default(),
                required_permissions: vec![Permission::FileSystem(
                    crate::manifest::FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: false,
                requires_confirmation: false,
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: ToolParams,
            _: &ExecutionContext,
        ) -> SkillResult<crate::skill::ToolOutput> {
            Ok(crate::skill::ToolOutput::success(
                serde_json::json!({"content": "file data"}),
            ))
        }
        fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
            vec![]
        }
        fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
            None
        }
        fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
            vec![]
        }
    }

    #[tokio::test]
    async fn filesystem_read_denied_when_not_granted() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.fsread".to_string());
        let manifest = test_manifest("test.fsread", vec![]); // no FS permission
        let skill = FsReadToolSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor.execute(&skill_id, "read", ToolParams::new()).await;
        assert!(result.is_err(), "FS read should be denied without grant");
        let msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            msg.contains("permission") && msg.contains("denied"),
            "expected permission denied, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn filesystem_read_allowed_when_granted() {
        let registry = Arc::new(SkillRegistry::new());
        let skill_id = SkillId("test.fsread_ok".to_string());
        let manifest = test_manifest(
            "test.fsread_ok",
            vec![Permission::FileSystem(
                crate::manifest::FileSystemPermission::Full,
            )],
        );
        let skill = FsReadToolSkill { manifest };
        registry
            .register(skill_id.clone(), Arc::new(skill))
            .unwrap();
        let executor = SkillExecutor::new(registry);
        let result = executor.execute(&skill_id, "read", ToolParams::new()).await;
        assert!(result.is_ok(), "FS read should be allowed with Full grant");
    }
}
