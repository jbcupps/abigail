//! In-memory `StreamBroker` implementation using tokio broadcast channels.
//!
//! Used for Phase 1 (no external dependencies) and for tests. Each topic
//! gets its own broadcast channel. Messages are ephemeral — they exist only
//! in the channel buffer and are lost when the broker is dropped.

use crate::broker::{MessageHandler, StreamBroker};
use crate::types::{StreamMessage, SubscriptionHandle, TopicConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

/// Type alias for consumer group tracking: maps (stream, topic, group) to presence flag.
type ConsumerGroupMap = HashMap<(String, String, String), bool>;

/// Key for a topic within a stream.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct TopicKey {
    stream: String,
    topic: String,
}

/// Internal state for a single topic.
struct TopicState {
    sender: broadcast::Sender<StreamMessage>,
    _config: TopicConfig,
}

/// In-memory stream broker backed by tokio broadcast channels.
///
/// Each `(stream, topic)` pair gets an independent broadcast channel with
/// a configurable buffer capacity. Consumer groups are tracked but since
/// broadcast channels don't support offset-based replay, `poll` returns
/// only messages received after the consumer group was created.
pub struct MemoryBroker {
    topics: Arc<RwLock<HashMap<TopicKey, TopicState>>>,
    /// Consumer group tracking: maps (stream, topic, group) -> receiver exists.
    consumer_groups: Arc<RwLock<ConsumerGroupMap>>,
    /// Default channel capacity for new topics.
    channel_capacity: usize,
}

impl MemoryBroker {
    /// Create a new in-memory broker with the given channel capacity.
    pub fn new(channel_capacity: usize) -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            consumer_groups: Arc::new(RwLock::new(HashMap::new())),
            channel_capacity,
        }
    }

    /// Get or create the broadcast sender for a topic.
    async fn get_or_create_sender(
        &self,
        stream: &str,
        topic: &str,
    ) -> broadcast::Sender<StreamMessage> {
        let key = TopicKey {
            stream: stream.to_string(),
            topic: topic.to_string(),
        };

        // Fast path: topic already exists
        {
            let topics = self.topics.read().await;
            if let Some(state) = topics.get(&key) {
                return state.sender.clone();
            }
        }

        // Slow path: create topic
        let mut topics = self.topics.write().await;
        // Double-check after acquiring write lock
        if let Some(state) = topics.get(&key) {
            return state.sender.clone();
        }

        let (sender, _) = broadcast::channel(self.channel_capacity);
        let state = TopicState {
            sender: sender.clone(),
            _config: TopicConfig::default(),
        };
        topics.insert(key, state);
        sender
    }
}

impl Default for MemoryBroker {
    fn default() -> Self {
        Self::new(256)
    }
}

#[async_trait::async_trait]
impl StreamBroker for MemoryBroker {
    async fn publish(
        &self,
        stream: &str,
        topic: &str,
        message: StreamMessage,
    ) -> anyhow::Result<()> {
        let sender = self.get_or_create_sender(stream, topic).await;
        // Ignore send errors (no active receivers is OK)
        let _ = sender.send(message);
        Ok(())
    }

