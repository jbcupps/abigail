use crate::state::AppState;
use abigail_capabilities::cognitive::validation::validate_api_key;
use abigail_core::{validate_local_llm_url, TrinityConfig};
use serde::{Deserialize, Serialize};
use tauri::State;

// ---------------------------------------------------------------------------
// Model registry DTOs
// ---------------------------------------------------------------------------

/// Info about a single model from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistryEntry {
    pub provider: String,
    pub model_id: String,
    pub display_name: Option<String>,
}

/// Summary of the entire model registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistrySummary {
    pub providers: Vec<String>,
    pub total_models: usize,
    pub models: Vec<ModelRegistryEntry>,
}

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

    crate::rebuild_router(&state).await
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

    crate::rebuild_router(&state).await
}

#[tauri::command]
pub async fn use_stored_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    let provider = provider.trim().to_lowercase();

    let is_cli = matches!(
        provider.as_str(),
        "claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli"
    );

    let key_str = {
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        match vault.get_secret(&provider) {
            Some(key) => key.to_string(),
            None if is_cli => "system".to_string(),
            None => return Err(format!("Provider '{}' not found in vault", provider)),
        }
    };

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let trinity = config.trinity.get_or_insert_with(TrinityConfig::default);
        trinity.ego_provider = Some(provider.clone());
        trinity.ego_api_key = Some(key_str);
        config.active_provider_preference = Some(provider);
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    crate::rebuild_router(&state).await?;
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
        superego_configured: false,
        routing_mode: serde_json::to_value(&config.routing_mode)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", config.routing_mode).to_lowercase()),
        council_providers: status.council_provider_count,
    })
}

/// Read-only routing diagnosis: shows what the router would do for a given
/// message without calling any LLM. Useful for operator debugging.
#[tauri::command]
pub fn diagnose_routing(
    state: State<AppState>,
    message: String,
) -> Result<abigail_router::RoutingDiagnosis, String> {
    let router = state.router.read().map_err(|e| e.to_string())?;
    Ok(router.diagnose(&message))
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
    crate::rebuild_router(&state).await
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
    crate::rebuild_router(&state).await?;
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

// ---------------------------------------------------------------------------
// Tier model assignment commands (Fast / Standard / Pro grid)
// ---------------------------------------------------------------------------

/// DTO for tier model assignments across all providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierModelAssignments {
    pub fast: std::collections::HashMap<String, String>,
    pub standard: std::collections::HashMap<String, String>,
    pub pro: std::collections::HashMap<String, String>,
}

/// Get all tier model assignments, falling back to defaults.
#[tauri::command]
pub fn get_tier_models(state: State<AppState>) -> Result<TierModelAssignments, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let tm = config
        .tier_models
        .clone()
        .unwrap_or_else(abigail_core::TierModels::defaults);
    Ok(TierModelAssignments {
        fast: tm.fast,
        standard: tm.standard,
        pro: tm.pro,
    })
}

/// Set a specific tier model for a provider.
///
/// `tier` must be one of: "fast", "standard", "pro".
#[tauri::command]
pub async fn set_tier_model(
    state: State<'_, AppState>,
    provider: String,
    tier: String,
    model: String,
) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let tier_models = config
            .tier_models
            .get_or_insert_with(abigail_core::TierModels::defaults);
        match tier.as_str() {
            "fast" => {
                tier_models.fast.insert(provider, model);
            }
            "standard" => {
                tier_models.standard.insert(provider, model);
            }
            "pro" => {
                tier_models.pro.insert(provider, model);
            }
            _ => {
                return Err(format!(
                    "Invalid tier '{}'. Must be fast, standard, or pro.",
                    tier
                ))
            }
        }
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router(&state).await?;
    Ok(())
}

