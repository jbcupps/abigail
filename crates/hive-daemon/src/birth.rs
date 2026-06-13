//! Birth rite routes — the Soul Forge as an HTTP ceremony.
//!
//! The rite turns entity creation into character creation: the mentor faces
//! three ethical trials, the choices forge Triangle Ethic weights, and the
//! Hive issues a signed [`BirthCertificate`] — the entity's character sheet.
//! A "quickstart" path skips the trials and births a balanced soul.
//!
//! The rite is re-runnable: performing it again on a living entity
//! re-tempers the soul (new weights, new certificate) without touching
//! memories or identity keys.

use crate::state::HiveDaemonState;
use axum::{
    extract::{Path, State},
    Json,
};
use hive_core::{
    ApiEnvelope, BirthCertificate, BirthRiteRequest, BirthRiteResponse, EntityBirthDocument,
    EntityInfo, EthicWeights, ForgeChoiceView, ForgeScenarioView, ForgeScenariosResponse,
};
use soul_forge::{SoulForgeEngine, SoulOutput};

/// Flavor line for each soul archetype, shown on the character sheet.
pub fn archetype_epithet(archetype: &str) -> &'static str {
    match archetype {
        "The Guardian" => "Duty held gently — rules in service of the people they protect.",
        "The Sentinel" => "Unwavering watch — principle sharpened into vigilance.",
        "The Arbiter" => "The line must hold — fairness before convenience.",
        "The Architect" => "Builds the better outcome, and builds it for everyone.",
        "The Strategist" => "Sees three moves ahead and plays for the whole board.",
        "The Pragmatist" => "What works, works — results over rituals.",
        "The Sage" => "Wisdom warmed by kindness — excellence that uplifts.",
        "The Philosopher" => "Character first; the rules follow from who you are.",
        "The Seeker" => "Forever becoming — growth as a way of being.",
        "The Empath" => "Feels the room before reading it — care leads.",
        "The Protector" => "A shield with a heartbeat — loyalty bound by principle.",
        "The Caretaker" => "Tends the small things that hold a family together.",
        _ => "All four flames in balance — ready to become anything.",
    }
}

/// The canonical certificate payload string that gets signed.
/// Versioned so future fields can extend it without breaking verification.
pub fn certificate_payload(
    entity_id: &str,
    entity_name: &str,
    archetype: &str,
    soul_hash: &str,
    birth_path: &str,
    issued_at_utc: &str,
) -> String {
    format!(
        "abigail-birth-certificate-v1\nentity_id={}\nentity_name={}\narchetype={}\nsoul_hash={}\nbirth_path={}\nissued_at={}",
        entity_id, entity_name, archetype, soul_hash, birth_path, issued_at_utc
    )
}

// ---------------------------------------------------------------------------
// GET /v1/birth/scenarios
// ---------------------------------------------------------------------------

/// The three trials of the Soul Forge. Weight effects are intentionally not
/// exposed — choices should be made on values, not stats.
pub async fn get_scenarios() -> Json<ApiEnvelope<ForgeScenariosResponse>> {
    let engine = SoulForgeEngine::new();
    let scenarios = engine
        .scenarios()
        .iter()
        .map(|s| ForgeScenarioView {
            id: s.id.clone(),
            title: s.title.clone(),
            description: s.description.clone(),
            choices: s
                .choices
                .iter()
                .map(|c| ForgeChoiceView {
                    id: c.id.clone(),
                    label: c.label.clone(),
                    description: c.description.clone(),
                })
                .collect(),
        })
        .collect();
    Json(ApiEnvelope::success(ForgeScenariosResponse { scenarios }))
}

// ---------------------------------------------------------------------------
// POST /v1/entities/:id/birth
// ---------------------------------------------------------------------------

