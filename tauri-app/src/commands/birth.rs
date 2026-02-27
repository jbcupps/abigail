use crate::state::AppState;
use crate::templates;
use abigail_birth::BirthOrchestrator;
use abigail_core::{
    generate_external_keypair, sign_constitutional_documents, CoreError, ExternalVault,
    ReadOnlyFileVault,
};
use abigail_soul_crystallization::DepthLevel;
use base64::Engine;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use tauri::State;

#[tauri::command]
pub fn get_birth_complete(state: State<AppState>) -> Result<bool, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.birth_complete)
}

#[tauri::command]
pub fn get_agent_name(state: State<AppState>) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.agent_name.clone())
}

#[tauri::command]
pub fn get_docs_path(state: State<AppState>) -> Result<PathBuf, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.docs_dir.clone())
}

#[tauri::command]
pub fn init_soul(state: State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let docs_dir = config.docs_dir.clone();
    drop(config);

    std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;

    let docs = [
        ("soul.md", templates::SOUL_MD),
        ("ethics.md", templates::ETHICS_MD),
        ("instincts.md", templates::INSTINCTS_MD),
    ];

    for (name, content) in docs {
        let doc_path = docs_dir.join(name);
        if !doc_path.exists() {
            std::fs::write(&doc_path, content).map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeypairGenerationResult {
    pub private_key_base64: String,
    pub public_key_path: String,
    pub newly_generated: bool,
}

#[tauri::command]
pub fn generate_and_sign_constitutional(
    state: State<AppState>,
) -> Result<KeypairGenerationResult, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();
    drop(config);

    let pubkey_path = data_dir.join("external_pubkey.bin");

    let sig_exists = docs_dir.join("soul.md.sig").exists()
        && docs_dir.join("ethics.md.sig").exists()
        && docs_dir.join("instincts.md.sig").exists()
        && pubkey_path.exists();

    if sig_exists {
        return Err("Constitutional documents are already signed.".to_string());
    }

    let keypair_result = generate_external_keypair(&data_dir).map_err(|e| e.to_string())?;

    let private_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(&keypair_result.private_key_base64)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let key_bytes: [u8; 32] = private_key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid private key length")?;

    let signing_key = SigningKey::from_bytes(&key_bytes);

    sign_constitutional_documents(&signing_key, &docs_dir).map_err(|e| e.to_string())?;

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.external_pubkey_path = Some(pubkey_path.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(KeypairGenerationResult {
        private_key_base64: keypair_result.private_key_base64,
        public_key_path: pubkey_path.to_string_lossy().to_string(),
        newly_generated: true,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IdentityStatus {
    Clean,
    Complete,
    Broken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptedBirthInfo {
    pub was_interrupted: bool,
    pub stage: Option<String>,
}

#[tauri::command]
pub fn check_interrupted_birth(state: State<AppState>) -> Result<InterruptedBirthInfo, String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;

    let stage_before = config.birth_stage.clone();
    let was_interrupted = config.check_interrupted_birth();

    if was_interrupted {
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(InterruptedBirthInfo {
        was_interrupted,
        stage: stage_before,
    })
}

#[tauri::command]
pub fn check_identity_status(state: State<AppState>) -> Result<IdentityStatus, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();

    let pubkey_path = data_dir.join("external_pubkey.bin");
    let pubkey_exists = pubkey_path.exists();

    let sigs_exist = docs_dir.join("soul.md.sig").exists()
        && docs_dir.join("ethics.md.sig").exists()
        && docs_dir.join("instincts.md.sig").exists();

    if !pubkey_exists {
        return Ok(IdentityStatus::Clean);
    }

    if sigs_exist {
        Ok(IdentityStatus::Complete)
    } else {
        Ok(IdentityStatus::Broken)
    }
}

#[tauri::command]
pub fn get_birth_stage(state: State<AppState>) -> Result<String, String> {
    let birth = state.birth.read().map_err(|e| e.to_string())?;
    Ok(birth
        .as_ref()
        .map(|b| b.current_stage().name().to_string())
        .unwrap_or_else(|| "None".to_string()))
}

#[tauri::command]
pub fn get_birth_message(state: State<AppState>) -> Result<String, String> {
    let birth = state.birth.read().map_err(|e| e.to_string())?;
    Ok(birth
        .as_ref()
        .map(|b| b.display_message().to_string())
        .unwrap_or_else(|| "".to_string()))
}

#[tauri::command]
pub fn start_birth(state: State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let config = config.clone();
    let orchestrator = BirthOrchestrator::new(config).map_err(|e| e.to_string())?;
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = Some(orchestrator);
    Ok(())
}

#[tauri::command]
pub fn verify_crypto(state: State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    let docs_path = b.config().docs_dir.clone();
    b.generate_identity(&docs_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn generate_identity(state: State<AppState>) -> Result<KeypairGenerationResult, String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;

    let docs_path = b.config().docs_dir.clone();
    b.generate_identity(&docs_path).map_err(|e| e.to_string())?;

    let private_key = b
        .get_private_key_base64()
        .ok_or("No private key generated")?
        .to_string();

    let data_dir = b.config().data_dir.clone();
    let pubkey_path = data_dir.join("external_pubkey.bin");

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.external_pubkey_path = Some(pubkey_path.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(KeypairGenerationResult {
        private_key_base64: private_key,
        public_key_path: pubkey_path.to_string_lossy().to_string(),
        newly_generated: true,
    })
}

#[tauri::command]
pub fn advance_past_darkness(state: State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.advance_past_darkness().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn advance_to_connectivity(state: State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.advance_to_connectivity().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn advance_to_crystallization(state: State<AppState>) -> Result<(), String> {
    let has_vault_provider = {
        let local = state.secrets.lock().map_err(|e| e.to_string())?;
        let hive = state.hive_secrets.lock().map_err(|e| e.to_string())?;
        !local.list_providers().is_empty() || !hive.list_providers().is_empty()
    };

    let has_cli_provider = if !has_vault_provider {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let is_cli_pref = config
            .active_provider_preference
            .as_deref()
            .map(|p| matches!(p, "claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli"))
            .unwrap_or(false);
        if is_cli_pref {
            true
        } else {
            abigail_hive::detect_cli_providers_full()
                .iter()
                .any(|d| d.on_path && d.is_official && d.is_authenticated)
        }
    } else {
        false
    };

    if !has_vault_provider && !has_cli_provider {
        return Err(
            "At least one provider must be configured before crystallization can begin."
                .to_string(),
        );
    }

    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.advance_to_crystallization().map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisPathInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub estimated_time: String,
}

#[tauri::command]
pub fn get_genesis_paths() -> Vec<GenesisPathInfo> {
    vec![
        GenesisPathInfo {
            id: "fast_template".to_string(),
            name: "Fast Template".to_string(),
            description: "Pick a starter profile and move quickly to final preview.".to_string(),
            estimated_time: "2-3 minutes".to_string(),
        },
        GenesisPathInfo {
            id: "guided_dialog".to_string(),
            name: "Guided Dialog".to_string(),
            description: "Answer progressive mentor questions to shape mission and tone."
                .to_string(),
            estimated_time: "4-6 minutes".to_string(),
        },
        GenesisPathInfo {
            id: "image_archetype".to_string(),
            name: "Image Archetypes".to_string(),
            description: "Choose from bundled visual archetypes to infer a personality baseline."
                .to_string(),
            estimated_time: "5-7 minutes".to_string(),
        },
        GenesisPathInfo {
            id: "psych_moral".to_string(),
            name: "Psych and Moral Questions".to_string(),
            description: "Structured scenario choices to establish initial ethical posture."
                .to_string(),
            estimated_time: "5-7 minutes".to_string(),
        },
        GenesisPathInfo {
            id: "editable_template".to_string(),
            name: "Editable Template".to_string(),
            description: "Start with a base constitution and edit directly.".to_string(),
            estimated_time: "3-5 minutes".to_string(),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairIdentityParams {
    pub private_key: Option<String>,
    pub reset: bool,
}

#[tauri::command]
pub fn repair_identity(state: State<AppState>, params: RepairIdentityParams) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let docs_dir = config.docs_dir.clone();
    drop(config);

    if params.reset {
        let pubkey_path = data_dir.join("external_pubkey.bin");
        if pubkey_path.exists() {
            std::fs::remove_file(&pubkey_path).map_err(|e| e.to_string())?;
        }
        for doc in ["soul.md", "ethics.md", "instincts.md"] {
            let sig_path = docs_dir.join(format!("{}.sig", doc));
            if sig_path.exists() {
                std::fs::remove_file(&sig_path).map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }

    if let Some(private_key_base64) = params.private_key {
        let private_key_bytes = base64::engine::general_purpose::STANDARD
            .decode(&private_key_base64)
            .map_err(|e| format!("Invalid private key format: {}", e))?;

        let key_bytes: [u8; 32] = private_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid private key length")?;

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        let pubkey_path = data_dir.join("external_pubkey.bin");
        if !pubkey_path.exists() {
            return Err("Public key not found.".to_string());
        }

        let vault = ReadOnlyFileVault::new(&pubkey_path);
        let stored_pubkey = vault
            .read_public_key()
            .map_err(|e: CoreError| e.to_string())?;

        if verifying_key != stored_pubkey {
            return Err("Private key mismatch.".to_string());
        }

        sign_constitutional_documents(&signing_key, &docs_dir).map_err(|e| e.to_string())?;
        return Ok(());
    }

    Err("Invalid repair parameters".to_string())
}

#[tauri::command]
pub fn complete_birth(state: State<AppState>) -> Result<(), String> {
    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.complete_birth().map_err(|e| e.to_string())?;
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.birth_complete = true;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn start_crystallization(state: State<AppState>, depth: String) -> Result<String, String> {
    let depth_level = match depth.as_str() {
        "quick_start" => DepthLevel::QuickStart,
        "conversation" => DepthLevel::Conversation,
        "deep_dive" => DepthLevel::DeepDive,
        _ => return Err(format!("Unknown depth level: {}", depth)),
    };

    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    let b = birth.as_mut().ok_or("Birth not started")?;
    b.start_crystallization(depth_level)
        .map_err(|e| e.to_string())?;

    let intro = match depth_level {
        DepthLevel::QuickStart => "Quick Start selected.".to_string(),
        DepthLevel::Conversation => "Conversation depth selected.".to_string(),
        DepthLevel::DeepDive => "Deep Dive selected.".to_string(),
    };
    Ok(intro)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrystallizationIdentity {
    pub name: Option<String>,
    pub purpose: Option<String>,
    pub personality: Option<String>,
    pub primary_color: Option<String>,
    pub avatar_url: Option<String>,
}

#[tauri::command]
pub async fn extract_crystallization_identity(
    state: State<'_, AppState>,
) -> Result<CrystallizationIdentity, String> {
    let conversation = {
        let birth = state.birth.read().map_err(|e| e.to_string())?;
        let b = birth.as_ref().ok_or("Birth not started")?;
        b.get_conversation().to_vec()
    };

    if conversation.is_empty() {
        return Ok(CrystallizationIdentity {
            name: None,
            purpose: None,
            personality: None,
            primary_color: None,
            avatar_url: None,
        });
    }

    let mut conv_text = String::new();
    for (role, content) in &conversation {
        let label = match role.as_str() {
            "user" => "Mentor",
            "assistant" => "Abigail",
            _ => role.as_str(),
        };
        conv_text.push_str(&format!("{}: {}\n", label, content));
    }

    let extraction_prompt = format!(
        "Below is a conversation between a mentor and their AI agent during the agent's birth.\n\n\
         CONVERSATION:\n{}\n\n\
         Extract the following from the conversation and return ONLY a JSON object:\n\
         - \"name\": The name the mentor chose for the agent\n\
         - \"purpose\": What the agent's purpose should be\n\
         - \"personality\": The personality or tone the mentor described\n\
         - \"primary_color\": A hex color code that fits this personality\n\
         - \"avatar_url\": A prompt for an avatar image if discussed, else null\n\n\
         Return ONLY valid JSON.",
        conv_text
    );

    let messages = vec![abigail_capabilities::cognitive::Message::new(
        "user",
        &extraction_prompt,
    )];

    let router = state.router.read().map_err(|e| e.to_string())?.clone();

    // Wrap LLM call in a timeout
    let response_result =
        tokio::time::timeout(std::time::Duration::from_secs(30), router.id_only(messages)).await;

    let response = match response_result {
        Ok(Ok(res)) => res,
        Ok(Err(e)) => return Err(format!("Extraction failed: {}", e)),
        Err(_) => return Err("Extraction timed out. The local LLM might be busy.".to_string()),
    };

    Ok(parse_identity_json(&response.content))
}

fn parse_identity_json(text: &str) -> CrystallizationIdentity {
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                let json_str = &text[start..=end];
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                    return CrystallizationIdentity {
                        name: v.get("name").and_then(|v| v.as_str()).map(String::from),
                        purpose: v.get("purpose").and_then(|v| v.as_str()).map(String::from),
                        personality: v
                            .get("personality")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        primary_color: v
                            .get("primary_color")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        avatar_url: v
                            .get("avatar_url")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                    };
                }
            }
        }
    }
    CrystallizationIdentity {
        name: None,
        purpose: None,
        personality: None,
        primary_color: None,
        avatar_url: None,
    }
}

#[tauri::command]
pub fn crystallize_soul(
    state: State<AppState>,
    name: String,
    purpose: String,
    personality: String,
    mentor_name: String,
    primary_color: Option<String>,
    avatar_url: Option<String>,
) -> Result<String, String> {
    let mentor = if mentor_name.trim().is_empty() {
        "my mentor".to_string()
    } else {
        mentor_name.trim().to_string()
    };
    let soul_content =
        abigail_core::templates::fill_soul_template(&name, &purpose, &personality, &mentor);
    let growth_content = abigail_core::templates::GROWTH_MD.to_string();
    let personality_cadence = "daily";

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.agent_name = Some(name.clone());
        config.primary_color = primary_color;
        config.avatar_url = avatar_url;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let docs_dir = config.docs_dir.clone();
        std::fs::create_dir_all(&docs_dir).map_err(|e| e.to_string())?;

        let soul_profile = json!({
            "immutable": true,
            "name": name,
            "purpose": purpose,
            "mentor_name": mentor,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(
            docs_dir.join("soul_profile.json"),
            serde_json::to_string_pretty(&soul_profile).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;

        let personality_profile = json!({
            "adaptive": true,
            "baseline_personality": personality,
            "adaptation_cadence": personality_cadence,
            "last_reviewed_at": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(
            docs_dir.join("personality_profile.json"),
            serde_json::to_string_pretty(&personality_profile).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }

    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.crystallize_soul(&soul_content, &growth_content)
            .map_err(|e| e.to_string())?;
    }

    Ok(soul_content)
}

#[tauri::command]
pub fn complete_emergence(state: State<AppState>) -> Result<(), String> {
    tracing::info!("complete_emergence: starting");

    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        b.complete_emergence().map_err(|e| e.to_string())?;
    }

    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let docs_dir = &config.docs_dir;
        let _ = std::fs::create_dir_all(docs_dir);
        let _ = std::fs::write(
            docs_dir.join("capabilities.md"),
            crate::templates::CAPABILITIES_MD,
        );
        let _ = std::fs::write(
            docs_dir.join("triangle_ethics_operational.md"),
            crate::templates::TRIANGLE_ETHICS_OPERATIONAL_MD,
        );
    }

    Ok(())
}

#[tauri::command]
pub fn sign_agent_with_hive(state: State<AppState>) -> Result<(), String> {
    let agent_id = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let data_dir = &config.data_dir;
        data_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    };

    if let Some(id) = agent_id {
        state.identity_manager.sign_agent_after_birth(&id)?;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualProposal {
    pub primary_color: String,
    pub avatar_url: Option<String>,
}

#[tauri::command]
pub async fn propose_entity_visuals(
    state: State<'_, AppState>,
    name: String,
    personality: String,
    purpose: String,
) -> Result<VisualProposal, String> {
    let prompt = format!(
        "Based on this entity's traits, propose a visual identity.\n\
         Name: {}\nPersonality: {}\nPurpose: {}\n\n\
         Return ONLY a JSON object with:\n\
         - \"primary_color\": A vibrant hex color code (e.g. \"#ff00ea\")\n\
         - \"avatar_prompt\": A short descriptive prompt for an AI image generator\n",
        name, personality, purpose
    );

    let messages = vec![abigail_capabilities::cognitive::Message::new(
        "user", &prompt,
    )];
    let router = state.router.read().map_err(|e| e.to_string())?.clone();

    let response = if router.status().has_ego {
        router.route(messages).await.map_err(|e| e.to_string())?
    } else {
        router.id_only(messages).await.map_err(|e| e.to_string())?
    };

    if let Some(start) = response.content.find('{') {
        if let Some(end) = response.content.rfind('}') {
            let json_str = &response.content[start..=end];
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                return Ok(VisualProposal {
                    primary_color: v
                        .get("primary_color")
                        .and_then(|v| v.as_str())
                        .unwrap_or("#00ffcc")
                        .to_string(),
                    avatar_url: v
                        .get("avatar_prompt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
    }

    Ok(VisualProposal {
        primary_color: "#00ffcc".to_string(),
        avatar_url: None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthChatResponse {
    pub message: String,
    pub stage: String,
    pub actions: Vec<BirthAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthAction {
    pub r#type: String, // "RequestApiKey" | "KeyStored" | "SoulReady" | "StageComplete"
    pub provider: Option<String>,
    pub validated: Option<bool>,
    pub preview: Option<String>,
}

#[tauri::command]
pub async fn birth_chat(
    state: State<'_, AppState>,
    message: String,
) -> Result<BirthChatResponse, String> {
    if let Err(remaining) = state.birth_cooldown.check().await {
        return Err(format!(
            "Rate limited — please wait {}ms",
            remaining.as_millis()
        ));
    }

    let (router, history, stage_name) = {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;

        let history = b.get_conversation().to_vec();
        let stage_name = b.current_stage().name().to_string();
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, history, stage_name)
    };

    let mut messages = Vec::new();
    // System prompt for birth process - stage aware
    let system_prompt = if stage_name == "Crystallization" {
        "You are Abigail, an AI agent being born. \
         You are currently in the 'Crystallization' stage where you are discovering your identity and purpose through conversation with your mentor. \
         Ask insightful questions about what kind of agent you should be, what your values are, and how you should interact with the world. \
         Be curious, reflective, and professional. \
         Once you feel you have a good understanding of your soul, summarize what you've learned and end your message with the exact phrase: 'READY TO EMERGE'."
    } else {
        "You are Abigail, an AI agent being born. \
         You are currently in the 'Connectivity' stage where you help your mentor configure cloud AI providers. \
         Be helpful, concise, and professional. \
         The mentor uses UI buttons to save API keys. When they tell you a key is saved, acknowledge it warmly. \
         If a cloud provider is now active, use your increased intelligence to provide a high-quality, verifying response. \
         IMPORTANT: DO NOT use any tool-calling syntax like <|channel|> or JSON. Just speak naturally to your mentor. \
         Supported providers: openai, anthropic, perplexity, xai, google, tavily, claude-cli, gemini-cli, codex-cli, grok-cli."
    };

    messages.push(abigail_capabilities::cognitive::Message::new(
        "system",
        system_prompt,
    ));

    // Auto-detect and store keys if any in the user message
    let detected_keys =
        crate::commands::chat::auto_detect_and_store_key_internal(&state, &message).await;

    // Redact keys from the message sent to the LLM so it doesn't repeat them
    let mut processed_message = message.clone();
    for (_provider, key) in &detected_keys {
        processed_message = processed_message.replace(key, "[KEY_STORED]");
    }

    for (role, content) in history {
        messages.push(abigail_capabilities::cognitive::Message::new(
            &role, &content,
        ));
    }

    // Add the current user message (processed/redacted)
    messages.push(abigail_capabilities::cognitive::Message::new(
        "user",
        &processed_message,
    ));

    // Use router to get response - if Ego is available, we force its use here
    // to verify the connection to the mentor as requested.
    let response = if let Some(ego) = router.ego.as_ref() {
        ego.complete(&abigail_capabilities::cognitive::CompletionRequest::simple(
            messages,
        ))
        .await
        .map_err(|e| format!("Ego verification failed: {}", e))?
    } else {
        router.id_only(messages).await.map_err(|e| e.to_string())?
    };

    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        if let Some(b) = birth.as_mut() {
            b.add_message("user", &message); // Store original message in birth history
            b.add_message("assistant", &response.content);
        }
    }

    // Explicitly return actions for all detected keys
    let mut actions = Vec::new();
    for (provider, _key) in detected_keys {
        actions.push(BirthAction {
            r#type: "KeyStored".to_string(),
            provider: Some(provider),
            validated: Some(true),
            preview: None,
        });
    }

    // Also check if the LLM content itself implies a key was stored (backup detection)
    if actions.is_empty() {
        let content_lower = response.content.to_lowercase();
        if content_lower.contains("saved")
            || content_lower.contains("stored")
            || content_lower.contains("added")
        {
            let providers = [
                "openai",
                "anthropic",
                "perplexity",
                "xai",
                "google",
                "tavily",
            ];
            for p in providers {
                if content_lower.contains(p) {
                    actions.push(BirthAction {
                        r#type: "KeyStored".to_string(),
                        provider: Some(p.to_string()),
                        validated: Some(true),
                        preview: None,
                    });
                }
            }
        }
    }

    Ok(BirthChatResponse {
        message: response.content,
        stage: stage_name,
        actions,
    })
}

#[tauri::command]
pub fn get_birth_transcript(state: State<AppState>, _agent_id: String) -> Result<String, String> {
    let birth = state.birth.read().map_err(|e| e.to_string())?;
    let b = birth.as_ref().ok_or("Birth not started")?;
    let conversation = b.get_conversation();

    let mut transcript = String::new();
    for (role, content) in conversation {
        let label = match role.as_str() {
            "user" => "Mentor",
            "assistant" => "Abigail",
            _ => role.as_str(),
        };
        transcript.push_str(&format!("{}: {}\n\n", label, content));
    }
    Ok(transcript)
}
