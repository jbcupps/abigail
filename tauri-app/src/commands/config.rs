use crate::state::{AppState, ForceOverride};
use abigail_capabilities::cognitive::validation::validate_api_key;
use abigail_core::{validate_local_llm_url, TrinityConfig};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

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
pub fn get_force_override(state: State<AppState>) -> Result<ForceOverride, String> {
    let fo = state.force_override.read().map_err(|e| e.to_string())?;
    Ok(fo.clone())
}

#[tauri::command]
pub fn set_force_override(
    state: State<'_, AppState>,
    force_override: ForceOverride,
) -> Result<(), String> {
    let mut fo = state.force_override.write().map_err(|e| e.to_string())?;
    *fo = force_override;
    Ok(())
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityTheme {
    pub primary_color: Option<String>,
    pub avatar_url: Option<String>,
    pub theme_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mode: String,
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
        theme_id: config.theme_id.clone(),
    })
}

#[tauri::command]
pub fn get_entity_theme_id(state: State<AppState>) -> Result<Option<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.theme_id.clone())
}

#[tauri::command]
pub fn set_entity_theme_id(state: State<AppState>, theme_id: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.theme_id = Some(theme_id);
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_hive_theme(state: State<AppState>) -> Result<String, String> {
    let gc = state
        .identity_manager
        .global_config()
        .read()
        .map_err(|e| e.to_string())?;
    Ok(gc.default_theme.clone())
}

#[tauri::command]
pub fn set_hive_theme(state: State<AppState>, theme_id: String) -> Result<(), String> {
    let im = &state.identity_manager;
    let mut gc = im.global_config().write().map_err(|e| e.to_string())?;
    gc.default_theme = theme_id;
    gc.save(&im.data_root()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_available_themes() -> Vec<ThemeInfo> {
    vec![
        ThemeInfo {
            id: "modern".to_string(),
            name: "Modern Clean".to_string(),
            description: "Professional dark theme with indigo accent".to_string(),
            mode: "dark".to_string(),
        },
        ThemeInfo {
            id: "phosphor".to_string(),
            name: "Phosphor Terminal".to_string(),
            description: "Green-on-black CRT terminal aesthetic".to_string(),
            mode: "dark".to_string(),
        },
        ThemeInfo {
            id: "classic".to_string(),
            name: "Classic Desktop".to_string(),
            description: "Retro beveled surfaces with system gray".to_string(),
            mode: "light".to_string(),
        },
    ]
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

/// Known LLM provider names. Vault keys not in this list (e.g. "tavily") are
/// filtered out so the provider dropdown only shows chat-capable providers.
const LLM_PROVIDER_NAMES: &[&str] = &[
    "openai",
    "anthropic",
    "google",
    "xai",
    "openrouter",
    "deepseek",
    "mistral",
    "groq",
    "together",
    "fireworks",
    "cohere",
    "local_llm",
];

fn is_llm_provider(name: &str) -> bool {
    let lower = name.to_lowercase();
    LLM_PROVIDER_NAMES.contains(&lower.as_str()) || lower.ends_with("-cli")
}

#[tauri::command]
pub fn get_stored_providers(state: State<AppState>) -> Result<Vec<String>, String> {
    let secrets = state.secrets.lock().map_err(|e| e.to_string())?;
    let mut providers: Vec<String> = secrets
        .list_providers()
        .into_iter()
        .map(|s| s.to_string())
        .filter(|s| is_llm_provider(s))
        .collect();

    // Include the explicitly-activated provider (e.g. a CLI tool selected by the
    // user) so it appears in the stored-providers list even without a vault secret.
    let config = state.config.read().map_err(|e| e.to_string())?;
    if let Some(ref active) = config.active_provider_preference {
        if !providers.contains(active) && is_llm_provider(active) {
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
    app: tauri::AppHandle,
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

    // Cross-write: provider keys that also serve as skill secrets (tavily,
    // perplexity) must be mirrored into the skills_secrets vault so that
    // skills can find them at runtime without a separate store_secret call.
    const SKILL_SECRET_PROVIDERS: &[&str] = &["tavily", "perplexity"];
    if SKILL_SECRET_PROVIDERS.contains(&provider.as_str()) {
        if let Ok(mut skills_vault) = state.skills_secrets.lock() {
            skills_vault.set_secret(&provider, &key);
            if let Err(e) = skills_vault.save() {
                tracing::warn!(
                    "Provider key '{}' saved but failed to mirror to skills_secrets: {}",
                    provider,
                    e
                );
            } else {
                tracing::info!(
                    "Provider key '{}' mirrored to skills_secrets vault",
                    provider
                );
            }
        }
    }

    // Fix 3: Auto-activate this provider if no active preference is set,
    // so the router picks it up and the frontend selectors reflect it.
    if is_llm_provider(&provider) {
        let should_activate = {
            let config = state.config.read().map_err(|e| e.to_string())?;
            config.active_provider_preference.is_none()
        };
        if should_activate {
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            config.active_provider_preference = Some(provider.clone());
            let _ = config.save(&config.config_path());
        }
    }

    if let Err(e) = crate::rebuild_router(&state).await {
        // Still emit the event so the frontend can refresh its state
        let _ = app.emit("provider-config-changed", ());
        return Ok(StoreKeyResult {
            success: true, // Key saved, but router update failed
            provider,
            validated,
            error: Some(format!("Key saved, but failed to rebuild router: {}", e)),
        });
    }

    // Fix 2: Await model discovery for this provider so the model registry
    // is populated before notifying the frontend. The background discovery
    // in rebuild_router is fire-and-forget; this ensures the frontend gets
    // real data when it re-fetches.
    if is_llm_provider(&provider) {
        let mut reg = state.model_registry.lock().await;
        match reg.refresh_provider(&provider, &key).await {
            Ok(cache) => {
                tracing::info!(
                    "store_provider_key: discovered {} model(s) for {}",
                    cache.models.len(),
                    provider
                );
            }
            Err(e) => {
                tracing::warn!(
                    "store_provider_key: model discovery failed for {}: {}",
                    provider,
                    e
                );
            }
        }
        // Persist updated catalog to config
        let catalog = reg.to_catalog();
        drop(reg);
        if let Ok(mut config) = state.config.write() {
            config.provider_catalog = catalog;
            let _ = config.save(&config.config_path());
        }
    }

    // Fix 1: Notify frontend that provider config changed so it can
    // re-fetch stored providers and the model registry.
    let _ = app.emit("provider-config-changed", ());

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

// ---------------------------------------------------------------------------
// Runtime mode
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_runtime_mode(state: State<AppState>) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(
        serde_json::to_string(&config.runtime_mode)
            .unwrap_or_else(|_| "\"in_process\"".to_string()),
    )
}

#[tauri::command]
pub async fn set_runtime_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let parsed: abigail_core::RuntimeMode =
        serde_json::from_str(&format!("\"{}\"", mode)).map_err(|e| e.to_string())?;

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.runtime_mode = parsed;
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // When switching to Daemon mode, start the managed daemons
    if parsed == abigail_core::RuntimeMode::Daemon {
        let data_dir = {
            state
                .config
                .read()
                .map_err(|e| e.to_string())?
                .data_dir
                .clone()
        };
        let mut mgr = state.daemon_manager.lock().await;
        *mgr = crate::daemon_manager::DaemonManager::new(data_dir);
        let hive_url = mgr.start_hive().await.map_err(|e| e.to_string())?;
        {
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            config.hive_daemon_url = hive_url;
        }

        let entity_id = {
            state
                .active_agent_id
                .read()
                .map_err(|e| e.to_string())?
                .clone()
        };
        if let Some(eid) = entity_id {
            let entity_url = mgr.start_entity(&eid).await.map_err(|e| e.to_string())?;
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            config.entity_daemon_url = entity_url;
        }
    } else {
        // Switching back to InProcess: stop managed daemons
        let mut mgr = state.daemon_manager.lock().await;
        mgr.shutdown();
    }

    Ok(())
}
