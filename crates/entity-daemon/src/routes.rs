//! Entity daemon HTTP route handlers.

use crate::state::EntityDaemonState;
use abigail_capabilities::cognitive::StreamEvent;
use abigail_queue::{JobPriority, JobRecord, JobSpec, RequiredCapability};
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use entity_core::{
    ApiEnvelope, CancelChatStreamResponse, CancelJobResponse, ChatRequest, ChatResponse,
    EntityStatus, JobStatusResponse, ListJobsResponse, MemoryEntry, MemoryInsertRequest,
    MemorySearchRequest, MemoryStats, QueueJobRecord, SkillInfo, SubmitJobRequest,
    SubmitJobResponse, ToolExecRequest, ToolExecResponse, ToolInfo, TopicResultsResponse,
};
use futures_util::{Stream, StreamExt};
use std::convert::Infallible;
use tokio_util::sync::CancellationToken;

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
    let session_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Archive the user turn (async, fire-and-forget via StreamBroker).
    let user_turn = abigail_memory::ConversationTurn::new(&session_id, "user", &body.message);
    crate::memory_consumer::publish_turn(state.stream_broker.clone(), user_turn);

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
            entity_chat::PromptMode::Full,
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
        let resp = state
            .router
            .route_unified(abigail_router::RoutingRequest::simple(messages))
            .await;
        resp.map(|r| entity_chat::ToolUseResult {
            content: r.completion.content,
            tool_calls_made: Vec::new(),
            execution_trace: r.trace,
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

            // Archive the assistant turn (async, fire-and-forget via StreamBroker).
            let asst_turn = abigail_memory::ConversationTurn::new(
                &session_id,
                "assistant",
                &tool_result.content,
            )
            .with_metadata(
                provider.clone(),
                model_used.clone(),
                tier.clone(),
                complexity_score,
            );
            crate::memory_consumer::publish_turn(state.stream_broker.clone(), asst_turn);

            state.maybe_auto_archive();

            Json(ApiEnvelope::success(ChatResponse {
                reply: tool_result.content,
                provider,
                tool_calls_made: tool_result.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: tool_result.execution_trace,
                session_id: Some(session_id),
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
    let session_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Archive user turn (async, fire-and-forget via StreamBroker).
    let user_turn = abigail_memory::ConversationTurn::new(&session_id, "user", &body.message);
    crate::memory_consumer::publish_turn(state.stream_broker.clone(), user_turn);

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
        entity_chat::PromptMode::Full,
    );

    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );
    let tools = entity_chat::build_tool_definitions(&state.registry);

    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Event>(64);

    // Create cancellation token and store it so POST /v1/chat/cancel can fire it.
    let cancel_token = CancellationToken::new();
    {
        let mut active = state.active_stream_cancel.lock().await;
        if let Some(prev) = active.replace(cancel_token.clone()) {
            prev.cancel();
        }
    }

    let router = state.router.clone();
    let executor = state.executor.clone();
    let memory = state.memory.clone();
    let broker_for_stream = state.stream_broker.clone();
    let archive_exporter = state.archive_exporter.clone();
    let turns_since_archive = state.turns_since_archive.clone();
    let cancel_state = state.active_stream_cancel.clone();
    let sid = session_id.clone();

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

        let pipeline_fut =
            entity_chat::stream_chat_pipeline(&router, &executor, messages, tools, tx);
        tokio::pin!(pipeline_fut);

        let result = tokio::select! {
            res = &mut pipeline_fut => res,
            _ = cancel_token.cancelled() => Err(anyhow::anyhow!("Interrupted by user")),
        };

        // Clear the stored token.
        {
            let mut active = cancel_state.lock().await;
            *active = None;
        }

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

                // Archive assistant turn (async, fire-and-forget via StreamBroker).
                let asst_turn =
                    abigail_memory::ConversationTurn::new(&sid, "assistant", &pipeline.content)
                        .with_metadata(
                            provider.clone(),
                            model_used.clone(),
                            tier.clone(),
                            complexity_score,
                        );
                crate::memory_consumer::publish_turn(broker_for_stream, asst_turn);

                // Trigger auto-archive if threshold reached.
                {
                    use std::sync::atomic::Ordering;
                    let count = turns_since_archive.fetch_add(1, Ordering::Relaxed) + 1;
                    if count >= crate::state::ARCHIVE_INTERVAL_TURNS {
                        turns_since_archive.store(0, Ordering::Relaxed);
                        if let Some(ref exp) = archive_exporter {
                            let m = memory.clone();
                            let e = exp.clone();
                            tokio::spawn(async move {
                                if let Err(err) = e.export(&m) {
                                    tracing::warn!("Auto-archive (stream) failed: {}", err);
                                }
                            });
                        }
                    }
                }

                let response = ChatResponse {
                    reply: pipeline.content,
                    provider,
                    tool_calls_made: pipeline.tool_calls_made,
                    tier,
                    model_used,
                    complexity_score,
                    execution_trace: pipeline.execution_trace,
                    session_id: Some(sid),
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
// GET /v1/governance/constraints — list learned constraints
// ---------------------------------------------------------------------------

pub async fn get_constraints(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<Vec<abigail_router::constraint_store::Constraint>>> {
    let store = state.constraints.read().await;
    Json(ApiEnvelope::success(store.all().to_vec()))
}

// ---------------------------------------------------------------------------
// DELETE /v1/governance/constraints — clear all learned constraints
// ---------------------------------------------------------------------------

pub async fn clear_constraints(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<serde_json::Value>> {
    let mut store = state.constraints.write().await;
    store.clear();
    if let Err(e) = store.save() {
        tracing::warn!("Failed to persist cleared constraints: {}", e);
    }
    Json(ApiEnvelope::success(serde_json::json!({ "cleared": true })))
}

// ---------------------------------------------------------------------------
// GET /v1/governance/status — governor metadata
// ---------------------------------------------------------------------------

pub async fn get_governance_status(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<serde_json::Value>> {
    let store = state.constraints.read().await;
    Json(ApiEnvelope::success(serde_json::json!({
        "constraints_count": store.len(),
        "governor": "ephemeral (created per-task)",
    })))
}

// ---------------------------------------------------------------------------
// POST /v1/chat/cancel — cancel the active streaming chat
// ---------------------------------------------------------------------------

pub async fn cancel_chat_stream(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<CancelChatStreamResponse>> {
    let mut active = state.active_stream_cancel.lock().await;
    if let Some(token) = active.take() {
        token.cancel();
        Json(ApiEnvelope::success(CancelChatStreamResponse {
            cancelled: true,
        }))
    } else {
        Json(ApiEnvelope::success(CancelChatStreamResponse {
            cancelled: false,
        }))
    }
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

// ---------------------------------------------------------------------------
// Queue API
// ---------------------------------------------------------------------------

/// POST /v1/jobs/submit
pub async fn submit_job(
    State(state): State<EntityDaemonState>,
    Json(body): Json<SubmitJobRequest>,
) -> Json<ApiEnvelope<SubmitJobResponse>> {
    let spec = JobSpec {
        goal: body.goal,
        topic: body.topic.clone(),
        capability: parse_capability(body.capability.as_deref()),
        priority: parse_priority(body.priority.as_deref()),
        time_budget_ms: body.time_budget_ms.unwrap_or(120_000),
        max_turns: body.max_turns.unwrap_or(10),
        system_context: body.system_context,
        allowed_skill_ids: body.allowed_skill_ids.unwrap_or_default(),
        ttl_seconds: body.ttl_seconds.unwrap_or(3600),
        input_data: body.input_data,
        parent_job_id: body.parent_job_id,
        cron_expression: None,
        is_recurring: false,
        significance_keywords: vec![],
        significance_threshold: 0.5,
        job_mode: "agentic_run".into(),
        goal_template: None,
        depends_on: vec![],
    };

    match state.job_queue.submit_job(spec).await {
        Ok(job_id) => Json(ApiEnvelope::success(SubmitJobResponse {
            job_id,
            topic: body.topic,
            status: "queued".to_string(),
        })),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

/// GET /v1/jobs/:job_id
pub async fn get_job_status(
    State(state): State<EntityDaemonState>,
    Path(job_id): Path<String>,
) -> Json<ApiEnvelope<JobStatusResponse>> {
    match state.job_queue.get_job(&job_id) {
        Ok(Some(job)) => Json(ApiEnvelope::success(JobStatusResponse {
            job: queue_job_record(job),
        })),
        Ok(None) => Json(ApiEnvelope::error(format!("Job '{}' not found", job_id))),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ListJobsQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default = "default_jobs_limit")]
    pub limit: usize,
}

fn default_jobs_limit() -> usize {
    50
}

/// GET /v1/jobs?status=queued&limit=50
pub async fn list_jobs(
    State(state): State<EntityDaemonState>,
    Query(query): Query<ListJobsQuery>,
) -> Json<ApiEnvelope<ListJobsResponse>> {
    match state
        .job_queue
        .list_jobs(query.status.as_deref(), query.limit)
    {
        Ok(jobs) => Json(ApiEnvelope::success(ListJobsResponse {
            jobs: jobs.into_iter().map(queue_job_record).collect(),
        })),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

/// POST /v1/jobs/:job_id/cancel
pub async fn cancel_job(
    State(state): State<EntityDaemonState>,
    Path(job_id): Path<String>,
) -> Json<ApiEnvelope<CancelJobResponse>> {
    match state.job_queue.cancel_job(&job_id).await {
        Ok(()) => Json(ApiEnvelope::success(CancelJobResponse {
            job_id,
            status: "cancelled".to_string(),
        })),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct TopicQuery {
    #[serde(default = "default_topic_limit")]
    pub limit: usize,
}

fn default_topic_limit() -> usize {
    50
}

/// GET /v1/topics/:topic/results?limit=50
pub async fn topic_results(
    State(state): State<EntityDaemonState>,
    Path(topic): Path<String>,
    Query(query): Query<TopicQuery>,
) -> Json<ApiEnvelope<TopicResultsResponse>> {
    let jobs = match state.job_queue.topic_results(&topic, query.limit) {
        Ok(records) => records,
        Err(e) => return Json(ApiEnvelope::error(e.to_string())),
    };
    let all_terminal = match state.job_queue.topic_all_terminal(&topic) {
        Ok(v) => v,
        Err(e) => return Json(ApiEnvelope::error(e.to_string())),
    };

    Json(ApiEnvelope::success(TopicResultsResponse {
        topic,
        all_terminal,
        jobs: jobs.into_iter().map(queue_job_record).collect(),
    }))
}

/// GET /v1/topics/:topic/watch
pub async fn watch_topic(
    State(state): State<EntityDaemonState>,
    Path(topic): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let broker = state.stream_broker.clone();
    let group_name = format!("topic-watch-{}-{}", topic, uuid::Uuid::new_v4());
    let topic_filter = topic.clone();

    tokio::spawn(async move {
        let tx_for_handler = tx.clone();
        let handler: abigail_streaming::broker::MessageHandler = Box::new(move |msg| {
            let topic_filter = topic_filter.clone();
            let tx_for_handler = tx_for_handler.clone();
            Box::pin(async move {
                if msg.headers.get("topic") != Some(&topic_filter) {
                    return;
                }
                let payload = match serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                    Ok(v) => v,
                    Err(_) => serde_json::json!({
                        "raw": String::from_utf8_lossy(&msg.payload).to_string()
                    }),
                };
                let _ = tx_for_handler
                    .send(
                        Event::default()
                            .event("job_event")
                            .data(payload.to_string()),
                    )
                    .await;
            })
        });

        match broker
            .subscribe("abigail", "job-events", &group_name, handler)
            .await
        {
            Ok(handle) => {
                tx.closed().await;
                handle.cancel();
            }
            Err(e) => {
                let _ = tx
                    .send(Event::default().event("error").data(e.to_string()))
                    .await;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(Ok);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn parse_capability(value: Option<&str>) -> RequiredCapability {
    value
        .map(RequiredCapability::from_str_lossy)
        .unwrap_or(RequiredCapability::General)
}

fn parse_priority(value: Option<&str>) -> JobPriority {
    match value.unwrap_or("normal").to_ascii_lowercase().as_str() {
        "low" => JobPriority::Low,
        "high" => JobPriority::High,
        "critical" => JobPriority::Critical,
        _ => JobPriority::Normal,
    }
}

fn queue_job_record(job: JobRecord) -> QueueJobRecord {
    QueueJobRecord {
        id: job.id,
        topic: job.topic,
        goal: job.goal,
        capability: job.capability.as_str().to_string(),
        priority: match job.priority {
            JobPriority::Low => "low".to_string(),
            JobPriority::Normal => "normal".to_string(),
            JobPriority::High => "high".to_string(),
            JobPriority::Critical => "critical".to_string(),
        },
        status: job.status.as_str().to_string(),
        time_budget_ms: job.time_budget_ms,
        max_turns: job.max_turns,
        system_context: job.system_context,
        allowed_skill_ids: job.allowed_skill_ids,
        input_data: job.input_data,
        parent_job_id: job.parent_job_id,
        agent_id: job.agent_id,
        model_used: job.model_used,
        provider_used: job.provider_used,
        result: job.result,
        error: job.error,
        turns_consumed: job.turns_consumed,
        ttl_seconds: job.ttl_seconds,
        created_at: job.created_at,
        started_at: job.started_at,
        completed_at: job.completed_at,
        expires_at: job.expires_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::{CompletionRequest, CompletionResponse, LlmProvider};
    use abigail_core::{AppConfig, ForceOverride, RoutingMode, TierModels, TierThresholds};
    use abigail_memory::MemoryStore;
    use abigail_queue::{
        MIGRATION_V3_JOB_QUEUE, MIGRATION_V4_ORCHESTRATION, MIGRATION_V5_DEPENDS_ON,
    };
    use abigail_router::IdEgoRouter;
    use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
    use abigail_streaming::MemoryBroker;
    use async_trait::async_trait;
    use axum::extract::{Path, Query, State};
    use axum::Json;
    use std::sync::Arc;

    struct MockProvider;

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(
            &self,
            _request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                content: "ok".to_string(),
                tool_calls: None,
            })
        }
    }

    fn build_state() -> EntityDaemonState {
        let router = IdEgoRouter {
            id: Arc::new(MockProvider),
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
        let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
        let docs_dir = std::env::temp_dir().join("abigail_routes_test_docs");
        let _ = std::fs::create_dir_all(&docs_dir);

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        conn.execute_batch(MIGRATION_V3_JOB_QUEUE).unwrap();
        for stmt in MIGRATION_V4_ORCHESTRATION.split(';') {
            let trimmed = stmt.trim();
            if !trimmed.is_empty() {
                conn.execute_batch(trimmed).unwrap_or_else(|_| {});
            }
        }
        for stmt in MIGRATION_V5_DEPENDS_ON.split(';') {
            let trimmed = stmt.trim();
            if !trimmed.is_empty() {
                conn.execute_batch(trimmed).unwrap_or_else(|_| {});
            }
        }
        let stream_broker: Arc<dyn abigail_streaming::StreamBroker> =
            Arc::new(MemoryBroker::new(128));
        let job_queue = Arc::new(abigail_queue::JobQueue::new(
            Arc::new(std::sync::Mutex::new(conn)),
            stream_broker.clone(),
        ));

        EntityDaemonState {
            entity_id: "test-entity".to_string(),
            config: AppConfig::default_paths(),
            router: Arc::new(router),
            registry,
            executor,
            docs_dir,
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
        }
    }

    #[tokio::test]
    async fn submit_and_get_job_status() {
        let state = build_state();
        let submit = SubmitJobRequest {
            goal: "Summarize docs".to_string(),
            topic: "research-1".to_string(),
            capability: Some("reasoning".to_string()),
            priority: Some("high".to_string()),
            time_budget_ms: Some(30_000),
            max_turns: Some(5),
            system_context: None,
            allowed_skill_ids: None,
            ttl_seconds: Some(600),
            input_data: None,
            parent_job_id: None,
        };

        let resp = submit_job(State(state.clone()), Json(submit)).await.0;
        assert!(resp.ok);
        let job_id = resp.data.unwrap().job_id;

        let status = get_job_status(State(state), Path(job_id)).await.0;
        assert!(status.ok);
        let status = status.data.unwrap();
        assert_eq!(status.job.topic, "research-1");
        assert_eq!(status.job.status, "queued");
        assert_eq!(status.job.capability, "reasoning");
    }

    #[tokio::test]
    async fn cancel_and_topic_results() {
        let state = build_state();
        let first = state
            .job_queue
            .submit_job(JobSpec {
                goal: "Task one".to_string(),
                topic: "batch-a".to_string(),
                capability: RequiredCapability::General,
                priority: JobPriority::Normal,
                time_budget_ms: 10_000,
                max_turns: 3,
                system_context: None,
                allowed_skill_ids: vec![],
                ttl_seconds: 3600,
                input_data: None,
                parent_job_id: None,
                cron_expression: None,
                is_recurring: false,
                significance_keywords: vec![],
                significance_threshold: 0.5,
                job_mode: "agentic_run".into(),
                goal_template: None,
                depends_on: vec![],
            })
            .await
            .unwrap();
        state
            .job_queue
            .mark_running(&first, "agent-1", "model", "provider")
            .await
            .unwrap();
        state
            .job_queue
            .mark_completed(&first, "done", 1)
            .await
            .unwrap();

        let second = state
            .job_queue
            .submit_job(JobSpec {
                goal: "Task two".to_string(),
                topic: "batch-a".to_string(),
                capability: RequiredCapability::General,
                priority: JobPriority::Normal,
                time_budget_ms: 10_000,
                max_turns: 3,
                system_context: None,
                allowed_skill_ids: vec![],
                ttl_seconds: 3600,
                input_data: None,
                parent_job_id: None,
                cron_expression: None,
                is_recurring: false,
                significance_keywords: vec![],
                significance_threshold: 0.5,
                job_mode: "agentic_run".into(),
                goal_template: None,
                depends_on: vec![],
            })
            .await
            .unwrap();

        let cancel = cancel_job(State(state.clone()), Path(second.clone()))
            .await
            .0;
        assert!(cancel.ok);
        assert_eq!(cancel.data.unwrap().status, "cancelled");

        let topic = topic_results(
            State(state),
            Path("batch-a".to_string()),
            Query(TopicQuery { limit: 20 }),
        )
        .await
        .0;
        assert!(topic.ok);
        let topic = topic.data.unwrap();
        assert!(topic.all_terminal);
        assert_eq!(topic.jobs.len(), 1);
        assert_eq!(topic.jobs[0].status, "completed");
    }
}
