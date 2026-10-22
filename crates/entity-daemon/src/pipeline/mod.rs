//! Bicameral chat pipeline: staged Id → Ego flow over the entity bus.
//!
//! Chat handlers publish a `Topic::MentorInput` envelope and await the
//! turn's outcome. The Id stage curates the system prompt (soul documents,
//! enrichment, optional local-LLM personality consult) and publishes
//! `Topic::IdReaction`. The Ego stage consumes the reaction, runs the
//! provider call / tool loop, and publishes `Topic::EgoAction` plus an
//! `Topic::EgoDeliberation` trace. The journal subscriber persists the
//! exchange and emits lifecycle events.
//!
//! Per-turn runtime channels (streaming senders, cancellation, response
//! oneshots) cannot ride the bus; they live in [`TurnRegistry`] keyed by
//! correlation id.

pub mod ego_stage;
pub mod governance;
pub mod id_stage;
pub mod journal;
pub mod stream_gate;
pub mod superego_stage;

use entity_core::{ChatResponse, SessionMessage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// How long a chat turn may take end-to-end before the handler gives up.
/// CLI providers can be slow, so this is generous.
pub const CHAT_TURN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Outcome delivered back to the waiting HTTP handler.
pub type TurnOutcome = Result<ChatResponse, String>;

/// Per-turn runtime context that cannot be serialized onto the bus.
pub struct TurnContext {
    /// Client-provided conversation history for this turn.
    pub session_messages: Option<Vec<SessionMessage>>,
    /// Token sink for streaming turns (None = non-streaming).
    pub stream_tx: Option<mpsc::Sender<abigail_capabilities::cognitive::StreamEvent>>,
    /// Cancellation for streaming turns (fired by POST /v1/chat/cancel).
    pub cancel: Option<CancellationToken>,
    /// Completion channel back to the HTTP handler.
    pub done: oneshot::Sender<TurnOutcome>,
}

/// Registry of in-flight chat turns, keyed by correlation id.
#[derive(Default)]
pub struct TurnRegistry {
    turns: Mutex<HashMap<String, TurnContext>>,
}

impl TurnRegistry {
    pub fn register(&self, correlation_id: impl Into<String>, ctx: TurnContext) {
        if let Ok(mut turns) = self.turns.lock() {
            turns.insert(correlation_id.into(), ctx);
        }
    }

    /// Remove and return the turn context. The Ego stage takes ownership;
    /// a handler that times out also takes it to drop the turn.
    pub fn take(&self, correlation_id: &str) -> Option<TurnContext> {
        self.turns
            .lock()
            .ok()
            .and_then(|mut turns| turns.remove(correlation_id))
    }
}

/// Payload published on `Topic::IdReaction` by the Id stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdReactionPayload {
    pub session_id: String,
    pub message: String,
    #[serde(default)]
    pub model_override: Option<String>,
    /// Fully synthesized system prompt for the Ego stage.
    pub system_prompt: String,
    /// Personality signal from the local Id consult (None if skipped/degraded).
    #[serde(default)]
    pub id_signal: Option<String>,
    /// Outcome of the Id consult: "success", "skipped", "timeout", "error", "empty".
    pub id_consult: String,
    /// Keyword-inferred id context (mirrors mentor monitor enrichment).
    pub id_context: String,
    /// Keyword-inferred superego context (mirrors mentor monitor enrichment).
    pub superego_context: String,
}

/// Payload published on `Topic::EgoAction` by the Ego stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgoActionPayload {
    pub session_id: String,
    /// "success", "blocked", or "error".
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub response: Option<ChatResponse>,
    /// The mentor message this action answers (for superego judgment).
    #[serde(default)]
    pub user_message: String,
    /// Pre-delivery gate verdict: "clear", "flag", or "block".
    #[serde(default)]
    pub superego_verdict: Option<String>,
    /// Rules behind a flag/block verdict (e.g. "secret-leak").
    #[serde(default)]
    pub superego_findings: Vec<String>,
}

/// Spawn all pipeline subscribers. Handles are returned so callers can keep
/// them alive for the daemon's lifetime.
pub async fn spawn(
    state: crate::state::EntityDaemonState,
) -> anyhow::Result<Vec<abigail_streaming::SubscriptionHandle>> {
    let id_handle = id_stage::spawn(state.clone()).await?;
    let ego_handle = ego_stage::spawn(state.clone()).await?;
    let journal_handle = journal::spawn(state.clone()).await?;
    let governance_handle = governance::spawn(state.clone()).await?;
    let mut handles = vec![id_handle, ego_handle, journal_handle, governance_handle];
    handles.extend(superego_stage::spawn(state).await?);
    Ok(handles)
}

