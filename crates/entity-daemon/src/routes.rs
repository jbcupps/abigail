//! Entity daemon HTTP route handlers.

use crate::state::EntityDaemonState;
use abigail_capabilities::cognitive::StreamEvent;
use abigail_queue::{JobPriority, JobRecord, JobSpec, RequiredCapability};
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use entity_core::{
    ApiEnvelope, CancelChatStreamResponse, CancelJobResponse, ChatRequest, ChatResponse,
    EntityOutboxStatus, EntityStatus, JobStatusResponse, ListJobsResponse, MemoryEntry,
    MemoryInsertRequest, MemorySearchRequest, MemoryStats, QueueJobRecord,
    RuntimeSessionStatusResponse, SkillApplyAcknowledgementList, SkillInfo, SubmitJobRequest,
    SubmitJobResponse, ToolExecRequest, ToolExecResponse, ToolInfo, TopicResultsResponse,
};
use futures_util::{Stream, StreamExt};
use std::convert::Infallible;
use tokio_util::sync::CancellationToken;

const BUS_STREAM: &str = abigail_streaming::BUS_STREAM;
const BUS_TOPIC: &str = abigail_streaming::Topic::JobEvents.as_str();

pub(crate) fn publish_chat_lifecycle_event(
    broker: std::sync::Arc<dyn abigail_streaming::StreamBroker>,
    session_id: String,
    entity_id: String,
    phase: &str,
    payload: serde_json::Value,
) {
    let phase = phase.to_string();
    let watch_topic = format!("chat-{}", session_id);
    tokio::spawn(async move {
        let _ = broker
            .ensure_topic(
                BUS_STREAM,
                BUS_TOPIC,
                abigail_streaming::TopicConfig::default(),
            )
            .await;
        let mut msg = abigail_streaming::StreamMessage::new(
            serde_json::json!({
                "kind": "chat_lifecycle",
                "phase": phase,
                "session_id": session_id,
                "entity_id": entity_id,
                "payload": payload,
                "timestamp_utc": chrono::Utc::now().to_rfc3339(),
            })
            .to_string()
            .into_bytes(),
        );
        msg.headers.insert("topic".to_string(), watch_topic);
        if let Err(e) = broker.publish(BUS_STREAM, BUS_TOPIC, msg).await {
            tracing::warn!("Failed to publish chat lifecycle event: {}", e);
        }
    });
}

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
    let router = state.router.current();
    let router_status = router.status();
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
        provider_health: router.health_board(),
    }))
}

// ---------------------------------------------------------------------------
// GET /v1/session/status
// ---------------------------------------------------------------------------

