//! Out-of-band chat-topic subscriber for memory correlation persistence.

use crate::{plan_secret_move, ConversationTurn, MemoryStore};
use abigail_streaming::{StreamBroker, SubscriptionHandle, Topic, TopicConfig, BUS_STREAM};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const STREAM: &str = BUS_STREAM;
const TOPIC: &str = Topic::MentorInput.as_str();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticCode {
    EnvelopeTruncated,
    EnvelopeInvalid,
    StageIgnored,
    SecretCaptureFailed,
    TurnPersisted,
    TurnPersistFailed,
}

/// Diagnostics must never contain envelope fields, identifiers, or error text.
/// The count saturates at 65,535, keeping every record small even for large input.
#[derive(Debug, Clone, Copy, Serialize)]
struct MemoryDiagnostic {
    code: DiagnosticCode,
    payload_bytes: u16,
}

impl MemoryDiagnostic {
    fn new(code: DiagnosticCode, payload_bytes: usize) -> Self {
        Self {
            code,
            payload_bytes: payload_bytes.min(u16::MAX as usize) as u16,
        }
    }
}

type DiagnosticSink = Arc<dyn Fn(MemoryDiagnostic) + Send + Sync>;

fn append_memory_superego_log(dir: &Path, diagnostic: MemoryDiagnostic) {
    if std::fs::create_dir_all(dir).is_err() {
        tracing::warn!(
            code = "diagnostic_directory_unavailable",
            "memory subscriber"
        );
        return;
    }

    let Ok(entry) = serde_json::to_string(&diagnostic) else {
        tracing::warn!(
            code = "diagnostic_serialization_failed",
            "memory subscriber"
        );
        return;
    };
    let path = dir.join("memory_superego_context.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            if writeln!(file, "{}", entry).is_err() {
                tracing::warn!(code = "diagnostic_write_failed", "memory subscriber");
            }
        }
        Err(_) => tracing::warn!(code = "diagnostic_open_failed", "memory subscriber"),
    }
}

/// Subscribe to `entity/chat-topic` and persist enriched chat-topic envelopes
/// into `conversation_turns` with correlation metadata.
pub async fn spawn_chat_topic_subscriber(
    broker: Arc<dyn StreamBroker>,
    memory: Arc<MemoryStore>,
) -> anyhow::Result<SubscriptionHandle> {
    let dir = PathBuf::from(
        std::env::var("HIVE_DOCUMENTS_PATH").unwrap_or_else(|_| "hive/documents".to_string()),
    );
    let diagnostics = Arc::new(move |diagnostic| append_memory_superego_log(&dir, diagnostic));
    spawn_with_diagnostics(broker, memory, diagnostics).await
}