    async fn poll(
        &self,
        stream: &str,
        topic: &str,
        _consumer_group: &str,
        batch_size: u32,
    ) -> anyhow::Result<Vec<StreamMessage>> {
        let sender = self.get_or_create_sender(stream, topic).await;
        let mut receiver = sender.subscribe();

        let mut messages = Vec::new();
        for _ in 0..batch_size {
            match receiver.try_recv() {
                Ok(msg) => messages.push(msg),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!("Consumer lagged by {} messages on {}/{}", n, stream, topic);
                    break;
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        Ok(messages)
    }

    async fn subscribe(
        &self,
        stream: &str,
        topic: &str,
        consumer_group: &str,
        handler: MessageHandler,
    ) -> anyhow::Result<SubscriptionHandle> {
        let sender = self.get_or_create_sender(stream, topic).await;
        let mut receiver = sender.subscribe();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let stream_name = stream.to_string();
        let topic_name = topic.to_string();
        let group_name = consumer_group.to_string();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => {
                        tracing::debug!(
                            "Subscription cancelled for {}/{} group={}",
                            stream_name, topic_name, group_name
                        );
                        break;
                    }
                    result = receiver.recv() => {
                        match result {
                            Ok(msg) => {
                                handler(msg).await;
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(
                                    "Subscriber lagged by {} on {}/{} group={}",
                                    n, stream_name, topic_name, group_name
                                );
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::info!(
                                    "Channel closed for {}/{} group={}",
                                    stream_name, topic_name, group_name
                                );
                                break;
                            }
                        }
                    }
                }
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
        let key = TopicKey {
            stream: stream.to_string(),
            topic: topic.to_string(),
        };

        let mut topics = self.topics.write().await;
        let capacity = self.channel_capacity;
        topics.entry(key).or_insert_with(|| {
            let (sender, _) = broadcast::channel(capacity);
            TopicState {
                sender,
                _config: config,
            }
        });
        Ok(())
    }

    async fn ensure_consumer_group(
        &self,
        stream: &str,
        topic: &str,
        group_name: &str,
    ) -> anyhow::Result<()> {
        let key = (
            stream.to_string(),
            topic.to_string(),
            group_name.to_string(),
        );
        let mut groups = self.consumer_groups.write().await;
        groups.entry(key).or_insert(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let broker = MemoryBroker::new(64);
        broker
            .ensure_topic("test-stream", "test-topic", TopicConfig::default())
            .await
            .unwrap();

        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        let handle = broker
            .subscribe(
                "test-stream",
                "test-topic",
                "test-group",
                Box::new(move |_msg| {
                    let c = count_clone.clone();
                    Box::pin(async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                }),
            )
            .await
            .unwrap();

        // Publish messages
        for i in 0..3 {
            let msg = StreamMessage::new(format!("message-{}", i).into_bytes());
            broker
                .publish("test-stream", "test-topic", msg)
                .await
                .unwrap();
        }

        // Give the subscriber time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(count.load(Ordering::SeqCst), 3);

        // Cancel subscription
        handle.cancel();
        assert!(handle.is_cancelled());
    }

    #[tokio::test]
    async fn test_publish_no_subscribers() {
        let broker = MemoryBroker::new(64);

        // Publishing without subscribers should not error
        let msg = StreamMessage::new(b"hello".to_vec());
        let result = broker.publish("s", "t", msg).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ensure_topic_idempotent() {
        let broker = MemoryBroker::new(64);

        broker
            .ensure_topic("s", "t", TopicConfig::default())
            .await
            .unwrap();
        broker
            .ensure_topic("s", "t", TopicConfig::default())
            .await
            .unwrap();

        let topics = broker.topics.read().await;
        assert_eq!(topics.len(), 1);
    }

    #[tokio::test]
    async fn test_ensure_consumer_group_idempotent() {
        let broker = MemoryBroker::new(64);

        broker.ensure_consumer_group("s", "t", "g1").await.unwrap();
        broker.ensure_consumer_group("s", "t", "g1").await.unwrap();

        let groups = broker.consumer_groups.read().await;
        assert_eq!(groups.len(), 1);
    }

    #[tokio::test]
    async fn test_auto_create_topic_on_publish() {
        let broker = MemoryBroker::new(64);

        // Topic doesn't exist yet, but publish should auto-create
        let msg = StreamMessage::new(b"auto".to_vec());
        broker.publish("s", "t", msg).await.unwrap();

        let topics = broker.topics.read().await;
        assert_eq!(topics.len(), 1);
    }

    #[tokio::test]
    async fn test_message_headers() {
        let mut headers = HashMap::new();
        headers.insert("event_type".to_string(), "job_queued".to_string());
        headers.insert("topic".to_string(), "research".to_string());

        let msg = StreamMessage::with_headers(b"payload".to_vec(), headers);
        assert_eq!(msg.headers.get("event_type").unwrap(), "job_queued");
        assert_eq!(msg.headers.get("topic").unwrap(), "research");
    }

    #[tokio::test]
    async fn test_message_json_payload() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct TestPayload {
            value: String,
        }

        let payload = TestPayload {
            value: "hello".to_string(),
        };
        let msg = StreamMessage::new(serde_json::to_vec(&payload).unwrap());

        let decoded: TestPayload = msg.payload_json().unwrap();
        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    async fn test_multiple_topics_independent() {
        let broker = MemoryBroker::new(64);

        let count_a = Arc::new(AtomicUsize::new(0));
        let count_b = Arc::new(AtomicUsize::new(0));

        let ca = count_a.clone();
        let _ha = broker
            .subscribe(
                "s",
                "topic-a",
                "g",
                Box::new(move |_| {
                    let c = ca.clone();
                    Box::pin(async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                }),
            )
            .await
            .unwrap();

        let cb = count_b.clone();
        let _hb = broker
            .subscribe(
                "s",
                "topic-b",
                "g",
                Box::new(move |_| {
                    let c = cb.clone();
                    Box::pin(async move {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                }),
            )
            .await
            .unwrap();

        // Publish to topic-a only
        broker
            .publish("s", "topic-a", StreamMessage::new(b"a".to_vec()))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(count_a.load(Ordering::SeqCst), 1);
        assert_eq!(count_b.load(Ordering::SeqCst), 0);
    }
}
