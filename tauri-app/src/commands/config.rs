use crate::state::AppState;
use abigail_core::{validate_local_llm_url, TrinityConfig};
use abigail_router::IdEgoRouter;
use serde::{Deserialize, Serialize};
use tauri::State;

#[tauri::command]
pub fn set_api_key(state: State<AppState>, key: String) -> Result<(), String> {
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

    crate::rebuild_router_with_superego(&state)
}

#[tauri::command]
pub async fn set_local_llm_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    let (local_url, ego_provider, ego_api_key, mode, superego_config) = {
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
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        let (ep, ek) = crate::determine_ego_provider(&config, &vault);
        let se = crate::extract_superego_config(&config);
        (
            config.local_llm_base_url.clone(),
            ep,
            ek,
            config.routing_mode,
            se,
        )
    };

    let mut new_router =
        IdEgoRouter::new_auto_detect(local_url, ego_provider.as_deref(), ego_api_key, mode).await;

    if let Some((se_provider, se_key)) = superego_config {
        let superego = crate::build_superego_llm_provider(&se_provider, &se_key);
        new_router = new_router.with_superego(superego);
    }

    let mut router = state.router.write().map_err(|e| e.to_string())?;
    *router = new_router;
    Ok(())
}

#[tauri::command]
pub fn set_superego_provider(
    state: State<AppState>,
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

    crate::rebuild_router_with_superego(&state)?;
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
pub fn set_active_provider(state: State<'_, AppState>, provider: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.active_provider_preference = Some(provider);
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
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

#[tauri::command]
pub fn get_entity_theme(state: State<AppState>) -> Result<EntityTheme, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(EntityTheme {
        primary_color: config.primary_color.clone(),
        avatar_url: config.avatar_url.clone(),
    })
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
