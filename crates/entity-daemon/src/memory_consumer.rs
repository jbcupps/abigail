//! Durable chat-turn commits plus background archive topic consumption.
//!
//! Chat awaits persistence on a blocking worker before reporting completion.
//! A separate consumer also accepts turns published to `Topic::MemoryArchive`.

use abigail_memory::{ConversationTurn, MemoryStore};
use abigail_streaming::{StreamBroker, SubscriptionHandle, Topic, BUS_STREAM};
use std::sync::Arc;

const STREAM: &str = BUS_STREAM;
const TOPIC: &str = Topic::MemoryArchive.as_str();
const CONSUMER_GROUP: &str = "memory-consumer";

/// Persist a committed turn before the HTTP/SSE completion event.
pub async fn persist_turn(
    memory: Arc<MemoryStore>,
    mut turn: ConversationTurn,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        turn.content = abigail_core::redact_secrets(&turn.content);
        memory
            .insert_turn_or_ignore(&turn)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
                Ok(mut turn) => {
                    turn.content = abigail_core::redact_secrets(&turn.content);
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
