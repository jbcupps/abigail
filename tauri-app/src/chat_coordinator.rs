use crate::rate_limit::format_cooldown_error;
use crate::state::AppState;
use abigail_capabilities::cognitive::{Message, StreamEvent, ToolDefinition};
use abigail_core::key_detection::{detect_api_keys, CLI_ALIASES};
use abigail_memory::ConversationTurn;
use abigail_router::IdEgoRouter;
use entity_core::{ChatResponse, SessionMessage, ToolCallRecord};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

pub const INTERNAL_EVENT_NAME: &str = "chat-internal-envelope";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InternalChatEnvelopeKind {
    Request,
    Metadata,
    Token,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalChatEnvelope {
    pub kind: InternalChatEnvelopeKind,
    pub correlation_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<InternalChatRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<InternalChatMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<ChatResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InternalChatRequest {
    pub mode: String,
    pub message_chars: usize,
    pub session_message_count: usize,
    pub target_input: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InternalChatMetadata {
    pub target_policy: String,
    pub target_effective: String,
    pub deprecated_target_input: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatCommandRequest {
    pub message: String,
    pub target: Option<String>,
    pub session_messages: Option<Vec<SessionMessage>>,
    pub session_id: Option<String>,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatExecutionMode {
    Streaming,
}

impl ChatExecutionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetResolution {
    effective: &'static str,
    deprecated_input: Option<String>,
}

struct PreparedChat {
    correlation_id: String,
    session_id: String,
    target_resolution: TargetResolution,
    router: IdEgoRouter,
    messages: Vec<Message>,
    tools: Vec<ToolDefinition>,
    model_override: Option<String>,
}

pub struct ChatCoordinator<'a> {
    state: &'a AppState,
}

impl<'a> ChatCoordinator<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }

    pub async fn execute_chat_stream(
        &self,
        app: AppHandle,
        request: ChatCommandRequest,
    ) -> Result<(), String> {
        let prepared = self
            .prepare_request(request.clone(), ChatExecutionMode::Streaming)
            .await?;

        self.emit_internal(
            &app,
            InternalChatEnvelope {
                kind: InternalChatEnvelopeKind::Request,
                correlation_id: prepared.correlation_id.clone(),
                session_id: prepared.session_id.clone(),
                request: Some(InternalChatRequest {
                    mode: ChatExecutionMode::Streaming.as_str().to_string(),
                    message_chars: request.message.chars().count(),
                    session_message_count: request.session_messages.unwrap_or_default().len(),
                    target_input: request.target,
                }),
                metadata: None,
                token: None,
                done: None,
                error: None,
            },
        );

        self.emit_internal(
            &app,
            InternalChatEnvelope {
                kind: InternalChatEnvelopeKind::Metadata,
                correlation_id: prepared.correlation_id.clone(),
                session_id: prepared.session_id.clone(),
                request: None,
                metadata: Some(InternalChatMetadata {
                    target_policy: "deprecated_ignored".to_string(),
                    target_effective: prepared.target_resolution.effective.to_string(),
                    deprecated_target_input: prepared.target_resolution.deprecated_input.clone(),
                }),
                token: None,
                done: None,
                error: None,
            },
        );

        let cancel_token = CancellationToken::new();

        {
            let mut active = self.state.active_chat_cancel.lock().await;
            if let Some(prev) = active.replace(cancel_token.clone()) {
                prev.cancel();
            }
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let app_clone = app.clone();
        let correlation_id = prepared.correlation_id.clone();
        let session_id = prepared.session_id.clone();
        let stream_task = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    let _ = app_clone.emit(
                        INTERNAL_EVENT_NAME,
                        &InternalChatEnvelope {
                            kind: InternalChatEnvelopeKind::Token,
                            correlation_id: correlation_id.clone(),
                            session_id: session_id.clone(),
                            request: None,
                            metadata: None,
                            token: Some(token),
                            done: None,
                            error: None,
                        },
                    );
                }
            }
        });

        let pipeline_fut = entity_chat::stream_chat_pipeline(
            &prepared.router,
            &self.state.executor,
            prepared.messages,
            prepared.tools,
            tx,
            prepared.model_override,
        );
        tokio::pin!(pipeline_fut);

        let result = tokio::select! {
            res = &mut pipeline_fut => res,
            _ = cancel_token.cancelled() => Err(anyhow::anyhow!("Interrupted by user")),
        };

        {
            let mut active = self.state.active_chat_cancel.lock().await;
            *active = None;
        }

        let _ = stream_task.await;

        match result {
            Ok(pipeline) => {
                let response = self
                    .build_response(
                        &prepared.session_id,
                        &prepared.router,
                        ChatPipelineResult {
                            content: pipeline.content,
                            tool_calls_made: pipeline.tool_calls_made,
                            execution_trace: pipeline.execution_trace,
                        },
                    )
                    .await;
                self.emit_internal(
                    &app,
                    InternalChatEnvelope {
                        kind: InternalChatEnvelopeKind::Done,
                        correlation_id: prepared.correlation_id,
                        session_id: prepared.session_id,
                        request: None,
                        metadata: None,
                        token: None,
                        done: Some(response),
                        error: None,
                    },
                );
            }
            Err(e) => {
                let msg = e.to_string();
                self.emit_internal(
                    &app,
                    InternalChatEnvelope {
                        kind: InternalChatEnvelopeKind::Error,
                        correlation_id: prepared.correlation_id,
                        session_id: prepared.session_id,
                        request: None,
                        metadata: None,
                        token: None,
                        done: None,
                        error: Some(msg),
                    },
                );
            }
        }

        Ok(())
    }

    async fn prepare_request(
        &self,
        request: ChatCommandRequest,
        mode: ChatExecutionMode,
    ) -> Result<PreparedChat, String> {
        if let Err(remaining) = self.state.chat_cooldown.check().await {
            return Err(format_cooldown_error(remaining));
        }

        let correlation_id = uuid::Uuid::new_v4().to_string();
        let session_id = normalize_session_id(request.session_id);
        let target_resolution = normalize_target_policy(request.target);

        auto_detect_and_store_key_internal(self.state, &request.message).await;
        archive_turn(self.state, &session_id, "user", &request.message, None).await;

        let (router, system_prompt) = build_router_and_prompt(self.state, &request.message)?;
        let messages = entity_chat::build_contextual_messages(
            &system_prompt,
            request.session_messages,
            &request.message,
        );
        let tools = entity_chat::build_tool_definitions(&self.state.registry);

        tracing::debug!(
            mode = %mode.as_str(),
            correlation_id = %correlation_id,
            session_id = %session_id,
            target_effective = %target_resolution.effective,
            deprecated_target = ?target_resolution.deprecated_input,
            model_override = ?request.model_override,
            "Prepared chat request"
        );

        Ok(PreparedChat {
            correlation_id,
            session_id,
            target_resolution,
            router,
            messages,
            tools,
            model_override: request.model_override,
        })
    }

    async fn build_response(
        &self,
        session_id: &str,
        router: &IdEgoRouter,
        pipeline: ChatPipelineResult,
    ) -> ChatResponse {
        let trace_ref = pipeline.execution_trace.as_ref();
        let tier = trace_ref
            .and_then(|t| t.final_tier())
            .map(|s| s.to_string());
        let model_used = trace_ref
            .and_then(|t| t.final_model())
            .map(|s| s.to_string());
        let complexity_score = trace_ref.and_then(|t| t.complexity_score);
        let provider = trace_ref
            .and_then(|t| t.final_provider())
            .map(|s| s.to_string())
            .or_else(|| Some(entity_chat::provider_label(router)));

        archive_turn(
            self.state,
            session_id,
            "assistant",
            &pipeline.content,
            Some(ArchiveMetadata {
                provider: provider.clone(),
                model_used: model_used.clone(),
                tier: tier.clone(),
                complexity_score,
            }),
        )
        .await;

        ChatResponse {
            reply: pipeline.content,
            provider,
            tool_calls_made: pipeline.tool_calls_made,
            tier,
            model_used,
            complexity_score,
            execution_trace: pipeline.execution_trace,
            session_id: Some(session_id.to_string()),
        }
    }

    fn emit_internal(&self, app: &AppHandle, envelope: InternalChatEnvelope) {
        let _ = app.emit(INTERNAL_EVENT_NAME, &envelope);
    }
}

