//! Triggers, skill events, and StreamBroker-based event publishing.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::manifest::SkillId;

/// Publish a skill event to the StreamBroker on the "abigail/skill-events" topic.
/// Fire-and-forget: logs a warning on failure but never blocks the caller.
pub async fn publish_skill_event(
    broker: &Arc<dyn abigail_streaming::StreamBroker>,
    event: SkillEvent,
) {
    let payload = match serde_json::to_vec(&event) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to serialize SkillEvent: {}", e);
            return;
        }
    };
    let mut msg = abigail_streaming::StreamMessage::new(payload);
    msg.headers
        .insert("skill_id".to_string(), event.skill_id.0.clone());
    msg.headers
        .insert("trigger".to_string(), event.trigger.clone());
    if let Err(e) = broker.publish("abigail", "skill-events", msg).await {
        tracing::warn!("Failed to publish skill event: {}", e);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDescriptor {
    pub name: String,
    pub description: String,
    pub payload_schema: serde_json::Value,
    pub frequency: TriggerFrequency,
    pub priority: TriggerPriority,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TriggerFrequency {
    Rare,
    Occasional,
    Frequent,
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TriggerPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvent {
    pub skill_id: SkillId,
    pub trigger: String,
    pub payload: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub priority: TriggerPriority,
}
