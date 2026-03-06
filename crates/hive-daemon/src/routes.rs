//! Hive daemon HTTP route handlers.

use crate::state::HiveDaemonState;
use axum::{
    extract::{Path, State},
    Json,
};
use hive_core::{
    ApiEnvelope, CreateEntityRequest, CreateEntityResponse, EntityInfo, HiveStatus, ProviderConfig,
    ProviderModelInfo, ProviderModelsRequest, ProviderModelsResponse, SecretListResponse,
    SecretValueResponse, SignEntityRequest, StoreSecretRequest,
};

// ---------------------------------------------------------------------------
// GET /health
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}

// ---------------------------------------------------------------------------
// GET /v1/status
// ---------------------------------------------------------------------------

pub async fn get_status(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<HiveStatus>> {
    match state.identity_manager.list_agents() {
        Ok(agents) => {
            let entities: Vec<EntityInfo> = agents
                .into_iter()
                .map(|a| EntityInfo {
                    id: a.id,
                    name: a.name,
                    birth_complete: a.birth_complete,
                    birth_date: a.birth_date,
                })
                .collect();
            let status = HiveStatus {
                master_key_loaded: true,
                entity_count: entities.len(),
                entities,
            };
            Json(ApiEnvelope::success(status))
        }
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/entities
// ---------------------------------------------------------------------------

pub async fn list_entities(
    State(state): State<HiveDaemonState>,
) -> Json<ApiEnvelope<Vec<EntityInfo>>> {
    match state.identity_manager.list_agents() {
        Ok(agents) => {
            let entities: Vec<EntityInfo> = agents
                .into_iter()
                .map(|a| EntityInfo {
                    id: a.id,
                    name: a.name,
                    birth_complete: a.birth_complete,
                    birth_date: a.birth_date,
                })
                .collect();
            Json(ApiEnvelope::success(entities))
        }
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/entities
// ---------------------------------------------------------------------------

pub async fn create_entity(
    State(state): State<HiveDaemonState>,
    Json(body): Json<CreateEntityRequest>,
) -> Json<ApiEnvelope<CreateEntityResponse>> {
    match state.identity_manager.create_agent(&body.name) {
        Ok((id, dir)) => Json(ApiEnvelope::success(CreateEntityResponse {
            id,
            directory: dir.to_string_lossy().to_string(),
        })),
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/entities/:id
// ---------------------------------------------------------------------------

pub async fn get_entity(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<EntityInfo>> {
    match state.identity_manager.list_agents() {
        Ok(agents) => {
            if let Some(agent) = agents.into_iter().find(|a| a.id == entity_id) {
                Json(ApiEnvelope::success(EntityInfo {
                    id: agent.id,
                    name: agent.name,
                    birth_complete: agent.birth_complete,
                    birth_date: agent.birth_date,
                }))
            } else {
                Json(ApiEnvelope::error(format!(
                    "Entity {} not found",
                    entity_id
                )))
            }
        }
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/entities/:id/provider-config
// ---------------------------------------------------------------------------

/// The critical endpoint: resolves provider configuration for an entity.
/// Entity-daemon calls this on startup to get its LLM provider config.
pub async fn get_provider_config(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<ProviderConfig>> {
    // Load the agent's AppConfig
    let config = match state.identity_manager.load_agent(&entity_id) {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    // Resolve via Hive priority chain
    match state.hive.resolve_config(&config) {
        Ok(hive_config) => Json(ApiEnvelope::success(ProviderConfig {
            local_llm_base_url: hive_config.local_llm_base_url,
            ego_provider_name: hive_config
                .ego_provider
                .as_ref()
                .map(|selection| selection.provider.clone()),
            ego_api_key: hive_config
                .ego_provider
                .as_ref()
                .and_then(|selection| selection.api_key()),
            ego_model: hive_config.ego_model,
            routing_mode: format!("{:?}", hive_config.routing_mode),
            cli_permission_mode: serde_json::to_value(hive_config.cli_permission_mode)
                .ok()
                .and_then(|v| v.as_str().map(String::from)),
        })),
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/entities/:id/sign
// ---------------------------------------------------------------------------

pub async fn sign_entity(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(_body): Json<SignEntityRequest>,
) -> Json<ApiEnvelope<String>> {
    match state.identity_manager.sign_agent_after_birth(&entity_id) {
        Ok(()) => Json(ApiEnvelope::success("signed".to_string())),
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/secrets
// ---------------------------------------------------------------------------

pub async fn store_secret(
    State(state): State<HiveDaemonState>,
    Json(body): Json<StoreSecretRequest>,
) -> Json<ApiEnvelope<String>> {
    if let Err(e) = abigail_core::ops::validate_secret_basic(&body.key, &body.value) {
        return Json(ApiEnvelope::error(e.to_string()));
    }

    if !abigail_core::is_reserved_provider_key(&body.key) {
        let preloaded = abigail_skills::preloaded_secret_keys();
        if !preloaded.contains(&body.key) {
            tracing::info!(
                "Secret key '{}' is not a reserved provider or preloaded skill key — accepting for entity-level validation",
                body.key
            );
        }
    }

    match state.hive_secrets.lock() {
        Ok(mut vault) => {
            vault.set_secret(&body.key, &body.value);
            match vault.save() {
                Ok(()) => Json(ApiEnvelope::success(format!(
                    "Secret '{}' stored",
                    body.key
                ))),
                Err(e) => Json(ApiEnvelope::error(e.to_string())),
            }
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/secrets/list
// ---------------------------------------------------------------------------

pub async fn list_secrets(
    State(state): State<HiveDaemonState>,
) -> Json<ApiEnvelope<SecretListResponse>> {
    match state.hive_secrets.lock() {
        Ok(vault) => {
            let keys: Vec<String> = vault
                .list_providers()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            Json(ApiEnvelope::success(SecretListResponse { keys }))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/secrets/:key
// ---------------------------------------------------------------------------

/// Fetch a single secret value by key (localhost-only, for entity daemon startup sync).
pub async fn get_secret(
    State(state): State<HiveDaemonState>,
    Path(key): Path<String>,
) -> Json<ApiEnvelope<SecretValueResponse>> {
    match state.hive_secrets.lock() {
        Ok(vault) => match vault.get_secret(&key) {
            Some(value) => Json(ApiEnvelope::success(SecretValueResponse {
                key,
                value: value.to_string(),
            })),
            None => Json(ApiEnvelope::error(format!("Secret '{}' not found", key))),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/providers/models
// ---------------------------------------------------------------------------

/// Discover available models from a provider using its API key.
pub async fn discover_models(
    State(_state): State<HiveDaemonState>,
    Json(body): Json<ProviderModelsRequest>,
) -> Json<ApiEnvelope<ProviderModelsResponse>> {
    match abigail_capabilities::cognitive::validation::discover_models(
        &body.provider,
        &body.api_key,
    )
    .await
    {
        Ok(models) => {
            let model_infos: Vec<ProviderModelInfo> = models
                .into_iter()
                .map(|m| ProviderModelInfo {
                    model_id: m.id,
                    display_name: m.display_name,
                })
                .collect();
            Json(ApiEnvelope::success(ProviderModelsResponse {
                provider: body.provider,
                models: model_infos,
            }))
        }
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}
