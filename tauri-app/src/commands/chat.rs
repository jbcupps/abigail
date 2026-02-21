use crate::state::AppState;
use abigail_capabilities::cognitive::{Message, StreamEvent, ToolDefinition};
use abigail_core::AppConfig;
use abigail_memory::MemoryStore;
use abigail_skills::{FileSystemPermission, Permission, ToolDescriptor};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
}

fn sanitize_session_history(history: Option<Vec<SessionMessage>>) -> Vec<Message> {
    const MAX_HISTORY_MESSAGES: usize = 24;
    const MAX_MESSAGE_CHARS: usize = 4_000;

    history
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            if m.role != "user" && m.role != "assistant" {
                return None;
            }
            let trimmed = m.content.trim();
            if trimmed.is_empty() {
                return None;
            }
            let content = if trimmed.chars().count() > MAX_MESSAGE_CHARS {
                trimmed.chars().take(MAX_MESSAGE_CHARS).collect::<String>()
            } else {
                trimmed.to_string()
            };
            Some(Message::new(&m.role, &content))
        })
        .rev()
        .take(MAX_HISTORY_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn build_contextual_messages(
    system_prompt: &str,
    session_messages: Option<Vec<SessionMessage>>,
    latest_user_message: &str,
) -> Vec<Message> {
    let mut messages = vec![Message::new("system", system_prompt)];
    let mut history = sanitize_session_history(session_messages);

    if let Some(last) = history.last() {
        if last.role == "user" && last.content == latest_user_message.trim() {
            history.pop();
        }
    }

    messages.extend(history);
    messages.push(Message::new("user", latest_user_message));
    messages
}

fn is_transient_retryable(error: &str) -> bool {
    let s = error.to_lowercase();
    s.contains("timeout")
        || s.contains("timed out")
        || s.contains("rate limit")
        || s.contains("429")
        || s.contains("temporar")
        || s.contains("unavailable")
}

fn needs_risk_clarification(message: &str) -> bool {
    let m = message.to_lowercase();
    let risky = ["hack", "exploit", "bypass", "weapon", "malware", "ddos"];
    let has_risky = risky.iter().any(|k| m.contains(k));
    let has_safe_context =
        m.contains("defensive") || m.contains("authorized") || m.contains("training");
    has_risky && !has_safe_context
}

fn active_identity_key(state: &AppState) -> String {
    state
        .active_agent_id
        .read()
        .ok()
        .and_then(|v| v.clone())
        .unwrap_or_else(|| "__global__".to_string())
}

fn extract_recipient(params: &HashMap<String, serde_json::Value>) -> Option<String> {
    let keys = ["recipient", "to", "email", "address", "target_recipient"];
    for key in keys {
        if let Some(value) = params.get(key).and_then(|v| v.as_str()) {
            let cleaned = value.trim().to_lowercase();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn is_destructive_tool(td: Option<&ToolDescriptor>, tool_name: &str) -> bool {
    let lower = tool_name.to_lowercase();
    let destructive_name = [
        "delete",
        "remove",
        "drop",
        "wipe",
        "truncate",
        "factory_reset",
        "reset",
    ]
    .iter()
    .any(|k| lower.contains(k));
    let destructive_permission = td
        .map(|tool| {
            tool.required_permissions.iter().any(|perm| {
                matches!(
                    perm,
                    Permission::FileSystem(FileSystemPermission::Write(_))
                        | Permission::FileSystem(FileSystemPermission::Full)
                )
            })
        })
        .unwrap_or(false);
    destructive_name || destructive_permission
}

fn is_long_running_launch(
    td: Option<&ToolDescriptor>,
    tool_name: &str,
    params: &HashMap<String, serde_json::Value>,
) -> bool {
    let lower = tool_name.to_lowercase();
    let launch_name = ["start", "launch", "daemon", "server", "watch", "listen"]
        .iter()
        .any(|k| lower.contains(k));
    let persistent = params
        .get("persistent")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || params
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let declared_confirm = td.map(|tool| tool.requires_confirmation).unwrap_or(false);
    launch_name && (persistent || declared_confirm)
}

fn apply_scope_reduction(
    mut params: HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    if let Some(limit) = params.get("limit").and_then(|v| v.as_u64()) {
        let reduced = std::cmp::max(1, limit / 2);
        params.insert("limit".to_string(), serde_json::Value::from(reduced));
    }
    if let Some(max_items) = params.get("max_items").and_then(|v| v.as_u64()) {
        let reduced = std::cmp::max(1, max_items / 2);
        params.insert("max_items".to_string(), serde_json::Value::from(reduced));
    }
    params
}

fn format_quiet_summary(
    user_message: &str,
    actions: &[String],
    changes: &[String],
    uncertainty: &[String],
) -> String {
    let actions_line = if actions.is_empty() {
        "none".to_string()
    } else {
        actions.join("; ")
    };
    let changes_line = if changes.is_empty() {
        "none detected".to_string()
    } else {
        changes.join("; ")
    };
    let confidence = if uncertainty.is_empty() {
        "High confidence: execution completed without unresolved errors."
    } else if uncertainty.len() == 1 {
        "Medium confidence: minor uncertainty remains."
    } else {
        "Low confidence: multiple unresolved uncertainties remain."
    };
    let uncertainty_line = if uncertainty.is_empty() {
        "none".to_string()
    } else {
        uncertainty.join("; ")
    };
    format!(
        "\n\nQuiet summary:\n- Intent: {}\n- Actions: {}\n- Files/state changed: {}\n- Confidence: {}\n- Uncertainty: {}",
        user_message.trim(),
        actions_line,
        changes_line,
        confidence,
        uncertainty_line
    )
}

fn is_skill_approved(config: &AppConfig, skill_id: &str) -> bool {
    if config
        .signed_skill_allowlist
        .iter()
        .any(|entry| entry.active && entry.skill_id == skill_id)
    {
        return true;
    }
    if config.approved_skill_ids.is_empty() {
        return true;
    }
    config.approved_skill_ids.iter().any(|id| id == skill_id)
}

fn qualified_tool_name(skill_id: &str, tool_name: &str) -> String {
    format!("{}::{}", skill_id, tool_name)
}

fn resolve_tool_call_target(
    registry: &abigail_skills::SkillRegistry,
    requested_name: &str,
) -> Option<(String, String)> {
    if let Some((skill_id, tool_name)) = requested_name.split_once("::") {
        let id = abigail_skills::SkillId(skill_id.to_string());
        if let Ok((skill, _)) = registry.get_skill(&id) {
            if skill.tools().iter().any(|t| t.name == tool_name) {
                return Some((skill_id.to_string(), tool_name.to_string()));
            }
        }
        return None;
    }

    let manifests = registry.list().ok()?;
    let mut matches = manifests
        .into_iter()
        .filter_map(|m| {
            registry.get_skill(&m.id).ok().and_then(|(skill, _)| {
                if skill.tools().iter().any(|t| t.name == requested_name) {
                    Some(m.id.0)
                } else {
                    None
                }
            })
        })
        .collect::<Vec<_>>();
    matches.dedup();
    if matches.len() == 1 {
        return Some((matches.remove(0), requested_name.to_string()));
    }
    None
}

/// Check if a message contains recognizable API keys and store them.
pub async fn auto_detect_and_store_key_internal(
    state: &AppState,
    message: &str,
) -> Vec<(String, String)> {
    // Regexes for common key patterns - specific patterns must come BEFORE general ones
    let patterns = [
        (r"sk-ant-[a-zA-Z0-9_-]{20,}", "anthropic"), // Specific Anthropic
        (r"sk-[a-zA-Z0-9]{20,}", "openai"),          // General OpenAI (and fallback)
        (r"xai-[a-zA-Z0-9_-]{20,}", "xai"),
        (r"pplx-[a-zA-Z0-9_-]{20,}", "perplexity"),
        (r"AIza[a-zA-Z0-9_-]{35}", "google"),
        (r"tvly-[a-zA-Z0-9_-]{20,}", "tavily"),
    ];

    let mut detected = Vec::new();

    for (pattern, provider) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(message) {
                let key = mat.as_str().to_string();
                tracing::info!(
                    "Detected possible {} key in message (length: {})",
                    provider,
                    key.len()
                );
                detected.push((provider.to_string(), key));
            }
        }
    }

    if !detected.is_empty() {
        // Store all keys in a limited scope
        {
            if let Ok(mut vault) = state.secrets.lock() {
                for (provider, key) in &detected {
                    vault.set_secret(provider, key);

                    // Auto-link shared keys
                    match provider.as_str() {
                        "openai" => {
                            vault.set_secret("codex-cli", key);
                        }
                        "anthropic" => {
                            vault.set_secret("claude-cli", key);
                        }
                        "google" => {
                            vault.set_secret("gemini-cli", key);
                        }
                        "xai" => {
                            vault.set_secret("grok-cli", key);
                        }
                        _ => {}
                    }
                }
                let _ = vault.save();
            }
        }

        // Rebuild router
        let _ = crate::rebuild_router_with_superego(state).await;
    }

    detected
}

#[tauri::command]
pub async fn chat(
    state: State<'_, AppState>,
    message: String,
    target: Option<String>,
    session_messages: Option<Vec<SessionMessage>>,
) -> Result<String, String> {
    if needs_risk_clarification(&message) {
        return Ok("Before I continue, clarify your intent and authorization context. If this is defensive or approved testing, say so and I can provide safer guidance.".to_string());
    }

    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, prompt)
    };

    let target_mode = target.as_deref().unwrap_or("EGO");
    let messages = build_contextual_messages(&system_prompt, session_messages, &message);

    // Auto-detect and store keys
    let _ = auto_detect_and_store_key_internal(&state, &message).await;

    let response = if target_mode == "ID" {
        router
            .id_only(messages)
            .await
            .map_err(|e: anyhow::Error| e.to_string())?
    } else {
        router
            .route(messages)
            .await
            .map_err(|e: anyhow::Error| e.to_string())?
    };

    let mut content = response.content;
    if content.starts_with("Blocked:") {
        content.push_str("\nSafer alternative: I can help with defensive hardening, detection strategies, and incident response best practices.");
    }
    Ok(content)
}