struct ChatPipelineResult {
    content: String,
    tool_calls_made: Vec<ToolCallRecord>,
    execution_trace: Option<entity_core::ExecutionTrace>,
}

struct ArchiveMetadata {
    provider: Option<String>,
    model_used: Option<String>,
    tier: Option<String>,
    complexity_score: Option<u8>,
}

async fn archive_turn(
    state: &AppState,
    session_id: &str,
    role: &str,
    content: &str,
    metadata: Option<ArchiveMetadata>,
) {
    let redacted = abigail_core::redact_secrets(content);
    let mut turn = ConversationTurn::new(session_id, role, &redacted);
    if let Some(m) = metadata {
        turn = turn.with_metadata(m.provider, m.model_used, m.tier, m.complexity_score);
    }
    match state.memory.read() {
        Ok(mem) => {
            if let Err(e) = mem.insert_turn(&turn) {
                tracing::warn!("Failed to archive {} turn: {}", role, e);
            }
        }
        Err(e) => tracing::warn!("Failed to acquire memory lock for {} turn: {}", role, e),
    }
}

fn build_router_and_prompt(
    state: &AppState,
    message: &str,
) -> Result<(IdEgoRouter, String), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let router = state.router.read().map_err(|e| e.to_string())?.clone();
    let status = router.status();

    let prompt = if status.mode == abigail_core::RoutingMode::CliOrchestrator {
        entity_chat::build_cli_system_prompt(
            &config.docs_dir,
            &config.agent_name,
            &state.registry,
            &state.instruction_registry,
            message,
        )
    } else {
        let base =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: None,
            routing_mode: Some(format!("{:?}", status.mode)),
            tier: None,
            complexity_score: None,
            entity_name: config.agent_name.clone(),
            entity_id: None,
            has_local_llm: status.has_local_http,
            last_provider_change_at: config
                .last_provider_change_at
                .as_ref()
                .filter(|ts| is_recent_provider_change(ts))
                .cloned(),
        };
        entity_chat::augment_system_prompt(
            &base,
            &state.registry,
            &state.instruction_registry,
            message,
            &runtime_ctx,
            entity_chat::PromptMode::Full,
        )
    };

    Ok((router, prompt))
}

