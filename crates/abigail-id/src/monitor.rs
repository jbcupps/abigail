//! Out-of-band Id monitor for quick safety/feasibility checks.

use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle, TopicConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const STREAM: &str = "entity";
const CHAT_TOPIC: &str = "chat-topic";
const SIGNAL_TOPIC: &str = "id-signals";
const CONSUMER_GROUP: &str = "id-monitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatTopicEnvelope {
    correlation_id: String,
    session_id: String,
    message: String,
    stage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdSafetySignal {
    pub correlation_id: String,
    pub session_id: String,
    pub risk_score: u8,
    pub safe: bool,
    pub reason: String,
    pub created_at_utc: chrono::DateTime<chrono::Utc>,
}

pub struct IdMonitor {
    broker: Arc<dyn StreamBroker>,
}

impl IdMonitor {
    pub fn new(broker: Arc<dyn StreamBroker>) -> Self {
        Self { broker }
    }

    pub async fn spawn(self) -> anyhow::Result<SubscriptionHandle> {
        self.broker
            .ensure_topic(STREAM, CHAT_TOPIC, TopicConfig::default())
            .await?;
        self.broker
            .ensure_topic(STREAM, SIGNAL_TOPIC, TopicConfig::default())
            .await?;
        self.broker
            .ensure_consumer_group(STREAM, CHAT_TOPIC, CONSUMER_GROUP)
            .await?;

        let broker = self.broker.clone();
        let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
            let broker = broker.clone();
            Box::pin(async move {
                let Ok(env) = serde_json::from_slice::<ChatTopicEnvelope>(&msg.payload) else {
                    return;
                };
                if env.stage != "enriched" {
                    return;
                }

                let lower = env.message.to_lowercase();
                let mut risk = 5u8;
                if lower.contains("delete")
                    || lower.contains("drop table")
                    || lower.contains("wipe")
                {
                    risk = 90;
                } else if lower.len() > 2000 {
                    risk = 45;
                } else if lower.contains("http://") || lower.contains("https://") {
                    risk = 25;
                }
                let safe = risk < 80;
                let reason = if safe {
                    "within id safety threshold".to_string()
                } else {
                    "high-risk destructive or unsafe pattern detected".to_string()
                };

                let signal = IdSafetySignal {
                    correlation_id: env.correlation_id.clone(),
                    session_id: env.session_id.clone(),
                    risk_score: risk,
                    safe,
                    reason,
                    created_at_utc: chrono::Utc::now(),
                };

                let Ok(payload) = serde_json::to_vec(&signal) else {
                    return;
                };
                let mut headers = HashMap::new();
                headers.insert("monitor".to_string(), "id".to_string());
                headers.insert("correlation_id".to_string(), signal.correlation_id.clone());
                headers.insert("risk_score".to_string(), signal.risk_score.to_string());

                if let Err(e) = broker
                    .publish(
                        STREAM,
                        SIGNAL_TOPIC,
                        StreamMessage::with_headers(payload, headers),
                    )
                    .await
                {
                    tracing::warn!("id monitor publish failed: {}", e);
                }
            })
        });

        let handle = self
            .broker
            .subscribe(STREAM, CHAT_TOPIC, CONSUMER_GROUP, handler)
            .await?;
        tracing::info!("Id monitor subscribed to {}/{}", STREAM, CHAT_TOPIC);
        Ok(handle)
    }
}
