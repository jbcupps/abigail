//! Iggy-backed `StreamBroker` implementation.
//!
//! Uses the native Iggy Rust client (`iggy` crate) for persistent topics and
//! consumer groups.

use crate::broker::{MessageHandler, StreamBroker};
use crate::types::{StreamMessage, SubscriptionHandle, TopicConfig};
use anyhow::Context;
use iggy::client::{Client, ConsumerGroupClient, MessageClient, StreamClient, TopicClient};
use iggy::clients::client::IggyClient;
use iggy::compression::compression_algorithm::CompressionAlgorithm;
use iggy::consumer::Consumer;
use iggy::identifier::Identifier;
use iggy::messages::poll_messages::PollingStrategy;
use iggy::messages::send_messages::{Message as IggyMessage, Partitioning};
use iggy::utils::expiry::IggyExpiry;
use iggy::utils::topic_size::MaxTopicSize;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;

/// Runtime configuration for `IggyBroker`.
#[derive(Debug, Clone)]
pub struct IggyBrokerConfig {
    /// Iggy connection string, e.g. `iggy://iggy:iggy@127.0.0.1:8090`.
    pub connection_string: String,
    /// Poll interval used by the fallback `subscribe()` loop.
    pub subscribe_poll_interval_ms: u64,
}

impl Default for IggyBrokerConfig {
    fn default() -> Self {
        Self {
            connection_string: "iggy://iggy:iggy@127.0.0.1:8090".to_string(),
            subscribe_poll_interval_ms: 50,
        }
    }
}

#[derive(Clone)]
pub struct IggyBroker {
    client: Arc<IggyClient>,
    cfg: IggyBrokerConfig,
}

impl IggyBroker {
    /// Create a broker from a full connection string.
    pub fn new(connection_string: impl Into<String>) -> anyhow::Result<Self> {
        let cfg = IggyBrokerConfig {
            connection_string: connection_string.into(),
            ..IggyBrokerConfig::default()
        };
        Self::with_config(cfg)
    }

    /// Create a broker from explicit configuration.
    pub fn with_config(cfg: IggyBrokerConfig) -> anyhow::Result<Self> {
        let client = IggyClient::from_connection_string(&cfg.connection_string)
            .context("Failed to initialize Iggy client from connection string")?;
        Ok(Self {
            client: Arc::new(client),
            cfg,
        })
    }

    async fn ensure_connected(&self) -> anyhow::Result<()> {
        self.client
            .connect()
            .await
            .context("Failed to connect/authenticate with Iggy")
    }

