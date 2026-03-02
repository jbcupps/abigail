//! Iggy-backed `StreamBroker` implementation.
//!
//! This is the production broker for alpha. It provides persistent topics and
//! consumer groups backed by Apache Iggy.

use crate::broker::{MessageHandler, StreamBroker};
use crate::types::{StreamMessage, SubscriptionHandle, TopicConfig};
use anyhow::Context;
use iggy::prelude::{
    AutoCommit, AutoCommitWhen, DirectConfig, IggyClient, IggyDuration, IggyMessage, Partitioning,
    PollingStrategy,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{sleep, timeout, Duration};
use tokio_util::sync::CancellationToken;

/// Runtime configuration for `IggyBroker`.
#[derive(Debug, Clone)]
pub struct IggyBrokerConfig {
    /// Iggy connection string, e.g. `iggy://iggy:iggy@127.0.0.1:8090`.
    pub connection_string: String,
    /// Producer batch length.
    pub producer_batch_length: u32,
    /// Producer linger time in milliseconds.
    pub producer_linger_ms: u64,
    /// Consumer polling interval in milliseconds.
    pub consumer_poll_interval_ms: u64,
    /// Upper bound wait for a single poll call in milliseconds.
    pub consumer_poll_timeout_ms: u64,
}

impl Default for IggyBrokerConfig {
    fn default() -> Self {
        Self {
            connection_string: "iggy://iggy:iggy@127.0.0.1:8090".to_string(),
            producer_batch_length: 1000,
            producer_linger_ms: 1,
            consumer_poll_interval_ms: 10,
            consumer_poll_timeout_ms: 100,
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

    fn producer_linger_duration(&self) -> anyhow::Result<IggyDuration> {
        IggyDuration::from_str(&format!("{}ms", self.cfg.producer_linger_ms))
            .map_err(|e| anyhow::anyhow!("Invalid producer linger duration: {}", e))
    }

    fn consumer_poll_interval_duration(&self) -> anyhow::Result<IggyDuration> {
        IggyDuration::from_str(&format!("{}ms", self.cfg.consumer_poll_interval_ms))
            .map_err(|e| anyhow::anyhow!("Invalid consumer poll interval: {}", e))
    }

    fn consumer_auto_commit_duration(&self) -> anyhow::Result<IggyDuration> {
        IggyDuration::from_str("1s")
            .map_err(|e| anyhow::anyhow!("Invalid consumer auto-commit duration: {}", e))
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
        let linger = self.producer_linger_duration()?;
        let mut producer = self
            .client
            .producer(stream, topic)
            .with_context(|| format!("Failed to create producer for {}/{}", stream, topic))?
            .direct(
                DirectConfig::builder()
                    .batch_length(self.cfg.producer_batch_length)
                    .linger_time(linger)
                    .build(),
            )
            .partitioning(Partitioning::balanced())
            .build();

        // init() validates/creates stream and topic as needed.
        producer
            .init()
            .await
            .with_context(|| format!("Failed to initialize producer for {}/{}", stream, topic))?;

        let serialized = serde_json::to_string(&message)
            .context("Failed to serialize StreamMessage for publish")?;
        let iggy_message = IggyMessage::from_str(&serialized)
            .context("Failed to create Iggy message from StreamMessage payload")?;
        producer
            .send(vec![iggy_message])
            .await
            .with_context(|| format!("Failed to publish message to {}/{}", stream, topic))?;

        Ok(())
    }

    async fn poll(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        batch_size: u32,
    ) -> anyhow::Result<Vec<StreamMessage>> {
        let poll_interval = self.consumer_poll_interval_duration()?;
        let auto_commit_every = self.consumer_auto_commit_duration()?;
        let mut consumer = self
            .client
            .consumer_group(consumer_group, stream, topic)
            .with_context(|| {
                format!(
                    "Failed to create consumer for {}/{}, group={}",
                    stream, topic, consumer_group
                )
            })?
            .create_consumer_group_if_not_exists()
            .auto_join_consumer_group()
            .auto_commit(AutoCommit::IntervalOrWhen(
                auto_commit_every,
                AutoCommitWhen::ConsumingAllMessages,
            ))
            .polling_strategy(PollingStrategy::next())
            .poll_interval(poll_interval)
            .batch_length(batch_size)
            .build();

        consumer
            .init()
            .await
            .with_context(|| format!("Failed to initialize consumer {}/{}", stream, topic))?;

        let mut out = Vec::new();
        let wait = Duration::from_millis(self.cfg.consumer_poll_timeout_ms);
        while out.len() < batch_size as usize {
            let next = timeout(wait, consumer.next()).await;
            let Some(polled) = (match next {
                Ok(value) => value,
                Err(_) => break,
            }) else {
                break;
            };

            let received = polled.with_context(|| {
                format!(
                    "Failed reading message from {}/{}, group={}",
                    stream, topic, consumer_group
                )
            })?;

            let payload = received.message.payload.as_ref();
            let msg: StreamMessage = serde_json::from_slice(payload)
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

                // Avoid hot-looping when no messages are available.
                sleep(Duration::from_millis(25)).await;
            }
        });

        Ok(SubscriptionHandle::new(cancel))
    }

    async fn ensure_topic(
        &self,
        stream: &str,
        topic: &str,
        _config: TopicConfig,
    ) -> anyhow::Result<()> {
        let linger = self.producer_linger_duration()?;
        let mut producer = self
            .client
            .producer(stream, topic)
            .with_context(|| format!("Failed to create producer for {}/{}", stream, topic))?
            .direct(
                DirectConfig::builder()
                    .batch_length(1)
                    .linger_time(linger)
                    .build(),
            )
            .partitioning(Partitioning::balanced())
            .build();

        producer
            .init()
            .await
            .with_context(|| format!("Failed to ensure topic {}/{}", stream, topic))?;
        Ok(())
    }

    async fn ensure_consumer_group(
        &self,
        stream: &str,
        topic: &str,
        group_name: &str,
    ) -> anyhow::Result<()> {
        let poll_interval = self.consumer_poll_interval_duration()?;
        let auto_commit_every = self.consumer_auto_commit_duration()?;
        let mut consumer = self
            .client
            .consumer_group(group_name, stream, topic)
            .with_context(|| {
                format!(
                    "Failed to create consumer group for {}/{}, group={}",
                    stream, topic, group_name
                )
            })?
            .create_consumer_group_if_not_exists()
            .auto_join_consumer_group()
            .auto_commit(AutoCommit::IntervalOrWhen(
                auto_commit_every,
                AutoCommitWhen::ConsumingAllMessages,
            ))
            .polling_strategy(PollingStrategy::next())
            .poll_interval(poll_interval)
            .batch_length(1)
            .build();

        consumer.init().await.with_context(|| {
            format!(
                "Failed to ensure consumer group for {}/{}, group={}",
                stream, topic, group_name
            )
        })?;
        Ok(())
    }
}
