//! Out-of-band chat-topic subscriber for memory correlation persistence.

use crate::{ConversationTurn, MemoryStore};
use abigail_streaming::{StreamBroker, SubscriptionHandle, TopicConfig};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
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

fn append_memory_superego_log(entry: &str) {
    let base =
        std::env::var("HIVE_DOCUMENTS_PATH").unwrap_or_else(|_| "hive/documents".to_string());
    let dir = Path::new(&base);
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!(
            "memory subscriber: failed to create hive documents dir {}: {}",
            dir.display(),
            e
        );
        return;
    }

    let path = dir.join("memory_superego_context.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{}", entry) {
                tracing::warn!(
                    "memory subscriber: failed writing log {}: {}",
                    path.display(),
                    e
                );
            }
        }
        Err(e) => tracing::warn!(
            "memory subscriber: failed opening log {}: {}",
            path.display(),
            e
        ),
    }
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

            append_memory_superego_log(&format!(
                "{} | source=memory_subscriber | correlation_id={} | session_id={} | id_context={} | superego_context={} | message={} | enriched={}",
                chrono::Utc::now().to_rfc3339(),
                env.correlation_id,
                env.session_id,
                env.id_context.clone().unwrap_or_else(|| "unknown".to_string()),
                env.superego_context.clone().unwrap_or_else(|| "unknown".to_string()),
                env.message.replace('\n', " "),
                env.enriched_preprompt.clone().unwrap_or_default().replace('\n', " ")
            ));

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
