//! Fire-and-forget memory persistence via StreamBroker topic consumer.
//!
//! Instead of blocking the chat request with synchronous `memory.insert_turn()`,
//! turns are published to the `"abigail/conversation-turns"` topic and consumed
//! asynchronously by a background task.

use abigail_memory::{ConversationTurn, MemoryStore};
use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle};
use std::sync::Arc;

const STREAM: &str = "abigail";
const TOPIC: &str = "conversation-turns";
const CONSUMER_GROUP: &str = "memory-consumer";

/// Publish a conversation turn to the StreamBroker for async persistence.
///
/// This is fire-and-forget: serialization or publish failures are logged but
/// never propagate to the caller.
pub fn publish_turn(broker: Arc<dyn StreamBroker>, turn: ConversationTurn) {
    tokio::spawn(async move {
        let payload = match serde_json::to_vec(&turn) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to serialize ConversationTurn: {}", e);
                return;
            }
        };
        let mut msg = StreamMessage::new(payload);
        msg.headers
            .insert("session_id".to_string(), turn.session_id.clone());
        msg.headers.insert("role".to_string(), turn.role.clone());
        if let Err(e) = broker.publish(STREAM, TOPIC, msg).await {
            tracing::warn!("Failed to publish conversation turn: {}", e);
        }
    });
}

/// Spawn a background consumer that persists conversation turns from the broker
/// into the MemoryStore. Returns the subscription handle for cancellation.
pub async fn spawn_memory_consumer(
    broker: Arc<dyn StreamBroker>,
    memory: Arc<MemoryStore>,
) -> anyhow::Result<SubscriptionHandle> {
    // Ensure the topic exists.
    broker
        .ensure_topic(STREAM, TOPIC, abigail_streaming::TopicConfig::default())
        .await?;
    broker
        .ensure_consumer_group(STREAM, TOPIC, CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let memory = memory.clone();
        Box::pin(async move {
            match serde_json::from_slice::<ConversationTurn>(&msg.payload) {
                Ok(turn) => {
                    if let Err(e) = memory.insert_turn(&turn) {
                        tracing::warn!("Memory consumer: failed to persist turn: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Memory consumer: failed to deserialize turn: {}", e);
                }
            }
        })
    });

    let handle = broker
        .subscribe(STREAM, TOPIC, CONSUMER_GROUP, handler)
        .await?;
    tracing::info!("Memory consumer subscribed to {}/{}", STREAM, TOPIC);
    Ok(handle)
}
