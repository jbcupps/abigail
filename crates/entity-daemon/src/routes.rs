//! Entity daemon HTTP route handlers.

use crate::state::EntityDaemonState;
use abigail_capabilities::cognitive::StreamEvent;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, MemoryEntry, MemoryInsertRequest,
    MemorySearchRequest, MemoryStats, SkillInfo, ToolExecRequest, ToolExecResponse, ToolInfo,
};
use futures_util::{Stream, StreamExt};
use std::convert::Infallible;

// ---------------------------------------------------------------------------
// GET /health
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------------------
// GET /v1/status
// ---------------------------------------------------------------------------

pub async fn get_status(State(state): State<EntityDaemonState>) -> Json<ApiEnvelope<EntityStatus>> {
    let router_status = state.router.status();
    let skills_count = state
        .registry
        .list()
        .map(|skills| skills.len())
        .unwrap_or(0);

    Json(ApiEnvelope::success(EntityStatus {
        entity_id: state.entity_id.clone(),
        name: state.config.agent_name.clone(),
        birth_complete: state.config.birth_complete,
        has_ego: router_status.has_ego,
        ego_provider: router_status.ego_provider,
        routing_mode: format!("{:?}", router_status.mode),
        skills_count,
    }))
}

// ---------------------------------------------------------------------------
// POST /v1/chat
// ---------------------------------------------------------------------------