/// Reset tier models back to defaults.
#[tauri::command]
pub async fn reset_tier_models(state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.tier_models = Some(abigail_core::TierModels::defaults());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router(&state).await?;
    Ok(())
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
    crate::rebuild_router(&state).await
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
    let mut providers: Vec<String> = secrets
        .list_providers()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Include the explicitly-activated provider (e.g. a CLI tool selected by the
    // user) so it appears in the stored-providers list even without a vault secret.
    let config = state.config.read().map_err(|e| e.to_string())?;
    if let Some(ref active) = config.active_provider_preference {
        if !providers.contains(active) {
            providers.push(active.clone());
        }
    }

    Ok(providers)
}

/// Detect which CLI tools are installed and reachable on PATH.
#[tauri::command]
pub fn detect_cli_providers() -> Vec<String> {
    let cli_tools: &[(&str, &str)] = &[
        ("claude-cli", "claude"),
        ("gemini-cli", "gemini"),
        ("codex-cli", "codex"),
        ("grok-cli", "grok"),
    ];
    cli_tools
        .iter()
        .filter(|(_, binary)| abigail_hive::is_binary_on_path(binary))
        .map(|(provider, _)| provider.to_string())
        .collect()
}

/// Full CLI detection: checks PATH, verifies official binary, and checks auth status.
#[tauri::command]
pub fn detect_cli_providers_full() -> Vec<abigail_capabilities::cognitive::CliDetectionResult> {
    abigail_hive::detect_cli_providers_full()
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

    if let Err(e) = crate::rebuild_router(&state).await {
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

// ---------------------------------------------------------------------------
// Model registry Tauri commands
// ---------------------------------------------------------------------------

/// Get the current model registry contents (all cached providers + models).
#[tauri::command]
pub async fn get_model_registry(
    state: State<'_, AppState>,
) -> Result<ModelRegistrySummary, String> {
    let reg = state.model_registry.lock().await;
    let providers: Vec<String> = reg.providers().iter().map(|s| s.to_string()).collect();
    let total_models = reg.total_models();

    let mut models = Vec::new();
    for provider in &providers {
        if let Some(cache) = reg.get_cached(provider) {
            for m in &cache.models {
                models.push(ModelRegistryEntry {
                    provider: provider.clone(),
                    model_id: m.id.clone(),
                    display_name: m.display_name.clone(),
                });
            }
        }
    }

    Ok(ModelRegistrySummary {
        providers,
        total_models,
        models,
    })
}

/// Discover (or re-discover) models for a specific provider.
///
/// Fetches from the provider API, updates the in-memory cache, and persists
/// the catalog to config.json.
#[tauri::command]
pub async fn discover_provider_models(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<ModelRegistryEntry>, String> {
    let api_key = {
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        vault
            .get_secret(&provider)
            .map(|s| s.to_string())
            .unwrap_or_default()
    };

    if api_key.is_empty() {
        return Err(format!(
            "No API key found for provider '{}'. Store a key first.",
            provider
        ));
    }

    let mut reg = state.model_registry.lock().await;
    let cache = reg
        .refresh_provider(&provider, &api_key)
        .await
        .map_err(|e| format!("Discovery failed for {}: {}", provider, e))?;

    let entries: Vec<ModelRegistryEntry> = cache
        .models
        .iter()
        .map(|m| ModelRegistryEntry {
            provider: provider.clone(),
            model_id: m.id.clone(),
            display_name: m.display_name.clone(),
        })
        .collect();

    // Persist updated catalog to config
    let catalog = reg.to_catalog();
    drop(reg);
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.provider_catalog = catalog;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(entries)
}

/// Refresh the entire model registry — re-discover models for all providers
/// that have stored API keys.
#[tauri::command]
pub async fn refresh_model_registry(
    state: State<'_, AppState>,
) -> Result<ModelRegistrySummary, String> {
    // Collect all providers that have stored keys
    let providers_with_keys: Vec<(String, String)> = {
        let vault = state.secrets.lock().map_err(|e| e.to_string())?;
        let known = ["openai", "anthropic", "google", "xai", "perplexity"];
        known
            .iter()
            .filter_map(|p| {
                vault
                    .get_secret(p)
                    .map(|k| k.to_string())
                    .filter(|k| !k.is_empty())
                    .map(|k| (p.to_string(), k))
            })
            .collect()
    };

    let mut reg = state.model_registry.lock().await;
    for (provider, key) in &providers_with_keys {
        match reg.refresh_provider(provider, key).await {
            Ok(cache) => {
                tracing::info!(
                    "ModelRegistry refresh: {} → {} model(s)",
                    provider,
                    cache.models.len()
                );
            }
            Err(e) => {
                tracing::warn!("ModelRegistry refresh failed for {}: {}", provider, e);
            }
        }
    }

    // Build summary
    let providers: Vec<String> = reg.providers().iter().map(|s| s.to_string()).collect();
    let total_models = reg.total_models();
    let mut models = Vec::new();
    for provider in &providers {
        if let Some(cache) = reg.get_cached(provider) {
            for m in &cache.models {
                models.push(ModelRegistryEntry {
                    provider: provider.clone(),
                    model_id: m.id.clone(),
                    display_name: m.display_name.clone(),
                });
            }
        }
    }

    // Persist updated catalog to config
    let catalog = reg.to_catalog();
    drop(reg);
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.provider_catalog = catalog;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    Ok(ModelRegistrySummary {
        providers,
        total_models,
        models,
    })
}

/// Get/set force override settings.
#[tauri::command]
pub fn get_force_override(state: State<AppState>) -> Result<serde_json::Value, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    serde_json::to_value(&config.force_override).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_force_override(
    state: State<'_, AppState>,
    force_override: serde_json::Value,
) -> Result<(), String> {
    let parsed: abigail_core::ForceOverride =
        serde_json::from_value(force_override).map_err(|e| e.to_string())?;
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.force_override = parsed;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router(&state).await
}

/// Get/set tier thresholds.
#[tauri::command]
pub fn get_tier_thresholds(state: State<AppState>) -> Result<serde_json::Value, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    serde_json::to_value(&config.tier_thresholds).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_tier_thresholds(
    state: State<'_, AppState>,
    tier_thresholds: serde_json::Value,
) -> Result<(), String> {
    let parsed: abigail_core::TierThresholds =
        serde_json::from_value(tier_thresholds).map_err(|e| e.to_string())?;
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.tier_thresholds = parsed;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    crate::rebuild_router(&state).await
}
