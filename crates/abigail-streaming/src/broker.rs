//! `StreamBroker` trait — the abstraction boundary for messaging backends.

use crate::types::{StreamMessage, SubscriptionHandle, TopicConfig};
use std::future::Future;
use std::pin::Pin;

/// Callback type for stream subscriptions.
pub type MessageHandler = Box<
    dyn Fn(StreamMessage) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// Trait abstracting over streaming message brokers.
///
/// Phase 1: `MemoryBroker` (tokio broadcast channels, in-process only).
/// Phase 3: `IggyBroker` (persistent, multi-consumer, ordered).
///
/// All methods are async to accommodate both in-memory and networked backends.
#[async_trait::async_trait]
pub trait StreamBroker: Send + Sync + 'static {
    /// Publish a message to a topic within a stream.
    async fn publish(
        &self,
        stream: &str,
        topic: &str,
        message: StreamMessage,
    ) -> anyhow::Result<()>;

    /// Poll messages from a consumer group's current offset.
    ///
    /// Returns up to `batch_size` messages starting from the consumer group's
    /// last acknowledged position. For `MemoryBroker`, this returns any messages
    /// currently buffered in the broadcast channel.
    async fn poll(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        batch_size: u32,
    ) -> anyhow::Result<Vec<StreamMessage>>;

    /// Subscribe to a topic with a callback handler.
    ///
    /// The handler is invoked for each new message. Returns a `SubscriptionHandle`
    /// that can be used to cancel the subscription.
    async fn subscribe(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        handler: MessageHandler,
    ) -> anyhow::Result<SubscriptionHandle>;

    /// Ensure a topic exists with the given configuration.
    ///
    /// Idempotent — if the topic already exists, this is a no-op.
    async fn ensure_topic(
        &self,
        stream: &str,
        topic: &str,
        config: TopicConfig,
    ) -> anyhow::Result<()>;

    /// Ensure a consumer group exists for a topic.
    ///
    /// Idempotent — if the group already exists, this is a no-op.
    async fn ensure_consumer_group(
        &self,
        stream: &str,
        topic: &str,
        group_name: &str,
    ) -> anyhow::Result<()>;
}