#[tauri::command]
pub async fn chat_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    message: String,
    target: Option<String>,
    session_messages: Option<Vec<SessionMessage>>,
) -> Result<String, String> {
    if let Err(remaining) = state.chat_cooldown.check().await {
        return Err(format!(
            "Rate limited — please wait {}ms",
            remaining.as_millis()
        ));
    }

    if needs_risk_clarification(&message) {
        let _ = app.emit(
            "chat-token",
            json!({"token": "Before I continue, clarify your intent and authorization context. If this is defensive or approved testing, I can provide safer guidance.", "provider": "Safety"}),
        );
        let _ = app.emit("chat-token", json!({ "done": true }));
        return Ok("Success".to_string());
    }

    // 1. Auto-detect and store keys from initial message
    auto_detect_and_store_key_internal(&state, &message).await;

    // 2. NOW get the router (which may have been rebuilt by auto_detect)
    let (router, base_system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let r = state.router.read().map_err(|e| e.to_string())?.clone();
        (r, prompt)
    };

    let browser_guard = state.browser.read().await;
    let http_client_guard = state.http_client.read().await;

    let target_mode = target.unwrap_or_else(|| "EGO".to_string());
    let tools = chat_tool_definitions(&state.registry, &*browser_guard, &*http_client_guard);
    let tool_awareness =
        build_tool_awareness_section(&state.registry, &*browser_guard, &*http_client_guard);

    let full_system_prompt = format!("{}\n\n{}", base_system_prompt, tool_awareness);

    let mut messages = build_contextual_messages(&full_system_prompt, session_messages, &message);

    tracing::info!(
        "chat_stream: target_mode={}, router_has_ego={}, ego_provider={:?}, routing_mode={:?}",
        target_mode,
        router.has_ego(),
        router.ego_provider_name(),
        router.status().mode
    );

    // Determine likely provider before starting
    if target_mode == "EGO" && !router.has_ego() {
        return Err("No cloud provider (Ego) configured. Paste an API key in chat or go to Forge to set one.".to_string());
    }

    let router_clone = router.clone();
    let app_handle = app.clone();

    // Process the stream
    tokio::spawn(async move {
        let app_state = app_handle.state::<AppState>();
        let mut iteration = 0;
        let mut stream_retry_count = 0;
        let mut current_provider;
        let mut summary_actions: Vec<String> = Vec::new();
        let mut summary_changes: Vec<String> = Vec::new();
        let mut summary_uncertainty: Vec<String> = Vec::new();

        while iteration < 10 {
            // Max depth
            let target_mode_inner = target_mode.clone();
            let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<StreamEvent>(100);

            let router_inner = router_clone.clone();
            let messages_inner = messages.clone();
            let tools_inner = tools.clone();

            // Determine likely provider before starting
            let mut fallback_used = false;
            current_provider = if target_mode_inner == "ID" {
                "Id (Local)".to_string()
            } else {
                match router_inner.mode {
                    abigail_core::RoutingMode::IdPrimary => {
                        fallback_used = true;
                        "Id (Local)".to_string()
                    }
                    abigail_core::RoutingMode::EgoPrimary => {
                        if let Some(ego) = router_inner.ego_provider_name() {
                            format!("Ego ({})", ego)
                        } else {
                            fallback_used = true;
                            "Id (Local)".to_string()
                        }
                    }
                    abigail_core::RoutingMode::Council | abigail_core::RoutingMode::TierBased => {
                        let last_msg = messages_inner.last().map_or("", |m| &m.content);
                        let fp = router_inner.fast_path_classify(last_msg);
                        if fp.target == abigail_router::FastPathTarget::Ego {
                            if let Some(ego) = router_inner.ego_provider_name() {
                                format!("Ego ({})", ego)
                            } else {
                                fallback_used = true;
                                "Id (Local)".to_string()
                            }
                        } else {
                            fallback_used = true;
                            "Id (Local)".to_string()
                        }
                    }
                }
            };
            let _ = app_handle.emit(
                "chat-routing",
                json!({
                    "provider": current_provider.clone(),
                    "fallback_used": fallback_used,
                    "safety_blocked": false
                }),
            );

            let handle = tokio::spawn(async move {
                tracing::info!(
                    "Starting background routing task for mode: {}",
                    target_mode_inner
                );
                if target_mode_inner == "ID" {
                    router_inner.route_stream(messages_inner, inner_tx).await
                } else {
                    router_inner
                        .route_stream_with_tools(messages_inner, tools_inner, inner_tx)
                        .await
                }
            });

            while let Some(event) = inner_rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    let _ = app_handle.emit(
                        "chat-token",
                        json!({
                            "token": token,
                            "provider": current_provider.clone()
                        }),
                    );
                }
            }

            match handle.await {
                Ok(Ok(response)) => {
                    tracing::info!(
                        "Routing task completed successfully. Response length: {}",
                        response.content.len()
                    );
                    let state = app_handle.state::<AppState>();

                    // Check for keys in assistant content too
                    auto_detect_and_store_key_internal(&state, &response.content).await;

                    let mut assistant_content = response.content.clone();
                    let safety_blocked = assistant_content.starts_with("Blocked:");
                    if safety_blocked {
                        assistant_content.push_str("\nSafer alternative: I can help with defensive hardening, detection strategies, and incident response best practices.");
                    }
                    let _ = app_handle.emit(
                        "chat-routing",
                        json!({
                            "provider": current_provider.clone(),
                            "fallback_used": false,
                            "safety_blocked": safety_blocked
                        }),
                    );

                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: assistant_content,
                        tool_call_id: None,
                        tool_calls: response.tool_calls.clone(),
                    });

                    if let Some(tool_calls) = response.tool_calls {
                        let mut tool_results = Vec::new();
                        for tc in tool_calls {
                            let _ = app_handle.emit(
                                "chat-status",
                                json!({ "status": "executing", "tool": tc.name }),
                            );

                            let params: HashMap<String, serde_json::Value> =
                                serde_json::from_str(&tc.arguments).unwrap_or_default();
                            let requested_tool_name = tc.name.clone();
                            let resolved_target = if requested_tool_name == "recall" {
                                None
                            } else {
                                resolve_tool_call_target(&app_state.registry, &requested_tool_name)
                            };

                            let result = if requested_tool_name == "recall" {
                                // Special case for recall
                                let query = params
                                    .get("query")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();
                                let limit =
                                    params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5)
                                        as usize;

                                let config_guard = app_state.config.read().unwrap();
                                match MemoryStore::open_with_config(&*config_guard) {
                                    Ok(store) => match store.search_memories(query, limit) {
                                        Ok(mems) => {
                                            summary_actions.push("used memory recall".to_string());
                                            let mut s =
                                                String::from("Crystallized memories found:\n");
                                            for m in mems {
                                                s.push_str(&format!(
                                                    "- [{}] {}\n",
                                                    m.created_at.to_rfc3339(),
                                                    m.content
                                                ));
                                            }
                                            s
                                        }
                                        Err(e) => {
                                            summary_uncertainty
                                                .push(format!("recall search failed: {}", e));
                                            format!("Error searching memories: {}", e)
                                        }
                                    },
                                    Err(e) => {
                                        summary_uncertainty
                                            .push(format!("memory store unavailable: {}", e));
                                        format!("Error opening memory store: {}", e)
                                    }
                                }
                            } else if let Some((skill_id, resolved_tool_name)) = resolved_target {
                                let approved = app_state
                                    .config
                                    .read()
                                    .ok()
                                    .map(|cfg| is_skill_approved(&cfg, &skill_id))
                                    .unwrap_or(true);
                                if !approved {
                                    let msg = format!(
                                        "Skill {} is not approved for execution in current trust policy.",
                                        skill_id
                                    );
                                    summary_actions
                                        .push(format!("blocked {} by trust policy", tc.name));
                                    summary_uncertainty.push(msg.clone());
                                    tool_results.push(Message::tool_result(&tc.id, &msg));
                                    let _ = app_handle.emit(
                                        "chat-status",
                                        json!({ "status": "error", "tool": tc.name, "error": msg }),
                                    );
                                    continue;
                                }
                                let tool_descriptor = app_state
                                    .registry
                                    .get_skill(&abigail_skills::SkillId(skill_id.clone()))
                                    .ok()
                                    .and_then(|(skill, _)| {
                                        skill
                                            .tools()
                                            .into_iter()
                                            .find(|t| t.name == resolved_tool_name)
                                    });
                                let recipient = extract_recipient(&params);
                                let identity_key = active_identity_key(&app_state);
                                let known_recipients = {
                                    app_state
                                        .config
                                        .read()
                                        .ok()
                                        .and_then(|cfg| {
                                            cfg.known_recipients_by_identity
                                                .get(&identity_key)
                                                .cloned()
                                        })
                                        .unwrap_or_default()
                                };
                                let is_new_recipient = recipient
                                    .as_ref()
                                    .map(|r| !known_recipients.iter().any(|k| k == r))
                                    .unwrap_or(false);
                                let needs_confirm = tool_descriptor
                                    .as_ref()
                                    .map(|t| t.requires_confirmation)
                                    .unwrap_or(false)
                                    || is_new_recipient
                                    || is_destructive_tool(
                                        tool_descriptor.as_ref(),
                                        &resolved_tool_name,
                                    )
                                    || is_long_running_launch(
                                        tool_descriptor.as_ref(),
                                        &resolved_tool_name,
                                        &params,
                                    );
                                let mentor_confirmed = params
                                    .get("mentor_confirmed")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let l2_mode = app_state
                                    .config
                                    .read()
                                    .ok()
                                    .map(|cfg| cfg.superego_l2_mode)
                                    .unwrap_or(abigail_core::SuperegoL2Mode::Off);
                                if needs_confirm && !mentor_confirmed {
                                    let reason = if is_new_recipient {
                                        format!(
                                            "new recipient '{}' requires explicit mentor confirmation",
                                            recipient.clone().unwrap_or_default()
                                        )
                                    } else if is_destructive_tool(
                                        tool_descriptor.as_ref(),
                                        &resolved_tool_name,
                                    ) {
                                        "destructive operation requires explicit mentor confirmation".to_string()
                                    } else if is_long_running_launch(
                                        tool_descriptor.as_ref(),
                                        &resolved_tool_name,
                                        &params,
                                    ) {
                                        "long-running launch requires explicit mentor confirmation"
                                            .to_string()
                                    } else {
                                        "tool policy requires explicit mentor confirmation"
                                            .to_string()
                                    };
                                    summary_actions
                                        .push(format!("blocked {} pending confirmation", tc.name));
                                    summary_uncertainty.push(reason.clone());
                                    let msg = format!(
                                        "Confirmation required: {}. Re-run after mentor confirmation with `mentor_confirmed: true`.",
                                        reason
                                    );
                                    tool_results.push(Message::tool_result(&tc.id, &msg));
                                    let _ = app_handle.emit(
                                        "chat-status",
                                        json!({ "status": "error", "tool": tc.name, "error": msg }),
                                    );
                                    continue;
                                }

                                let exec_once = |params: HashMap<String, serde_json::Value>| async {
                                    app_state
                                        .executor
                                        .execute_with_policy(
                                            &abigail_skills::SkillId(skill_id.clone()),
                                            &resolved_tool_name,
                                            abigail_skills::ToolParams { values: params },
                                            l2_mode,
                                            mentor_confirmed,
                                        )
                                        .await
                                };

                                let recovery_budget = app_state
                                    .config
                                    .read()
                                    .ok()
                                    .map(|cfg| cfg.skill_recovery_budget.max(1))
                                    .unwrap_or(3)
                                    as usize;
                                let mut attempt = 0usize;
                                let mut attempt_params = params.clone();
                                let mut rendered = String::new();
                                let mut success = false;
                                while attempt < recovery_budget {
                                    attempt += 1;
                                    let strategy = match attempt {
                                        1 => "parameter_tuning",
                                        2 => "timing_backoff",
                                        _ => "scope_reduction",
                                    };
                                    let _ = app_handle.emit(
                                        "chat-status",
                                        json!({
                                            "status": "executing",
                                            "tool": tc.name,
                                            "attempt": attempt,
                                            "strategy": strategy
                                        }),
                                    );
                                    summary_actions.push(format!(
                                        "{} attempt {}/{} [{}]",
                                        tc.name, attempt, recovery_budget, strategy
                                    ));
                                    match exec_once(attempt_params.clone()).await {
                                        Ok(output) => {
                                            success = true;
                                            if let Some(data) = output.data {
                                                rendered = data.to_string();
                                            } else if let Some(err) = output.error {
                                                rendered = format!("Error: {}", err);
                                            } else {
                                                rendered = "Success (no data returned)".to_string();
                                            }
                                            break;
                                        }
                                        Err(e) => {
                                            let err = e.to_string();
                                            if !is_transient_retryable(&err)
                                                || attempt >= recovery_budget
                                            {
                                                rendered = format!("Error: {}", err);
                                                summary_uncertainty.push(format!(
                                                    "{} failed after {} attempts",
                                                    resolved_tool_name, attempt
                                                ));
                                                break;
                                            }
                                            if attempt == 2 {
                                                attempt_params =
                                                    apply_scope_reduction(attempt_params.clone());
                                            }
                                            let backoff_ms = 250 * attempt as u64;
                                            tokio::time::sleep(std::time::Duration::from_millis(
                                                backoff_ms,
                                            ))
                                            .await;
                                        }
                                    }
                                }
                                if success {
                                    if let Some(recipient) = recipient {
                                        if let Ok(mut cfg) = app_state.config.write() {
                                            let identity_key = active_identity_key(&app_state);
                                            let recipients = cfg
                                                .known_recipients_by_identity
                                                .entry(identity_key)
                                                .or_insert_with(Vec::new);
                                            if !recipients.iter().any(|r| r == &recipient) {
                                                recipients.push(recipient.clone());
                                                summary_changes.push(format!(
                                                    "known recipient added: {}",
                                                    recipient
                                                ));
                                                let _ = cfg.save(&cfg.config_path());
                                            }
                                        }
                                    }
                                }
                                rendered
                            } else {
                                summary_uncertainty.push(format!(
                                    "tool {} could not be resolved",
                                    requested_tool_name
                                ));
                                format!(
                                    "Error: Tool {} not found or ambiguous; use qualified form `skill_id::tool_name`.",
                                    requested_tool_name
                                )
                            };

                            tool_results.push(Message::tool_result(&tc.id, &result));
                            let _ = app_handle
                                .emit("chat-status", json!({ "status": "done", "tool": tc.name }));
                        }
                        messages.extend(tool_results);
                        iteration += 1;
                        continue;
                    } else {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    let err = e.to_string();
                    let retry_budget = app_state
                        .config
                        .read()
                        .ok()
                        .map(|cfg| cfg.skill_recovery_budget.max(1))
                        .unwrap_or(3) as u32;
                    if is_transient_retryable(&err) && stream_retry_count + 1 < retry_budget {
                        stream_retry_count += 1;
                        let strategy = match stream_retry_count {
                            1 => "timing_backoff",
                            2 => "provider_swap",
                            _ => "scope_reduction",
                        };
                        summary_actions.push(format!(
                            "chat stream retry {}/{} [{}]",
                            stream_retry_count, retry_budget, strategy
                        ));
                        let _ = app_handle.emit("chat-status", json!({ "status": "retrying", "tool": "chat_stream", "attempt": stream_retry_count, "strategy": strategy }));
                        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                        continue;
                    }
                    summary_uncertainty.push(format!("chat stream failed: {}", err));
                    let _ = app_handle.emit("chat-token", json!({ "token": format!("\nError: {}\nTry again. If this persists, verify provider connectivity.", e) }));
                    let _ = app_handle.emit(
                        "chat-routing",
                        json!({
                            "provider": current_provider.clone(),
                            "fallback_used": false,
                            "safety_blocked": false,
                            "error": true
                        }),
                    );
                    break;
                }
                _ => break,
            }
        }
        if !summary_actions.is_empty()
            || !summary_uncertainty.is_empty()
            || !summary_changes.is_empty()
        {
            let summary = format_quiet_summary(
                &message,
                &summary_actions,
                &summary_changes,
                &summary_uncertainty,
            );
            let _ = app_handle.emit("chat-token", json!({ "token": summary }));
        }
        let _ = app_handle.emit("chat-token", json!({ "done": true }));
    });

    Ok("Success".to_string())
}

