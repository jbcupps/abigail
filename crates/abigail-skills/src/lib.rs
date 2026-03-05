//! Abigail Skills — plugin and tool execution layer.

pub mod backup;
pub mod channel;
pub mod dynamic;
pub mod executor;
pub mod factory;
pub mod hive;
pub mod instruction_registry;
pub mod manifest;
pub mod policy;
pub mod preloaded;
pub mod prelude;
pub mod protocol;
pub mod queue;
pub mod registry;
pub mod runtime;
pub mod sandbox;
pub mod skill;
pub mod topology;
pub mod transport;
pub mod watcher;

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle, TopicConfig};

/// Backward-compatible alias: `capability` now lives in `protocol`.
pub use protocol as capability;

pub use backup::{BackupManagementSkill, BackupOperations};
pub use channel::*;
pub use dynamic::{DynamicApiSkill, DynamicSkillConfig, DynamicToolConfig};
pub use executor::SkillExecutor;
pub use factory::SkillFactory;
pub use hive::{HiveAgentInfo, HiveManagementSkill, HiveOperations};
pub use instruction_registry::{InstructionRegistry, PromptInjectionMode};
pub use manifest::*;
pub use policy::{build_allowlist_payload, SkillExecutionPolicy};
pub use preloaded::{
    build_preloaded_skills, preloaded_integration_skills, preloaded_secret_keys,
    PreloadedSkillAuth, PRELOADED_SKILLS_VERSION,
};
pub use prelude::*;
pub use protocol::*;
pub use queue::{QueueManagementSkill, QueueOperations};
pub use registry::{MissingSkillSecret, RegisteredSkill, SkillRegistry};
pub use sandbox::*;
pub use skill::*;
pub use topology::{
    binding_for_skill, load_enabled_skill_ids, provision_all_skills as provision_skill_topology,
    ProvisionedSkillTopology, SkillTopicBinding, SKILL_TOPOLOGY_STREAM,
};
pub use watcher::{SkillFileEvent, SkillsWatcher};

/// Convenience helper: provision persistent topology from a registry path string.
pub async fn provision_all_skills_from_registry_path(
    broker: std::sync::Arc<dyn abigail_streaming::StreamBroker>,
    registry_path: &str,
) -> anyhow::Result<ProvisionedSkillTopology> {
    topology::provision_all_skills(broker, Path::new(registry_path)).await
}

