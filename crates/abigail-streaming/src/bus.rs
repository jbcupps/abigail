//! Typed bus contract — canonical topics and envelope for the entity bus.
//!
//! The bus is the integration boundary between entity participants (Id, Ego,
//! Superego, memory, skills, queue, governance). All canonical inter-component
//! traffic uses a [`Topic`] variant on the single [`BUS_STREAM`] stream;
//! ad-hoc string topics are reserved for dynamic per-session/per-task channels
//! (e.g. `chat-{session_id}`, `agentic:{task_id}`).
//!
//! Adding a topic requires a note in `documents/ORIONII_ALIGNMENT_PLAN.md`.

use crate::types::{StreamMessage, TopicConfig};
use crate::StreamBroker;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

/// The single stream all canonical entity topics live on.
pub const BUS_STREAM: &str = "entity";

/// Canonical topic set for the entity bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Mentor/user input entering the cognitive pipeline.
    MentorInput,
    /// Id stage output: curated system prompt + personality signal for Ego.
    IdReaction,
    /// Out-of-band Id safety/feasibility signals.
    IdSignal,
    /// Ego reasoning trace tap for audit (ExecutionTrace and deliberation).
    EgoDeliberation,
    /// Ego's committed response — the only payload that reaches the mentor.
    EgoAction,
    /// Conscience check requests awaiting evaluation.
    ConscienceCheck,
    /// Superego/conscience verdicts and ethical signals.
    SuperegoEvaluation,
    /// Job queue lifecycle events (queued/running/completed, sub-agents).
    JobEvents,
    /// Skill/tool execution audit events.
    SkillExecuted,
    /// Conversation turns bound for async memory persistence.
    MemoryArchive,
    /// Outbox records bound for hive-daemon sync (the egress seam).
    HiveOutbound,
    /// Hive → entity governance: config, assignment, and policy refresh.
    GovernanceInbound,
}

impl Topic {
    /// Stable topic name used on the wire and in broker topic registries.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Topic::MentorInput => "mentor.input",
            Topic::IdReaction => "id.reaction",
            Topic::IdSignal => "id.signal",
            Topic::EgoDeliberation => "ego.deliberation",
            Topic::EgoAction => "ego.action",
            Topic::ConscienceCheck => "conscience.check",
            Topic::SuperegoEvaluation => "superego.evaluation",
            Topic::JobEvents => "job.events",
            Topic::SkillExecuted => "skill.executed",
            Topic::MemoryArchive => "memory.archive",
            Topic::HiveOutbound => "hive.outbound",
            Topic::GovernanceInbound => "governance.inbound",
        }
    }

    /// Resolve a wire name back to a canonical topic.
    pub fn from_name(name: &str) -> Option<Topic> {
        match name {
            "mentor.input" => Some(Topic::MentorInput),
            "id.reaction" => Some(Topic::IdReaction),
            "id.signal" => Some(Topic::IdSignal),
            "ego.deliberation" => Some(Topic::EgoDeliberation),
            "ego.action" => Some(Topic::EgoAction),
            "conscience.check" => Some(Topic::ConscienceCheck),
            "superego.evaluation" => Some(Topic::SuperegoEvaluation),
            "job.events" => Some(Topic::JobEvents),
            "skill.executed" => Some(Topic::SkillExecuted),
            "memory.archive" => Some(Topic::MemoryArchive),
            "hive.outbound" => Some(Topic::HiveOutbound),
            "governance.inbound" => Some(Topic::GovernanceInbound),
            _ => None,
        }
    }

    /// Ensure this topic exists on the bus stream. Idempotent.
    pub async fn ensure(&self, broker: &dyn StreamBroker) -> anyhow::Result<()> {
        broker
            .ensure_topic(BUS_STREAM, self.as_str(), TopicConfig::default())
            .await
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical message envelope for the entity bus.
///
/// Every envelope carries the entity it belongs to, a correlation id that
/// traces one mentor interaction through the whole pipeline, and (once the
/// publisher knows it) a `soul_ref` — the hash of the entity's signed soul
/// documents, proving which identity version produced the message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub topic: Topic,
    pub entity_id: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// Hash reference to the entity's signed soul/constitution documents.
    #[serde(default)]
    pub soul_ref: Option<String>,
    /// Traces one mentor interaction across pipeline stages.
    pub correlation_id: String,
    pub payload: serde_json::Value,
}

