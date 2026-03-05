//! Skill topology provisioning for persistent request/response topics.
//!
//! This module provisions a static stream topology from `skills/registry.toml`
//! and starts one subscriber worker per enabled skill entry.

use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle, TopicConfig};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

/// Stream name used for persistent skill request/response topics.
pub const SKILL_TOPOLOGY_STREAM: &str = "entity";

#[derive(Debug, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    skill: Vec<RegistrySkillEntry>,
}

#[derive(Debug, Deserialize)]
struct RegistrySkillEntry {
    id: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Topic names and worker group for one skill.
#[derive(Debug, Clone)]
pub struct SkillTopicBinding {
    pub request_topic: String,
    pub response_topic: String,
    pub worker_group: String,
}

/// Result of provisioning persistent skill topology.
pub struct ProvisionedSkillTopology {
    pub bindings: HashMap<String, SkillTopicBinding>,
    worker_handles: Vec<SubscriptionHandle>,
}

impl ProvisionedSkillTopology {
    /// Number of skills provisioned.
    pub fn skill_count(&self) -> usize {
        self.bindings.len()
    }

    /// Cancel all active worker subscriptions.
    pub fn cancel_all(&self) {
        for h in &self.worker_handles {
            h.cancel();
        }
    }
}

impl Drop for ProvisionedSkillTopology {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// Parse enabled skill IDs from `registry.toml`.
pub fn load_enabled_skill_ids(registry_path: &Path) -> anyhow::Result<Vec<String>> {
    let toml_text = std::fs::read_to_string(registry_path).map_err(|e| {
        anyhow::anyhow!(
            "failed reading registry.toml at {}: {}",
            registry_path.display(),
            e
        )
    })?;
    let parsed: RegistryFile = toml::from_str(&toml_text).map_err(|e| {
        anyhow::anyhow!(
            "failed parsing registry.toml at {}: {}",
            registry_path.display(),
            e
        )
    })?;

    let mut unique = BTreeSet::new();
    for s in parsed.skill {
        if s.enabled {
            let id = s.id.trim();
            if !id.is_empty() {
                unique.insert(id.to_string());
            }
        }
    }
    Ok(unique.into_iter().collect())
}

fn sanitize_topic_name(skill_id: &str) -> String {
    skill_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build the persistent request/response topic binding for a skill id.
pub fn binding_for_skill(skill_id: &str) -> SkillTopicBinding {
    let name = sanitize_topic_name(skill_id);
    let base = format!("topic.skill.{}", name);
    SkillTopicBinding {
        request_topic: format!("{}.request", base),
        response_topic: format!("{}.response", base),
        worker_group: format!("skill-worker.{}", name),
    }
}

/// Provision request/response topics and worker subscribers for all enabled
/// skills defined in `registry.toml`.
pub async fn provision_all_skills(
    broker: Arc<dyn StreamBroker>,
    registry_path: &Path,
) -> anyhow::Result<ProvisionedSkillTopology> {
    let skill_ids = load_enabled_skill_ids(registry_path)?;
    let mut bindings = HashMap::new();
    let mut worker_handles = Vec::new();

    for skill_id in skill_ids {
        let binding = binding_for_skill(&skill_id);

        broker
            .ensure_topic(
                SKILL_TOPOLOGY_STREAM,
                &binding.request_topic,
                TopicConfig::default(),
            )
            .await?;
        broker
            .ensure_topic(
                SKILL_TOPOLOGY_STREAM,
                &binding.response_topic,
                TopicConfig::default(),
            )
            .await?;
        broker
            .ensure_consumer_group(
                SKILL_TOPOLOGY_STREAM,
                &binding.request_topic,
                &binding.worker_group,
            )
            .await?;

        let broker_for_handler = broker.clone();
        let request_topic = binding.request_topic.clone();
        let response_topic = binding.response_topic.clone();
        let worker_group = binding.worker_group.clone();
        let skill_id_for_handler = skill_id.clone();

        let handle = broker
            .subscribe(
                SKILL_TOPOLOGY_STREAM,
                &binding.request_topic,
                &binding.worker_group,
                Box::new(move |msg: StreamMessage| {
                    let broker = broker_for_handler.clone();
                    let request_topic = request_topic.clone();
                    let response_topic = response_topic.clone();
                    let worker_group = worker_group.clone();
                    let skill_id = skill_id_for_handler.clone();

                    Box::pin(async move {
                        let payload = serde_json::json!({
                            "status": "received",
                            "skill_id": skill_id,
                            "worker_group": worker_group,
                            "request_topic": request_topic,
                            "request_message_id": msg.id.to_string(),
                        });
                        let Ok(payload_bytes) = serde_json::to_vec(&payload) else {
                            tracing::warn!("skill topology worker: failed to serialize response");
                            return;
                        };

                        let mut headers = msg.headers.clone();
                        headers.insert("skill_id".to_string(), skill_id.clone());
                        headers.insert("worker_group".to_string(), worker_group.clone());
                        headers.insert("request_message_id".to_string(), msg.id.to_string());

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
                                skill_id,
                                e
                            );
                        }
                    })
                }),
            )
            .await?;

        bindings.insert(skill_id, binding);
        worker_handles.push(handle);
    }

    Ok(ProvisionedSkillTopology {
        bindings,
        worker_handles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_streaming::MemoryBroker;
    use std::time::Duration;
    use tokio::sync::{oneshot, Mutex};

    fn temp_registry(contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("abigail_skill_topology_tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("registry.toml");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn load_enabled_skill_ids_filters_disabled_and_dedupes() {
        let path = temp_registry(
            r#"
[[skill]]
id = "a"
enabled = true

[[skill]]
id = "a"
enabled = true

[[skill]]
id = "b"
enabled = false
"#,
        );
        let ids = load_enabled_skill_ids(&path).unwrap();
        assert_eq!(ids, vec!["a".to_string()]);
    }

    #[tokio::test]
    async fn provision_all_skills_creates_worker_and_response_flow() {
        let path = temp_registry(
            r#"
[[skill]]
id = "com.abigail.skills.test"
enabled = true
"#,
        );
        let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());
        let topology = provision_all_skills(broker.clone(), &path).await.unwrap();
        assert_eq!(topology.skill_count(), 1);

        let binding = binding_for_skill("com.abigail.skills.test");
        let (tx, rx) = oneshot::channel::<StreamMessage>();
        let tx_cell = Arc::new(Mutex::new(Some(tx)));
        let tx_cell_closure = tx_cell.clone();

        let _response_sub = broker
            .subscribe(
                SKILL_TOPOLOGY_STREAM,
                &binding.response_topic,
                "test-response-reader",
                Box::new(move |msg| {
                    let tx_cell = tx_cell_closure.clone();
                    Box::pin(async move {
                        if let Some(sender) = tx_cell.lock().await.take() {
                            let _ = sender.send(msg);
                        }
                    })
                }),
            )
            .await
            .unwrap();

        broker
            .publish(
                SKILL_TOPOLOGY_STREAM,
                &binding.request_topic,
                StreamMessage::new(b"{}".to_vec()),
            )
            .await
            .unwrap();

        let msg = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&msg.payload).unwrap();
        assert_eq!(body["status"], "received");
        assert_eq!(body["skill_id"], "com.abigail.skills.test");
    }
}