    async fn ensure_stream_and_topic(
        &self,
        stream: &str,
        topic: &str,
        topic_cfg: &TopicConfig,
    ) -> anyhow::Result<()> {
        self.ensure_connected().await?;

        let stream_id = Identifier::named(stream).map_err(|e| anyhow::anyhow!("{}", e))?;
        let topic_id = Identifier::named(topic).map_err(|e| anyhow::anyhow!("{}", e))?;

        if self
            .client
            .get_stream(&stream_id)
            .await
            .context("Iggy get_stream failed")?
            .is_none()
        {
            self.client
                .create_stream(stream, None)
                .await
                .with_context(|| format!("Failed to create Iggy stream '{}'", stream))?;
        }

        if self
            .client
            .get_topic(&stream_id, &topic_id)
            .await
            .context("Iggy get_topic failed")?
            .is_none()
        {
            self.client
                .create_topic(
                    &stream_id,
                    topic,
                    topic_cfg.partitions.max(1),
                    CompressionAlgorithm::None,
                    None,
                    None,
                    expiry_from_topic_config(topic_cfg),
                    MaxTopicSize::ServerDefault,
                )
                .await
                .with_context(|| format!("Failed to create Iggy topic '{}/{}'", stream, topic))?;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl StreamBroker for IggyBroker {
    async fn publish(
        &self,
        stream: &str,
        topic: &str,
        message: StreamMessage,
    ) -> anyhow::Result<()> {
        self.ensure_stream_and_topic(stream, topic, &TopicConfig::default())
            .await?;

        let stream_id = Identifier::named(stream).map_err(|e| anyhow::anyhow!("{}", e))?;
        let topic_id = Identifier::named(topic).map_err(|e| anyhow::anyhow!("{}", e))?;
        let payload = serde_json::to_vec(&message)
            .context("Failed to serialize StreamMessage for Iggy publish")?;
        let mut messages = vec![IggyMessage::new(None, Bytes::from(payload), None)];

        self.client
            .send_messages(
                &stream_id,
                &topic_id,
                &Partitioning::balanced(),
                &mut messages,
            )
            .await
            .with_context(|| format!("Failed to publish to Iggy topic '{}/{}'", stream, topic))
    }

    async fn poll(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        batch_size: u32,
    ) -> anyhow::Result<Vec<StreamMessage>> {
        self.ensure_stream_and_topic(stream, topic, &TopicConfig::default())
            .await?;
        self.ensure_consumer_group(stream, topic, consumer_group)
            .await?;

        let stream_id = Identifier::named(stream).map_err(|e| anyhow::anyhow!("{}", e))?;
        let topic_id = Identifier::named(topic).map_err(|e| anyhow::anyhow!("{}", e))?;
        let group_id = Identifier::named(consumer_group).map_err(|e| anyhow::anyhow!("{}", e))?;
        let consumer = Consumer::group(group_id.clone());

        let polled = self
            .client
            .poll_messages(
                &stream_id,
                &topic_id,
                None,
                &consumer,
                &PollingStrategy::next(),
                batch_size.max(1),
                true, // auto-commit server offset for this group
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to poll from Iggy topic '{}/{}' for group '{}'",
                    stream, topic, consumer_group
                )
            })?;

        let mut out = Vec::new();
        for polled_msg in polled.messages {
            let msg: StreamMessage = serde_json::from_slice(polled_msg.payload.as_ref())
                .context("Failed to deserialize StreamMessage from Iggy payload")?;
            out.push(msg);
        }
        Ok(out)
    }

    async fn subscribe(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        handler: MessageHandler,
    ) -> anyhow::Result<SubscriptionHandle> {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let broker = self.clone();
        let stream = stream.to_string();
        let topic = topic.to_string();
        let group = consumer_group.to_string();
        let poll_every = Duration::from_millis(self.cfg.subscribe_poll_interval_ms.max(10));

        tokio::spawn(async move {
            while !cancel_clone.is_cancelled() {
                match broker.poll(&stream, &topic, &group, 64).await {
                    Ok(messages) => {
                        for msg in messages {
                            if cancel_clone.is_cancelled() {
                                break;
                            }
                            handler(msg).await;
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            "Iggy subscription poll failed for {}/{}, group={}: {}",
                            stream,
                            topic,
                            group,
                            err
                        );
                    }
                }
                sleep(poll_every).await;
            }
        });

        Ok(SubscriptionHandle::new(cancel))
    }

    async fn ensure_topic(
        &self,
        stream: &str,
        topic: &str,
        config: TopicConfig,
    ) -> anyhow::Result<()> {
        self.ensure_stream_and_topic(stream, topic, &config).await
    }

    async fn ensure_consumer_group(
        &self,
        stream: &str,
        topic: &str,
        group_name: &str,
    ) -> anyhow::Result<()> {
        self.ensure_stream_and_topic(stream, topic, &TopicConfig::default())
            .await?;

        let stream_id = Identifier::named(stream).map_err(|e| anyhow::anyhow!("{}", e))?;
        let topic_id = Identifier::named(topic).map_err(|e| anyhow::anyhow!("{}", e))?;
        let group_id = Identifier::named(group_name).map_err(|e| anyhow::anyhow!("{}", e))?;

        if self
            .client
            .get_consumer_group(&stream_id, &topic_id, &group_id)
            .await
            .context("Iggy get_consumer_group failed")?
            .is_none()
        {
            self.client
                .create_consumer_group(&stream_id, &topic_id, group_name, None)
                .await
                .with_context(|| {
                    format!(
                        "Failed to create Iggy consumer group '{}' for {}/{}",
                        group_name, stream, topic
                    )
                })?;
        }

        // Join on ensure so polling with this group can start immediately.
        self.client
            .join_consumer_group(&stream_id, &topic_id, &group_id)
            .await
            .with_context(|| {
                format!(
                    "Failed to join Iggy consumer group '{}' for {}/{}",
                    group_name, stream, topic
                )
            })?;

        Ok(())
    }
}

fn expiry_from_topic_config(cfg: &TopicConfig) -> IggyExpiry {
    if cfg.retention_seconds == 0 {
        return IggyExpiry::NeverExpire;
    }
    // IggyDuration::from(u64) expects microseconds.
    let micros = cfg.retention_seconds.saturating_mul(1_000_000);
    IggyExpiry::from(micros)
}
