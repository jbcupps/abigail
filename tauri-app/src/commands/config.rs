use crate::state::AppState;
use abigail_core::{validate_local_llm_url, TrinityConfig};
use abigail_capabilities::cognitive::validation::validate_api_key;
use serde::{Deserialize, Serialize};
use tauri::State;

#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("API key cannot be empty".to_string());
    }

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.openai_api_key = Some(key);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    crate::rebuild_router_with_superego(&state).await
}

#[tauri::command]
pub async fn set_local_llm_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.local_llm_base_url = if url.is_empty() {
            None
        } else {
            let normalized = validate_local_llm_url(&url).map_err(|e| e.to_string())?;
            Some(normalized)
        };
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    crate::rebuild_router_with_superego(&state).await
}

#[tauri::command]
pub async fn set_superego_provider(
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    let provider = provider.trim().to_lowercase();
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Superego API key cannot be empty".to_string());
    }
    if provider.is_empty() {
        return Err("Superego provider cannot be empty".to_string());
    }

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let trinity = config.trinity.get_or_insert_with(TrinityConfig::default);
        trinity.superego_provider = Some(provider.clone());
        trinity.superego_api_key = Some(key.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    crate::rebuild_router_with_superego(&state).await?;
    Ok(())
}

#[tauri::command]
pub async fn use_stored_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    let provider = provider.trim().to_lowercase();

    let key_str = {
        // Validate it's in the vault
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        let key = vault
            .get_secret(&provider)
            .ok_or_else(|| format!("Provider '{}' not found in vault", provider))?;
        key.to_string()
    };

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let trinity = config.trinity.get_or_insert_with(TrinityConfig::default);
        trinity.ego_provider = Some(provider);
        trinity.ego_api_key = Some(key_str);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    crate::rebuild_router_with_superego(&state).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStatus {
    pub id_provider: String,
    pub id_url: Option<String>,
    pub ego_configured: bool,
    pub ego_provider: Option<String>,
    pub superego_configured: bool,
    pub routing_mode: String,
    pub council_providers: usize,
}

#[tauri::command]
pub fn get_router_status(state: State<AppState>) -> Result<RouterStatus, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let router = state.router.read().map_err(|e| e.to_string())?;
    let status = router.status();

    Ok(RouterStatus {
        id_provider: if status.has_local_http {
            "local_http".to_string()
        } else {
            "candle_stub".to_string()
        },
        id_url: config.local_llm_base_url.clone(),
        ego_configured: status.has_ego,
        ego_provider: status.ego_provider,
        superego_configured: status.has_superego,
        routing_mode: serde_json::to_value(&config.routing_mode)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", config.routing_mode).to_lowercase()),
        council_providers: status.council_provider_count,
    })
}

#[tauri::command]
pub async fn set_active_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    let provider = provider.trim().to_lowercase();
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.active_provider_preference = Some(provider);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router_with_superego(&state).await
}

#[tauri::command]
pub fn get_active_provider(state: State<AppState>) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.active_provider_preference.clone())
}

#[tauri::command]
pub async fn set_ego_model(
    state: State<'_, AppState>,
    provider: String,
    model: String,
) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let tier_models = config
            .tier_models
            .get_or_insert_with(abigail_core::TierModels::defaults);
        tier_models.standard.insert(provider, model);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router_with_superego(&state).await?;
    Ok(())
}

#[tauri::command]
pub fn get_ego_model(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    if let Some(tm) = &config.tier_models {
        Ok(tm.standard.get(&provider).cloned())
    } else {
        Ok(abigail_core::TierModels::defaults()
            .standard
            .get(&provider)
            .cloned())
    }
}

#[tauri::command]
pub fn get_superego_l2_mode(state: State<'_, AppState>) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    serde_json::to_string(&config.superego_l2_mode).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTheme {
    pub primary_color: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentitySharingSettings {
    pub skills_sharing_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualAdaptationSettings {
    pub allow_minor_adjustments: bool,
    pub allow_avatar_swap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDisclosureSettings {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeUiSettings {
    pub advanced_mode: bool,
}

#[tauri::command]
pub async fn set_routing_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let parsed: abigail_core::RoutingMode =
        serde_json::from_str(&format!("\"{}\"", mode)).map_err(|e| e.to_string())?;
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.routing_mode = parsed;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router_with_superego(&state).await
}

#[tauri::command]
pub fn get_entity_theme(state: State<AppState>) -> Result<EntityTheme, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(EntityTheme {
        primary_color: config.primary_color.clone(),
        avatar_url: config.avatar_url.clone(),
    })
}

#[tauri::command]
pub fn get_identity_sharing_settings(
    state: State<AppState>,
) -> Result<IdentitySharingSettings, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(IdentitySharingSettings {
        skills_sharing_enabled: config.share_skills_across_identities,
    })
}

#[tauri::command]
pub fn set_identity_sharing_settings(
    state: State<AppState>,
    skills_sharing_enabled: bool,
) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.share_skills_across_identities = skills_sharing_enabled;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_visual_adaptation_settings(
    state: State<AppState>,
) -> Result<VisualAdaptationSettings, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(VisualAdaptationSettings {
        allow_minor_adjustments: config.allow_minor_visual_adaptation,
        allow_avatar_swap: config.allow_avatar_swap,
    })
}

