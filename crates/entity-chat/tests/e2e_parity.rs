//! E2E parity tests for the entity-chat pipeline with a mock LLM provider.
//!
//! These tests exercise the same code path used by both Tauri and entity-daemon:
//!   augment_system_prompt → build_contextual_messages → build_tool_definitions → run_tool_use_loop
//!
//! A deterministic MockProvider ensures tests are repeatable without API keys.

use abigail_capabilities::cognitive::{
    CompletionRequest, CompletionResponse, LlmProvider, ToolCall,
};
use abigail_core::RoutingMode;
use abigail_router::IdEgoRouter;
use abigail_skills::manifest::{SkillId, SkillManifest};
use abigail_skills::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillHealth, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams,
};
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use async_trait::async_trait;
use entity_core::SessionMessage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Mock LLM Provider
// ---------------------------------------------------------------------------

struct MockProvider {
    call_count: AtomicUsize,
    /// When true, the first call returns a tool_call; the second returns text.
    simulate_tool_use: bool,
}

impl MockProvider {
    fn text_only() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            simulate_tool_use: false,
        }
    }

    fn with_tool_use() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
            simulate_tool_use: true,
        }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn complete(&self, _request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);

        if self.simulate_tool_use && n == 0 {
            Ok(CompletionResponse {
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "call_001".into(),
                    name: "test.echo::do_echo".into(),
                    arguments: r#"{"input":"hello"}"#.into(),
                }]),
            })
        } else {
            Ok(CompletionResponse {
                content: format!("Mock response #{}", n + 1),
                tool_calls: None,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Stub Skill (mirrors the one in entity-chat unit tests)
// ---------------------------------------------------------------------------

struct EchoSkill {
    manifest: SkillManifest,
}

impl EchoSkill {
    fn new() -> Self {
        Self {
            manifest: SkillManifest {
                id: SkillId("test.echo".into()),
                name: "Echo".into(),
                version: "1.0".into(),
                description: "Echoes input".into(),
                license: None,
                category: "Test".into(),
                keywords: vec![],
                runtime: "Native".into(),
                min_abigail_version: "0.1.0".into(),
                platforms: vec!["All".into()],
                capabilities: vec![],
                permissions: vec![],
                secrets: vec![],
                config_defaults: HashMap::new(),
            },
        }
    }
}

#[async_trait]
impl Skill for EchoSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }
    async fn initialize(&mut self, _: SkillConfig) -> SkillResult<()> {
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
            name: "do_echo".into(),
            description: "Echoes input back".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "input": { "type": "string" } },
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
        _: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        let input = params
            .values
            .get("input")
            .cloned()
            .unwrap_or(serde_json::json!("none"));
        Ok(ToolOutput::success(serde_json::json!({ "echoed": input })))
    }
    fn capabilities(&self) -> Vec<abigail_skills::manifest::CapabilityDescriptor> {
        vec![]
    }
    fn get_capability(&self, _: &str) -> Option<&dyn std::any::Any> {
        None
    }
    fn triggers(&self) -> Vec<abigail_skills::channel::TriggerDescriptor> {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Helper: build a router + registry + executor
// ---------------------------------------------------------------------------

fn build_test_env(
    provider: impl LlmProvider + 'static,
) -> (IdEgoRouter, Arc<SkillRegistry>, Arc<SkillExecutor>) {
    let router = IdEgoRouter {
        id: Arc::new(provider),
        ego: None,
        ego_provider: None,
        local_http: None,
        mode: RoutingMode::EgoPrimary,
    };
    let registry = Arc::new(SkillRegistry::new());
    registry
        .register(SkillId("test.echo".into()), Arc::new(EchoSkill::new()))
        .unwrap();
    let executor = Arc::new(SkillExecutor::new(registry.clone()));
    (router, registry, executor)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[allow(deprecated)]
async fn simple_message_no_tools_returns_text() {
    let (router, registry, _executor) = build_test_env(MockProvider::text_only());
    let instr = InstructionRegistry::empty();

    let system = entity_chat::augment_system_prompt(
        "You are a test bot.",
        &registry,
        &instr,
        "hi",
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = entity_chat::build_contextual_messages(&system, None, "hi");

    let response = router.route(messages).await.unwrap();
    assert_eq!(response.content, "Mock response #1");
    assert!(response.tool_calls.is_none());
}

#[tokio::test]
async fn tool_use_loop_executes_tool_then_returns_text() {
    let (router, registry, executor) = build_test_env(MockProvider::with_tool_use());
    let instr = InstructionRegistry::empty();

    let system = entity_chat::augment_system_prompt(
        "You are a test bot.",
        &registry,
        &instr,
        "echo hello",
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = entity_chat::build_contextual_messages(&system, None, "echo hello");
    let tools = entity_chat::build_tool_definitions(&registry);
    assert!(!tools.is_empty(), "should have tool definitions");

    let result = entity_chat::run_tool_use_loop(&router, &executor, messages, tools)
        .await
        .unwrap();

    assert_eq!(result.content, "Mock response #2");
    assert_eq!(result.tool_calls_made.len(), 1);
    assert_eq!(result.tool_calls_made[0].skill_id, "test.echo");
    assert_eq!(result.tool_calls_made[0].tool_name, "do_echo");
    assert!(result.tool_calls_made[0].success);
}

#[tokio::test]
#[allow(deprecated)]
async fn session_history_deduplication() {
    let (router, registry, _executor) = build_test_env(MockProvider::text_only());
    let instr = InstructionRegistry::empty();

    let history = vec![
        SessionMessage {
            role: "user".into(),
            content: "hello".into(),
        },
        SessionMessage {
            role: "assistant".into(),
            content: "world".into(),
        },
        SessionMessage {
            role: "user".into(),
            content: "hello".into(),
        },
    ];

    let system = entity_chat::augment_system_prompt(
        "You are a test bot.",
        &registry,
        &instr,
        "hello",
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = entity_chat::build_contextual_messages(&system, Some(history), "hello");

    // System + 2 history (last "hello" deduped) + user = 4
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[1].content, "hello");
    assert_eq!(messages[2].content, "world");
    assert_eq!(messages[3].content, "hello");

    let response = router.route(messages).await.unwrap();
    assert_eq!(response.content, "Mock response #1");
}

#[tokio::test]
#[allow(deprecated)]
async fn long_message_is_capped() {
    let (router, registry, _) = build_test_env(MockProvider::text_only());
    let instr = InstructionRegistry::empty();

    let long_msg = "x".repeat(5000);
    let history = vec![SessionMessage {
        role: "user".into(),
        content: long_msg.clone(),
    }];

    let system = entity_chat::augment_system_prompt(
        "prompt",
        &registry,
        &instr,
        "hi",
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = entity_chat::build_contextual_messages(&system, Some(history), "hi");

    // History message should be capped at 4000 chars
    let hist_msg = &messages[1];
    assert_eq!(hist_msg.content.len(), 4000);

    let _ = router.route(messages).await.unwrap();
}

#[tokio::test]
async fn tool_definitions_include_registered_skills() {
    let registry = Arc::new(SkillRegistry::new());
    registry
        .register(SkillId("test.echo".into()), Arc::new(EchoSkill::new()))
        .unwrap();

    let defs = entity_chat::build_tool_definitions(&registry);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "test.echo::do_echo");
    assert!(defs[0].description.contains("Echoes"));
}

#[tokio::test]
async fn rounds_only_returns_final_text_when_no_tools_called() {
    let (router, registry, executor) = build_test_env(MockProvider::text_only());
    let instr = InstructionRegistry::empty();

    let system = entity_chat::augment_system_prompt(
        "You are a test bot.",
        &registry,
        &instr,
        "hi",
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let mut messages = entity_chat::build_contextual_messages(&system, None, "hi");
    let tools = entity_chat::build_tool_definitions(&registry);

    let result =
        entity_chat::run_tool_use_loop_rounds_only(&router, &executor, &mut messages, &tools, None)
            .await
            .unwrap();

    assert!(result.final_text.is_some());
    assert_eq!(result.final_text.unwrap(), "Mock response #1");
    assert!(result.tool_calls_made.is_empty());
}
