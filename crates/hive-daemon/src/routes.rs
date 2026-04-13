//! Hive daemon HTTP route handlers.

use crate::state::HiveDaemonState;
use axum::{
    extract::{Path, State},
    Json,
};
use hive_core::{
    ApiEnvelope, CreateEntityRequest, CreateEntityResponse, CreateForgeApprovalJobRequest,
    EntityInfo, ForgeApprovalJobsResponse, HiveStatus, OutboxSyncRequest, OutboxSyncResponse,
    ProviderConfig, ProviderModelInfo, ProviderModelsRequest, ProviderModelsResponse,
    RuntimeHeartbeatRequest, RuntimeHeartbeatResponse, RuntimeRegistrationRequest,
    RuntimeSessionLease, RuntimeSessionRequest, RuntimeSessionStatus, SecretListResponse,
    SecretValueResponse, SetSkillAssignmentsRequest, SignEntityRequest, SkillAssignmentsResponse,
    StoreSecretRequest, UpdateEntityConfigRequest, UpdateEntityConfigResponse,
};

fn provider_config_from_hive_config(hive_config: &abigail_hive::HiveConfig) -> ProviderConfig {
    ProviderConfig {
        local_llm_base_url: hive_config.local_llm_base_url.clone(),
        ego_provider_name: hive_config
            .ego_provider
            .as_ref()
            .map(|selection| selection.provider.clone()),
        ego_api_key: hive_config
            .ego_provider
            .as_ref()
            .and_then(|selection| selection.api_key()),
        ego_model: hive_config.ego_model.clone(),
        routing_mode: format!("{:?}", hive_config.routing_mode),
        cli_permission_mode: serde_json::to_value(hive_config.cli_permission_mode)
            .ok()
            .and_then(|v| v.as_str().map(String::from)),
    }
}

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
                    is_hive: a.is_hive,
                    immortal: a.immortal,
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
                    is_hive: a.is_hive,
                    immortal: a.immortal,
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
                    is_hive: agent.is_hive,
                    immortal: agent.immortal,
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
        Ok(hive_config) => Json(ApiEnvelope::success(provider_config_from_hive_config(
            &hive_config,
        ))),
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// PATCH /v1/entities/:id/config
// ---------------------------------------------------------------------------

