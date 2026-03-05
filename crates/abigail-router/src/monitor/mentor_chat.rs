//! Mentor chat monitor for preprompt enrichment over topic transport.
//!
//! Flow:
//! 1) request envelope is published to `entity/chat-topic`
//! 2) monitor subscriber injects minimal preprompt + id/superego context
//! 3) enriched envelope is republished to `entity/chat-topic`

use crate::router::IdEgoRouter;
use abigail_core::constitutional::{infer_id_context, infer_superego_context};
use abigail_core::load_preprompt_context;
use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle, TopicConfig};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub const STREAM: &str = "entity";
pub const CHAT_TOPIC: &str = "chat-topic";
const MONITOR_GROUP: &str = "mentor-chat-monitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentorChatEnvelope {
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
    pub created_at_utc: chrono::DateTime<chrono::Utc>,
}

impl MentorChatEnvelope {
    pub fn request(
        correlation_id: String,
        session_id: String,
        entity_id: String,
        message: String,
        selected_model: Option<String>,
    ) -> Self {
        Self {
            correlation_id,
            session_id,
            entity_id,
            message,
            selected_model,
            stage: "request".to_string(),
            enriched_preprompt: None,
            id_context: None,
            superego_context: None,
            created_at_utc: Utc::now(),
        }
    }
}

fn build_fallback_preprompt(envelope: &MentorChatEnvelope) -> String {
    let model = envelope
        .selected_model
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let id_context = infer_id_context(&envelope.message.to_lowercase());
    let superego_context = infer_superego_context(&envelope.message.to_lowercase());
    format!(
        "Mentor monitor context:\n- correlation: {}\n- selected model subscriber: {}\n- id context: {}\n- superego context: {}\n- out-of-band observers: memory/id/superego non-blocking",
        envelope.correlation_id, model, id_context, superego_context
    )
}

/// Monitor subscriber that enriches mentor chat envelopes.
pub struct MentorChatMonitor {
    broker: Arc<dyn StreamBroker>,
}

impl MentorChatMonitor {
    pub fn new(broker: Arc<dyn StreamBroker>) -> Self {
        Self { broker }
    }

    pub async fn spawn(self) -> anyhow::Result<SubscriptionHandle> {
        self.broker
            .ensure_topic(STREAM, CHAT_TOPIC, TopicConfig::default())
            .await?;
        self.broker
            .ensure_consumer_group(STREAM, CHAT_TOPIC, MONITOR_GROUP)
            .await?;

        let broker = self.broker.clone();
        let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
            let broker = broker.clone();
            Box::pin(async move {
                let Ok(mut envelope) = serde_json::from_slice::<MentorChatEnvelope>(&msg.payload)
                else {
                    return;
                };
                if envelope.stage == "enriched" {
                    return;
                }

                let lower = envelope.message.to_lowercase();
                let id_context = infer_id_context(&lower).to_string();
                let superego_context = infer_superego_context(&lower).to_string();

                envelope.stage = "enriched".to_string();
                envelope.id_context = Some(id_context.clone());
                envelope.superego_context = Some(superego_context.clone());
                envelope.enriched_preprompt = Some(
                    load_preprompt_context(&envelope.message)
                        .await
                        .unwrap_or_else(|_| build_fallback_preprompt(&envelope)),
                );

                let Ok(payload) = serde_json::to_vec(&envelope) else {
                    return;
                };

                let mut headers = HashMap::new();
                headers.insert("stage".to_string(), "enriched".to_string());
                headers.insert(
                    "correlation_id".to_string(),
                    envelope.correlation_id.clone(),
                );
                headers.insert("entity_id".to_string(), envelope.entity_id.clone());
                headers.insert("id_context".to_string(), id_context);
                headers.insert("superego_context".to_string(), superego_context);

                let _ = broker
                    .publish(
                        STREAM,
                        CHAT_TOPIC,
                        StreamMessage::with_headers(payload, headers),
                    )
                    .await;
            })
        });

        let handle = self
            .broker
            .subscribe(STREAM, CHAT_TOPIC, MONITOR_GROUP, handler)
            .await?;
        tracing::info!("MentorChatMonitor subscribed to {}/{}", STREAM, CHAT_TOPIC);
        Ok(handle)
    }
}