pub async fn chat(
    State(state): State<EntityDaemonState>,
    Json(body): Json<ChatRequest>,
) -> Json<ApiEnvelope<ChatResponse>> {
    let status = state.router.status();

    let system_prompt = if status.mode == abigail_core::RoutingMode::CliOrchestrator {
        entity_chat::build_cli_system_prompt(
            &state.docs_dir,
            &state.config.agent_name,
            &state.registry,
            &state.instruction_registry,
            &body.message,
        )
    } else {
        let base_prompt = abigail_core::system_prompt::build_system_prompt(
            &state.docs_dir,
            &state.config.agent_name,
        );
        let (tier, model_used, complexity_score) =
            state.router.tier_metadata_for_message(&body.message);
        let runtime_ctx = entity_chat::RuntimeContext {
            provider_name: status.ego_provider.clone(),
            model_id: model_used,
            routing_mode: Some(format!("{:?}", status.mode)),
            tier,
            complexity_score,
            entity_name: state.config.agent_name.clone(),
            entity_id: Some(state.entity_id.clone()),
            has_local_llm: status.has_local_http,
            last_provider_change_at: None,
        };
        entity_chat::augment_system_prompt(
            &base_prompt,
            &state.registry,
            &state.instruction_registry,
            &body.message,
            &runtime_ctx,
        )
    };

    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );

    let tools = entity_chat::build_tool_definitions(&state.registry);

    // Chat never uses Id; Id is for background tasks only. Always route (Ego when available).
    let result = if tools.is_empty() {
        let traced = state.router.route_traced(messages).await;
        traced.map(|(r, trace)| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            execution_trace: Some(trace),
        })
    } else {
        entity_chat::run_tool_use_loop(&state.router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let tier = tool_result.tier().map(|s| s.to_string());
            let model_used = tool_result.model_used().map(|s| s.to_string());
            let complexity_score = tool_result.complexity_score();
            let provider = tool_result
                .execution_trace
                .as_ref()
                .and_then(|t| t.final_provider())
                .map(|s| s.to_string())
                .or_else(|| Some(entity_chat::provider_label(&state.router)));

            Json(ApiEnvelope::success(ChatResponse {
                reply: tool_result.content,
                provider,
                tool_calls_made: tool_result.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: tool_result.execution_trace,
            }))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/chat/stream — SSE streaming variant
// ---------------------------------------------------------------------------

pub async fn chat_stream(
    State(state): State<EntityDaemonState>,
    Json(body): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let base_prompt =
        abigail_core::system_prompt::build_system_prompt(&state.docs_dir, &state.config.agent_name);
    let (tier, model_used, complexity_score) =
        state.router.tier_metadata_for_message(&body.message);
    let router_status = state.router.status();

    let runtime_ctx = entity_chat::RuntimeContext {
        provider_name: router_status.ego_provider.clone(),
        model_id: model_used,
        routing_mode: Some(format!("{:?}", router_status.mode)),
        tier,
        complexity_score,
        entity_name: state.config.agent_name.clone(),
        entity_id: Some(state.entity_id.clone()),
        has_local_llm: router_status.has_local_http,
        last_provider_change_at: None,
    };

    let system_prompt = entity_chat::augment_system_prompt(
        &base_prompt,
        &state.registry,
        &state.instruction_registry,
        &body.message,
        &runtime_ctx,
    );

    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );
    let tools = entity_chat::build_tool_definitions(&state.registry);
    let target: String = body.target.unwrap_or_else(|| "AUTO".to_string());

    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Event>(64);

    let router = state.router.clone();
    let executor = state.executor.clone();

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);
        let sse_fwd = sse_tx.clone();
        let fwd_task = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    let _ = sse_fwd
                        .send(Event::default().event("token").data(token))
                        .await;
                }
            }
        });

        let result =
            entity_chat::stream_chat_pipeline(&router, &executor, messages, tools, &target, tx)
                .await;

        let _ = fwd_task.await;

        match result {
            Ok(pipeline) => {
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
                    .or_else(|| Some(entity_chat::provider_label(&router)));

                let response = ChatResponse {
                    reply: pipeline.content,
                    provider,
                    tool_calls_made: pipeline.tool_calls_made,
                    tier,
                    model_used,
                    complexity_score,
                    execution_trace: pipeline.execution_trace,
                };
                let _ = sse_tx
                    .send(
                        Event::default()
                            .event("done")
                            .data(serde_json::to_string(&response).unwrap_or_default()),
                    )
                    .await;
            }
            Err(e) => {
                let _ = sse_tx
                    .send(Event::default().event("error").data(e.to_string()))
                    .await;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(sse_rx).map(Ok);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// GET /v1/routing/diagnose
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct DiagnoseQuery {
    #[serde(default = "default_diagnose_message")]
    pub message: String,
}

fn default_diagnose_message() -> String {
    "hello".to_string()
}

pub async fn diagnose_routing(
    State(state): State<EntityDaemonState>,
    Query(query): Query<DiagnoseQuery>,
) -> Json<ApiEnvelope<abigail_router::RoutingDiagnosis>> {
    let diagnosis = state.router.diagnose(&query.message);
    Json(ApiEnvelope::success(diagnosis))
}

// ---------------------------------------------------------------------------
// GET /v1/skills
// ---------------------------------------------------------------------------

pub async fn list_skills(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<Vec<SkillInfo>>> {
    match state.registry.list() {
        Ok(manifests) => {
            let skills: Vec<SkillInfo> = manifests
                .into_iter()
                .map(|m| {
                    // Get tools for this skill
                    let tools = state
                        .registry
                        .get_skill(&m.id)
                        .map(|(skill, _)| {
                            skill
                                .tools()
                                .into_iter()
                                .map(|t| ToolInfo {
                                    name: t.name,
                                    description: t.description,
                                    autonomous: t.autonomous,
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    SkillInfo {
                        id: m.id.0,
                        name: m.name,
                        version: m.version,
                        description: m.description,
                        tools,
                    }
                })
                .collect();
            Json(ApiEnvelope::success(skills))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/tools/execute
// ---------------------------------------------------------------------------

pub async fn execute_tool(
    State(state): State<EntityDaemonState>,
    Json(body): Json<ToolExecRequest>,
) -> Json<ApiEnvelope<ToolExecResponse>> {
    use abigail_skills::manifest::SkillId;
    use abigail_skills::skill::ToolParams;

    let skill_id = SkillId(body.skill_id);
    // Build ToolParams from the JSON value
    let params = if let Some(obj) = body.params.as_object() {
        let mut tp = ToolParams::new();
        for (k, v) in obj {
            tp.values.insert(k.clone(), v.clone());
        }
        tp
    } else {
        ToolParams::new()
    };

    match state
        .executor
        .execute(&skill_id, &body.tool_name, params)
        .await
    {
        Ok(output) => Json(ApiEnvelope::success(ToolExecResponse {
            success: output.success,
            output: output.data.unwrap_or(serde_json::Value::Null),
            error: None,
        })),
        Err(e) => Json(ApiEnvelope::success(ToolExecResponse {
            success: false,
            output: serde_json::Value::Null,
            error: Some(e.to_string()),
        })),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/memory/stats
// ---------------------------------------------------------------------------

pub async fn memory_stats(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<MemoryStats>> {
    let count = match state.memory.count_memories() {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e.to_string())),
    };
    let has_birth = match state.memory.has_birth() {
        Ok(b) => b,
        Err(e) => return Json(ApiEnvelope::error(e.to_string())),
    };
    Json(ApiEnvelope::success(MemoryStats {
        memory_count: count,
        has_birth,
    }))
}

// ---------------------------------------------------------------------------
// POST /v1/memory/search
// ---------------------------------------------------------------------------

pub async fn memory_search(
    State(state): State<EntityDaemonState>,
    Json(body): Json<MemorySearchRequest>,
) -> Json<ApiEnvelope<Vec<MemoryEntry>>> {
    match state.memory.search_memories(&body.query, body.limit) {
        Ok(memories) => {
            let entries: Vec<MemoryEntry> = memories
                .into_iter()
                .map(|m| MemoryEntry {
                    id: m.id,
                    content: m.content,
                    weight: m.weight.as_str().to_string(),
                    created_at: m.created_at.to_rfc3339(),
                })
                .collect();
            Json(ApiEnvelope::success(entries))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/memory/recent?limit=N
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub struct RecentQuery {
    #[serde(default = "default_recent_limit")]
    pub limit: usize,
}

fn default_recent_limit() -> usize {
    20
}

pub async fn memory_recent(
    State(state): State<EntityDaemonState>,
    Query(query): Query<RecentQuery>,
) -> Json<ApiEnvelope<Vec<MemoryEntry>>> {
    match state.memory.recent_memories(query.limit) {
        Ok(memories) => {
            let entries: Vec<MemoryEntry> = memories
                .into_iter()
                .map(|m| MemoryEntry {
                    id: m.id,
                    content: m.content,
                    weight: m.weight.as_str().to_string(),
                    created_at: m.created_at.to_rfc3339(),
                })
                .collect();
            Json(ApiEnvelope::success(entries))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/memory/insert
// ---------------------------------------------------------------------------

pub async fn memory_insert(
    State(state): State<EntityDaemonState>,
    Json(body): Json<MemoryInsertRequest>,
) -> Json<ApiEnvelope<MemoryEntry>> {
    use abigail_memory::Memory;

    let memory = match body.weight.as_str() {
        "distilled" => Memory::distilled(body.content),
        "crystallized" => Memory::crystallized(body.content),
        _ => Memory::ephemeral(body.content),
    };

    let entry = MemoryEntry {
        id: memory.id.clone(),
        content: memory.content.clone(),
        weight: memory.weight.as_str().to_string(),
        created_at: memory.created_at.to_rfc3339(),
    };

    match state.memory.insert_memory(&memory) {
        Ok(()) => {
            if let Some(ref hook) = state.memory_hook {
                if let Err(e) = hook.on_memory_persisted(
                    &entry.id,
                    &entry.content,
                    &entry.weight,
                    &entry.created_at,
                ) {
                    tracing::warn!("Memory hook error: {}", e);
                }
            }
            Json(ApiEnvelope::success(entry))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}
