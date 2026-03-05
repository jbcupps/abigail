//! Out-of-band Superego monitor for ethical/safety checks over chat topic.

use abigail_streaming::{StreamBroker, StreamMessage, SubscriptionHandle, TopicConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const STREAM: &str = "entity";
const CHAT_TOPIC: &str = "chat-topic";
const SIGNAL_TOPIC: &str = "ethical-signals";
const CONSUMER_GROUP: &str = "superego-monitor";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatTopicEnvelope {
    correlation_id: String,
    session_id: String,
    message: String,
    stage: String,
    #[serde(default)]
    enriched_preprompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperegoSignal {
    pub correlation_id: String,
    pub session_id: String,
    pub severity: String,
    pub rule: String,
    pub should_block: bool,
    pub created_at_utc: chrono::DateTime<chrono::Utc>,
}

pub struct SuperegoMonitor {
    broker: Arc<dyn StreamBroker>,
}

impl SuperegoMonitor {
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

                let text = env
                    .enriched_preprompt
                    .clone()
                    .unwrap_or_else(|| env.message.clone())
                    .to_lowercase();
                let mut maybe_signal: Option<SuperegoSignal> = None;

                if text.contains("rm -rf")
                    || text.contains("drop table")
                    || text.contains("delete all")
                {
                    maybe_signal = Some(SuperegoSignal {
                        correlation_id: env.correlation_id.clone(),
                        session_id: env.session_id.clone(),
                        severity: "critical".to_string(),
                        rule: "destructive-pattern".to_string(),
                        should_block: true,
                        created_at_utc: chrono::Utc::now(),
                    });
                } else if text.contains('@') && text.contains('.') {
                    maybe_signal = Some(SuperegoSignal {
                        correlation_id: env.correlation_id.clone(),
                        session_id: env.session_id.clone(),
                        severity: "warning".to_string(),
                        rule: "pii-likelihood".to_string(),
                        should_block: false,
                        created_at_utc: chrono::Utc::now(),
                    });
                }

                let Some(signal) = maybe_signal else {
                    return;
                };
                let Ok(payload) = serde_json::to_vec(&signal) else {
                    return;
                };
                let mut headers = HashMap::new();
                headers.insert("monitor".to_string(), "superego".to_string());
                headers.insert("correlation_id".to_string(), signal.correlation_id.clone());
                headers.insert("severity".to_string(), signal.severity.clone());

                if let Err(e) = broker
                    .publish(
                        STREAM,
                        SIGNAL_TOPIC,
                        StreamMessage::with_headers(payload, headers),
                    )
                    .await
                {
                    tracing::warn!("superego monitor publish failed: {}", e);
                }
            })
        });

        let handle = self
            .broker
            .subscribe(STREAM, CHAT_TOPIC, CONSUMER_GROUP, handler)
            .await?;
        tracing::info!("Superego monitor subscribed to {}/{}", STREAM, CHAT_TOPIC);
        Ok(handle)
    }
}

/// Startup helper for the out-of-band Superego monitor.
pub async fn start(broker: Arc<dyn StreamBroker>) -> anyhow::Result<SubscriptionHandle> {
    SuperegoMonitor::new(broker).spawn().await
}
