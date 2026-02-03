//! Triggers, skill events, and event bus.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::manifest::SkillId;

/// Event bus for skill events. Subscribe to receive events; publish from skills or registry.
pub struct EventBus {
    sender: broadcast::Sender<SkillEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SkillEvent> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: SkillEvent) {
        let _ = self.sender.send(event);
    }

    pub fn sender(&self) -> broadcast::Sender<SkillEvent> {
        self.sender.clone()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
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