impl Envelope {
    pub fn new(
        topic: Topic,
        entity_id: impl Into<String>,
        correlation_id: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            topic,
            entity_id: entity_id.into(),
            occurred_at: chrono::Utc::now(),
            soul_ref: None,
            correlation_id: correlation_id.into(),
            payload,
        }
    }

    pub fn with_soul_ref(mut self, soul_ref: impl Into<String>) -> Self {
        self.soul_ref = Some(soul_ref.into());
        self
    }

    /// Serialize into a `StreamMessage` with routing headers.
    pub fn to_message(&self) -> anyhow::Result<StreamMessage> {
        let payload = serde_json::to_vec(self)?;
        let mut headers = HashMap::new();
        headers.insert("topic".to_string(), self.topic.as_str().to_string());
        headers.insert("entity_id".to_string(), self.entity_id.clone());
        headers.insert("correlation_id".to_string(), self.correlation_id.clone());
        if let Some(ref soul_ref) = self.soul_ref {
            headers.insert("soul_ref".to_string(), soul_ref.clone());
        }
        Ok(StreamMessage::with_headers(payload, headers))
    }

    /// Deserialize from a `StreamMessage` payload.
    pub fn from_message(msg: &StreamMessage) -> anyhow::Result<Envelope> {
        Ok(serde_json::from_slice(&msg.payload)?)
    }

    /// Publish this envelope on its topic. Ensures the topic exists first.
    pub async fn publish(&self, broker: &dyn StreamBroker) -> anyhow::Result<()> {
        self.topic.ensure(broker).await?;
        broker
            .publish(BUS_STREAM, self.topic.as_str(), self.to_message()?)
            .await
    }
}

/// Compute a soul reference from the bytes of signed soul documents.
///
/// Format: `sha256:<hex>` — consistent with soul-forge's SHA-256 soul hash.
pub fn compute_soul_ref(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryBroker;
    use std::sync::Arc;

    #[test]
    fn topic_names_roundtrip() {
        for topic in [
            Topic::MentorInput,
            Topic::IdReaction,
            Topic::IdSignal,
            Topic::EgoDeliberation,
            Topic::EgoAction,
            Topic::ConscienceCheck,
            Topic::SuperegoEvaluation,
            Topic::JobEvents,
            Topic::SkillExecuted,
            Topic::MemoryArchive,
            Topic::HiveOutbound,
            Topic::GovernanceInbound,
        ] {
            assert_eq!(Topic::from_name(topic.as_str()), Some(topic));
        }
        assert_eq!(Topic::from_name("not-a-topic"), None);
    }

    #[test]
    fn envelope_message_roundtrip() {
        let env = Envelope::new(
            Topic::EgoAction,
            "entity-1",
            "corr-1",
            serde_json::json!({"response": "hello"}),
        )
        .with_soul_ref("sha256:abc");

        let msg = env.to_message().unwrap();
        assert_eq!(msg.headers.get("topic").unwrap(), "ego.action");
        assert_eq!(msg.headers.get("correlation_id").unwrap(), "corr-1");
        assert_eq!(msg.headers.get("soul_ref").unwrap(), "sha256:abc");

        let back = Envelope::from_message(&msg).unwrap();
        assert_eq!(back.topic, Topic::EgoAction);
        assert_eq!(back.entity_id, "entity-1");
        assert_eq!(back.payload["response"], "hello");
    }

    #[test]
    fn soul_ref_is_deterministic() {
        let a = compute_soul_ref(b"soul document bytes");
        let b = compute_soul_ref(b"soul document bytes");
        assert_eq!(a, b);
        assert!(a.starts_with("sha256:"));
        assert_ne!(a, compute_soul_ref(b"different bytes"));
    }

    #[tokio::test]
    async fn envelope_publish_and_receive() {
        let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());
        Topic::EgoAction.ensure(broker.as_ref()).await.unwrap();
        broker
            .ensure_consumer_group(BUS_STREAM, Topic::EgoAction.as_str(), "test-group")
            .await
            .unwrap();

        let (tx, rx) = tokio::sync::oneshot::channel::<Envelope>();
        let tx_cell = Arc::new(tokio::sync::Mutex::new(Some(tx)));
        let handler: crate::broker::MessageHandler = Box::new(move |msg| {
            let tx_cell = tx_cell.clone();
            Box::pin(async move {
                if let Ok(env) = Envelope::from_message(&msg) {
                    if let Some(sender) = tx_cell.lock().await.take() {
                        let _ = sender.send(env);
                    }
                }
            })
        });
        let _sub = broker
            .subscribe(BUS_STREAM, Topic::EgoAction.as_str(), "test-group", handler)
            .await
            .unwrap();

        Envelope::new(
            Topic::EgoAction,
            "e1",
            "c1",
            serde_json::json!({"ok": true}),
        )
        .publish(broker.as_ref())
        .await
        .unwrap();

        let got = tokio::time::timeout(std::time::Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.correlation_id, "c1");
    }
}