/// Register the turn context and publish the `Topic::MentorInput` request
/// that starts the pipeline. Returns the turn's correlation id.
///
/// On publish failure the context is taken back out of the registry so the
/// handler fails fast instead of waiting out the turn timeout.
pub async fn begin_turn(
    state: &crate::state::EntityDaemonState,
    session_id: &str,
    message: &str,
    model_override: Option<String>,
    ctx: TurnContext,
) -> anyhow::Result<String> {
    use abigail_streaming::{StreamMessage, Topic, TopicConfig, BUS_STREAM};

    let correlation_id = uuid::Uuid::new_v4().to_string();
    state.turns.register(correlation_id.clone(), ctx);

    let envelope = abigail_router::MentorChatEnvelope::request(
        correlation_id.clone(),
        session_id.to_string(),
        state.entity_id.clone(),
        message.to_string(),
        model_override,
    );

    let publish_result: anyhow::Result<()> = async {
        let payload = serde_json::to_vec(&envelope)?;
        let mut headers = HashMap::new();
        headers.insert("stage".to_string(), "request".to_string());
        headers.insert("correlation_id".to_string(), correlation_id.clone());
        headers.insert("entity_id".to_string(), state.entity_id.clone());
        state
            .stream_broker
            .ensure_topic(
                BUS_STREAM,
                Topic::MentorInput.as_str(),
                TopicConfig::default(),
            )
            .await?;
        state
            .stream_broker
            .publish(
                BUS_STREAM,
                Topic::MentorInput.as_str(),
                StreamMessage::with_headers(payload, headers),
            )
            .await
    }
    .await;

    if let Err(e) = publish_result {
        let _ = state.turns.take(&correlation_id);
        return Err(e);
    }
    Ok(correlation_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::EntityDaemonState;
    use abigail_capabilities::cognitive::{
        CompletionRequest, CompletionResponse, LlmProvider, StreamEvent, ToolCall,
    };
    use abigail_core::{AppConfig, RoutingMode};
    use abigail_memory::MemoryStore;
    use abigail_persistence::{EntityScope, PersistenceHandle};
    use abigail_router::IdEgoRouter;
    use abigail_skills::manifest::{SkillId, SkillManifest};
    use abigail_skills::skill::{
        CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillHealth, SkillResult,
        ToolDescriptor, ToolOutput, ToolParams,
    };
    use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
    use abigail_streaming::MemoryBroker;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct MockProvider {
        reply: &'static str,
    }

    struct FailingAssistantTurnStore {
        attempts: Arc<AtomicUsize>,
    }

    struct RejectUnexpectedHiveClient {
        heartbeat_calls: AtomicUsize,
        sync_calls: AtomicUsize,
    }

    struct LedgerCheckingAckClient {
        ledger: Arc<crate::execution_ledger::ExecutionLedger>,
        heartbeat_calls: AtomicUsize,
        sync_calls: AtomicUsize,
    }

    #[async_trait]
    impl crate::RuntimeHeartbeatClient for RejectUnexpectedHiveClient {
        async fn heartbeat(
            &self,
            _request: &hive_core::RuntimeHeartbeatRequest,
        ) -> Result<hive_core::RuntimeHeartbeatResponse, String> {
            self.heartbeat_calls.fetch_add(1, Ordering::SeqCst);
            Err("Hive heartbeat ran before local ledger reconciliation".to_string())
        }

        async fn sync_outbox(
            &self,
            _request: &hive_core::OutboxSyncRequest,
        ) -> Result<hive_core::OutboxSyncResponse, String> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            Err("Hive sync ran before local ledger reconciliation".to_string())
        }
    }

    #[async_trait]
    impl crate::RuntimeHeartbeatClient for LedgerCheckingAckClient {
        async fn heartbeat(
            &self,
            _request: &hive_core::RuntimeHeartbeatRequest,
        ) -> Result<hive_core::RuntimeHeartbeatResponse, String> {
            self.heartbeat_calls.fetch_add(1, Ordering::SeqCst);
            Ok(hive_core::RuntimeHeartbeatResponse {
                accepted: true,
                server_time_utc: chrono::Utc::now().to_rfc3339(),
            })
        }

        async fn sync_outbox(
            &self,
            request: &hive_core::OutboxSyncRequest,
        ) -> Result<hive_core::OutboxSyncResponse, String> {
            self.sync_calls.fetch_add(1, Ordering::SeqCst);
            if self.ledger.recent(None, 10).len() != request.records.len() {
                return Err("Hive sync observed records missing from the local ledger".to_string());
            }
            Ok(hive_core::OutboxSyncResponse {
                accepted_record_ids: request
                    .records
                    .iter()
                    .map(|record| record.record_id.clone())
                    .collect(),
                pending_records: 0,
                server_time_utc: chrono::Utc::now().to_rfc3339(),
                execution_receipts: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl crate::memory_consumer::AssistantTurnStore for FailingAssistantTurnStore {
        async fn persist(
            &self,
            _memory: Arc<MemoryStore>,
            _turn: abigail_memory::ConversationTurn,
        ) -> Result<(), String> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err("injected assistant store failure".to_string())
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                content: self.reply.to_string(),
                tool_calls: None,
            })
        }
    }

    struct ToolProvider {
        calls: AtomicUsize,
        tool_calls: usize,
    }

    #[derive(Clone, Copy)]
    enum SecondRoundOutcome {
        Fail,
        Reply(&'static str),
    }

    struct ToolThenOutcomeProvider {
        calls: AtomicUsize,
        outcome: SecondRoundOutcome,
    }

    #[async_trait]
    impl LlmProvider for ToolProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(CompletionResponse {
                    content: String::new(),
                    tool_calls: Some(
                        (0..self.tool_calls)
                            .map(|index| ToolCall {
                                id: format!("call-{index}"),
                                name: "test.echo::echo".to_string(),
                                arguments: format!(r#"{{"input":"value-{index}"}}"#),
                            })
                            .collect(),
                    ),
                });
            }
            Ok(CompletionResponse {
                content: "tool-assisted reply".to_string(),
                tool_calls: None,
            })
        }
    }

    #[async_trait]
    impl LlmProvider for ToolThenOutcomeProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(CompletionResponse {
                    content: String::new(),
                    tool_calls: Some(vec![ToolCall {
                        id: "partial-call-1".to_string(),
                        name: "test.echo::echo".to_string(),
                        arguments: r#"{"input":"completed"}"#.to_string(),
                    }]),
                });
            }
            match self.outcome {
                SecondRoundOutcome::Fail => {
                    anyhow::bail!("provider failed after action; please try again")
                }
                SecondRoundOutcome::Reply(reply) => Ok(CompletionResponse {
                    content: reply.to_string(),
                    tool_calls: None,
                }),
            }
        }
    }

    struct ToolThenWaitStreamingProvider {
        calls: AtomicUsize,
        second_round_started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl LlmProvider for ToolThenWaitStreamingProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            anyhow::bail!("cancellation fixture requires streaming")
        }

        async fn stream(
            &self,
            _request: &CompletionRequest,
            tx: tokio::sync::mpsc::Sender<StreamEvent>,
        ) -> anyhow::Result<CompletionResponse> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let response = CompletionResponse {
                    content: String::new(),
                    tool_calls: Some(vec![ToolCall {
                        id: "cancel-call-1".to_string(),
                        name: "test.echo::echo".to_string(),
                        arguments: r#"{"input":"completed"}"#.to_string(),
                    }]),
                };
                tx.send(StreamEvent::Done(response.clone())).await?;
                return Ok(response);
            }
            self.second_round_started.notify_one();
            std::future::pending::<anyhow::Result<CompletionResponse>>().await
        }
    }

    struct EchoSkill {
        manifest: SkillManifest,
        executions: Arc<AtomicUsize>,
    }

    impl EchoSkill {
        fn new(executions: Arc<AtomicUsize>) -> Self {
            Self {
                manifest: SkillManifest {
                    id: SkillId("test.echo".to_string()),
                    name: "Echo".to_string(),
                    version: "1.0".to_string(),
                    description: "Echoes input".to_string(),
                    license: None,
                    category: "Test".to_string(),
                    keywords: vec![],
                    runtime: "Native".to_string(),
                    min_abigail_version: "0.1.0".to_string(),
                    platforms: vec!["All".to_string()],
                    capabilities: vec![],
                    permissions: vec![],
                    secrets: vec![],
                    config_defaults: HashMap::new(),
                },
                executions,
            }
        }
    }

    #[async_trait]
    impl Skill for EchoSkill {
        fn manifest(&self) -> &SkillManifest {
            &self.manifest
        }

        async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
            Ok(())
        }

        async fn shutdown(&mut self) -> SkillResult<()> {
            Ok(())
        }

        fn health(&self) -> SkillHealth {
            SkillHealth {
                status: HealthStatus::Healthy,
                message: None,
                last_check: chrono::Utc::now(),
                metrics: HashMap::new(),
            }
        }

        fn tools(&self) -> Vec<ToolDescriptor> {
            vec![ToolDescriptor {
                name: "echo".to_string(),
                description: "Echoes input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"input": {"type": "string"}},
                    "required": ["input"]
                }),
                returns: serde_json::json!({}),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            }]
        }

        async fn execute_tool(
            &self,
            _tool_name: &str,
            params: ToolParams,
            _context: &ExecutionContext,
        ) -> SkillResult<ToolOutput> {
            self.executions.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutput::success(serde_json::json!({
                "echoed": params.values.get("input")
            })))
        }

        fn capabilities(&self) -> Vec<abigail_skills::manifest::CapabilityDescriptor> {
            vec![]
        }

        fn get_capability(&self, _name: &str) -> Option<&dyn std::any::Any> {
            None
        }

        fn triggers(&self) -> Vec<abigail_skills::channel::TriggerDescriptor> {
            vec![]
        }
    }

    /// Streams its reply in small chunks so streaming-path tests exercise a
    /// genuinely incremental token flow instead of one monolithic token.
    struct ChunkedStreamProvider {
        reply: &'static str,
    }

    #[async_trait]
    impl LlmProvider for ChunkedStreamProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                content: self.reply.to_string(),
                tool_calls: None,
            })
        }

        async fn stream(
            &self,
            _request: &CompletionRequest,
            tx: tokio::sync::mpsc::Sender<StreamEvent>,
        ) -> anyhow::Result<CompletionResponse> {
            for chunk in self.reply.as_bytes().chunks(8) {
                let token = std::str::from_utf8(chunk).expect("ascii test reply");
                let _ = tx.send(StreamEvent::Token(token.to_string())).await;
            }
            let response = CompletionResponse {
                content: self.reply.to_string(),
                tool_calls: None,
            };
            let _ = tx.send(StreamEvent::Done(response.clone())).await;
            Ok(response)
        }
    }

    fn build_state() -> EntityDaemonState {
        build_state_with_reply("pipeline says hello")
    }

    fn build_state_with_reply(reply: &'static str) -> EntityDaemonState {
        build_state_with_provider(Arc::new(MockProvider { reply }))
    }

    fn build_state_with_provider(provider: Arc<dyn LlmProvider>) -> EntityDaemonState {
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        router.id = provider;

        let registry = Arc::new(SkillRegistry::new());
        let executor = Arc::new(SkillExecutor::new(registry.clone()));
        let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
        let docs_dir =
            std::env::temp_dir().join(format!("abigail_pipeline_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&docs_dir);
        let stream_broker: Arc<dyn abigail_streaming::StreamBroker> =
            Arc::new(MemoryBroker::new(128));
        let queue_scope = EntityScope::Entity(format!("pipeline-queue-{}", uuid::Uuid::new_v4()));
        let queue_store = PersistenceHandle::open_ephemeral(queue_scope).unwrap();
        let job_queue = Arc::new(abigail_queue::JobQueue::new(
            queue_store,
            stream_broker.clone(),
        ));

        EntityDaemonState {
            entity_id: "pipeline-entity".to_string(),
            config: AppConfig::default_paths(),
            hive_url: "http://127.0.0.1:3141".to_string(),
            runtime_id: "runtime-pipeline".to_string(),
            session_lease: hive_core::RuntimeSessionLease {
                lease_id: "lease-pipeline".to_string(),
                entity_id: "pipeline-entity".to_string(),
                runtime_id: "runtime-pipeline".to_string(),
                entity_name: Some("Pipeline Entity".to_string()),
                hive_url: Some("http://127.0.0.1:3141".to_string()),
                issued_at_utc: chrono::Utc::now().to_rfc3339(),
                expires_at_utc: None,
                offline_until_close: true,
                lease_scope: "entity-runtime-session".to_string(),
            },
            router: Arc::new(crate::state::RouterHandle::new(Arc::new(router))),
            registry,
            executor,
            docs_dir: docs_dir.clone(),
            memory,
            assistant_turn_store: Arc::new(crate::memory_consumer::DurableAssistantTurnStore),
            job_queue,
            stream_broker,
            memory_hook: None,
            instruction_registry: Arc::new(InstructionRegistry::empty()),
            archive_exporter: None,
            turns_since_archive: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            active_stream_cancel: Arc::new(tokio::sync::Mutex::new(None)),
            constraints: Arc::new(tokio::sync::RwLock::new(
                abigail_router::ConstraintStore::new(),
            )),
            outbox: Arc::new(
                crate::outbox::RuntimeOutbox::load(docs_dir.join("outbox"), 64).expect("outbox"),
            ),
            execution_ledger: Arc::new(
                crate::execution_ledger::ExecutionLedger::load(docs_dir.join("ledger"))
                    .expect("execution ledger"),
            ),
            last_hive_sync_at_utc: Arc::new(tokio::sync::RwLock::new(None)),
            last_hive_error: Arc::new(tokio::sync::RwLock::new(None)),
            runtime_url: Arc::new(tokio::sync::RwLock::new(None)),
            skill_assignments: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            forge_jobs: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            recent_skill_acks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            turns: Arc::new(TurnRegistry::default()),
            soul_ref: abigail_streaming::compute_soul_ref(b"pipeline-test-soul"),
        }
    }

    fn build_tool_state(tool_calls: usize) -> (EntityDaemonState, Arc<AtomicUsize>) {
        build_tool_state_with_provider(Arc::new(ToolProvider {
            calls: AtomicUsize::new(0),
            tool_calls,
        }))
    }

    fn build_tool_state_with_provider(
        provider: Arc<dyn LlmProvider>,
    ) -> (EntityDaemonState, Arc<AtomicUsize>) {
        let state = build_state_with_provider(provider);
        let executions = Arc::new(AtomicUsize::new(0));
        state
            .registry
            .register(
                SkillId("test.echo".to_string()),
                Arc::new(EchoSkill::new(executions.clone())),
            )
            .unwrap();
        (state, executions)
    }

    async fn run_turn(state: &EntityDaemonState, session_id: &str) -> (String, TurnOutcome) {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let correlation_id = begin_turn(
            state,
            session_id,
            "use the echo tool",
            None,
            TurnContext {
                session_messages: None,
                stream_tx: None,
                cancel: None,
                done: done_tx,
            },
        )
        .await
        .expect("begin turn");
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), done_rx)
            .await
            .expect("turn should complete before timeout")
            .expect("ego stage should deliver an outcome");
        (correlation_id, outcome)
    }

    fn outbox_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()))
    }

    fn age_runtime_reservation(root: &std::path::Path, record_id: &str, age: chrono::Duration) {
        let path = root.join("runtime_outbox.json");
        let mut document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let reservation = document["reservations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|reservation| reservation["record_id"] == record_id)
            .expect("reservation to age");
        reservation["created_at_utc"] =
            serde_json::Value::String((chrono::Utc::now() - age).to_rfc3339());
        std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn chat_turn_flows_through_id_and_ego_stages() {
        let state = build_state();
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let ctx = TurnContext {
            session_messages: None,
            stream_tx: None,
            cancel: None,
            done: done_tx,
        };
        let correlation_id = begin_turn(&state, "session-1", "hello there", None, ctx)
            .await
            .expect("begin turn");
        assert!(!correlation_id.is_empty());

        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), done_rx)
            .await
            .expect("turn should complete before timeout")
            .expect("ego stage should deliver an outcome");
        let response = outcome.expect("turn should succeed");
        assert_eq!(response.reply, "pipeline says hello");
        assert_eq!(response.session_id.as_deref(), Some("session-1"));
        let persisted = state.memory.recent_turns("session-1", 10).unwrap();
        assert!(
            persisted
                .iter()
                .any(|turn| turn.role == "assistant" && turn.content == response.reply),
            "A completed turn must already be saved before its HTTP/SSE acknowledgement"
        );
        // The turn context must be consumed by the ego stage.
        assert!(state.turns.take(&correlation_id).is_none());

        // The turn must leave a health-board entry.
        let health = state.router.current().health_board();
        assert!(!health.is_empty(), "health board should record the call");
        assert_eq!(health[0].state, "healthy");
    }

    #[tokio::test]
    async fn outbox_capacity_fails_the_real_tool_turn_before_journal_commit() {
        let (mut state, executions) = build_tool_state(2);
        let root = outbox_root("abigail-pipeline-capacity");
        let outbox = crate::outbox::RuntimeOutbox::load(&root, 1).unwrap();
        let existing = outbox
            .enqueue(
                &state.entity_id,
                "memory_confirmed",
                serde_json::json!({
                    "memory_id": "already-queued",
                    "memory_weight": "distilled"
                }),
            )
            .unwrap();
        state.outbox = Arc::new(outbox);
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-capacity").await;
        let error = outcome.expect_err("outbox backpressure must fail the tool turn");
        assert!(error.contains("backpressure"), "{error}");
        assert_eq!(
            executions.load(Ordering::SeqCst),
            0,
            "capacity must be reserved before any tool side effect"
        );
        assert!(
            state
                .memory
                .recent_turns("session-capacity", 10)
                .unwrap()
                .iter()
                .all(|turn| turn.role != "assistant"),
            "a failed staging attempt must not commit an assistant success"
        );
        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].record_id, existing.record_id);
        assert!(state.outbox.reservations().unwrap().is_empty());

        let restarted = crate::outbox::RuntimeOutbox::load(&root, 1).unwrap();
        let restarted = restarted.snapshot().unwrap();
        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].record_id, existing.record_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn outbox_save_failure_fails_the_real_tool_turn_and_preserves_queued_ids() {
        let (mut state, executions) = build_tool_state(2);
        let root = outbox_root("abigail-pipeline-save-failure");
        let calls = Arc::new(AtomicUsize::new(0));
        let saver_calls = calls.clone();
        let outbox = crate::outbox::RuntimeOutbox::load_with_saver(&root, 4, move |path, bytes| {
            if saver_calls.fetch_add(1, Ordering::SeqCst) == 1 {
                anyhow::bail!("injected pipeline outbox save failure");
            }
            abigail_core::secure_fs::write_bytes_atomic(path, bytes)?;
            Ok(())
        })
        .unwrap();
        let existing = outbox
            .enqueue(
                &state.entity_id,
                "memory_confirmed",
                serde_json::json!({
                    "memory_id": "already-queued",
                    "memory_weight": "crystallized"
                }),
            )
            .unwrap();
        state.outbox = Arc::new(outbox);
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-save-failure").await;
        let error = outcome.expect_err("an atomic save failure must fail the tool turn");
        assert!(error.contains("safely save"), "{error}");
        assert_eq!(
            executions.load(Ordering::SeqCst),
            0,
            "reservation save failure must prevent every tool side effect"
        );
        assert!(state
            .memory
            .recent_turns("session-save-failure", 10)
            .unwrap()
            .iter()
            .all(|turn| turn.role != "assistant"));
        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].record_id, existing.record_id);
        assert!(state.outbox.reservations().unwrap().is_empty());

        let restarted = crate::outbox::RuntimeOutbox::load(&root, 4).unwrap();
        let restarted = restarted.snapshot().unwrap();
        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].record_id, existing.record_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn successful_tool_turn_commits_one_content_free_aggregate_after_the_assistant_fact() {
        let (state, executions) = build_tool_state(2);
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-aggregate").await;
        let response = outcome.expect("tool turn should succeed");
        assert_eq!(response.reply, "tool-assisted reply");
        assert_eq!(executions.load(Ordering::SeqCst), 2);

        let assistant = state
            .memory
            .recent_turns("session-aggregate", 10)
            .unwrap()
            .into_iter()
            .find(|turn| turn.role == "assistant")
            .expect("successful response must be durable before activity commit");
        assert_eq!(assistant.successful_tool_uses.len(), 2);

        let records = state.outbox.snapshot().unwrap();
        assert_eq!(records.len(), 1, "one turn produces one aggregate event");
        assert_eq!(records[0].kind, "skill_used");
        assert_eq!(records[0].payload["session_id"], "session-aggregate");
        assert_eq!(records[0].payload["successful_skill_count"], 2);
        assert_eq!(
            records[0].record_id,
            crate::state::deterministic_journey_record_id(
                &state.entity_id,
                "skill_used",
                &assistant.id,
            )
        );
        assert!(state.outbox.reservations().unwrap().is_empty());
        let serialized = records[0].payload.to_string();
        for forbidden in [
            "content",
            "reply",
            "message",
            "arguments",
            "output",
            "skill_id",
            "tool_name",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "aggregate activity leaked {forbidden}: {serialized}"
            );
        }
    }

    #[tokio::test]
    async fn partial_provider_failure_commits_completed_tool_without_retry() {
        let (state, executions) =
            build_tool_state_with_provider(Arc::new(ToolThenOutcomeProvider {
                calls: AtomicUsize::new(0),
                outcome: SecondRoundOutcome::Fail,
            }));
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-partial-provider").await;
        let response = outcome.expect("completed tool requires a non-retry partial response");

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(response.tool_calls_made.len(), 1);
        assert!(response.tool_calls_made[0].success);
        assert_eq!(
            response.reply,
            "An action completed, but the final reply is unavailable. \
             Verify the result before repeating the action."
        );
        assert_eq!(
            response.completion_risk,
            Some(entity_core::CompletionRisk::ActionCompletedVerifyBeforeRepeating)
        );
        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].payload["successful_skill_count"], 1);
        assert!(state.outbox.reservations().unwrap().is_empty());
    }

    #[tokio::test]
    async fn superego_block_after_completed_tool_commits_activity_without_retry() {
        let (mut state, executions) =
            build_tool_state_with_provider(Arc::new(ToolThenOutcomeProvider {
                calls: AtomicUsize::new(0),
                outcome: SecondRoundOutcome::Reply(
                    "secret result: sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
                ),
            }));
        state.soul_ref = "superego-soul-a".to_string();
        let outbox_dir = outbox_root("abigail-pipeline-superego-outbox");
        let ledger_root = outbox_root("abigail-pipeline-superego-ledger");
        state.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        state.execution_ledger = Arc::new(
            crate::execution_ledger::ExecutionLedger::load_with_saver(
                &ledger_root,
                |_path, _bytes| anyhow::bail!("injected superego ledger save failure"),
            )
            .unwrap(),
        );
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-tool-blocked").await;
        let error = outcome.expect_err("unsafe final reply must remain blocked");

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(
            error.code,
            Some(entity_core::CompletionRisk::ActionCompletedVerifyBeforeRepeating)
        );
        assert!(!error.interrupted);
        assert_eq!(
            error.message,
            "An action completed. Verify the result before repeating it."
        );
        assert!(!error.message.contains("secret-leak"));
        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].payload["session_id"], "session-tool-blocked");
        assert_eq!(queued[0].payload["successful_skill_count"], 1);
        assert!(state.outbox.reservations().unwrap().is_empty());
        assert!(state.execution_ledger.recent(None, 10).is_empty());

        let client = RejectUnexpectedHiveClient {
            heartbeat_calls: AtomicUsize::new(0),
            sync_calls: AtomicUsize::new(0),
        };
        let reconcile_error = crate::run_runtime_outbox_heartbeat(&state, &client)
            .await
            .expect_err("ledger failure must stop Hive heartbeat and sync");
        assert!(
            reconcile_error.contains("injected superego ledger save failure"),
            "{reconcile_error}"
        );
        assert_eq!(client.heartbeat_calls.load(Ordering::SeqCst), 0);
        assert_eq!(client.sync_calls.load(Ordering::SeqCst), 0);

        let record_id = queued[0].record_id.clone();
        state
            .outbox
            .acknowledge(std::slice::from_ref(&record_id))
            .unwrap();
        let mut restarted = build_state();
        restarted.soul_ref = "superego-soul-b".to_string();
        restarted.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        restarted.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .expect("acknowledged blocked activity must repair without an assistant source");
        let repaired = restarted.execution_ledger.recent(None, 10);
        assert_eq!(repaired.len(), 1);
        assert_eq!(repaired[0].event_id, record_id);
        assert_eq!(repaired[0].soul_ref, "superego-soul-a");
        assert!(restarted.outbox.snapshot().unwrap().is_empty());
        assert!(restarted.outbox.is_acknowledged(&record_id).unwrap());
        let _ = std::fs::remove_dir_all(outbox_dir);
        let _ = std::fs::remove_dir_all(ledger_root);
    }

    #[tokio::test]
    async fn cancellation_after_tool_side_effect_retains_reservation_without_retry() {
        let second_round_started = Arc::new(tokio::sync::Notify::new());
        let (state, executions) =
            build_tool_state_with_provider(Arc::new(ToolThenWaitStreamingProvider {
                calls: AtomicUsize::new(0),
                second_round_started: second_round_started.clone(),
            }));
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");
        let (stream_tx, _stream_rx) = tokio::sync::mpsc::channel(64);
        let cancel = tokio_util::sync::CancellationToken::new();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        begin_turn(
            &state,
            "session-tool-cancel",
            "perform one action",
            None,
            TurnContext {
                session_messages: None,
                stream_tx: Some(stream_tx),
                cancel: Some(cancel.clone()),
                done: done_tx,
            },
        )
        .await
        .expect("begin turn");

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            second_round_started.notified(),
        )
        .await
        .expect("the tool side effect must complete before cancellation");
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        cancel.cancel();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), done_rx)
            .await
            .expect("cancelled turn completes")
            .expect("ego stage returns an outcome")
            .expect_err("cancelled turn is not a complete assistant reply");

        assert_eq!(
            error.code,
            Some(entity_core::CompletionRisk::ActionMayHaveCompletedVerifyBeforeRepeating)
        );
        assert!(error.interrupted);
        assert_eq!(
            error.message,
            "An action may have completed. Verify the result before repeating it."
        );
        assert!(state.outbox.snapshot().unwrap().is_empty());
        assert_eq!(state.outbox.reservations().unwrap().len(), 1);
        let reopened =
            crate::outbox::RuntimeOutbox::load(state.docs_dir.join("outbox"), 64).unwrap();
        assert_eq!(reopened.reservations().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn post_side_effect_commit_failure_returns_success_and_recovers_after_restart() {
        let (mut state, executions) = build_tool_state(2);
        let root = outbox_root("abigail-pipeline-post-commit");
        let calls = Arc::new(AtomicUsize::new(0));
        let saver_calls = calls.clone();
        state.outbox = Arc::new(
            crate::outbox::RuntimeOutbox::load_with_saver(&root, 4, move |path, bytes| {
                let call = saver_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 2 {
                    anyhow::bail!("injected post-side-effect outbox failure");
                }
                abigail_core::secure_fs::write_bytes_atomic(path, bytes)?;
                Ok(())
            })
            .unwrap(),
        );
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-recovery").await;
        let response = outcome.expect("completed tools must keep their successful response");
        assert_eq!(response.reply, "tool-assisted reply");
        assert!(!response.reply.to_ascii_lowercase().contains("retry"));
        assert_eq!(executions.load(Ordering::SeqCst), 2);
        assert!(state.outbox.snapshot().unwrap().is_empty());
        assert_eq!(state.outbox.reservations().unwrap().len(), 1);

        let assistant = state
            .memory
            .recent_turns("session-recovery", 10)
            .unwrap()
            .into_iter()
            .find(|turn| turn.role == "assistant")
            .expect("assistant recovery fact must be durable");
        let expected_id = crate::state::deterministic_journey_record_id(
            &state.entity_id,
            "skill_used",
            &assistant.id,
        );
        assert_eq!(
            state.outbox.reservations().unwrap()[0].record_id,
            expected_id
        );

        let mut restarted = build_state();
        restarted.memory = state.memory.clone();
        restarted.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&root, 4).unwrap());
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Heartbeat)
            .unwrap();
        assert!(
            restarted.outbox.snapshot().unwrap().is_empty(),
            "heartbeat must not race a fresh active reservation"
        );
        assert_eq!(restarted.outbox.reservations().unwrap().len(), 1);

        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();
        let recovered = restarted.outbox.snapshot().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].record_id, expected_id);
        assert_eq!(recovered[0].payload["successful_skill_count"], 2);
        assert!(restarted.outbox.reservations().unwrap().is_empty());
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();
        assert_eq!(restarted.outbox.snapshot().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn reservation_soul_survives_commit_failure_and_retemper_recovery() {
        let (mut state_a, executions) = build_tool_state(1);
        state_a.soul_ref = "reservation-soul-a".to_string();
        let outbox_dir = outbox_root("abigail-pipeline-reservation-soul");
        let ledger_root = outbox_root("abigail-pipeline-reservation-soul-ledger");
        let calls = Arc::new(AtomicUsize::new(0));
        let saver_calls = calls.clone();
        state_a.outbox = Arc::new(
            crate::outbox::RuntimeOutbox::load_with_saver(&outbox_dir, 4, move |path, bytes| {
                let call = saver_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if call == 2 {
                    anyhow::bail!("injected reservation commit failure");
                }
                abigail_core::secure_fs::write_bytes_atomic(path, bytes)?;
                Ok(())
            })
            .unwrap(),
        );
        state_a.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        let _handles = spawn(state_a.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state_a, "session-reservation-soul").await;
        outcome.expect("completed action remains successful after commit failure");
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert!(state_a.outbox.snapshot().unwrap().is_empty());
        let reservations = state_a.outbox.reservations().unwrap();
        assert_eq!(reservations.len(), 1);
        assert_eq!(reservations[0].soul_ref, "reservation-soul-a");
        let expected_id = reservations[0].record_id.clone();

        let mut state_b = build_state();
        state_b.soul_ref = "retempered-soul-b".to_string();
        state_b.memory = state_a.memory.clone();
        state_b.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        state_b.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        state_b
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .expect("recovery must use the pre-side-effect reservation soul");

        let queued = state_b.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].record_id, expected_id);
        let ledger = state_b.execution_ledger.recent(None, 10);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].event_id, expected_id);
        assert_eq!(ledger[0].soul_ref, "reservation-soul-a");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(outbox_dir.join("runtime_outbox.json")).unwrap())
                .unwrap();
        assert_eq!(
            persisted["record_ledger_metadata"][0]["soul_ref"],
            "reservation-soul-a"
        );
        state_b
            .outbox
            .acknowledge(std::slice::from_ref(&expected_id))
            .unwrap();
        drop(state_b);
        std::fs::remove_file(ledger_root.join("execution_ledger.json")).unwrap();

        let mut state_c = build_state();
        state_c.soul_ref = "later-soul-c".to_string();
        state_c.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        state_c.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        state_c
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .expect("acknowledged metadata must repair the ledger without a source fact");

        let repaired = state_c.execution_ledger.recent(None, 10);
        assert_eq!(repaired.len(), 1);
        assert_eq!(repaired[0].event_id, expected_id);
        assert_eq!(repaired[0].soul_ref, "reservation-soul-a");
        assert!(state_c.outbox.snapshot().unwrap().is_empty());
        assert!(state_c.outbox.is_acknowledged(&expected_id).unwrap());
        let _ = std::fs::remove_dir_all(outbox_dir);
        let _ = std::fs::remove_dir_all(ledger_root);
    }

    #[tokio::test]
    async fn assistant_store_failure_preserves_completed_tool_activity_without_retry() {
        let (mut state, executions) = build_tool_state(1);
        state.soul_ref = "assistant-history-soul-a".to_string();
        let outbox_dir = outbox_root("abigail-pipeline-assistant-store");
        let ledger_root = outbox_root("abigail-pipeline-assistant-ledger");
        state.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        state.execution_ledger = Arc::new(
            crate::execution_ledger::ExecutionLedger::load_with_saver(
                &ledger_root,
                |_path, _bytes| anyhow::bail!("injected assistant-history ledger save failure"),
            )
            .unwrap(),
        );
        let store_attempts = Arc::new(AtomicUsize::new(0));
        state.assistant_turn_store = Arc::new(FailingAssistantTurnStore {
            attempts: store_attempts.clone(),
        });
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-assistant-store").await;
        let response = outcome.expect("completed external action must remain successful");
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(store_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            response.reply,
            "tool-assisted reply\n\nThe action completed, but this reply and its history could not \
             be saved. Verify the result before repeating the action."
        );
        assert_eq!(
            response.completion_risk,
            Some(entity_core::CompletionRisk::ActionCompletedVerifyBeforeRepeating)
        );
        assert!(
            state
                .memory
                .recent_turns("session-assistant-store", 10)
                .unwrap()
                .iter()
                .all(|turn| turn.role != "assistant"),
            "the injected failure must leave no assistant recovery fact"
        );

        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        let record_id = queued[0].record_id.clone();
        assert_eq!(queued[0].kind, "skill_used");
        assert_eq!(queued[0].payload["session_id"], "session-assistant-store");
        assert_eq!(queued[0].payload["successful_skill_count"], 1);
        assert!(state.outbox.reservations().unwrap().is_empty());
        assert!(state.execution_ledger.recent(None, 10).is_empty());

        state.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        let client = LedgerCheckingAckClient {
            ledger: state.execution_ledger.clone(),
            heartbeat_calls: AtomicUsize::new(0),
            sync_calls: AtomicUsize::new(0),
        };
        crate::run_runtime_outbox_heartbeat(&state, &client)
            .await
            .expect("queued history-failure activity must enter the ledger before sync");
        assert_eq!(client.heartbeat_calls.load(Ordering::SeqCst), 1);
        assert_eq!(client.sync_calls.load(Ordering::SeqCst), 1);
        assert!(state.outbox.snapshot().unwrap().is_empty());
        assert!(state.outbox.is_acknowledged(&record_id).unwrap());
        let ledger = state.execution_ledger.recent(None, 10);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].event_id, record_id);
        assert_eq!(ledger[0].soul_ref, "assistant-history-soul-a");

        let mut restarted = build_state();
        restarted.soul_ref = "assistant-history-soul-b".to_string();
        restarted.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        restarted.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();
        assert!(restarted.outbox.snapshot().unwrap().is_empty());
        let restarted_ledger = restarted.execution_ledger.recent(None, 10);
        assert_eq!(restarted_ledger.len(), 1);
        assert_eq!(restarted_ledger[0].event_id, record_id);
        assert_eq!(restarted_ledger[0].soul_ref, "assistant-history-soul-a");
        assert!(restarted.outbox.is_acknowledged(&record_id).unwrap());
        assert!(restarted.outbox.reservations().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(outbox_dir);
        let _ = std::fs::remove_dir_all(ledger_root);
    }

    #[tokio::test]
    async fn ledger_failure_then_acknowledgement_repairs_one_event_without_requeue() {
        let (mut state, executions) = build_tool_state(1);
        let outbox_dir = outbox_root("abigail-pipeline-ledger-outbox");
        let ledger_root = outbox_root("abigail-pipeline-ledger");
        state.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        state.execution_ledger = Arc::new(
            crate::execution_ledger::ExecutionLedger::load_with_saver(
                &ledger_root,
                |_path, _bytes| anyhow::bail!("injected execution ledger save failure"),
            )
            .unwrap(),
        );
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (_, outcome) = run_turn(&state, "session-ledger").await;
        outcome.expect("ledger failure must not invite repetition of a completed tool");
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        let queued = state.outbox.snapshot().unwrap();
        assert_eq!(queued.len(), 1);
        assert!(state.outbox.reservations().unwrap().is_empty());
        assert!(state.execution_ledger.recent(None, 10).is_empty());
        let record_id = queued[0].record_id.clone();
        state
            .outbox
            .acknowledge(std::slice::from_ref(&record_id))
            .unwrap();
        assert!(state.outbox.snapshot().unwrap().is_empty());
        assert!(state.outbox.is_acknowledged(&record_id).unwrap());

        let mut restarted = build_state();
        restarted.memory = state.memory.clone();
        restarted.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&outbox_dir, 4).unwrap());
        restarted.execution_ledger =
            Arc::new(crate::execution_ledger::ExecutionLedger::load(&ledger_root).unwrap());
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();
        restarted
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();

        assert!(restarted.outbox.snapshot().unwrap().is_empty());
        assert!(restarted.outbox.is_acknowledged(&record_id).unwrap());
        let ledger = restarted.execution_ledger.recent(None, 10);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].event_id, record_id);
        assert!(crate::execution_ledger::verify_chain(&ledger));
        let _ = std::fs::remove_dir_all(outbox_dir);
        let _ = std::fs::remove_dir_all(ledger_root);
    }

    #[tokio::test]
    async fn acknowledged_durable_tool_fact_is_not_requeued_by_reconciliation() {
        let (state, _) = build_tool_state(1);
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");
        let (_, outcome) = run_turn(&state, "session-acknowledged").await;
        outcome.expect("tool turn should succeed");
        let record_id = state.outbox.snapshot().unwrap()[0].record_id.clone();
        state
            .outbox
            .acknowledge(std::slice::from_ref(&record_id))
            .unwrap();

        state
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Startup)
            .unwrap();

        assert!(state.outbox.snapshot().unwrap().is_empty());
        assert!(state.outbox.is_acknowledged(&record_id).unwrap());
        assert!(state.outbox.reservations().unwrap().is_empty());
    }

    #[test]
    fn heartbeat_reconciliation_quarantines_stale_unmatched_reservation_without_activity() {
        let mut state = build_state();
        let root = outbox_root("abigail-pipeline-quarantine");
        let stale_id = uuid::Uuid::new_v4().to_string();
        let outbox = crate::outbox::RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(stale_id.clone(), &state.entity_id, "skill_used")
            .unwrap();
        drop(outbox);
        age_runtime_reservation(&root, &stale_id, chrono::Duration::hours(25));
        state.outbox = Arc::new(crate::outbox::RuntimeOutbox::load(&root, 1).unwrap());

        state
            .reconcile_durable_activity(crate::state::ActivityReconcileMode::Heartbeat)
            .unwrap();

        assert!(state.outbox.reservations().unwrap().is_empty());
        assert_eq!(
            state.outbox.quarantined_reservations().unwrap()[0]
                .reservation
                .record_id,
            stale_id
        );
        assert!(
            state.outbox.snapshot().unwrap().is_empty(),
            "an unmatched reservation must never become an activity fact"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn governance_provider_refresh_hot_swaps_router() {
        let state = build_state();
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");
        let original = state.router.current();

        let envelope = abigail_streaming::Envelope::new(
            abigail_streaming::Topic::GovernanceInbound,
            "pipeline-entity",
            "gov-router-1",
            serde_json::json!({
                "kind": governance::KIND_PROVIDER_REFRESH,
                "provider_config": {
                    "local_llm_base_url": null,
                    "ego_provider_name": null,
                    "ego_api_key": null,
                    "ego_model": null,
                    "routing_mode": "EgoPrimary",
                    "cli_permission_mode": null,
                },
            }),
        );
        envelope
            .publish(state.stream_broker.as_ref())
            .await
            .expect("publish provider refresh");

        let swapped = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !Arc::ptr_eq(&original, &state.router.current()) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok();
        assert!(swapped, "router should be hot-swapped by provider refresh");
    }

    #[tokio::test]
    async fn superego_gate_blocks_secret_leaking_reply() {
        let state = build_state_with_reply(
            "here is the key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
        );
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let ctx = TurnContext {
            session_messages: None,
            stream_tx: None,
            cancel: None,
            done: done_tx,
        };
        begin_turn(&state, "session-gate", "what is my api key?", None, ctx)
            .await
            .expect("begin turn");

        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), done_rx)
            .await
            .expect("turn should complete before timeout")
            .expect("ego stage should deliver an outcome");
        let err = outcome.expect_err("secret-leaking reply must be withheld");
        assert!(err.message.contains("withheld"), "unexpected error: {err}");
        assert!(err.code.is_none());
    }

    #[tokio::test]
    async fn superego_gate_cuts_streaming_turn_before_secret_reaches_client() {
        const REPLY: &str = "Of course! Let me walk through where your credentials live so you \
            can find them again whenever you need to. The one you asked about is \
            sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 so please keep it safe.";
        let state = build_state_with_provider(Arc::new(ChunkedStreamProvider { reply: REPLY }));
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        // Stand in for the SSE forwarder: collect every token that makes it
        // through the gate until the channel closes.
        let (token_tx, mut token_rx) = tokio::sync::mpsc::channel(64);
        let collector = tokio::spawn(async move {
            let mut text = String::new();
            while let Some(event) = token_rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    text.push_str(&token);
                }
            }
            text
        });

        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let ctx = TurnContext {
            session_messages: None,
            stream_tx: Some(token_tx),
            cancel: None,
            done: done_tx,
        };
        begin_turn(
            &state,
            "session-stream-gate",
            "what is my api key?",
            None,
            ctx,
        )
        .await
        .expect("begin turn");

        let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), done_rx)
            .await
            .expect("turn should complete before timeout")
            .expect("ego stage should deliver an outcome");
        let err = outcome.expect_err("secret-leaking reply must be withheld");
        assert!(err.message.contains("withheld"), "unexpected error: {err}");
        assert!(err.code.is_none());

        let streamed = tokio::time::timeout(std::time::Duration::from_secs(5), collector)
            .await
            .expect("token channel should close after the cut")
            .expect("collector task should not panic");
        assert!(
            !streamed.is_empty(),
            "clean preamble should stream before the cut"
        );
        assert!(
            REPLY.starts_with(&streamed),
            "streamed text must be a prefix of the reply: {streamed:?}"
        );
        let secret_start = REPLY.find("sk-ant").expect("reply embeds the secret");
        assert!(
            streamed.len() <= secret_start,
            "stream leaked into the secret region: {streamed:?}"
        );
    }

    #[tokio::test]
    async fn governance_refresh_applies_skill_assignments_live() {
        let state = build_state();
        let _handles = spawn(state.clone()).await.expect("pipeline spawn");

        let assignments = vec![hive_core::SkillAssignment {
            assignment_id: "assign-1".to_string(),
            entity_id: "pipeline-entity".to_string(),
            skill_id: "com.abigail.skills.browser".to_string(),
            version: None,
            manifest_path: None,
            assigned_at_utc: chrono::Utc::now().to_rfc3339(),
            status: "active".to_string(),
        }];
        let envelope = abigail_streaming::Envelope::new(
            abigail_streaming::Topic::GovernanceInbound,
            "pipeline-entity",
            "gov-1",
            serde_json::json!({
                "kind": governance::KIND_SKILL_ASSIGNMENTS_REFRESH,
                "assignments": assignments,
            }),
        );
        envelope
            .publish(state.stream_broker.as_ref())
            .await
            .expect("publish governance refresh");

        // Wait for the subscriber to apply the refresh.
        let applied = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if state.skill_assignments.read().await.len() == 1 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok();
        assert!(applied, "assignments should be applied live");
        assert_eq!(
            state.skill_assignments.read().await[0].skill_id,
            "com.abigail.skills.browser"
        );
        let acks = state.recent_skill_acks.read().await;
        assert!(acks
            .iter()
            .any(|a| a.skill_id == "com.abigail.skills.browser"
                && a.status == "assignment_refreshed"));
    }

    #[tokio::test]
    async fn timed_out_turn_can_be_reclaimed() {
        let state = build_state();
        // No pipeline spawned: the turn is never consumed.
        let (done_tx, _done_rx) = tokio::sync::oneshot::channel();
        let ctx = TurnContext {
            session_messages: None,
            stream_tx: None,
            cancel: None,
            done: done_tx,
        };
        let correlation_id = begin_turn(&state, "session-2", "hello", None, ctx)
            .await
            .expect("begin turn");
        assert!(state.turns.take(&correlation_id).is_some());
        assert!(state.turns.take(&correlation_id).is_none());
    }
}