#[tauri::command]
pub fn set_visual_adaptation_settings(
    state: State<AppState>,
    allow_minor_adjustments: bool,
    allow_avatar_swap: bool,
) -> Result<(), String> {
    if allow_avatar_swap {
        return Err("Avatar swaps are disabled by current identity policy.".to_string());
    }
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.allow_minor_visual_adaptation = allow_minor_adjustments;
    config.allow_avatar_swap = false;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_memory_disclosure_settings(
    state: State<AppState>,
) -> Result<MemoryDisclosureSettings, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(MemoryDisclosureSettings {
        enabled: config.memory_disclosure_enabled,
    })
}

#[tauri::command]
pub fn set_memory_disclosure_settings(state: State<AppState>, enabled: bool) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.memory_disclosure_enabled = enabled;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_forge_ui_settings(state: State<AppState>) -> Result<ForgeUiSettings, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(ForgeUiSettings {
        advanced_mode: config.forge_advanced_mode,
    })
}

#[tauri::command]
pub fn set_forge_advanced_mode(state: State<AppState>, advanced_mode: bool) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.forge_advanced_mode = advanced_mode;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stored_providers(state: State<AppState>) -> Result<Vec<String>, String> {
    let secrets = state.secrets.lock().map_err(|e| e.to_string())?;
    Ok(secrets
        .list_providers()
        .into_iter()
        .map(|s| s.to_string())
        .collect())
}

#[tauri::command]
pub fn set_superego_l2_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let parsed: abigail_core::SuperegoL2Mode =
        serde_json::from_str(&format!("\"{}\"", mode)).map_err(|e| e.to_string())?;
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.superego_l2_mode = parsed;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    let mut router = state.router.write().map_err(|e| e.to_string())?;
    router.set_superego_l2_mode(parsed);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreKeyResult {
    pub success: bool,
    pub provider: String,
    pub validated: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn store_provider_key(
    state: State<'_, AppState>,
    provider: String,
    key: String,
    validate: bool,
) -> Result<StoreKeyResult, String> {
    let provider = provider.trim().to_lowercase();
    let key = key.trim().to_string();

    if key.is_empty() {
        return Ok(StoreKeyResult {
            success: false,
            provider,
            validated: false,
            error: Some("Key cannot be empty".to_string()),
        });
    }

    // Optional: validate with provider before saving.
    let validated = if validate {
        match validate_api_key(&provider, &key).await {
            Ok(_) => true,
            Err(e) => {
                return Ok(StoreKeyResult {
                    success: false,
                    provider,
                    validated: false,
                    error: Some(e.to_string()),
                })
            }
        }
    } else {
        false
    };

    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault.set_secret(&provider, &key);

        // Auto-link shared keys
        match provider.as_str() {
            "openai" => {
                vault.set_secret("codex-cli", &key);
            }
            "anthropic" => {
                vault.set_secret("claude-cli", &key);
            }
            "google" => {
                vault.set_secret("gemini-cli", &key);
            }
            "xai" => {
                vault.set_secret("grok-cli", &key);
            }
            "codex-cli" => {
                vault.set_secret("openai", &key);
            }
            "claude-cli" => {
                vault.set_secret("anthropic", &key);
            }
            "gemini-cli" => {
                vault.set_secret("google", &key);
            }
            "grok-cli" => {
                vault.set_secret("xai", &key);
            }
            _ => {}
        }

        if let Err(e) = vault.save() {
            return Ok(StoreKeyResult {
                success: false,
                provider,
                validated: false,
                error: Some(format!("Failed to save secret: {}", e)),
            });
        }
    }

    if let Err(e) = crate::rebuild_router_with_superego(&state).await {
        return Ok(StoreKeyResult {
            success: true, // Key saved, but router update failed
            provider,
            validated,
            error: Some(format!("Key saved, but failed to rebuild router: {}", e)),
        });
    }

    Ok(StoreKeyResult {
        success: true,
        provider,
        validated,
        error: None,
    })
}
