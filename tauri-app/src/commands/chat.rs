use crate::state::AppState;
use abigail_capabilities::cognitive::{Message, StreamEvent, ToolDefinition};
use abigail_memory::MemoryStore;
use serde_json::json;
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State};

/// Check if a message contains recognizable API keys and store them.
pub async fn auto_detect_and_store_key_internal(state: &AppState, message: &str) -> Vec<(String, String)> {
    // Regexes for common key patterns
    let patterns = [
        (r"sk-[a-zA-Z0-9]{20,}", "openai"),
        (r"sk-ant-[a-zA-Z0-9]{20,}", "anthropic"),
        (r"xai-[a-zA-Z0-9]{20,}", "xai"),
        (r"pplx-[a-zA-Z0-9]{20,}", "perplexity"),
        (r"AIza[a-zA-Z0-9_-]{35}", "google"),
        (r"tvly-[a-zA-Z0-9]{20,}", "tavily"),
    ];

    let mut detected = Vec::new();

    for (pattern, provider) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for mat in re.find_iter(message) {
                let key = mat.as_str().to_string();
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
                        "openai" => { vault.set_secret("codex-cli", key); }
                        "anthropic" => { vault.set_secret("claude-cli", key); }
                        "google" => { vault.set_secret("gemini-cli", key); }
                        "xai" => { vault.set_secret("grok-cli", key); }
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
) -> Result<String, String> {
    let (router, system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, prompt)
    };

    let target_mode = target.as_deref().unwrap_or("EGO");
    let messages = vec![
        Message::new("system", &system_prompt),
        Message::new("user", &message),
    ];

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

    Ok(response.content)
}

#[tauri::command]
pub async fn chat_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    message: String,
    target: Option<String>,
) -> Result<String, String> {
    if let Err(remaining) = state.chat_cooldown.check().await {
        return Err(format!(
            "Rate limited — please wait {}ms",
            remaining.as_millis()
        ));
    }

    let (router, base_system_prompt) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let prompt =
            abigail_core::system_prompt::build_system_prompt(&config.docs_dir, &config.agent_name);
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, prompt)
    };

    let browser_guard = state.browser.read().await;
    let http_client_guard = state.http_client.read().await;

    let target_mode = target.unwrap_or_else(|| "EGO".to_string());
    let tools = chat_tool_definitions(&state.registry, &*browser_guard, &*http_client_guard);
    let tool_awareness =
        build_tool_awareness_section(&state.registry, &*browser_guard, &*http_client_guard);

    let full_system_prompt = format!("{}\n\n{}", base_system_prompt, tool_awareness);

    let mut messages = vec![
        Message::new("system", &full_system_prompt),
        Message::new("user", &message),
    ];

    // Auto-detect and store keys from initial message
    auto_detect_and_store_key_internal(&state, &message).await;

    let router_clone = router.clone();
    let app_handle = app.clone();
    
    // Process the stream
    tokio::spawn(async move {
        let mut iteration = 0;
        while iteration < 10 { // Max depth
            let target_mode_inner = target_mode.clone();
            let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<StreamEvent>(100);
            
            let router_inner = router_clone.clone();
            let messages_inner = messages.clone();
            let tools_inner = tools.clone();
            
            let handle = tokio::spawn(async move {
                if target_mode_inner == "ID" {
                    router_inner.route_stream(messages_inner, inner_tx).await
                } else {
                    router_inner.route_stream_with_tools(messages_inner, tools_inner, inner_tx).await
                }
            });

            while let Some(event) = inner_rx.recv().await {
                if let StreamEvent::Token(token) = event {
                    let _ = app_handle.emit("chat-token", json!({ "token": token }));
                }
            }

            match handle.await {
                Ok(Ok(response)) => {
                    let state = app_handle.state::<AppState>();
                    
                    // Check for keys in assistant content too
                    auto_detect_and_store_key_internal(&state, &response.content).await;
                    
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: response.content.clone(),
                        tool_call_id: None,
                        tool_calls: response.tool_calls.clone(),
                    });
                    
                    if let Some(tool_calls) = response.tool_calls {
                        let mut tool_results = Vec::new();
                        for tc in tool_calls {
                            let _ = app_handle.emit("chat-status", json!({ "status": "executing", "tool": tc.name }));
                            
                            let params: HashMap<String, serde_json::Value> = serde_json::from_str(&tc.arguments).unwrap_or_default();
                            
                            // Resolve skill ID
                            let skill_id = if tc.name == "recall" {
                                "builtin.memory".to_string() 
                            } else {
                                // Try to find which skill has this tool
                                state.registry.list().ok().and_then(|list| {
                                    list.into_iter().find(|m| {
                                        if let Ok((skill, _)) = state.registry.get_skill(&m.id) {
                                            skill.tools().iter().any(|t| t.name == tc.name)
                                        } else {
                                            false
                                        }
                                    }).map(|m| m.id.0)
                                }).unwrap_or_default()
                            };

                            let result = if tc.name == "recall" {
                                // Special case for recall
                                let query = params.get("query").and_then(|v| v.as_str()).unwrap_or_default();
                                let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
                                
                                let config_guard = state.config.read().unwrap();
                                match MemoryStore::open_with_config(&*config_guard) {
                                    Ok(store) => {
                                        match store.search_memories(query, limit) {
                                            Ok(mems) => {
                                                let mut s = String::from("Crystallized memories found:\n");
                                                for m in mems {
                                                    s.push_str(&format!("- [{}] {}\n", m.created_at.to_rfc3339(), m.content));
                                                }
                                                s
                                            }
                                            Err(e) => format!("Error searching memories: {}", e),
                                        }
                                    }
                                    Err(e) => format!("Error opening memory store: {}", e),
                                }
                            } else if !skill_id.is_empty() {
                                match state.executor.execute(&abigail_skills::SkillId(skill_id.clone()), &tc.name, abigail_skills::ToolParams { values: params }).await {
                                    Ok(output) => {
                                        if let Some(data) = output.data {
                                            data.to_string()
                                        } else if let Some(err) = output.error {
                                            format!("Error: {}", err)
                                        } else {
                                            "Success (no data returned)".to_string()
                                        }
                                    },
                                    Err(e) => format!("Error: {}", e),
                                }
                            } else {
                                format!("Error: Tool {} not found", tc.name)
                            };

                            tool_results.push(Message::tool_result(&tc.id, &result));
                            let _ = app_handle.emit("chat-status", json!({ "status": "done", "tool": tc.name }));
                        }
                        messages.extend(tool_results);
                        iteration += 1;
                        continue;
                    } else {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    let _ = app_handle.emit("chat-token", json!({ "token": format!("\nError: {}", e) }));
                    break;
                }
                _ => break,
            }
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
                        name: td.name.clone(),
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
                    s.push_str(&format!("- **{}**: {}\n", t.name, t.description));
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
