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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeUndoStatus {
    pub available: bool,
    pub steps: usize,
    pub window_minutes: u32,
}

static FORGE_AUDIT: OnceLock<Mutex<Vec<ForgeAuditEvent>>> = OnceLock::new();
static FORGE_UNDO: OnceLock<Mutex<Vec<ForgeUndoEntry>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeChoiceDto {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeScenarioDto {
    pub id: String,
    pub title: String,
    pub description: String,
    pub choices: Vec<ForgeChoiceDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeDisplayOutput {
    pub archetype: String,
    pub soul_hash: String,
    pub sigil: String,
}

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
pub fn get_forge_scenarios() -> Result<Vec<ForgeScenarioDto>, String> {
    let engine = soul_forge::SoulForgeEngine::new();
    Ok(engine
        .scenarios()
        .iter()
        .map(|scenario| ForgeScenarioDto {
            id: scenario.id.clone(),
            title: scenario.title.clone(),
            description: scenario.description.clone(),
            choices: scenario
                .choices
                .iter()
                .map(|choice| ForgeChoiceDto {
                    id: choice.id.clone(),
                    label: choice.label.clone(),
                    description: choice.description.clone(),
                })
                .collect(),
        })
        .collect())
}

#[tauri::command]
pub fn crystallize_forge(choices: Vec<String>) -> Result<ForgeDisplayOutput, String> {
    let engine = soul_forge::SoulForgeEngine::new();
    if choices.len() != engine.scenarios().len() {
        return Err(format!(
            "Expected {} choices, got {}",
            engine.scenarios().len(),
            choices.len()
        ));
    }

    let ordered_choices = engine
        .scenarios()
        .iter()
        .zip(choices)
        .map(|(scenario, choice_id)| (scenario.id.clone(), choice_id))
        .collect::<Vec<_>>();
    let output = engine.crystallize(&ordered_choices)?;

    Ok(ForgeDisplayOutput {
        archetype: output.archetype,
        soul_hash: output.soul_hash,
        sigil: output.sigil,
    })
}

#[cfg(test)]
mod forge_command_tests {
    use super::*;
    use std::collections::BTreeSet;

    fn keys(value: &serde_json::Value) -> BTreeSet<&str> {
        value
            .as_object()
            .expect("expected JSON object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn scenario_command_exposes_only_weight_free_presentation_fields() {
        let scenarios = get_forge_scenarios().expect("scenario command should succeed");
        let value = serde_json::to_value(&scenarios).expect("scenarios should serialize");
        let scenarios_json = value.as_array().expect("scenarios should be an array");

        assert_eq!(scenarios_json.len(), 3);
        for scenario in scenarios_json {
            assert_eq!(
                keys(scenario),
                BTreeSet::from(["choices", "description", "id", "title"])
            );
            let choices = scenario["choices"]
                .as_array()
                .expect("scenario choices should be an array");
            assert!(!choices.is_empty());
            for choice in choices {
                assert_eq!(keys(choice), BTreeSet::from(["description", "id", "label"]));
            }
        }

        let encoded = serde_json::to_string(&value).expect("scenario JSON should encode");
        for hidden in [
            "\"weights\"",
            "\"deontology\"",
            "\"teleology\"",
            "\"areteology\"",
            "\"welfare\"",
        ] {
            assert!(
                !encoded.contains(hidden),
                "scenario response leaked hidden field {hidden}: {encoded}"
            );
        }
    }

    #[test]
    fn crystallize_command_maps_ordered_choice_ids_and_returns_display_dto() {
        let engine = soul_forge::SoulForgeEngine::new();
        let choices: Vec<String> = engine
            .scenarios()
            .iter()
            .map(|scenario| scenario.choices[0].id.clone())
            .collect();
        let expected_pairs: Vec<(String, String)> = engine
            .scenarios()
            .iter()
            .zip(&choices)
            .map(|(scenario, choice)| (scenario.id.clone(), choice.clone()))
            .collect();
        let expected = engine
            .crystallize(&expected_pairs)
            .expect("engine fixture should crystallize");

        let output =
            crystallize_forge(choices).expect("ordered command choices should crystallize");
        assert_eq!(output.archetype, expected.archetype);
        assert_eq!(output.soul_hash, expected.soul_hash);
        assert_eq!(output.sigil, expected.sigil);

        let value = serde_json::to_value(output).expect("output should serialize");
        assert_eq!(
            keys(&value),
            BTreeSet::from(["archetype", "sigil", "soul_hash"])
        );
        let encoded = serde_json::to_string(&value).expect("output JSON should encode");
        for hidden in [
            "\"weights\"",
            "\"deontology\"",
            "\"teleology\"",
            "\"areteology\"",
            "\"welfare\"",
            "\"choices_made\"",
        ] {
            assert!(
                !encoded.contains(hidden),
                "crystallize response leaked hidden field {hidden}: {encoded}"
            );
        }
    }

    #[test]
    fn crystallize_command_rejects_invalid_ordered_choices() {
        let engine = soul_forge::SoulForgeEngine::new();
        let valid: Vec<String> = engine
            .scenarios()
            .iter()
            .map(|scenario| scenario.choices[0].id.clone())
            .collect();

        let wrong_count = crystallize_forge(valid[..valid.len() - 1].to_vec())
            .expect_err("missing scenario choice must fail");
        assert!(wrong_count.contains("Expected 3 choices"));

        let mut unknown = valid.clone();
        unknown[0] = "not-a-real-choice".to_string();
        let unknown_error =
            crystallize_forge(unknown).expect_err("unknown scenario choice must fail");
        assert!(unknown_error.contains("Unknown choice"));

        let mut wrong_scenario = valid;
        wrong_scenario[0] = engine.scenarios()[1].choices[0].id.clone();
        let wrong_scenario_error =
            crystallize_forge(wrong_scenario).expect_err("choice from another scenario must fail");
        assert!(wrong_scenario_error.contains("Unknown choice"));
        assert!(wrong_scenario_error.contains(&engine.scenarios()[0].id));
    }
}

#[tauri::command]
pub fn preview_forge_primary_intelligence(
    state: State<AppState>,
    provider: String,
    _model: String,
    routing_mode: String,
    superego_mode: Option<String>,
) -> Result<ForgeChangePreview, String> {
    // Legacy/transitional field: accepted for compatibility with existing clients.
    // Planned removal window: after 2026-03-31 cleanup review.
    let _ = superego_mode.as_deref();
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
    _model: String,
    routing_mode: String,
    superego_mode: Option<String>,
) -> Result<ForgeApplyResult, String> {
    // Legacy/transitional field: accepted for compatibility with existing clients.
    // Planned removal window: after 2026-03-31 cleanup review.
    let _ = superego_mode.as_deref();
    let mut changes_applied = Vec::new();
    let parsed_mode: abigail_core::RoutingMode =
        serde_json::from_str(&format!("\"{}\"", routing_mode)).map_err(|e| e.to_string())?;

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let snapshot = ForgeConfigSnapshot {
            active_provider_preference: config.active_provider_preference.clone(),
            routing_mode: config.routing_mode,
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
        if config.routing_mode != parsed_mode {
            changes_applied.push(format!("Routing mode set to {}", routing_mode));
            config.routing_mode = parsed_mode;
        }

        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    if let Err(e) = crate::rebuild_router(&state).await {
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
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router(&state).await?;
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
    let provider = {
        let router = state.router.read().map_err(|e| e.to_string())?;
        router.best_available_provider()
    };

    let (stage, history) = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        (b.current_stage(), b.get_conversation().to_vec())
    };

    let mut engine = abigail_birth::BirthChatEngine::new(stage, history);
    let result = engine
        .process_genesis_message(provider.as_deref(), &message)
        .await
        .map_err(|e| e.to_string())?;

    // Sync conversation back to orchestrator
    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        if let Some(b) = birth.as_mut() {
            b.add_message("user", &message);
            b.add_message("assistant", &result.message);
        }
    }

    let is_complete = result
        .actions
        .iter()
        .any(|a| a.action_type == abigail_birth::BirthActionType::StageComplete);

    Ok(serde_json::json!({
        "message": result.message,
        "complete": is_complete
    }))
}