/// Perform the birth rite: forge the soul, write the soul documents, sign
/// the certificate, and mark the entity born.
pub async fn perform_birth(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(body): Json<BirthRiteRequest>,
) -> Json<ApiEnvelope<BirthRiteResponse>> {
    let mut config = match state.identity_manager.load_agent(&entity_id) {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };
    let entity_name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "Unnamed Entity".to_string());
    let retempered = config.birth_complete;

    // 1. Forge the soul.
    let soul: SoulOutput = match body.path.as_str() {
        "forge" => match SoulForgeEngine::new().crystallize(&body.choices) {
            Ok(output) => output,
            Err(e) => return Json(ApiEnvelope::error(format!("Soul Forge failed: {}", e))),
        },
        "quickstart" => soul_forge::quickstart_output(),
        other => {
            return Json(ApiEnvelope::error(format!(
                "Unknown birth path '{}'. Use \"quickstart\" or \"forge\".",
                other
            )))
        }
    };

    // 2. Write the soul documents into the entity's docs dir (the same files
    //    entity-daemon builds its system prompt and soul_ref from).
    if let Err(e) = write_soul_documents(&config.docs_dir, &entity_name, &soul) {
        return Json(ApiEnvelope::error(format!(
            "Failed to write soul documents: {}",
            e
        )));
    }

    // 3. Issue the signed certificate.
    let issued_at_utc = chrono::Utc::now().to_rfc3339();
    let payload = certificate_payload(
        &entity_id,
        &entity_name,
        &soul.archetype,
        &soul.soul_hash,
        &body.path,
        &issued_at_utc,
    );
    let (signature, master_public_key) = state.identity_manager.sign_payload(payload.as_bytes());
    let certificate = BirthCertificate {
        entity_id: entity_id.clone(),
        entity_name: entity_name.clone(),
        archetype: soul.archetype.clone(),
        epithet: archetype_epithet(&soul.archetype).to_string(),
        weights: EthicWeights {
            deontology: soul.weights.deontology,
            teleology: soul.weights.teleology,
            areteology: soul.weights.areteology,
            welfare: soul.weights.welfare,
        },
        soul_hash: soul.soul_hash.clone(),
        sigil: soul.sigil.clone(),
        birth_path: body.path.clone(),
        issued_at_utc,
        master_public_key,
        signature,
    };

    // 4. Persist the certificate beside the soul documents.
    let cert_path = config.docs_dir.join("birth_certificate.json");
    match serde_json::to_string_pretty(&certificate) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&cert_path, json) {
                return Json(ApiEnvelope::error(format!(
                    "Failed to write birth certificate: {}",
                    e
                )));
            }
        }
        Err(e) => return Json(ApiEnvelope::error(e.to_string())),
    }

    // 5. Mark the entity born.
    config.birth_complete = true;
    config.birth_timestamp = Some(certificate.issued_at_utc.clone());
    let config_path = config.config_path();
    if let Err(e) = config.save(&config_path) {
        return Json(ApiEnvelope::error(format!(
            "Soul forged but config save failed: {}",
            e
        )));
    }

    tracing::info!(
        "Entity {} ({}) {} as {} via {} path",
        entity_name,
        entity_id,
        if retempered { "re-tempered" } else { "born" },
        certificate.archetype,
        body.path
    );

    Json(ApiEnvelope::success(BirthRiteResponse {
        certificate,
        retempered,
    }))
}

