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
    // 1. Build system prompt from constitutional documents, augmented with tool + skill instructions
    let base_prompt =
        abigail_core::system_prompt::build_system_prompt(&state.docs_dir, &state.config.agent_name);
    let system_prompt = entity_chat::augment_system_prompt(
        &base_prompt,
        &state.registry,
        &state.instruction_registry,
        &body.message,
    );

    // 2. Build contextual messages with sanitization + deduplication
    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );

    // 3. Build tool definitions from registered skills
    let tools = entity_chat::build_tool_definitions(&state.registry);

    // 4. Compute tier metadata from the user's message
    let (tier, model_used, complexity_score) =
        state.router.tier_metadata_for_message(&body.message);

    // 5. Route — use tool-use loop if tools are available, plain route otherwise
    let target = body.target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target == "ID" {
        // No tools or explicit Id-only: simple route
        let res = if target == "ID" {
            state.router.id_only(messages).await
        } else {
            state.router.route(messages).await
        };
        res.map(|r| entity_chat::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
            tier: tier.clone(),
            model_used: model_used.clone(),
            complexity_score,
        })
    } else {
        // Tools available: run the agentic tool-use loop
        entity_chat::run_tool_use_loop(&state.router, &state.executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let provider = if state.router.has_ego() {
                state
                    .router
                    .ego_provider_name()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "id".to_string())
            } else {
                "id".to_string()
            };

            // 6. Return response with tier metadata
            Json(ApiEnvelope::success(ChatResponse {
                reply: tool_result.content,
                provider: Some(provider),
                tool_calls_made: tool_result.tool_calls_made,
                tier: tool_result.tier,
                model_used: tool_result.model_used,
                complexity_score: tool_result.complexity_score,
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
    let system_prompt = entity_chat::augment_system_prompt(
        &base_prompt,
        &state.registry,
        &state.instruction_registry,
        &body.message,
    );

    let mut messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );
    let tools = entity_chat::build_tool_definitions(&state.registry);
    let (tier, model_used, complexity_score) =
        state.router.tier_metadata_for_message(&body.message);
    let target: String = body.target.unwrap_or_else(|| "AUTO".to_string());

    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Event>(64);

    let router = state.router.clone();
    let executor = state.executor.clone();

    tokio::spawn(async move {
        // Run non-streaming tool rounds if tools are available.
        let mut tool_calls_made = Vec::new();
        if !tools.is_empty() && target != "ID" {
            match entity_chat::run_tool_use_loop_rounds_only(
                &router,
                &executor,
                &mut messages,
                &tools,
            )
            .await
            {
                Ok(intermediate) => {
                    tool_calls_made = intermediate.tool_calls_made;
                    if let Some(final_text) = intermediate.final_text {
                        let provider = provider_label(&router);
                        let response = ChatResponse {
                            reply: final_text,
                            provider: Some(provider),
                            tool_calls_made,
                            tier,
                            model_used,
                            complexity_score,
                        };
                        let _ = sse_tx
                            .send(
                                Event::default()
                                    .event("done")
                                    .data(serde_json::to_string(&response).unwrap_or_default()),
                            )
                            .await;
                        return;
                    }
                }
                Err(e) => {
                    let _ = sse_tx
                        .send(Event::default().event("error").data(e.to_string()))
                        .await;
                    return;
                }
            }
        }

        // Stream the final (or only) LLM response.
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

        let router_clone = router.clone();
        let tools_clone = tools.clone();
        let messages_clone = messages.clone();
        let stream_handle = tokio::spawn(async move {
            if target == "ID" {
                router_clone.id_stream(messages_clone, stream_tx).await
            } else if tools_clone.is_empty() {
                router_clone.route_stream(messages_clone, stream_tx).await
            } else {
                router_clone
                    .route_stream_with_tools(messages_clone, tools_clone, stream_tx)
                    .await
            }
        });

        while let Some(event) = stream_rx.recv().await {
            match event {
                StreamEvent::Token(token) => {
                    let _ = sse_tx
                        .send(Event::default().event("token").data(token))
                        .await;
                }
                StreamEvent::Done(_) => {}
            }
        }

        match stream_handle.await {
            Ok(Ok(final_response)) => {
                let provider = provider_label(&router);
                let response = ChatResponse {
                    reply: final_response.content,
                    provider: Some(provider),
                    tool_calls_made,
                    tier,
                    model_used,
                    complexity_score,
                };
                let _ = sse_tx
                    .send(
                        Event::default()
                            .event("done")
                            .data(serde_json::to_string(&response).unwrap_or_default()),
                    )
                    .await;
            }
            Ok(Err(e)) => {
                let _ = sse_tx
                    .send(Event::default().event("error").data(e.to_string()))
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

fn provider_label(router: &abigail_router::IdEgoRouter) -> String {
    if router.has_ego() {
        router
            .ego_provider_name()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "id".to_string())
    } else {
        "id".to_string()
    }
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
