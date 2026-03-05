//! Out-of-band chat-topic subscriber for memory correlation persistence.

use crate::{ConversationTurn, MemoryStore};
use abigail_streaming::{StreamBroker, SubscriptionHandle, TopicConfig};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const STREAM: &str = "entity";
const TOPIC: &str = "chat-topic";
const CONSUMER_GROUP: &str = "memory-chat-topic-subscriber";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTopicEnvelope {
    pub correlation_id: String,
    pub session_id: String,
    pub entity_id: String,
    pub message: String,
    #[serde(default)]
    pub selected_model: Option<String>,
    pub stage: String,
    #[serde(default)]
    pub enriched_preprompt: Option<String>,
    #[serde(default)]
    pub id_context: Option<String>,
    #[serde(default)]
    pub superego_context: Option<String>,
}

/// Subscribe to `entity/chat-topic` and persist enriched chat-topic envelopes
/// into `conversation_turns` with correlation metadata.
pub async fn spawn_chat_topic_subscriber(
    broker: Arc<dyn StreamBroker>,
    memory: Arc<MemoryStore>,
) -> anyhow::Result<SubscriptionHandle> {
    broker
        .ensure_topic(STREAM, TOPIC, TopicConfig::default())
        .await?;
    broker
        .ensure_consumer_group(STREAM, TOPIC, CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let memory = memory.clone();
        Box::pin(async move {
            let Ok(env) = serde_json::from_slice::<ChatTopicEnvelope>(&msg.payload) else {
                return;
            };
            if env.stage != "enriched" {
                return;
            }

            let content = env
                .enriched_preprompt
                .clone()
                .unwrap_or_else(|| env.message.clone());
            let id_ctx = env.id_context.unwrap_or_else(|| "unknown".to_string());
            let superego_ctx = env
                .superego_context
                .unwrap_or_else(|| "unknown".to_string());
            let provider = Some(format!(
                "chat-topic:{}|id:{}|superego:{}",
                env.correlation_id, id_ctx, superego_ctx
            ));
            let model = env.selected_model.clone();
            let tier = Some("mentor-monitor".to_string());
            let turn = ConversationTurn::new(&env.session_id, "mentor_monitor", &content)
                .with_metadata(provider, model, tier, None);

            if let Err(e) = memory.insert_turn(&turn) {
                tracing::warn!(
                    "memory chat-topic subscriber: failed to persist turn: {}",
                    e
                );
            }
        })
    });

    let handle = broker
        .subscribe(STREAM, TOPIC, CONSUMER_GROUP, handler)
        .await?;
    tracing::info!(
        "Memory chat-topic subscriber started on {}/{}",
        STREAM,
        TOPIC
    );
    Ok(handle)
}

/// Startup helper for the out-of-band memory monitor.
pub async fn start(
    broker: Arc<dyn StreamBroker>,
    memory: Arc<MemoryStore>,
) -> anyhow::Result<SubscriptionHandle> {
    spawn_chat_topic_subscriber(broker, memory).await
}