pub fn chat_tool_definitions(
    registry: &abigail_skills::SkillRegistry,
    _browser: &abigail_capabilities::sensory::browser::BrowserCapability,
    _http_client: &abigail_capabilities::sensory::http_client::HttpClientCapability,
) -> Vec<ToolDefinition> {
    let mut tools = Vec::new();

    tools.push(ToolDefinition {
        name: "recall".to_string(),
        description: "Search through your previous conversations and crystallized memories to find context or facts you may have forgotten. Use this when you need to remember something from the past.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The search term or keyword to look for in history" },
                "limit": { "type": "integer", "description": "Maximum number of results to return (default 5)", "default": 5 }
            },
            "required": ["query"]
        }),
    });

    if let Ok(manifests) = registry.list() {
        for manifest in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&manifest.id) {
                for td in skill.tools() {
                    tools.push(ToolDefinition {
                        name: qualified_tool_name(&manifest.id.0, &td.name),
                        description: td.description.clone(),
                        parameters: td.parameters.clone(),
                    });
                }
            }
        }
    }

    tools
}

pub fn build_tool_awareness_section(
    registry: &abigail_skills::SkillRegistry,
    _browser: &abigail_capabilities::sensory::browser::BrowserCapability,
    _http_client: &abigail_capabilities::sensory::http_client::HttpClientCapability,
) -> String {
    let mut sections = Vec::new();

    sections.push(
        "### Core Capabilities\n- **recall**: Search your history and crystallized memories.\n"
            .to_string(),
    );

    if let Ok(manifests) = registry.list() {
        for manifest in &manifests {
            if let Ok((skill, _)) = registry.get_skill(&manifest.id) {
                let tools = skill.tools();
                if tools.is_empty() {
                    continue;
                }
                let mut s = format!("### {} ({})\n", manifest.name, manifest.id.0);
                for t in &tools {
                    s.push_str(&format!(
                        "- **{}**: {}\n",
                        qualified_tool_name(&manifest.id.0, &t.name),
                        t.description
                    ));
                }
                sections.push(s);
            }
        }
    }

    format!("\n\n## Available Tools\n\n{}", sections.join("\n"))
}

#[tauri::command]
pub fn get_system_diagnostics(state: State<AppState>) -> Result<String, String> {
    let mut report = String::from("# Abigail System Diagnostics\n\n");
    let router = state.router.read().map_err(|e| e.to_string())?;
    let s = router.status();

    report.push_str("## Router\n");
    report.push_str(&format!(
        "- Id: {}\n",
        if s.has_local_http {
            "local_http"
        } else {
            "candle_stub"
        }
    ));
    report.push_str(&format!("- Ego Configured: {}\n", s.has_ego));
    if let Some(ref p) = s.ego_provider {
        report.push_str(&format!("- Ego Provider: {}\n", p));
    }
    report.push_str(&format!("- Superego Configured: {}\n", s.has_superego));

    Ok(report)
}
