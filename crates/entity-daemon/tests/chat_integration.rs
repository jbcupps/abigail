//! Integration test for the entity-daemon POST /v1/chat endpoint.
//!
//! Starts an in-process Axum server with a mock LLM provider and verifies
//! the HTTP response matches the expected ApiEnvelope<ChatResponse> structure.

use abigail_capabilities::cognitive::{CompletionRequest, CompletionResponse, LlmProvider};
use abigail_core::{AppConfig, ForceOverride, RoutingMode, TierModels, TierThresholds};
use abigail_memory::MemoryStore;
use abigail_router::IdEgoRouter;
use abigail_skills::channel::EventBus;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use async_trait::async_trait;
use axum::routing::{get, post};
use axum::Router;
use entity_core::{ApiEnvelope, ChatRequest, ChatResponse};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock Provider
// ---------------------------------------------------------------------------

struct MockProvider {
    call_count: AtomicUsize,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn complete(&self, _request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(CompletionResponse {
            content: format!("Daemon mock response #{}", n + 1),
            tool_calls: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_daemon_state() -> entity_daemon_test_state::EntityDaemonState {
    let router = IdEgoRouter {
        id: Arc::new(MockProvider::new()),
        ego: None,
        ego_provider: None,
        council: None,
        local_http: None,
        mode: RoutingMode::TierBased,
        tier_models: TierModels::default(),
        tier_thresholds: TierThresholds::default(),
        force_override: ForceOverride::default(),
    };

    let registry = Arc::new(SkillRegistry::new());
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    let event_bus = Arc::new(EventBus::new(16));
    let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
    let instruction_registry = Arc::new(InstructionRegistry::empty());

    let config = AppConfig::default_paths();
    let docs_dir = std::env::temp_dir().join("abigail_daemon_test_docs");
    let _ = std::fs::create_dir_all(&docs_dir);

    entity_daemon_test_state::EntityDaemonState {
        entity_id: "test-entity-001".to_string(),
        config,
        router: Arc::new(router),
        registry,
        executor,
        event_bus,
        docs_dir,
        memory,
        memory_hook: None,
        instruction_registry,
    }
}

/// Minimal re-export of the state struct (entity-daemon is a binary crate,
/// so we reconstruct it here for testing).
mod entity_daemon_test_state {
    use abigail_core::AppConfig;
    use abigail_memory::MemoryStore;
    use abigail_router::IdEgoRouter;
    use abigail_skills::channel::EventBus;
    use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
    use entity_core::ChatMemoryHook;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[derive(Clone)]
    pub struct EntityDaemonState {
        pub entity_id: String,
        pub config: AppConfig,
        pub router: Arc<IdEgoRouter>,
        pub registry: Arc<SkillRegistry>,
        pub executor: Arc<SkillExecutor>,
        pub event_bus: Arc<EventBus>,
        pub docs_dir: PathBuf,
        pub memory: Arc<MemoryStore>,
        pub memory_hook: Option<Arc<dyn ChatMemoryHook>>,
        pub instruction_registry: Arc<InstructionRegistry>,
    }
}

/// Recreate the chat route handler using the same entity-chat engine path.
async fn chat_handler(
    axum::extract::State(state): axum::extract::State<entity_daemon_test_state::EntityDaemonState>,
    axum::Json(body): axum::Json<ChatRequest>,
) -> axum::Json<ApiEnvelope<ChatResponse>> {
    let base_prompt =
        abigail_core::system_prompt::build_system_prompt(&state.docs_dir, &state.config.agent_name);
    let system_prompt = entity_chat::augment_system_prompt(
        &base_prompt,
        &state.registry,
        &state.instruction_registry,
        &body.message,
    );
    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );
    let tools = entity_chat::build_tool_definitions(&state.registry);
    let (tier, model_used, complexity_score) =
        state.router.tier_metadata_for_message(&body.message);

    let target = body.target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target == "ID" {
        let res = state.router.route(messages).await;
        res.map(|r| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            tier: tier.clone(),
            model_used: model_used.clone(),
            complexity_score,
        })
    } else {
        entity_chat::run_tool_use_loop(&state.router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => axum::Json(ApiEnvelope::success(ChatResponse {
            reply: tool_result.content,
            provider: Some("id".to_string()),
            tool_calls_made: tool_result.tool_calls_made,
            tier: tool_result.tier,
            model_used: tool_result.model_used,
            complexity_score: tool_result.complexity_score,
        })),
        Err(e) => axum::Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chat_endpoint_returns_valid_response() {
    let state = build_daemon_state();

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/chat", post(chat_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/chat", addr))
        .json(&ChatRequest {
            message: "hello".into(),
            target: None,
            session_messages: None,
        })
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let envelope: ApiEnvelope<ChatResponse> = resp.json().await.unwrap();
    assert!(envelope.ok);
    let data = envelope.data.unwrap();
    assert!(data.reply.contains("Daemon mock response"));
    assert_eq!(data.provider.as_deref(), Some("id"));
}

#[tokio::test]
async fn chat_endpoint_with_session_history() {
    let state = build_daemon_state();

    let app = Router::new()
        .route("/v1/chat", post(chat_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/chat", addr))
        .json(&ChatRequest {
            message: "follow up".into(),
            target: None,
            session_messages: Some(vec![
                entity_core::SessionMessage {
                    role: "user".into(),
                    content: "hi".into(),
                },
                entity_core::SessionMessage {
                    role: "assistant".into(),
                    content: "hello!".into(),
                },
            ]),
        })
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let envelope: ApiEnvelope<ChatResponse> = resp.json().await.unwrap();
    assert!(envelope.ok);
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let state = build_daemon_state();

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/chat", post(chat_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}