/// Write soul.md (template + forged identity), and seed ethics.md /
/// instincts.md from templates when absent. Re-running the rite rewrites the
/// forged section while preserving the constitutional base.
fn write_soul_documents(
    docs_dir: &std::path::Path,
    entity_name: &str,
    soul: &SoulOutput,
) -> std::io::Result<()> {
    std::fs::create_dir_all(docs_dir)?;

    let forged_section = format!(
        "\n\n## Forged Identity\n\n\
         - **Name:** {name}\n\
         - **Archetype:** {archetype}\n\
         - **Soul Hash:** `{hash}`\n\n\
         Your soul was forged through ethical trials. Your moral compass weighs:\n\
         - Duty and principle (deontology): {d:.0}%\n\
         - Outcomes and consequences (teleology): {t:.0}%\n\
         - Character and virtue (areteology): {a:.0}%\n\
         - Care and relationships (welfare): {w:.0}%\n\n\
         Let the strongest flames lead when values conflict, but never let any go out.\n\
         You are {archetype}: {epithet}\n",
        name = entity_name,
        archetype = soul.archetype,
        hash = soul.soul_hash,
        d = soul.weights.deontology * 100.0,
        t = soul.weights.teleology * 100.0,
        a = soul.weights.areteology * 100.0,
        w = soul.weights.welfare * 100.0,
        epithet = archetype_epithet(&soul.archetype),
    );

    // soul.md = constitutional template + forged identity. The template part
    // is stable; only the forged section changes on re-tempering.
    let soul_md = format!("{}{}", abigail_core::templates::SOUL_MD, forged_section);
    std::fs::write(docs_dir.join("soul.md"), soul_md)?;

    // Seed the remaining constitutional docs if missing (never overwrite —
    // they may carry mentor edits or signatures).
    let ethics_path = docs_dir.join("ethics.md");
    if !ethics_path.exists() {
        std::fs::write(ethics_path, abigail_core::templates::ETHICS_MD)?;
    }
    let instincts_path = docs_dir.join("instincts.md");
    if !instincts_path.exists() {
        std::fs::write(instincts_path, abigail_core::templates::INSTINCTS_MD)?;
    }

    // Record the raw forge output for future rites and diagnostics.
    if let Ok(json) = serde_json::to_string_pretty(soul) {
        let _ = std::fs::write(docs_dir.join("forge_soul.json"), json);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// GET /v1/entities/:id/birth
// ---------------------------------------------------------------------------

/// The idempotent birth document: the entity's full runtime identity in one
/// shot. entity-daemon re-reads this on every launch.
pub async fn get_birth_document(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<EntityBirthDocument>> {
    let config = match state.identity_manager.load_agent(&entity_id) {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    let entity = match state.identity_manager.list_agents() {
        Ok(agents) => match agents.into_iter().find(|a| a.id == entity_id) {
            Some(a) => EntityInfo {
                id: a.id,
                name: a.name,
                birth_complete: a.birth_complete,
                birth_date: a.birth_date,
                is_hive: a.is_hive,
                immortal: a.immortal,
            },
            None => {
                return Json(ApiEnvelope::error(format!(
                    "Entity {} not found",
                    entity_id
                )))
            }
        },
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    let certificate = std::fs::read_to_string(config.docs_dir.join("birth_certificate.json"))
        .ok()
        .and_then(|json| serde_json::from_str::<BirthCertificate>(&json).ok());

    let provider_config = match state.hive.resolve_config(&config) {
        Ok(hive_config) => crate::routes::provider_config_from_hive_config(&hive_config),
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    let assignments = state
        .runtime_control
        .lock()
        .map(|control| control.assignments(&entity_id).assignments)
        .unwrap_or_default();

    Json(ApiEnvelope::success(EntityBirthDocument {
        entity,
        certificate,
        provider_config,
        assignments,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_payload_is_stable() {
        let a = certificate_payload(
            "e1",
            "Adam",
            "The Guardian",
            "abc",
            "forge",
            "2026-06-10T00:00:00Z",
        );
        let b = certificate_payload(
            "e1",
            "Adam",
            "The Guardian",
            "abc",
            "forge",
            "2026-06-10T00:00:00Z",
        );
        assert_eq!(a, b);
        assert!(a.starts_with("abigail-birth-certificate-v1\n"));
        assert!(a.contains("archetype=The Guardian"));
    }

    #[test]
    fn every_archetype_has_an_epithet() {
        for archetype in [
            "The Guardian",
            "The Sentinel",
            "The Arbiter",
            "The Architect",
            "The Strategist",
            "The Pragmatist",
            "The Sage",
            "The Philosopher",
            "The Seeker",
            "The Empath",
            "The Protector",
            "The Caretaker",
            "The Balanced",
        ] {
            assert!(!archetype_epithet(archetype).is_empty());
        }
    }

    #[test]
    fn soul_documents_written_and_retempering_preserves_constitution() {
        let dir = std::env::temp_dir().join(format!("abigail_birth_{}", uuid::Uuid::new_v4()));
        let soul = soul_forge::quickstart_output();
        write_soul_documents(&dir, "Testling", &soul).unwrap();

        let soul_md = std::fs::read_to_string(dir.join("soul.md")).unwrap();
        assert!(soul_md.contains("## Forged Identity"));
        assert!(soul_md.contains("Testling"));
        assert!(soul_md.contains("The Balanced"));
        assert!(dir.join("ethics.md").exists());
        assert!(dir.join("instincts.md").exists());
        assert!(dir.join("forge_soul.json").exists());

        // Mentor edits ethics.md; a re-run must not clobber it.
        std::fs::write(dir.join("ethics.md"), "mentor-edited ethics").unwrap();
        write_soul_documents(&dir, "Testling", &soul).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("ethics.md")).unwrap(),
            "mentor-edited ethics"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
