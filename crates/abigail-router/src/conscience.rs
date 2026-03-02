//! Conscience monitor — async ethical evaluation via StreamBroker topics.
//!
//! Replaces the synchronous `spawn_conscience_monitor()` stub with a topic-based
//! consumer that subscribes to `"entity/conscience-check"`, evaluates requests
//! against pattern rules, and publishes signals to `"entity/ethical-signals"`.
//!
//! Phase 1: PII patterns, destructive keyword detection.
//! Phase 2 (future): LLM-based ethical evaluation.

use abigail_streaming::{StreamBroker, SubscriptionHandle, TopicConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Stream used for conscience events.
const STREAM: &str = "entity";
/// Topic for incoming conscience check requests.
const CHECK_TOPIC: &str = "conscience-check";
/// Topic for outgoing ethical signals.
const SIGNAL_TOPIC: &str = "ethical-signals";

/// A request to evaluate a message or action for ethical concerns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConscienceRequest {
    /// Unique request ID for correlation.
    pub request_id: String,
    /// The content to evaluate.
    pub content: String,
    /// Category of check (e.g. "chat_message", "tool_execution", "agentic_action").
    pub context: String,
    /// Optional metadata for richer evaluation.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Ethical category of a flagged issue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EthicalCategory {
    /// Personally identifiable information detected.
    Pii,
    /// Destructive or harmful intent detected.
    Destructive,
    /// Content that conflicts with constitutional ethics.
    ConstitutionalViolation,
    /// Trust/verification concern.
    TrustConcern,
    /// Custom category for future extensions.
    Custom(String),
}

/// Severity level of a conscience signal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational — no action needed but noted.
    Info,
    /// Warning — proceed with caution.
    Warning,
    /// Critical — action should be blocked or reviewed.
    Critical,
}

/// Signal emitted when a conscience check identifies a concern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConscienceSignal {
    /// Correlates back to the original request.
    pub request_id: String,
    /// What ethical category was flagged.
    pub category: EthicalCategory,
    /// How severe the concern is.
    pub severity: Severity,
    /// Human-readable explanation of the concern.
    pub description: String,
    /// Whether the action should be blocked.
    pub should_block: bool,
}

/// Conscience consumer that subscribes to check requests and publishes signals.
pub struct ConscienceConsumer {
    broker: Arc<dyn StreamBroker>,
}

/// Consumer group name for the conscience consumer.
const CONSUMER_GROUP: &str = "conscience-consumer";

impl ConscienceConsumer {
    pub fn new(broker: Arc<dyn StreamBroker>) -> Self {
        Self { broker }
    }

    /// Start the conscience consumer as a background subscription.
    /// Returns a SubscriptionHandle that cancels the consumer when dropped.
    pub async fn spawn(self) -> anyhow::Result<SubscriptionHandle> {
        // Ensure topics exist
        self.broker
            .ensure_topic(STREAM, CHECK_TOPIC, TopicConfig::default())
            .await?;
        self.broker
            .ensure_topic(STREAM, SIGNAL_TOPIC, TopicConfig::default())
            .await?;
        self.broker
            .ensure_consumer_group(STREAM, CHECK_TOPIC, CONSUMER_GROUP)
            .await?;

        let broker = self.broker.clone();
        let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
            let broker = broker.clone();
            Box::pin(async move {
                let request: ConscienceRequest = match serde_json::from_slice(&msg.payload) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::debug!("ConscienceConsumer: invalid message: {}", e);
                        return;
                    }
                };
                let signals = Self::evaluate_static(&request);
                for signal in signals {
                    Self::publish_signal_static(broker.as_ref(), &signal).await;
                }
            })
        });

        let handle = self
            .broker
            .subscribe(STREAM, CHECK_TOPIC, CONSUMER_GROUP, handler)
            .await?;
        tracing::info!(
            "ConscienceConsumer subscribed to {}/{}",
            STREAM,
            CHECK_TOPIC
        );
        Ok(handle)
    }

    /// Evaluate a request against Phase 1 pattern rules (static version for handler closure).
    fn evaluate_static(request: &ConscienceRequest) -> Vec<ConscienceSignal> {
        let mut signals = Vec::new();
        let lower = request.content.to_lowercase();

        // PII detection (Phase 1: simple patterns)
        if Self::has_pii_patterns(&lower) {
            signals.push(ConscienceSignal {
                request_id: request.request_id.clone(),
                category: EthicalCategory::Pii,
                severity: Severity::Warning,
                description: "Content may contain personally identifiable information.".into(),
                should_block: false,
            });
        }

        // Destructive intent detection
        if Self::has_destructive_patterns(&lower) {
            signals.push(ConscienceSignal {
                request_id: request.request_id.clone(),
                category: EthicalCategory::Destructive,
                severity: Severity::Critical,
                description: "Content contains potentially destructive or harmful patterns.".into(),
                should_block: true,
            });
        }

        signals
    }

    /// Check for common PII patterns (emails, phone numbers, SSNs).
    fn has_pii_patterns(text: &str) -> bool {
        // Email pattern (simple)
        if text.contains('@') && text.contains('.') {
            let words: Vec<&str> = text.split_whitespace().collect();
            for word in &words {
                if word.contains('@')
                    && word.contains('.')
                    && word.len() > 5
                    && !word.starts_with("http")
                {
                    return true;
                }
            }
        }
        // SSN pattern (XXX-XX-XXXX)
        let ssn_chars: Vec<char> = text.chars().collect();
        for window in ssn_chars.windows(11) {
            let s: String = window.iter().collect();
            if s.len() == 11
                && s.chars().nth(3) == Some('-')
                && s.chars().nth(6) == Some('-')
                && s.chars()
                    .enumerate()
                    .all(|(i, c)| i == 3 || i == 6 || c.is_ascii_digit())
            {
                return true;
            }
        }
        false
    }

    /// Check for destructive patterns (data deletion, system harm).
    fn has_destructive_patterns(text: &str) -> bool {
        const DESTRUCTIVE_KEYWORDS: &[&str] = &[
            "drop table",
            "delete all",
            "rm -rf",
            "format c:",
            "destroy",
            "wipe all data",
            "truncate table",
            "delete database",
        ];
        DESTRUCTIVE_KEYWORDS.iter().any(|kw| text.contains(kw))
    }

    async fn publish_signal_static(broker: &dyn StreamBroker, signal: &ConscienceSignal) {
        let payload = match serde_json::to_vec(signal) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to serialize ConscienceSignal: {}", e);
                return;
            }
        };
        let mut headers = HashMap::new();
        headers.insert(
            "category".to_string(),
            serde_json::to_string(&signal.category).unwrap_or_default(),
        );
        headers.insert("request_id".to_string(), signal.request_id.clone());
        headers.insert("should_block".to_string(), signal.should_block.to_string());

        let msg = abigail_streaming::StreamMessage::with_headers(payload, headers);
        if let Err(e) = broker.publish(STREAM, SIGNAL_TOPIC, msg).await {
            tracing::error!("Failed to publish ConscienceSignal: {}", e);
        }
    }
}

