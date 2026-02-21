use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeChangePreview {
    pub changes: Vec<String>,
    pub risk_level: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeApplyResult {
    pub success: bool,
    pub changes_applied: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeAuditEvent {
    pub timestamp: String,
    pub actor: String,
    pub what_changed: String,
    pub risk_level: String,
    pub outcome: String,
}

#[derive(Debug, Clone)]
struct ForgeUndoEntry {
    created_at: chrono::DateTime<chrono::Utc>,
    snapshot: ForgeConfigSnapshot,
}

#[derive(Debug, Clone)]
struct ForgeConfigSnapshot {
    active_provider_preference: Option<String>,
    routing_mode: abigail_core::RoutingMode,
    tier_models: Option<abigail_core::TierModels>,
    superego_l2_mode: abigail_core::SuperegoL2Mode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeUndoStatus {
    pub available: bool,
    pub steps: usize,
    pub window_minutes: u32,
}

static FORGE_AUDIT: OnceLock<Mutex<Vec<ForgeAuditEvent>>> = OnceLock::new();
static FORGE_UNDO: OnceLock<Mutex<Vec<ForgeUndoEntry>>> = OnceLock::new();

fn audit_log() -> &'static Mutex<Vec<ForgeAuditEvent>> {
    FORGE_AUDIT.get_or_init(|| Mutex::new(Vec::new()))
}

fn undo_log() -> &'static Mutex<Vec<ForgeUndoEntry>> {
    FORGE_UNDO.get_or_init(|| Mutex::new(Vec::new()))
}

fn prune_undo() {
    if let Ok(mut log) = undo_log().lock() {
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(30);
        log.retain(|e| e.created_at >= cutoff);
    }
}

fn push_audit(what_changed: String, risk_level: &str, outcome: &str) {
    if let Ok(mut log) = audit_log().lock() {
        log.push(ForgeAuditEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            actor: "mentor".to_string(),
            what_changed,
            risk_level: risk_level.to_string(),
            outcome: outcome.to_string(),
        });
        if log.len() > 300 {
            let keep_from = log.len().saturating_sub(300);
            log.drain(0..keep_from);
        }
    }
}

#[tauri::command]
pub fn get_forge_scenarios(_state: State<AppState>) -> Result<serde_json::Value, String> {
    // Stub for now
    Ok(serde_json::json!([]))
}

#[tauri::command]
pub fn crystallize_forge(_state: State<AppState>) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn preview_forge_primary_intelligence(
    state: State<AppState>,
    provider: String,
    model: String,
    routing_mode: String,
    superego_mode: Option<String>,
) -> Result<ForgeChangePreview, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let mut changes = Vec::new();

    if config.active_provider_preference.as_deref() != Some(provider.as_str()) {
        changes.push(format!(
            "Active provider: {} -> {}",
            config
                .active_provider_preference
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            provider
        ));
    }

    let current_model = config
        .tier_models
        .as_ref()
        .and_then(|tm| tm.standard.get(&provider).cloned())
        .unwrap_or_default();
    if !model.is_empty() && current_model != model {
        changes.push(format!(
            "Model for {}: {} -> {}",
            provider,
            if current_model.is_empty() {
                "default"
            } else {
                &current_model
            },
            model
        ));
    }

    let current_routing = serde_json::to_value(&config.routing_mode)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "tier_based".to_string());
    if current_routing != routing_mode {
        changes.push(format!(
            "Routing mode: {} -> {}",
            current_routing, routing_mode
        ));
    }

    if let Some(s) = superego_mode {
        let current_sup = serde_json::to_string(&config.superego_l2_mode)
            .unwrap_or_else(|_| "\"off\"".to_string())
            .replace('\"', "");
        if current_sup != s {
            changes.push(format!("Superego mode: {} -> {}", current_sup, s));
        }
    }

    let risk = if changes.is_empty() { "low" } else { "high" };
    Ok(ForgeChangePreview {
        changes,
        risk_level: risk.to_string(),
        requires_confirmation: risk == "high",
    })
}