async fn spawn_with_diagnostics(
    broker: Arc<dyn StreamBroker>,
    memory: Arc<MemoryStore>,
    diagnostics: DiagnosticSink,
) -> anyhow::Result<SubscriptionHandle> {
    broker
        .ensure_topic(STREAM, TOPIC, TopicConfig::default())
        .await?;
    broker
        .ensure_consumer_group(STREAM, TOPIC, CONSUMER_GROUP)
        .await?;

    let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
        let memory = memory.clone();
        let diagnostics = diagnostics.clone();
        Box::pin(async move {
            let report = |code| diagnostics(MemoryDiagnostic::new(code, msg.payload.len()));
            let env = match serde_json::from_slice::<ChatTopicEnvelope>(&msg.payload) {
                Ok(env) => env,
                Err(error) => {
                    report(if error.is_eof() {
                        DiagnosticCode::EnvelopeTruncated
                    } else {
                        DiagnosticCode::EnvelopeInvalid
                    });
                    return;
                }
            };
            if env.stage != "enriched" {
                report(DiagnosticCode::StageIgnored);
                return;
            }

            let api_key_secret =
                !abigail_core::key_detection::detect_api_keys(&env.message).is_empty();
            if api_key_secret {
                if memory
                    .capture_secret_message(
                        &env.entity_id,
                        &env.session_id,
                        "user",
                        &env.message,
                        "entity/mentor.input",
                    )
                    .is_err()
                {
                    report(DiagnosticCode::SecretCaptureFailed);
                }
            }

            let content = env
                .enriched_preprompt
                .clone()
                .unwrap_or_else(|| env.message.clone());
            let content = plan_secret_move(Some(&env.entity_id), &env.message)
                .map(|plan| plan.redacted_excerpt)
                .unwrap_or(content);
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

            report(if memory.insert_turn(&turn).is_ok() {
                DiagnosticCode::TurnPersisted
            } else {
                DiagnosticCode::TurnPersistFailed
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_streaming::{MemoryBroker, StreamMessage};
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    // These tests use deterministic synthetic inputs, the real broker, and the
    // real ephemeral MemoryStore. They do not call a model or demonstrate learning.
    struct TestLog(PathBuf);

    impl TestLog {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "abigail-memory-diagnostics-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }

        fn read(&self) -> String {
            std::fs::read_to_string(self.0.join("memory_superego_context.log")).unwrap()
        }
    }

    impl Drop for TestLog {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("memory_superego_context.log"));
            let _ = std::fs::remove_file(self.0.join("blocked"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    async fn subscribe(
        dir: PathBuf,
    ) -> (
        Arc<dyn StreamBroker>,
        Arc<MemoryStore>,
        SubscriptionHandle,
        UnboundedReceiver<MemoryDiagnostic>,
    ) {
        let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::default());
        let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
        let (tx, rx) = unbounded_channel();
        let diagnostics = Arc::new(move |diagnostic| {
            append_memory_superego_log(&dir, diagnostic);
            let _ = tx.send(diagnostic);
        });
        let handle = spawn_with_diagnostics(broker.clone(), memory.clone(), diagnostics)
            .await
            .unwrap();
        (broker, memory, handle, rx)
    }

    async fn publish(broker: &Arc<dyn StreamBroker>, payload: Vec<u8>) {
        broker
            .publish(STREAM, TOPIC, StreamMessage::new(payload))
            .await
            .unwrap();
    }

    async fn receive(rx: &mut UnboundedReceiver<MemoryDiagnostic>, expected: DiagnosticCode) {
        let diagnostic = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("subscriber did not complete")
            .expect("diagnostic channel closed");
        assert_eq!(diagnostic.code, expected);
    }

    fn envelope() -> ChatTopicEnvelope {
        ChatTopicEnvelope {
            correlation_id: "sensitive-correlation\nforged-record=true".to_string(),
            session_id: "sensitive-session".to_string(),
            entity_id: "00000000-0000-4000-8000-000000000001".to_string(),
            message: "synthetic private message".to_string(),
            selected_model: Some("sensitive-model".to_string()),
            stage: "enriched".to_string(),
            enriched_preprompt: Some("synthetic private prompt".to_string()),
            id_context: Some("sensitive-id-context".to_string()),
            superego_context: Some("sensitive-superego-context".to_string()),
        }
    }

    fn assert_content_free_records(log: &str, count: usize) {
        let records: Vec<_> = log.lines().collect();
        assert_eq!(records.len(), count);
        for line in records {
            assert!(line.len() < 100, "diagnostic record must stay bounded");
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            let fields = record.as_object().unwrap();
            assert_eq!(fields.len(), 2);
            assert!(matches!(
                fields["code"].as_str().unwrap(),
                "envelope_truncated"
                    | "envelope_invalid"
                    | "stage_ignored"
                    | "secret_capture_failed"
                    | "turn_persisted"
                    | "turn_persist_failed"
            ));
            assert!(fields["payload_bytes"].as_u64().unwrap() <= u16::MAX as u64);
        }
    }

    #[tokio::test]
    async fn subscriber_diagnostics_exclude_content_and_preserve_secret_memory() {
        let log = TestLog::new();
        let (broker, memory, _handle, mut rx) = subscribe(log.0.clone()).await;
        let mut env = envelope();
        let synthetic_key = "sk-syntheticabcdefghijklmnopqrstuvwxyz";
        env.message = format!("Synthetic test API key: {synthetic_key}");
        env.enriched_preprompt = Some("sensitive-prompt".repeat(6000));
        publish(&broker, serde_json::to_vec(&env).unwrap()).await;
        receive(&mut rx, DiagnosticCode::TurnPersisted).await;

        let turns = memory.recent_turns(&env.session_id, 10).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, "mentor_monitor");
        assert!(turns[0].content.contains("secret moved to Secrets Vault"));
        assert!(!turns[0].content.contains(synthetic_key));
        let topics = memory.list_protected_topics(10).unwrap();
        assert_eq!(topics.len(), 1);
        let protected = memory
            .protected_topic_entries(&topics[0].topic_name, 10)
            .unwrap();
        assert_eq!(protected.len(), 1);
        assert_eq!(protected[0].content, env.message);

        let text = log.read();
        assert_content_free_records(&text, 1);
        assert!(!text.contains(synthetic_key));
        assert!(!text.contains("sensitive-"));
        assert!(!text.contains(&env.entity_id));
        assert!(!text.contains("forged-record"));
        let record: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(record["payload_bytes"], u16::MAX);
    }

    #[tokio::test]
    async fn subscriber_diagnoses_invalid_input_and_continues_persisting() {
        let log = TestLog::new();
        let (broker, memory, _handle, mut rx) = subscribe(log.0.clone()).await;
        publish(
            &broker,
            br#"{"message":"synthetic-secret-truncated"#.to_vec(),
        )
        .await;
        receive(&mut rx, DiagnosticCode::EnvelopeTruncated).await;
        publish(
            &broker,
            br#"{"correlation_id":{"synthetic-secret-invalid":"private"}}"#.to_vec(),
        )
        .await;
        receive(&mut rx, DiagnosticCode::EnvelopeInvalid).await;

        let mut env = envelope();
        env.stage = "sensitive-ignored-stage".to_string();
        publish(&broker, serde_json::to_vec(&env).unwrap()).await;
        receive(&mut rx, DiagnosticCode::StageIgnored).await;
        assert_eq!(memory.total_turn_count().unwrap(), 0);

        env.stage = "enriched".to_string();
        publish(&broker, serde_json::to_vec(&env).unwrap()).await;
        receive(&mut rx, DiagnosticCode::TurnPersisted).await;
        let turns = memory.recent_turns(&env.session_id, 10).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].content, env.enriched_preprompt.unwrap());
        let text = log.read();
        assert_content_free_records(&text, 4);
        assert!(!text.contains("synthetic-secret"));
        assert!(!text.contains("sensitive-"));
    }

    #[tokio::test]
    async fn subscriber_persists_when_diagnostic_directory_is_unavailable() {
        let log = TestLog::new();
        let blocked = log.0.join("blocked");
        std::fs::write(&blocked, "synthetic path obstruction").unwrap();
        let (broker, memory, _handle, mut rx) = subscribe(blocked).await;
        let env = envelope();
        publish(&broker, serde_json::to_vec(&env).unwrap()).await;
        receive(&mut rx, DiagnosticCode::TurnPersisted).await;
        let turns = memory.recent_turns(&env.session_id, 10).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].content, env.enriched_preprompt.unwrap());
    }
}
