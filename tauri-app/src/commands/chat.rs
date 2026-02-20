use crate::state::AppState;
use abigail_capabilities::cognitive::{Message, ToolDefinition};
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

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

    let target_mode = target.as_deref().unwrap_or("EGO");
    let tools = chat_tool_definitions(&state.registry, &*browser_guard, &*http_client_guard);
    let tool_awareness =
        build_tool_awareness_section(&state.registry, &*browser_guard, &*http_client_guard);

    let full_system_prompt = format!("{}\n\n{}", base_system_prompt, tool_awareness);

    let messages = vec![
        Message::new("system", &full_system_prompt),
        Message::new("user", &message),
    ];

    let response = if target_mode == "ID" {
        router
            .id_only(messages)
            .await
            .map_err(|e: anyhow::Error| e.to_string())?
    } else {
        router
            .route_with_tools(messages, tools)
            .await
            .map_err(|e: anyhow::Error| e.to_string())?
    };

    let _ = app.emit("chat-token", response.content.clone());

    Ok("Success".to_string())
}

pub fn chat_tool_definitions(
    registry: &abigail_skills::SkillRegistry,
    browser: &abigail_capabilities::sensory::browser::BrowserCapability,
    http_client: &abigail_capabilities::sensory::http_client::HttpClientCapability,
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

    tools.extend(browser.tool_definitions());
    tools.extend(http_client.tool_definitions());

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
    browser: &abigail_capabilities::sensory::browser::BrowserCapability,
    http_client: &abigail_capabilities::sensory::http_client::HttpClientCapability,
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
