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
        CompletionRequest, CompletionResponse, LlmProvider, StreamEvent,
    };
    use abigail_core::{AppConfig, RoutingMode};
    use abigail_memory::MemoryStore;
    use abigail_persistence::{EntityScope, PersistenceHandle};
    use abigail_router::IdEgoRouter;
    use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
    use abigail_streaming::MemoryBroker;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockProvider {
        reply: &'static str,
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
        // The turn context must be consumed by the ego stage.
        assert!(state.turns.take(&correlation_id).is_none());

        // The turn must leave a health-board entry.
        let health = state.router.current().health_board();
        assert!(!health.is_empty(), "health board should record the call");
        assert_eq!(health[0].state, "healthy");
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
        assert!(err.contains("withheld"), "unexpected error: {err}");
        assert!(err.contains("secret-leak"), "unexpected error: {err}");
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
        assert!(err.contains("withheld"), "unexpected error: {err}");
        assert!(err.contains("secret-leak"), "unexpected error: {err}");

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
