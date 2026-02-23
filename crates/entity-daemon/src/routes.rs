//! Entity daemon HTTP route handlers.

use crate::state::EntityDaemonState;
use axum::extract::{Query, State};
use axum::Json;
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, MemoryEntry, MemoryInsertRequest,
    MemorySearchRequest, MemoryStats, SkillInfo, ToolExecRequest, ToolExecResponse, ToolInfo,
};

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
    use crate::chat_pipeline;

    // 1. Risk clarification check
    if chat_pipeline::needs_risk_clarification(&body.message) {
        return Json(ApiEnvelope::success(ChatResponse {
            reply: "Before I continue, clarify your intent and authorization context. \
                    If this is defensive or approved testing, say so and I can provide safer guidance."
                .to_string(),
            provider: Some("safety".to_string()),
            tool_calls_made: Vec::new(),
        }));
    }

    // 2. Build system prompt from constitutional documents
    let base_system_prompt =
        abigail_core::system_prompt::build_system_prompt(&state.docs_dir, &state.config.agent_name);

    // 3. Build tool awareness section from registered skills
    let tool_awareness = chat_pipeline::build_tool_awareness_section(&state.registry);

    // 4. Combine system prompt + tool awareness
    let full_system_prompt = if tool_awareness.is_empty() {
        base_system_prompt
    } else {
        format!("{}\n\n{}", base_system_prompt, tool_awareness)
    };

    // 5. Build contextual messages with sanitization + deduplication
    let messages = chat_pipeline::build_contextual_messages(
        &full_system_prompt,
        body.session_messages,
        &body.message,
    );

    // 6. Build tool definitions from registered skills
    let tools = chat_pipeline::build_tool_definitions(&state.registry);

    // 7. Route — use tool-use loop if tools are available, plain route otherwise
    let target = body.target.as_deref().unwrap_or("AUTO");
    let result = if tools.is_empty() || target == "ID" {
        // No tools or explicit Id-only: simple route
        let res = if target == "ID" {
            state.router.id_only(messages).await
        } else {
            state.router.route(messages).await
        };
        res.map(|r| chat_pipeline::ToolUseResult {
            content: r.content,
            tool_calls_made: Vec::new(),
        })
    } else {
        // Tools available: run the agentic tool-use loop
        chat_pipeline::run_tool_use_loop(&state.router, &state.executor, messages, tools).await
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

            // 8. Append safety alternative if response starts with "Blocked:"
            let mut content = tool_result.content;
            if content.starts_with("Blocked:") {
                content.push_str("\nSafer alternative: I can help with defensive hardening, detection strategies, and incident response best practices.");
            }

            // 9. Persist chat exchange as ephemeral memories (fire-and-forget)
            persist_chat_memories(&state, &body.message, &content);

            Json(ApiEnvelope::success(ChatResponse {
                reply: content,
                provider: Some(provider),
                tool_calls_made: tool_result.tool_calls_made,
            }))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

/// Persist user message and assistant reply as ephemeral memories.
/// Best-effort: logs warnings but doesn't fail the chat response.
fn persist_chat_memories(state: &EntityDaemonState, user_msg: &str, assistant_reply: &str) {
    use abigail_memory::Memory;

    let user_memory = Memory::ephemeral(format!("User: {}", user_msg));
    if let Err(e) = state.memory.insert_memory(&user_memory) {
        tracing::warn!("Failed to persist user memory: {}", e);
    }

    let assistant_memory = Memory::ephemeral(format!("Assistant: {}", assistant_reply));
    if let Err(e) = state.memory.insert_memory(&assistant_memory) {
        tracing::warn!("Failed to persist assistant memory: {}", e);
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
        Ok(()) => Json(ApiEnvelope::success(entry)),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}
