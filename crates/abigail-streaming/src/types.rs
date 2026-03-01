//! Core types for the streaming abstraction.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

/// A message published to or consumed from a stream topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMessage {
    /// Unique message identifier.
    pub id: uuid::Uuid,
    /// Serialized payload (typically JSON).
    pub payload: Vec<u8>,
    /// Key-value headers for routing and metadata.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// When the message was created.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl StreamMessage {
    /// Create a new message with the given payload.
    pub fn new(payload: Vec<u8>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            payload,
            headers: HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a new message with payload and headers.
    pub fn with_headers(payload: Vec<u8>, headers: HashMap<String, String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            payload,
            headers,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Convenience: deserialize the payload as JSON.
    pub fn payload_json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }
}

/// Configuration for a stream topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicConfig {
    /// Number of partitions (relevant for Iggy; ignored by MemoryBroker).
    pub partitions: u32,
    /// How long messages are retained, in seconds.
    pub retention_seconds: u64,
    /// Whether messages should be encrypted at rest.
    pub encryption: bool,
}

impl Default for TopicConfig {
    fn default() -> Self {
        Self {
            partitions: 1,
            retention_seconds: 86400, // 24 hours
            encryption: false,
        }
    }
}

/// Handle to a running subscription. Dropping or cancelling stops the subscription.
pub struct SubscriptionHandle {
    cancel: CancellationToken,
}

impl SubscriptionHandle {
    /// Create a new handle wrapping a cancellation token.
    pub fn new(cancel: CancellationToken) -> Self {
        Self { cancel }
    }

    /// Cancel the subscription.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Check if the subscription has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}