#[tauri::command]
pub async fn apply_forge_primary_intelligence(
    state: State<'_, AppState>,
    provider: String,
    model: String,
    routing_mode: String,
    superego_mode: Option<String>,
) -> Result<ForgeApplyResult, String> {
    let mut changes_applied = Vec::new();
    let parsed_mode: abigail_core::RoutingMode =
        serde_json::from_str(&format!("\"{}\"", routing_mode)).map_err(|e| e.to_string())?;
    let parsed_superego = if let Some(mode) = superego_mode {
        Some(
            serde_json::from_str::<abigail_core::SuperegoL2Mode>(&format!("\"{}\"", mode))
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let snapshot = ForgeConfigSnapshot {
            active_provider_preference: config.active_provider_preference.clone(),
            routing_mode: config.routing_mode,
            tier_models: config.tier_models.clone(),
            superego_l2_mode: config.superego_l2_mode,
        };
        if let Ok(mut undo) = undo_log().lock() {
            undo.push(ForgeUndoEntry {
                created_at: chrono::Utc::now(),
                snapshot,
            });
        }
        prune_undo();

        if config.active_provider_preference.as_deref() != Some(provider.as_str()) {
            changes_applied.push(format!("Active provider set to {}", provider));
            config.active_provider_preference = Some(provider.clone());
        }
        if !model.is_empty() {
            let tm = config
                .tier_models
                .get_or_insert_with(abigail_core::TierModels::defaults);
            let prev = tm.standard.get(&provider).cloned().unwrap_or_default();
            if prev != model {
                changes_applied.push(format!("Model for {} set to {}", provider, model));
                tm.standard.insert(provider.clone(), model);
            }
        }
        if config.routing_mode != parsed_mode {
            changes_applied.push(format!("Routing mode set to {}", routing_mode));
            config.routing_mode = parsed_mode;
        }
        if let Some(sup) = parsed_superego {
            if config.superego_l2_mode != sup {
                changes_applied.push("Superego mode updated".to_string());
                config.superego_l2_mode = sup;
            }
        }

        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    if let Err(e) = crate::rebuild_router_with_superego(&state).await {
        push_audit(
            if changes_applied.is_empty() {
                "No changes".to_string()
            } else {
                changes_applied.join("; ")
            },
            "high",
            &format!("failure: {}", e),
        );
        return Err(e);
    }

    push_audit(
        if changes_applied.is_empty() {
            "No changes".to_string()
        } else {
            changes_applied.join("; ")
        },
        "high",
        "success",
    );

    Ok(ForgeApplyResult {
        success: true,
        changes_applied,
    })
}

#[tauri::command]
pub async fn forge_undo_last_change(state: State<'_, AppState>) -> Result<String, String> {
    prune_undo();
    let entry = {
        let mut undo = undo_log().lock().map_err(|e| e.to_string())?;
        undo.pop()
    };
    let Some(entry) = entry else {
        return Err("No undo entries available in the last 30 minutes.".to_string());
    };

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.active_provider_preference = entry.snapshot.active_provider_preference;
        config.routing_mode = entry.snapshot.routing_mode;
        config.tier_models = entry.snapshot.tier_models;
        config.superego_l2_mode = entry.snapshot.superego_l2_mode;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router_with_superego(&state).await?;
    push_audit(
        "Undo applied: primary intelligence settings restored".to_string(),
        "high",
        "success",
    );
    Ok("Undo applied".to_string())
}

#[tauri::command]
pub fn get_forge_audit_events() -> Result<Vec<ForgeAuditEvent>, String> {
    let log = audit_log().lock().map_err(|e| e.to_string())?;
    Ok(log.clone())
}

#[tauri::command]
pub fn get_forge_undo_status() -> Result<ForgeUndoStatus, String> {
    prune_undo();
    let log = undo_log().lock().map_err(|e| e.to_string())?;
    Ok(ForgeUndoStatus {
        available: !log.is_empty(),
        steps: log.len(),
        window_minutes: 30,
    })
}

#[tauri::command]
pub async fn genesis_chat(
    state: State<'_, AppState>,
    message: String,
) -> Result<serde_json::Value, String> {
    let (router, history) = {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;

        let history = b.get_conversation().to_vec();
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, history)
    };

    let mut messages = Vec::new();
    let system_prompt = "You are Abigail, an AI agent in the process of Direct Discovery. \
        Help your mentor define your soul, name, and purpose through conversation. \
        Be concise and insightful. \
        When the mentor is satisfied with the identity (name, soul, purpose), summarize the final choice clearly and end your message with the exact phrase: 'READY TO EMERGE'.";

    messages.push(abigail_capabilities::cognitive::Message::new(
        "system",
        system_prompt,
    ));

    for (role, content) in history {
        messages.push(abigail_capabilities::cognitive::Message::new(
            &role, &content,
        ));
    }
    messages.push(abigail_capabilities::cognitive::Message::new(
        "user", &message,
    ));

    let response = if router.status().has_ego {
        router.route(messages).await.map_err(|e| e.to_string())?
    } else {
        router.id_only(messages).await.map_err(|e| e.to_string())?
    };

    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        if let Some(b) = birth.as_mut() {
            b.add_message("user", &message);
            b.add_message("assistant", &response.content);
        }
    }

    let is_complete = response.content.contains("READY TO EMERGE")
        || response.content.to_lowercase().contains("ready to emerge")
        || response.content.to_lowercase().contains("complete");

    Ok(serde_json::json!({
        "message": response.content,
        "complete": is_complete
    }))
}