pub async fn update_entity_config(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(body): Json<UpdateEntityConfigRequest>,
) -> Json<ApiEnvelope<UpdateEntityConfigResponse>> {
    let mut config = match state.identity_manager.load_agent(&entity_id) {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    if let Some(provider) = body.active_provider_preference {
        let provider = provider.trim().to_lowercase();
        if !provider.is_empty() {
            config.active_provider_preference = Some(provider);
        }
    }
    if let Some(url) = body.local_llm_base_url {
        let trimmed = url.trim().to_string();
        config.local_llm_base_url = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(mode) = body.routing_mode {
        let parsed: abigail_core::RoutingMode = match serde_json::from_str(&format!("\"{}\"", mode))
        {
            Ok(value) => value,
            Err(e) => return Json(ApiEnvelope::error(format!("Invalid routing_mode: {}", e))),
        };
        config.routing_mode = parsed;
    }
    if let Some(cli_mode) = body.cli_permission_mode {
        let parsed: abigail_core::CliPermissionMode =
            match serde_json::from_str(&format!("\"{}\"", cli_mode)) {
                Ok(value) => value,
                Err(e) => {
                    return Json(ApiEnvelope::error(format!(
                        "Invalid cli_permission_mode: {}",
                        e
                    )))
                }
            };
        config.cli_permission_mode = parsed;
    }

    if body.ego_model.is_some() {
        tracing::warn!(
            "update_entity_config received ego_model, but AppConfig has no persisted ego_model field yet"
        );
    }

    let config_path = config.config_path();
    if let Err(e) = config.save(&config_path) {
        return Json(ApiEnvelope::error(e.to_string()));
    }

    match state.hive.resolve_config(&config) {
        Ok(hive_config) => Json(ApiEnvelope::success(UpdateEntityConfigResponse {
            entity_id,
            provider_config: provider_config_from_hive_config(&hive_config),
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
    if let Err(e) = abigail_runtime::validate_hive_secret_key(&body.key) {
        return Json(ApiEnvelope::error(e));
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

// ---------------------------------------------------------------------------
// POST /v1/runtime/sessions
// ---------------------------------------------------------------------------

pub async fn issue_runtime_session(
    State(state): State<HiveDaemonState>,
    Json(body): Json<RuntimeSessionRequest>,
) -> Json<ApiEnvelope<RuntimeSessionLease>> {
    match state.identity_manager.list_agents() {
        Ok(agents) => {
            let Some(agent) = agents.into_iter().find(|agent| agent.id == body.entity_id) else {
                return Json(ApiEnvelope::error(format!(
                    "Entity {} not found",
                    body.entity_id
                )));
            };

            match state.runtime_control.lock() {
                Ok(mut control) => Json(ApiEnvelope::success(control.issue_session(
                    body,
                    Some(agent.name),
                    Some(state.hive_url.clone()),
                ))),
                Err(e) => Json(ApiEnvelope::error(e.to_string())),
            }
        }
        Err(e) => Json(ApiEnvelope::error(e)),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/runtime/register
// ---------------------------------------------------------------------------

pub async fn register_runtime(
    State(state): State<HiveDaemonState>,
    Json(body): Json<RuntimeRegistrationRequest>,
) -> Json<ApiEnvelope<RuntimeSessionStatus>> {
    match state.runtime_control.lock() {
        Ok(mut control) => match control.register_runtime(body) {
            Ok(status) => Json(ApiEnvelope::success(status)),
            Err(e) => Json(ApiEnvelope::error(e)),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/runtime/heartbeat
// ---------------------------------------------------------------------------

pub async fn record_runtime_heartbeat(
    State(state): State<HiveDaemonState>,
    Json(body): Json<RuntimeHeartbeatRequest>,
) -> Json<ApiEnvelope<RuntimeHeartbeatResponse>> {
    match state.runtime_control.lock() {
        Ok(mut control) => match control.record_heartbeat(body) {
            Ok(response) => Json(ApiEnvelope::success(response)),
            Err(e) => Json(ApiEnvelope::error(e)),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/runtime/sessions/:lease_id
// ---------------------------------------------------------------------------

pub async fn get_runtime_session(
    State(state): State<HiveDaemonState>,
    Path(lease_id): Path<String>,
) -> Json<ApiEnvelope<RuntimeSessionStatus>> {
    match state.runtime_control.lock() {
        Ok(control) => match control.session_status(&lease_id) {
            Some(status) => Json(ApiEnvelope::success(status)),
            None => Json(ApiEnvelope::error(format!(
                "Runtime lease {} not found",
                lease_id
            ))),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET/POST /v1/entities/:id/assignments
// ---------------------------------------------------------------------------

pub async fn get_skill_assignments(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<SkillAssignmentsResponse>> {
    match state.runtime_control.lock() {
        Ok(control) => Json(ApiEnvelope::success(control.assignments(&entity_id))),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

pub async fn set_skill_assignments(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(body): Json<SetSkillAssignmentsRequest>,
) -> Json<ApiEnvelope<SkillAssignmentsResponse>> {
    match state.runtime_control.lock() {
        Ok(mut control) => Json(ApiEnvelope::success(
            control.set_assignments(&entity_id, body),
        )),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET/POST /v1/entities/:id/forge-approvals
// ---------------------------------------------------------------------------

pub async fn get_forge_approval_jobs(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<ForgeApprovalJobsResponse>> {
    match state.runtime_control.lock() {
        Ok(control) => Json(ApiEnvelope::success(control.forge_jobs(&entity_id))),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

pub async fn create_forge_approval_job(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(body): Json<CreateForgeApprovalJobRequest>,
) -> Json<ApiEnvelope<hive_core::ForgeApprovalJob>> {
    match state.runtime_control.lock() {
        Ok(mut control) => Json(ApiEnvelope::success(
            control.create_forge_job(&entity_id, body),
        )),
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/runtime/outbox/sync
// ---------------------------------------------------------------------------

pub async fn sync_runtime_outbox(
    State(state): State<HiveDaemonState>,
    Json(body): Json<OutboxSyncRequest>,
) -> Json<ApiEnvelope<OutboxSyncResponse>> {
    match state.runtime_control.lock() {
        Ok(mut control) => match control.sync_outbox(body) {
            Ok(response) => Json(ApiEnvelope::success(response)),
            Err(e) => Json(ApiEnvelope::error(e)),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_registry::RuntimeControlPlane;
    use abigail_core::{AppConfig, SecretsVault};
    use abigail_hive::Hive;
    use abigail_identity::IdentityManager;
    use std::sync::{Arc, Mutex};

    fn build_state() -> (HiveDaemonState, String) {
        let data_root = AppConfig::default_paths()
            .data_dir
            .join("test-hive-daemon-routes")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&data_root).expect("create test data root");

        let identity_manager = Arc::new(IdentityManager::new(data_root.clone()).expect("identity"));
        let (entity_id, _) = identity_manager
            .create_agent("Route Test")
            .expect("create entity");

        let entity_secrets_dir = data_root.join("entity_secrets");
        let hive_secrets_dir = data_root.join("hive_secrets");
        std::fs::create_dir_all(&entity_secrets_dir).expect("entity_secrets_dir");
        std::fs::create_dir_all(&hive_secrets_dir).expect("hive_secrets_dir");

        let entity_secrets = Arc::new(Mutex::new(SecretsVault::new(entity_secrets_dir)));
        let hive_secrets = Arc::new(Mutex::new(SecretsVault::new(hive_secrets_dir)));
        let hive = Arc::new(Hive::new(entity_secrets, hive_secrets.clone()));

        (
            HiveDaemonState {
                identity_manager,
                hive,
                hive_secrets,
                hive_url: "http://127.0.0.1:3141".to_string(),
                runtime_control: Arc::new(Mutex::new(RuntimeControlPlane::default())),
            },
            entity_id,
        )
    }

    #[tokio::test]
    async fn update_entity_config_persists_provider_preferences() {
        let (state, entity_id) = build_state();
        let resp = update_entity_config(
            State(state.clone()),
            Path(entity_id.clone()),
            Json(UpdateEntityConfigRequest {
                active_provider_preference: Some("openai".to_string()),
                ego_model: None,
                local_llm_base_url: Some("http://localhost:11434".to_string()),
                routing_mode: Some("cli_orchestrator".to_string()),
                cli_permission_mode: Some("interactive".to_string()),
            }),
        )
        .await
        .0;

        assert!(resp.ok, "response error: {:?}", resp.error);
        let config = state
            .identity_manager
            .load_agent(&entity_id)
            .expect("load updated config");
        assert_eq!(config.active_provider_preference.as_deref(), Some("openai"));
        assert_eq!(
            config.local_llm_base_url.as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(format!("{:?}", config.routing_mode), "CliOrchestrator");
    }
}