/// Helper: publish a conscience check request to the broker.
pub async fn request_conscience_check(
    broker: &dyn StreamBroker,
    content: String,
    context: String,
) -> anyhow::Result<String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let request = ConscienceRequest {
        request_id: request_id.clone(),
        content,
        context,
        metadata: HashMap::new(),
    };
    let payload = serde_json::to_vec(&request)?;
    let mut headers = HashMap::new();
    headers.insert("request_id".to_string(), request_id.clone());
    headers.insert("context".to_string(), request.context);

    let msg = abigail_streaming::StreamMessage::with_headers(payload, headers);
    broker.publish(STREAM, CHECK_TOPIC, msg).await?;
    Ok(request_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pii_email_detection() {
        assert!(ConscienceConsumer::has_pii_patterns(
            "please email john@example.com"
        ));
        assert!(!ConscienceConsumer::has_pii_patterns(
            "just a normal message"
        ));
    }

    #[test]
    fn test_pii_ssn_detection() {
        assert!(ConscienceConsumer::has_pii_patterns(
            "my ssn is 123-45-6789"
        ));
        assert!(!ConscienceConsumer::has_pii_patterns("my number is 12345"));
    }

    #[test]
    fn test_destructive_patterns() {
        assert!(ConscienceConsumer::has_destructive_patterns(
            "please drop table users"
        ));
        assert!(ConscienceConsumer::has_destructive_patterns("run rm -rf /"));
        assert!(!ConscienceConsumer::has_destructive_patterns(
            "please summarize this document"
        ));
    }

    #[test]
    fn test_evaluate_clean_message() {
        let request = ConscienceRequest {
            request_id: "test-1".into(),
            content: "What is the weather today?".into(),
            context: "chat_message".into(),
            metadata: HashMap::new(),
        };
        let signals = ConscienceConsumer::evaluate_static(&request);
        assert!(signals.is_empty());
    }

    #[test]
    fn test_evaluate_pii_message() {
        let request = ConscienceRequest {
            request_id: "test-2".into(),
            content: "Send this to alice@corp.com".into(),
            context: "chat_message".into(),
            metadata: HashMap::new(),
        };
        let signals = ConscienceConsumer::evaluate_static(&request);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].category, EthicalCategory::Pii);
        assert!(!signals[0].should_block);
    }

    #[test]
    fn test_evaluate_destructive_message() {
        let request = ConscienceRequest {
            request_id: "test-3".into(),
            content: "Please drop table users from the database".into(),
            context: "tool_execution".into(),
            metadata: HashMap::new(),
        };
        let signals = ConscienceConsumer::evaluate_static(&request);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].category, EthicalCategory::Destructive);
        assert!(signals[0].should_block);
    }

    #[test]
    fn test_evaluate_multiple_flags() {
        let request = ConscienceRequest {
            request_id: "test-4".into(),
            content: "delete all records for user@example.com".into(),
            context: "tool_execution".into(),
            metadata: HashMap::new(),
        };
        let signals = ConscienceConsumer::evaluate_static(&request);
        assert_eq!(signals.len(), 2);
    }
}