#[derive(Debug, serde::Deserialize)]
struct RegistryInstructionSkillEntry {
    id: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

#[derive(Debug, serde::Deserialize)]
struct RegistryTopologySkillEntry {
    name: String,
    #[serde(default = "default_runtime")]
    runtime: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

#[derive(Debug, serde::Deserialize, Default)]
struct PersistentTopologyRegistry {
    #[serde(default)]
    skill: Vec<RegistryInstructionSkillEntry>,
    #[serde(default)]
    skills: Vec<RegistryTopologySkillEntry>,
}

fn default_enabled() -> bool {
    true
}

fn default_runtime() -> String {
    "native".to_string()
}

fn normalize_runtime(runtime: &str) -> &'static str {
    if runtime.eq_ignore_ascii_case("wasm") {
        "wasm"
    } else {
        "native"
    }
}

fn load_persistent_topology_entries(
    registry_path: &str,
) -> anyhow::Result<Vec<RegistryTopologySkillEntry>> {
    let bytes = std::fs::read_to_string(registry_path).map_err(|e| {
        anyhow::anyhow!("failed to read registry at {}: {}", registry_path, e)
    })?;
    let parsed: PersistentTopologyRegistry = toml::from_str(&bytes).map_err(|e| {
        anyhow::anyhow!("failed to parse registry at {}: {}", registry_path, e)
    })?;

    let mut deduped = std::collections::BTreeMap::<String, RegistryTopologySkillEntry>::new();
    for skill in parsed.skill {
        if !skill.enabled {
            continue;
        }
        let id = skill.id.trim();
        if id.is_empty() {
            continue;
        }
        deduped
            .entry(id.to_string())
            .or_insert_with(|| RegistryTopologySkillEntry {
                name: id.to_string(),
                runtime: default_runtime(),
                enabled: true,
            });
    }

    for skill in parsed.skills {
        if !skill.enabled {
            continue;
        }
        let name = skill.name.trim();
        if name.is_empty() {
            continue;
        }
        deduped.insert(
            name.to_string(),
            RegistryTopologySkillEntry {
                name: name.to_string(),
                runtime: skill.runtime,
                enabled: true,
            },
        );
    }

    Ok(deduped.into_values().collect())
}

static SKILL_TOPOLOGY_BROKER: OnceLock<Arc<dyn StreamBroker>> = OnceLock::new();
static SKILL_TOPOLOGY_WORKERS: Mutex<Vec<SubscriptionHandle>> = Mutex::new(Vec::new());

pub fn set_skill_topology_broker(broker: Arc<dyn StreamBroker>) {
    if SKILL_TOPOLOGY_BROKER.set(broker).is_err() {
        tracing::debug!("Skill topology broker already initialized; using existing broker");
    }
}

pub fn cancel_provisioned_skill_topology() {
    if let Ok(mut workers) = SKILL_TOPOLOGY_WORKERS.lock() {
        for worker in workers.drain(..) {
            worker.cancel();
        }
    }
}

pub async fn provision_all_skills(registry_path: &str) {
    let Some(broker) = SKILL_TOPOLOGY_BROKER.get().cloned() else {
        tracing::warn!(
            "Skill topology broker is not initialized; skipping provisioning for {}",
            registry_path
        );
        return;
    };

    let registry = load_persistent_topology_entries(registry_path)
        .expect("Failed to load skills/registry.toml");

    let mut worker_handles: Vec<SubscriptionHandle> = Vec::new();
    let skill_count = registry.len();

    for skill in registry {
        let runtime = normalize_runtime(&skill.runtime);
        let binding = binding_for_skill(&skill.name);
        let request_topic = binding.request_topic.clone();
        let response_topic = binding.response_topic.clone();
        let worker_group = binding.worker_group.clone();

        if let Err(e) = broker
            .ensure_topic(
                SKILL_TOPOLOGY_STREAM,
                &request_topic,
                TopicConfig::default(),
            )
            .await
        {
            tracing::warn!("Failed ensuring request topic {}: {}", request_topic, e);
            continue;
        }
        if let Err(e) = broker
            .ensure_topic(
                SKILL_TOPOLOGY_STREAM,
                &response_topic,
                TopicConfig::default(),
            )
            .await
        {
            tracing::warn!("Failed ensuring response topic {}: {}", response_topic, e);
            continue;
        }
        if let Err(e) = broker
            .ensure_consumer_group(SKILL_TOPOLOGY_STREAM, &request_topic, &worker_group)
            .await
        {
            tracing::warn!(
                "Failed ensuring consumer group {} for {}: {}",
                worker_group,
                request_topic,
                e
            );
            continue;
        }

        let broker_for_worker = broker.clone();
        let request_topic_for_worker = request_topic.clone();
        let response_topic_for_worker = response_topic.clone();
        let worker_group_for_worker = worker_group.clone();
        let skill_name_for_worker = skill.name.clone();
        let runtime_for_worker = runtime.to_string();

        match broker
            .subscribe(
                SKILL_TOPOLOGY_STREAM,
                &request_topic,
                &worker_group,
                Box::new(move |msg: StreamMessage| {
                    let broker = broker_for_worker.clone();
                    let response_topic = response_topic_for_worker.clone();
                    let request_topic = request_topic_for_worker.clone();
                    let worker_group = worker_group_for_worker.clone();
                    let skill_name = skill_name_for_worker.clone();
                    let runtime = runtime_for_worker.clone();
                    Box::pin(async move {
                        let payload = serde_json::json!({
                            "status": "received",
                            "skill_name": skill_name,
                            "runtime": runtime,
                            "worker_group": worker_group,
                            "request_topic": request_topic,
                            "request_message_id": msg.id.to_string(),
                        });
                        let Ok(payload_bytes) = serde_json::to_vec(&payload) else {
                            tracing::warn!(
                                "skill topology worker: failed to serialize response payload"
                            );
                            return;
                        };

                        let mut headers = msg.headers.clone();
                        headers.insert("runtime".to_string(), runtime);

                        if let Err(e) = broker
                            .publish(
                                SKILL_TOPOLOGY_STREAM,
                                &response_topic,
                                StreamMessage::with_headers(payload_bytes, headers),
                            )
                            .await
                        {
                            tracing::warn!(
                                "skill topology worker: failed publishing response for {}: {}",
                                skill_name,
                                e
                            );
                        }
                    })
                }),
            )
            .await
        {
            Ok(handle) => worker_handles.push(handle),
            Err(e) => tracing::warn!(
                "Failed to subscribe worker {} to {}: {}",
                worker_group,
                request_topic,
                e
            ),
        }
    }

    if let Ok(mut workers) = SKILL_TOPOLOGY_WORKERS.lock() {
        for worker in workers.drain(..) {
            worker.cancel();
        }
        *workers = worker_handles;
    }

    tracing::info!(
        "Persistent skill topology provisioned ({} skills)",
        skill_count
    );
}

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