pub async fn get_session_status(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<RuntimeSessionStatusResponse>> {
    let runtime_url = state.runtime_url.read().await.clone();
    let last_hive_sync_at_utc = state.last_hive_sync_at_utc.read().await.clone();
    let last_hive_error = state.last_hive_error.read().await.clone();
    let assignment_count = state.skill_assignments.read().await.len();

    Json(ApiEnvelope::success(RuntimeSessionStatusResponse {
        lease: state.session_lease.clone(),
        connected_to_hive: last_hive_error.is_none(),
        runtime_url,
        last_hive_sync_at_utc,
        last_hive_error,
        assignment_count,
    }))
}

// ---------------------------------------------------------------------------
// GET /v1/outbox/status
// ---------------------------------------------------------------------------

pub async fn get_outbox_status(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<EntityOutboxStatus>> {
    match state.outbox.status() {
        Ok(status) => Json(ApiEnvelope::success(status)),
        Err(error) => Json(ApiEnvelope::error(error)),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/chat
// ---------------------------------------------------------------------------

pub async fn chat(
    State(state): State<EntityDaemonState>,
    Json(body): Json<ChatRequest>,
) -> Json<ApiEnvelope<ChatResponse>> {
    let session_id = body
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model_override = body.model_override.clone();
    publish_chat_lifecycle_event(
        state.stream_broker.clone(),
        session_id.clone(),
        state.entity_id.clone(),
        "chat_started",
        serde_json::json!({
            "message_preview": body.message.chars().take(200).collect::<String>(),
            "model_override": model_override.clone(),
        }),
    );

    // Register selected model as the entity subscriber identity for mentor chat topic.
    let _subscriber_group = state
        .router
        .current()
        .register_selected_model_subscriber(&state.entity_id, model_override.clone());

    // Archive the user turn (async, fire-and-forget via StreamBroker).
    let user_turn = abigail_memory::ConversationTurn::new(&session_id, "user", &body.message);
    crate::memory_consumer::publish_turn(state.stream_broker.clone(), user_turn);
    let _ = state.queue_outbox_record(
        "chat_user_turn",
        serde_json::json!({
            "session_id": session_id.clone(),
            "message": body.message.clone(),
        }),
    );

    // Hand the turn to the Id → Ego pipeline and await the committed action.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let ctx = crate::pipeline::TurnContext {
        session_messages: body.session_messages,
        stream_tx: None,
        cancel: None,
        done: done_tx,
    };
    let correlation_id =
        match crate::pipeline::begin_turn(&state, &session_id, &body.message, model_override, ctx)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                return Json(ApiEnvelope::error(format!(
                    "failed to start chat turn: {e}"
                )))
            }
        };

    match tokio::time::timeout(crate::pipeline::CHAT_TURN_TIMEOUT, done_rx).await {
        Ok(Ok(Ok(response))) => Json(ApiEnvelope::success(response)),
        Ok(Ok(Err(e))) => Json(ApiEnvelope::error(e)),
        Ok(Err(_)) => Json(ApiEnvelope::error(
            "chat pipeline dropped the turn".to_string(),
        )),
        Err(_) => {
            let _ = state.turns.take(&correlation_id);
            Json(ApiEnvelope::error("chat turn timed out".to_string()))
        }
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
    let model_override = body.model_override.clone();
    publish_chat_lifecycle_event(
        state.stream_broker.clone(),
        session_id.clone(),
        state.entity_id.clone(),
        "chat_started",
        serde_json::json!({
            "message_preview": body.message.chars().take(200).collect::<String>(),
            "model_override": model_override.clone(),
        }),
    );

    // Register selected model as the entity subscriber identity for mentor chat topic.
    let _subscriber_group = state
        .router
        .current()
        .register_selected_model_subscriber(&state.entity_id, model_override.clone());

    // Archive user turn (async, fire-and-forget via StreamBroker).
    let user_turn = abigail_memory::ConversationTurn::new(&session_id, "user", &body.message);
    crate::memory_consumer::publish_turn(state.stream_broker.clone(), user_turn);
    let _ = state.queue_outbox_record(
        "chat_user_turn",
        serde_json::json!({
            "session_id": session_id.clone(),
            "message": body.message.clone(),
        }),
    );

    let (sse_tx, sse_rx) = tokio::sync::mpsc::channel::<Event>(64);
    let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<StreamEvent>(64);

    // Create cancellation token and store it so POST /v1/chat/cancel can fire it.
    let cancel_token = CancellationToken::new();
    {
        let mut active = state.active_stream_cancel.lock().await;
        if let Some(prev) = active.replace(cancel_token.clone()) {
            prev.cancel();
        }
    }

    // Hand the turn to the Id → Ego pipeline; tokens arrive via token_rx.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let ctx = crate::pipeline::TurnContext {
        session_messages: body.session_messages,
        stream_tx: Some(token_tx),
        cancel: Some(cancel_token.clone()),
        done: done_tx,
    };
    let begin =
        crate::pipeline::begin_turn(&state, &session_id, &body.message, model_override, ctx).await;

    let cancel_state = state.active_stream_cancel.clone();
    let turns = state.turns.clone();
    tokio::spawn(async move {
        let correlation_id = match begin {
            Ok(id) => id,
            Err(e) => {
                let _ = sse_tx
                    .send(
                        Event::default()
                            .event("error")
                            .data(format!("failed to start chat turn: {e}")),
                    )
                    .await;
                return;
            }
        };

        let sse_fwd = sse_tx.clone();
        let fwd_task = tokio::spawn(async move {
            while let Some(event) = token_rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    let _ = sse_fwd
                        .send(Event::default().event("token").data(token))
                        .await;
                }
            }
        });

        let outcome = tokio::time::timeout(crate::pipeline::CHAT_TURN_TIMEOUT, done_rx).await;

        // Clear the stored cancellation token.
        {
            let mut active = cancel_state.lock().await;
            *active = None;
        }

        match outcome {
            Ok(Ok(Ok(response))) => {
                // Drain remaining tokens before emitting the final event.
                let _ = fwd_task.await;
                let _ = sse_tx
                    .send(
                        Event::default()
                            .event("done")
                            .data(serde_json::to_string(&response).unwrap_or_default()),
                    )
                    .await;
            }
            Ok(Ok(Err(e))) => {
                fwd_task.abort();
                let _ = sse_tx.send(Event::default().event("error").data(e)).await;
            }
            Ok(Err(_)) => {
                fwd_task.abort();
                let _ = sse_tx
                    .send(
                        Event::default()
                            .event("error")
                            .data("chat pipeline dropped the turn"),
                    )
                    .await;
            }
            Err(_) => {
                let _ = turns.take(&correlation_id);
                fwd_task.abort();
                let _ = sse_tx
                    .send(Event::default().event("error").data("chat turn timed out"))
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
    let diagnosis = state.router.current().diagnose(&query.message);
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
// GET /v1/skills/acks
// ---------------------------------------------------------------------------

pub async fn list_skill_apply_acknowledgements(
    State(state): State<EntityDaemonState>,
) -> Json<ApiEnvelope<SkillApplyAcknowledgementList>> {
    let acknowledgements = state.recent_skill_acks.read().await.clone();
    Json(ApiEnvelope::success(SkillApplyAcknowledgementList {
        acknowledgements,
    }))
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
    let confirmed = body
        .params
        .get("mentor_confirmed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let params = if let Some(obj) = body.params.as_object() {
        let mut tp = ToolParams::new();
        for (k, v) in obj {
            if k != "mentor_confirmed" {
                tp.values.insert(k.clone(), v.clone());
            }
        }
        tp
    } else {
        ToolParams::new()
    };

    match state
        .executor
        .execute_with_confirmation(&skill_id, &body.tool_name, params, confirmed)
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
            let _ = state.queue_outbox_record(
                "memory_insert",
                serde_json::json!({
                    "id": entry.id.clone(),
                    "content": entry.content.clone(),
                    "weight": entry.weight.clone(),
                    "created_at": entry.created_at.clone(),
                }),
            );
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
        parent_correlation_id: body.parent_correlation_id,
        depth: body.depth.unwrap_or(0),
        provider_profile: body.provider_profile,
        cron_expression: None,
        is_recurring: false,
        significance_keywords: vec![],
        significance_threshold: 0.5,
        job_mode: "agentic_run".into(),
        goal_template: None,
        depends_on: vec![],
        execution_mode: abigail_queue::ExecutionMode::Mediated,
        direct_tool_call: None,
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
///
/// SSE endpoint that streams job events filtered by topic.
/// Each connection gets an ephemeral consumer group (short suffix, not full UUID)
/// so Iggy-backed brokers don't accumulate many long-lived groups.
pub async fn watch_topic(
    State(state): State<EntityDaemonState>,
    Path(topic): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    let broker = state.stream_broker.clone();
    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
    let group_name = format!("watch-{}-{}", topic, short_id);
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
            .subscribe(BUS_STREAM, BUS_TOPIC, &group_name, handler)
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
    use abigail_core::{AppConfig, RoutingMode};
    use abigail_memory::MemoryStore;
    use abigail_persistence::{EntityScope, PersistenceHandle};
    use abigail_router::IdEgoRouter;
    use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
    use abigail_streaming::{MemoryBroker, TopicConfig};
    use async_trait::async_trait;
    use axum::extract::{Path, Query, State};
    use axum::Json;
    use std::path::PathBuf;
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
        let mut router = IdEgoRouter::new(None, None, None, None, RoutingMode::EgoPrimary);
        router.id = Arc::new(MockProvider);
        router.ego = None;
        router.ego_provider = None;
        router.local_http = None;
        router.mode = RoutingMode::EgoPrimary;

        let registry = Arc::new(SkillRegistry::new());
        let executor = Arc::new(SkillExecutor::new(registry.clone()));
        let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
        let docs_dir = test_scratch_dir("abigail_routes_test_docs");
        let _ = std::fs::create_dir_all(&docs_dir);

        let stream_broker: Arc<dyn abigail_streaming::StreamBroker> =
            Arc::new(MemoryBroker::new(128));
        let queue_store = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let job_queue = Arc::new(abigail_queue::JobQueue::new(
            queue_store,
            stream_broker.clone(),
        ));

        EntityDaemonState {
            entity_id: "test-entity".to_string(),
            config: AppConfig::default_paths(),
            hive_url: "http://127.0.0.1:3141".to_string(),
            runtime_id: "runtime-test".to_string(),
            session_lease: hive_core::RuntimeSessionLease {
                lease_id: "lease-test".to_string(),
                entity_id: "test-entity".to_string(),
                runtime_id: "runtime-test".to_string(),
                entity_name: Some("Test Entity".to_string()),
                hive_url: Some("http://127.0.0.1:3141".to_string()),
                issued_at_utc: chrono::Utc::now().to_rfc3339(),
                expires_at_utc: None,
                offline_until_close: true,
                lease_scope: "entity-runtime-session".to_string(),
            },
            router: Arc::new(crate::state::RouterHandle::new(Arc::new(router))),
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
            outbox: Arc::new(
                crate::outbox::RuntimeOutbox::load(
                    test_scratch_dir("abigail_routes_test_outbox"),
                    64,
                )
                .expect("outbox"),
            ),
            last_hive_sync_at_utc: Arc::new(tokio::sync::RwLock::new(None)),
            last_hive_error: Arc::new(tokio::sync::RwLock::new(None)),
            runtime_url: Arc::new(tokio::sync::RwLock::new(None)),
            skill_assignments: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            forge_jobs: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            recent_skill_acks: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            turns: Arc::new(crate::pipeline::TurnRegistry::default()),
            soul_ref: abigail_streaming::compute_soul_ref(b"test-soul"),
        }
    }

    fn test_scratch_dir(prefix: &str) -> PathBuf {
        std::env::current_dir()
            .map(|dir| dir.join("target").join("test-data"))
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(format!("{prefix}_{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn chat_lifecycle_event_is_published_with_topic_header() {
        let broker: Arc<dyn abigail_streaming::StreamBroker> = Arc::new(MemoryBroker::new(64));
        broker
            .ensure_topic(BUS_STREAM, BUS_TOPIC, TopicConfig::default())
            .await
            .expect("ensure topic");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<abigail_streaming::StreamMessage>(4);
        let handle = broker
            .subscribe(
                BUS_STREAM,
                BUS_TOPIC,
                "test-lifecycle",
                Box::new(move |msg| {
                    let tx = tx.clone();
                    Box::pin(async move {
                        let _ = tx.send(msg).await;
                    })
                }),
            )
            .await
            .expect("subscribe");

        publish_chat_lifecycle_event(
            broker.clone(),
            "session-123".to_string(),
            "entity-abc".to_string(),
            "chat_started",
            serde_json::json!({ "message_preview": "hello" }),
        );

        let first = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for lifecycle event")
            .expect("event message");
        handle.cancel();
        assert_eq!(
            first.headers.get("topic").map(String::as_str),
            Some("chat-session-123")
        );
        let payload: serde_json::Value =
            serde_json::from_slice(&first.payload).expect("payload json");
        assert_eq!(payload["phase"], "chat_started");
        assert_eq!(payload["entity_id"], "entity-abc");
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
            parent_correlation_id: None,
            depth: None,
            provider_profile: None,
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
                parent_correlation_id: None,
                depth: 0,
                provider_profile: None,
                cron_expression: None,
                is_recurring: false,
                significance_keywords: vec![],
                significance_threshold: 0.5,
                job_mode: "agentic_run".into(),
                goal_template: None,
                depends_on: vec![],
                execution_mode: abigail_queue::ExecutionMode::Mediated,
                direct_tool_call: None,
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
                parent_correlation_id: None,
                depth: 0,
                provider_profile: None,
                cron_expression: None,
                is_recurring: false,
                significance_keywords: vec![],
                significance_threshold: 0.5,
                job_mode: "agentic_run".into(),
                goal_template: None,
                depends_on: vec![],
                execution_mode: abigail_queue::ExecutionMode::Mediated,
                direct_tool_call: None,
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