fn normalize_session_id(session_id: Option<String>) -> String {
    let trimmed = session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    trimmed.unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn normalize_target_policy(target: Option<String>) -> TargetResolution {
    match target
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
    {
        None => TargetResolution {
            effective: "AUTO",
            deprecated_input: None,
        },
        Some(input) => TargetResolution {
            effective: "AUTO",
            deprecated_input: Some(input),
        },
    }
}

pub async fn auto_detect_and_store_key_internal(
    state: &AppState,
    message: &str,
) -> Vec<(String, String)> {
    let detected = detect_api_keys(message);

    if !detected.is_empty() {
        {
            if let Ok(mut vault) = state.secrets.lock() {
                for (provider, key) in &detected {
                    vault.set_secret(provider, key);
                    for (src, alias) in CLI_ALIASES {
                        if provider == src {
                            vault.set_secret(alias, key);
                        }
                    }
                }
                let _ = vault.save();
            }
        }
        let _ = crate::rebuild_router(state).await;
    }

    detected
}

fn is_recent_provider_change(ts: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| {
            let age = chrono::Utc::now().signed_duration_since(dt);
            age.num_minutes() < 10
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_session_id_and_generates_when_missing() {
        let generated = normalize_session_id(None);
        assert!(uuid::Uuid::parse_str(&generated).is_ok());

        let existing = normalize_session_id(Some(" session-123 ".to_string()));
        assert_eq!(existing, "session-123");
    }

    #[test]
    fn target_policy_deprecates_and_ignores_input() {
        let none = normalize_target_policy(None);
        assert_eq!(none.effective, "AUTO");
        assert_eq!(none.deprecated_input, None);

        let ego = normalize_target_policy(Some("ego".to_string()));
        assert_eq!(ego.effective, "AUTO");
        assert_eq!(ego.deprecated_input, Some("EGO".to_string()));

        let id = normalize_target_policy(Some("ID".to_string()));
        assert_eq!(id.effective, "AUTO");
        assert_eq!(id.deprecated_input, Some("ID".to_string()));
    }

    #[test]
    fn envelope_contract_serializes_expected_shape() {
        let envelope = InternalChatEnvelope {
            kind: InternalChatEnvelopeKind::Metadata,
            correlation_id: uuid::Uuid::new_v4().to_string(),
            session_id: "session-abc".to_string(),
            request: None,
            metadata: Some(InternalChatMetadata {
                target_policy: "deprecated_ignored".to_string(),
                target_effective: "AUTO".to_string(),
                deprecated_target_input: Some("EGO".to_string()),
            }),
            token: None,
            done: None,
            error: None,
        };

        let value = serde_json::to_value(&envelope).expect("serialize envelope");
        assert_eq!(value["kind"], "metadata");
        assert!(value.get("correlation_id").is_some());
        assert_eq!(value["session_id"], "session-abc");
        assert_eq!(value["metadata"]["target_policy"], "deprecated_ignored");
    }
}