/// Request enriched preprompt from the monitor over the chat topic.
pub async fn request_enriched_preprompt(
    broker: Arc<dyn StreamBroker>,
    router: Arc<IdEgoRouter>,
    entity_id: &str,
    session_id: &str,
    message: &str,
    model_override: Option<String>,
) -> Option<String> {
    let group = router.register_selected_model_subscriber(entity_id, model_override.clone());

    if broker
        .ensure_topic(STREAM, CHAT_TOPIC, TopicConfig::default())
        .await
        .is_err()
    {
        return None;
    }
    if broker
        .ensure_consumer_group(STREAM, CHAT_TOPIC, &group)
        .await
        .is_err()
    {
        return None;
    }

    let correlation_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx_cell = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let corr_for_cb = correlation_id.clone();
    let tx_cell_for_cb = tx_cell.clone();

    let sub = match broker
        .subscribe(
            STREAM,
            CHAT_TOPIC,
            &group,
            Box::new(move |msg| {
                let corr = corr_for_cb.clone();
                let tx_cell = tx_cell_for_cb.clone();
                Box::pin(async move {
                    let Ok(env) = serde_json::from_slice::<MentorChatEnvelope>(&msg.payload) else {
                        return;
                    };
                    if env.stage == "enriched"
                        && env.correlation_id == corr
                        && env.enriched_preprompt.is_some()
                    {
                        let mut guard = tx_cell.lock().await;
                        if let Some(sender) = guard.take() {
                            let _ = sender.send(env.enriched_preprompt.unwrap_or_default());
                        }
                    }
                })
            }),
        )
        .await
    {
        Ok(h) => h,
        Err(_) => return None,
    };

    let envelope = MentorChatEnvelope::request(
        correlation_id.clone(),
        session_id.to_string(),
        entity_id.to_string(),
        message.to_string(),
        model_override,
    );
    if let Ok(payload) = serde_json::to_vec(&envelope) {
        let mut headers = HashMap::new();
        headers.insert("stage".to_string(), "request".to_string());
        headers.insert("correlation_id".to_string(), correlation_id);
        headers.insert("entity_subscriber_group".to_string(), group);
        let _ = broker
            .publish(
                STREAM,
                CHAT_TOPIC,
                StreamMessage::with_headers(payload, headers),
            )
            .await;
    }

    let out = tokio::time::timeout(Duration::from_millis(500), rx)
        .await
        .ok()
        .and_then(|v| v.ok());
    sub.cancel();
    out
}

pub fn inject_preprompt(base_prompt: &str, preprompt: Option<String>) -> String {
    match preprompt {
        Some(p) if !p.trim().is_empty() => format!("{}\n\n## Mentor Monitor\n{}\n", base_prompt, p),
        _ => base_prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::RoutingMode;
    use abigail_streaming::MemoryBroker;

    #[tokio::test]
    async fn request_enriched_roundtrip() {
        let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());
        let monitor = MentorChatMonitor::new(broker.clone());
        let _h = monitor.spawn().await.unwrap();
        let router = Arc::new(IdEgoRouter::new(
            None,
            None,
            None,
            None,
            RoutingMode::EgoPrimary,
        ));

        let got = request_enriched_preprompt(
            broker,
            router,
            "entity-1",
            "session-1",
            "hello",
            Some("gpt-4.1".to_string()),
        )
        .await;
        assert!(got.is_some());
        let out = got.unwrap();
        assert!(out.contains("Constitutional Monitor Context"));
        assert!(out.contains("Runtime Signals"));
    }
}
