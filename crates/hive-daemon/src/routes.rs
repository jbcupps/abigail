//! Hive daemon HTTP route handlers.

use crate::state::HiveDaemonState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use hive_core::{
    ApiEnvelope, BestModelResponse, CliDetectResponse, CliProviderDetection, CreateEntityRequest,
    CreateEntityResponse, CreateForgeApprovalJobRequest, EntityInfo, EntityOpenResponse,
    ExecutionReceiptsResponse, ForgeApprovalJobsResponse, HelperInfo, HiveDefaultResponse,
    HiveStatus, LocalProviderRequest, LocalProviderResponse, OutboxSyncRequest, OutboxSyncResponse,
    ProviderConfig, ProviderModelInfo, ProviderModelsRequest, ProviderModelsResponse,
    ProviderProfileResponse, RuntimeHeartbeatRequest, RuntimeHeartbeatResponse,
    RuntimeRegistrationRequest, RuntimeSessionLease, RuntimeSessionRequest, RuntimeSessionStatus,
    SecretListResponse, SecretValueResponse, SetHiveDefaultRequest, SetSkillAssignmentsRequest,
    SignEntityRequest, SkillAssignmentsResponse, StoreSecretRequest, UpdateEntityConfigRequest,
    UpdateEntityConfigResponse,
};

pub(crate) fn provider_config_from_hive_config(
    hive_config: &abigail_hive::HiveConfig,
) -> ProviderConfig {
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
            let local_configured = state
                .identity_manager
                .hive_agent_id()
                .ok()
                .and_then(|id| state.identity_manager.load_agent(&id).ok())
                .and_then(|config| config.local_llm_base_url)
                .is_some_and(|url| !url.trim().is_empty());
            let inference_configured = state
                .identity_manager
                .hive_agent_id()
                .ok()
                .and_then(|id| state.identity_manager.load_agent(&id).ok())
                .and_then(|config| state.hive.resolve_config(&config).ok())
                .is_some_and(|config| config.ego_provider.is_some());
            let any_provider_configured = local_configured || inference_configured;
            let helper_local_url = state.helper_url.lock().ok().and_then(|g| g.clone());
            let helper_running = helper_local_url.is_some();
            let ready_state = if any_provider_configured {
                "ready"
            } else {
                "needs_provider"
            };

            let status = HiveStatus {
                master_key_loaded: true,
                entity_count: entities.len(),
                entities,
                ready_state: ready_state.to_string(),
                any_provider_configured,
                setup_complete: any_provider_configured,
                helper: Some(HelperInfo {
                    running: helper_running,
                    local_url: helper_local_url,
                }),
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

    match resolve_entity_provider_config(&state, &config) {
        Ok(provider_config) => Json(ApiEnvelope::success(provider_config)),
        Err(error) => Json(ApiEnvelope::error(error)),
    }
}

pub(crate) fn resolve_entity_provider_config(
    state: &HiveDaemonState,
    config: &abigail_core::AppConfig,
) -> Result<ProviderConfig, String> {
    let mut config = config.clone();
    // Inherit the Hive-level default provider/model when this entity has none of
    // its own, so a name-only entity "just works" with the family's provider.
    if !config.is_hive && config.active_provider_preference.is_none() && config.ego_model.is_none()
    {
        if let Ok(hive_id) = state.identity_manager.hive_agent_id() {
            if let Ok(hive_config) = state.identity_manager.load_agent(&hive_id) {
                config.active_provider_preference = hive_config.active_provider_preference;
                config.ego_model = hive_config.ego_model;
                if config.local_llm_base_url.is_none() {
                    config.local_llm_base_url = hive_config.local_llm_base_url;
                }
            }
        }
    }

    // Resolve via Hive priority chain
    match state.hive.resolve_config(&config) {
        Ok(hive_config) => Ok(provider_config_from_hive_config(&hive_config)),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/providers/profiles/:name
// ---------------------------------------------------------------------------

/// Resolve a named provider profile for sub-agent delegation. The profile
/// name is a provider name; the Hive resolves credentials through its vaults
/// and environment so an entity can run a sub-agent on a provider other than
/// its Ego.
pub async fn get_provider_profile(
    State(state): State<HiveDaemonState>,
    Path(name): Path<String>,
) -> Json<ApiEnvelope<ProviderProfileResponse>> {
    match state.hive.resolve_provider_profile(&name) {
        Some(selection) => Json(ApiEnvelope::success(ProviderProfileResponse {
            name,
            provider_name: selection.provider.clone(),
            api_key: selection.api_key(),
            model: None,
        })),
        None => Json(ApiEnvelope::error(format!(
            "No credentials available for provider profile '{}'",
            name
        ))),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/providers/best
// ---------------------------------------------------------------------------

/// Return the best available provider/model, ranked by capability tier.
pub async fn get_best_model(
    State(state): State<HiveDaemonState>,
) -> Json<ApiEnvelope<BestModelResponse>> {
    let resp = match state.hive.resolve_best_model() {
        Some(best) => BestModelResponse {
            provider: Some(best.provider),
            model: best.model,
            tier: Some(best.tier.as_str().to_string()),
            reason: Some(best.reason),
        },
        None => BestModelResponse::default(),
    };
    Json(ApiEnvelope::success(resp))
}

// ---------------------------------------------------------------------------
// GET /v1/providers/detect
// ---------------------------------------------------------------------------

/// Detect CLI provider tools (claude, gemini, codex, grok) installed on PATH,
/// including whether they are signed in.
pub async fn detect_cli(
    State(_state): State<HiveDaemonState>,
) -> Json<ApiEnvelope<CliDetectResponse>> {
    let providers = abigail_hive::detect_cli_providers_full()
        .into_iter()
        .map(|d| CliProviderDetection {
            provider: d.provider_name,
            on_path: d.on_path,
            is_official: d.is_official,
            is_authenticated: d.is_authenticated,
            auth_hint: d.auth_hint,
        })
        .collect();
    Json(ApiEnvelope::success(CliDetectResponse { providers }))
}

// ---------------------------------------------------------------------------
// POST /v1/providers/hive-default
// ---------------------------------------------------------------------------

fn cli_connection_failure(label: &str, sign_in_hint: &str, error: &anyhow::Error) -> String {
    let description = format!("{:#}", error).to_ascii_lowercase();
    if [
        "authentication",
        "unauthorized",
        "auth required",
        "auth_required",
        "login required",
        "sign in",
        "sign-in",
    ]
    .iter()
    .any(|cause| description.contains(cause))
    {
        return match label {
            "Codex" => "Codex needs sign-in. Sign in again with codex login, then check the connection in Hive.".to_string(),
            "Grok" => "Grok needs sign-in. Sign in again with grok login, then check the connection in Hive.".to_string(),
            _ => format!("{} needs sign-in. {}", label, sign_in_hint),
        };
    }
    if ["usage limit", "rate limit", "rate_limit", "credit", "quota"]
        .iter()
        .any(|cause| description.contains(cause))
    {
        return format!("{} has reached an account usage limit. Wait for the limit to reset or choose another model connection in Hive.", label);
    }
    if [
        "unsupported version",
        "update grok",
        "update codex",
        "update claude",
        "update the official",
        "upgrade",
        "version is not supported",
    ]
    .iter()
    .any(|cause| description.contains(cause))
    {
        return format!("{} needs a newer supported version. Update the official CLI, then check the connection in Hive.", label);
    }
    if [
        "model unavailable",
        "model is unavailable",
        "model not found",
        "model_not_found",
        "model availability",
        "unsupported model",
    ]
    .iter()
    .any(|cause| description.contains(cause))
    {
        return format!("{} cannot use the selected model. Choose another model in the CLI or another model connection in Hive.", label);
    }
    let isolation_failure = description.contains("isolation")
        || description.contains("isolate")
        || (["hook", "plugin", "mcp", "lsp", "extension"]
            .iter()
            .any(|feature| description.contains(feature))
            && ["active", "configured", "preflight"]
                .iter()
                .any(|state| description.contains(state)));
    if isolation_failure {
        format!(
            "{} cannot make a chat connection while its extensions or hooks are active. Disable them in the CLI and try again, or choose another model connection in Hive.",
            label
        )
    } else {
        format!("{} connection check failed. {}", label, sign_in_hint)
    }
}

/// Set the Hive-level default provider/model that newly created entities
/// inherit. With an empty body, seeds from the best available provider.
pub async fn set_hive_default(
    State(state): State<HiveDaemonState>,
    Json(body): Json<SetHiveDefaultRequest>,
) -> Json<ApiEnvelope<HiveDefaultResponse>> {
    // Resolve which provider/model to store: explicit, or best available.
    let (provider, model) = match body.provider.clone() {
        Some(p) => (Some(p.trim().to_lowercase()), body.model.clone()),
        None => match state.hive.resolve_best_model() {
            Some(best)
                if !best.provider.ends_with("-cli")
                    || matches!(
                        best.provider.as_str(),
                        "claude-cli" | "codex-cli" | "grok-cli"
                    ) =>
            {
                (Some(best.provider), best.model)
            }
            Some(_) | None => (None, None),
        },
    };
    if provider.as_deref().is_some_and(|name| {
        name.ends_with("-cli") && !matches!(name, "claude-cli" | "codex-cli" | "grok-cli")
    }) {
        return Json(ApiEnvelope::error(
            "This installed tool is not supported yet. Use Claude, Codex, Grok, or connect a cloud or local model.",
        ));
    }
    if let Some(cli_name) = provider.as_deref().filter(|name| name.ends_with("-cli")) {
        use abigail_capabilities::cognitive::{
            CliLlmProvider, CliVariant, CompletionRequest, LlmProvider, Message,
        };
        let (variant, label, sign_in_hint) = match cli_name {
            "claude-cli" => (
                CliVariant::ClaudeCode,
                "Claude",
                "Open Claude and check its sign-in and selected model, then try again.",
            ),
            "codex-cli" => (
                CliVariant::OpenAiCodex,
                "Codex",
                "Open Codex and check its account connection and selected model, then try again.",
            ),
            "grok-cli" => (
                CliVariant::XaiGrokCli,
                "Grok",
                "Open Grok and check its account connection and selected model, then try again.",
            ),
            _ => unreachable!("Unsupported CLI providers are rejected above"),
        };
        let api_key = match state.hive_secrets.lock() {
            Ok(vault) => vault
                .get_secret(cli_name)
                .filter(|key| !key.trim().is_empty())
                .unwrap_or("system")
                .to_string(),
            Err(error) => return Json(ApiEnvelope::error(error.to_string())),
        };
        let cli = match CliLlmProvider::with_permission_mode(
            variant,
            api_key,
            abigail_core::CliPermissionMode::AllowListOnly,
        ) {
            Ok(cli) => cli,
            Err(_) => {
                return Json(ApiEnvelope::error(format!(
                    "{} could not be connected. {}",
                    label, sign_in_hint
                )))
            }
        };
        let mut request = CompletionRequest::simple(vec![Message::new(
            "user",
            "Reply with exactly READY to confirm the model connection.",
        )]);
        request.model_override = model.clone();
        match tokio::time::timeout(std::time::Duration::from_secs(60), cli.complete(&request)).await
        {
            Ok(Ok(response)) if !response.content.trim().is_empty() => {}
            Ok(Ok(_)) => {
                return Json(ApiEnvelope::error(format!(
                    "{} did not return a model response. {}",
                    label, sign_in_hint
                )))
            }
            Ok(Err(error)) => {
                // CLI stderr may contain account or credential details. Keep
                // the user-facing error actionable without returning it.
                return Json(ApiEnvelope::error(cli_connection_failure(
                    label,
                    sign_in_hint,
                    &error,
                )));
            }
            Err(_) => {
                return Json(ApiEnvelope::error(format!(
                    "{} connection check timed out. {}",
                    label, sign_in_hint
                )))
            }
        }
    }

    let hive_id = match state.identity_manager.hive_agent_id() {
        Ok(id) => id,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };
    let mut config = match state.identity_manager.load_agent(&hive_id) {
        Ok(c) => c,
        Err(e) => return Json(ApiEnvelope::error(e)),
    };

    config.active_provider_preference = provider
        .as_ref()
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty());
    config.ego_model = model
        .as_ref()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    let config_path = config.config_path();
    if let Err(e) = config.save(&config_path) {
        return Json(ApiEnvelope::error(e.to_string()));
    }

    Json(ApiEnvelope::success(HiveDefaultResponse {
        provider: config.active_provider_preference.clone(),
        model: config.ego_model.clone(),
    }))
}

/// Validate a real local model before saving it as the family's default.
pub async fn set_local_provider(
    State(state): State<HiveDaemonState>,
    Json(body): Json<LocalProviderRequest>,
) -> Json<ApiEnvelope<LocalProviderResponse>> {
    let base_url = body
        .base_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_string();
    if let Err(error) = abigail_core::validate_local_llm_url(&base_url) {
        return Json(ApiEnvelope::error(error.to_string()));
    }
    let provider =
        match abigail_capabilities::cognitive::LocalHttpProvider::connect(&base_url).await {
            Ok(provider) => provider,
            Err(error) => {
                return Json(ApiEnvelope::error(format!(
                    "Unable to connect to a loaded local model: {}",
                    error
                )))
            }
        };
    if let Err(error) = provider.heartbeat().await {
        return Json(ApiEnvelope::error(format!(
            "The local model could not answer a test message: {}",
            error
        )));
    }
    let hive_id = match state.identity_manager.hive_agent_id() {
        Ok(id) => id,
        Err(error) => return Json(ApiEnvelope::error(error)),
    };
    let mut config = match state.identity_manager.load_agent(&hive_id) {
        Ok(config) => config,
        Err(error) => return Json(ApiEnvelope::error(error)),
    };
    config.local_llm_base_url = Some(base_url.clone());
    config.active_provider_preference = Some("local".to_string());
    config.ego_model = None;
    if let Err(error) = config.save(&config.config_path()) {
        return Json(ApiEnvelope::error(error.to_string()));
    }
    Json(ApiEnvelope::success(LocalProviderResponse {
        base_url,
        model: provider.model().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// POST /v1/entities/:id/open
// ---------------------------------------------------------------------------

/// Ensure the entity's daemon is running (starting it on demand if needed) and
/// return its local URL. Idempotent — reuses a live daemon.
pub async fn open_entity(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<EntityOpenResponse>> {
    match state.supervisor.ensure_entity_running(&entity_id).await {
        Ok(local_url) => Json(ApiEnvelope::success(EntityOpenResponse {
            entity_id,
            local_url,
        })),
        Err(e) => Json(ApiEnvelope::error(format!("{:#}", e))),
    }
}

// ---------------------------------------------------------------------------
// POST /v1/entities/:id/close
// ---------------------------------------------------------------------------

/// Stop the entity's daemon (called when its window closes). The immortal Hive
/// helper is managed separately and is never affected by this.
pub async fn close_entity(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<String>> {
    match state.identity_manager.hive_agent_id() {
        Ok(hive_id) if hive_id == entity_id => {
            return Json(ApiEnvelope::error(
                "Abigail Hive remains available while family Entities are closed.",
            ));
        }
        Ok(_) => {}
        Err(error) => return Json(ApiEnvelope::error(error)),
    }
    state.supervisor.stop_entity(&entity_id);
    match state.runtime_control.lock() {
        Ok(mut control) => {
            control.revoke_entity_sessions(&entity_id);
        }
        Err(error) => return Json(ApiEnvelope::error(error.to_string())),
    }
    Json(ApiEnvelope::success(format!(
        "Entity {} stopped",
        entity_id
    )))
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

    if let Some(model) = body.ego_model {
        let trimmed = model.trim().to_string();
        config.ego_model = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
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
    if let Err(error) =
        abigail_capabilities::cognitive::validation::validate_api_key(&body.provider, &body.api_key)
            .await
    {
        return Json(ApiEnvelope::error(format!(
            "Unable to connect this model provider: {}",
            error
        )));
    }
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
    headers: HeaderMap,
    Json(body): Json<RuntimeSessionRequest>,
) -> (StatusCode, Json<ApiEnvelope<RuntimeSessionLease>>) {
    let authorized = bearer_token(&headers).is_some_and(|capability| {
        crate::supervisor::consume_runtime_bootstrap(&body.entity_id, capability)
    });
    if !authorized {
        return forbidden_runtime();
    }
    let response = match state.identity_manager.list_agents() {
        Ok(agents) => {
            let Some(agent) = agents.into_iter().find(|agent| agent.id == body.entity_id) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ApiEnvelope::error("Entity not found")),
                );
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
    };
    (StatusCode::OK, response)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
}

fn forbidden_runtime<T: serde::Serialize>() -> (StatusCode, Json<ApiEnvelope<T>>) {
    (
        StatusCode::FORBIDDEN,
        Json(ApiEnvelope::error("Runtime authorization required")),
    )
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
    headers: HeaderMap,
) -> (StatusCode, Json<ApiEnvelope<RuntimeSessionStatus>>) {
    if bearer_token(&headers) != Some(lease_id.as_str()) {
        return forbidden_runtime();
    }
    let response = match state.runtime_control.lock() {
        Ok(control) => match control.session_status(&lease_id) {
            Some(status) => Json(ApiEnvelope::success(status)),
            None => Json(ApiEnvelope::error(format!(
                "Runtime lease {} not found",
                lease_id
            ))),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    };
    (StatusCode::OK, response)
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
        Ok(mut control) => match control
            .sync_outbox(body, |payload| state.identity_manager.sign_payload(payload))
        {
            Ok(response) => Json(ApiEnvelope::success(response)),
            Err(e) => Json(ApiEnvelope::error(e)),
        },
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/entities/:id/execution/receipts
// ---------------------------------------------------------------------------

pub async fn get_execution_receipts(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<ApiEnvelope<ExecutionReceiptsResponse>>) {
    let response = match state.runtime_control.lock() {
        Ok(control) => {
            let authorized = bearer_token(&headers)
                .and_then(|lease_id| control.session_status(lease_id))
                .is_some_and(|session| session.lease.entity_id == entity_id);
            if !authorized {
                return forbidden_runtime();
            }
            Json(ApiEnvelope::success(
                control.execution_receipts(&entity_id, 500),
            ))
        }
        Err(e) => Json(ApiEnvelope::error(e.to_string())),
    };
    (StatusCode::OK, response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_registry::RuntimeControlPlane;
    use abigail_core::{AppConfig, SecretsVault};
    use abigail_hive::Hive;
    use abigail_identity::IdentityManager;
    use std::sync::{Arc, Mutex};

    #[test]
    fn cli_probe_errors_explain_isolation_without_disclosing_cli_details() {
        let private_details = "sk-private-fixture C:\\Users\\private-account\\config";
        let isolation = cli_connection_failure(
            "Grok",
            "Check Grok sign-in.",
            &anyhow::anyhow!("isolation blocked: active MCP plugin {}", private_details),
        );
        assert!(isolation.contains("chat connection"));
        assert!(isolation.contains("Disable them in the CLI"));
        assert!(!isolation.contains(private_details));
        assert!(!isolation.contains("sk-private-fixture"));
        let auth = cli_connection_failure(
            "Codex",
            "Check Codex account sign-in.",
            &anyhow::anyhow!("authentication failed {}", private_details),
        );
        assert!(auth.contains("codex login"));
        assert!(!auth.contains(private_details));
        assert!(!auth.contains("sk-private-fixture"));
    }

    #[test]
    fn cli_probe_errors_offer_fixed_account_version_and_model_recovery() {
        let cases = [
            ("Grok", "authentication required", "grok login"),
            (
                "Codex",
                "rate_limit exceeded",
                "Wait for the limit to reset",
            ),
            (
                "Grok",
                "insufficient credits",
                "another model connection in Hive",
            ),
            (
                "Grok",
                "isolation preflight failed; update Grok Build",
                "Update the official CLI",
            ),
            ("Codex", "unsupported version", "Update the official CLI"),
            (
                "Codex",
                "model unavailable",
                "Choose another model in the CLI",
            ),
            (
                "Grok",
                "model availability",
                "Choose another model in the CLI",
            ),
            ("Codex", "unknown failure", "Check account sign-in"),
        ];
        for (label, cause, recovery) in cases {
            let message = cli_connection_failure(
                label,
                "Check account sign-in.",
                &anyhow::anyhow!(
                    "{} sk-private-fixture C:\\Users\\private-account\\config",
                    cause
                ),
            );
            assert!(message.contains(recovery), "{cause}: {message}");
            assert!(!message.contains("sk-private-fixture"));
            assert!(!message.contains("private-account"));
        }
    }

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
                helper_url: Arc::new(Mutex::new(None)),
                supervisor: crate::supervisor::HiveSupervisor::new(
                    "http://127.0.0.1:3141".to_string(),
                    data_root.clone(),
                ),
            },
            entity_id,
        )
    }

    fn runtime_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
        headers
    }

    #[tokio::test]
    async fn durable_persistence_bridge_authorizes_each_entity_and_rejects_scope_escapes() {
        use abigail_persistence::{EntityScope, PersistenceHandle, QueryBinding, RemoteOperation};

        let (state, alpha_id) = build_state();
        let (beta_id, _) = state.identity_manager.create_agent("Beta").unwrap();
        let (alpha_lease, beta_lease) = {
            let mut control = state.runtime_control.lock().unwrap();
            let alpha = control.issue_session(
                RuntimeSessionRequest {
                    entity_id: alpha_id.clone(),
                    runtime_id: None,
                },
                Some("Alpha".into()),
                Some(state.hive_url.clone()),
            );
            let beta = control.issue_session(
                RuntimeSessionRequest {
                    entity_id: beta_id.clone(),
                    runtime_id: None,
                },
                Some("Beta".into()),
                Some(state.hive_url.clone()),
            );
            (alpha, beta)
        };
        // Use the installed product's durable store, not the CI memory-store
        // path. Priming Hive scope also matches production startup ordering.
        let shared_path =
            abigail_identity::HiveEntity::memory_db_path(state.identity_manager.data_root());
        let _hive_store = PersistenceHandle::open(&shared_path, EntityScope::Hive).unwrap();
        assert!(shared_path.is_dir());

        let record_id = "alpha-private-memory";
        let (status, response) = crate::persistence::request(
            State(state.clone()), Path(alpha_id.clone()), runtime_headers(&alpha_lease.lease_id),
            Json(RemoteOperation::Create {
                table: "memory_entry".into(), id: record_id.into(),
                value: serde_json::json!({"id": record_id, "content": "Alpha only", "created_at": "2026-10-02T12:00:00Z"}),
            }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            response.0.ok,
            "authorized durable write: {:?}",
            response.0.error
        );

        let select_record = || RemoteOperation::Select {
            table: "memory_entry".into(),
            id: record_id.into(),
        };
        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(alpha_id.clone()),
            runtime_headers(&alpha_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            response.0.ok,
            "authorized durable select: {:?}",
            response.0.error
        );
        assert_eq!(response.0.result["content"], "Alpha only");

        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(beta_id.clone()),
            runtime_headers(&alpha_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "Alpha bearer must not authorize Beta path"
        );
        assert!(!response.0.ok);
        assert!(response.0.result.is_null());

        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(beta_id.clone()),
            runtime_headers(&beta_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.0.ok, "Beta own select: {:?}", response.0.error);
        assert!(
            response.0.result.is_null(),
            "Beta must not see Alpha's record"
        );

        let beta_database = EntityScope::Entity(beta_id.clone()).database_name();
        for sql in [
            "REMOVE NS abigail".to_string(),
            format!("REMOVE DB {}", beta_database),
        ] {
            let (status, response) = crate::persistence::request(
                State(state.clone()),
                Path(alpha_id.clone()),
                runtime_headers(&alpha_lease.lease_id),
                Json(RemoteOperation::Execute {
                    sql: sql.clone(),
                    bindings: vec![],
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(!response.0.ok, "scope control must be rejected: {}", sql);
            assert!(response.0.error.is_some());
        }
        for sql in [
            format!("SELECT * FROM (REMOVE DB {})", beta_database),
            "SELECT * FROM memory_entry; REMOVE NS abigail".to_string(),
        ] {
            let (status, response) = crate::persistence::request(
                State(state.clone()),
                Path(alpha_id.clone()),
                runtime_headers(&alpha_lease.lease_id),
                Json(RemoteOperation::QueryVec {
                    sql: sql.clone(),
                    bindings: vec![],
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(
                !response.0.ok,
                "nested/compound scope control must be rejected: {}",
                sql
            );
        }

        // Real parameter binding and ordering remain usable after blocked
        // attempts, and the two databases still contain separate records.
        let (status, response) = crate::persistence::request(
            State(state.clone()), Path(alpha_id.clone()), runtime_headers(&alpha_lease.lease_id),
            Json(RemoteOperation::QueryVec {
                sql: "SELECT * FROM memory_entry WHERE content = $content ORDER BY created_at DESC LIMIT 10".into(),
                bindings: vec![QueryBinding::new("content", "Alpha only").unwrap()],
            }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            response.0.ok,
            "ordinary scoped SELECT: {:?}",
            response.0.error
        );
        let records = response.0.result.as_array().expect("SELECT records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["content"], "Alpha only");
        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(beta_id.clone()),
            runtime_headers(&beta_lease.lease_id),
            Json(RemoteOperation::QueryVec {
                sql: "SELECT * FROM memory_entry".into(),
                bindings: vec![],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.0.ok);
        assert_eq!(response.0.result, serde_json::json!([]));

        let response = close_entity(State(state.clone()), Path(alpha_id.clone())).await;
        assert!(response.0.ok, "explicit family close succeeds");
        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(alpha_id.clone()),
            runtime_headers(&alpha_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "closed lease must not authorize durable reads"
        );
        assert!(!response.0.ok);
        let new_alpha_lease = state.runtime_control.lock().unwrap().issue_session(
            RuntimeSessionRequest {
                entity_id: alpha_id.clone(),
                runtime_id: None,
            },
            Some("Alpha reopened".into()),
            Some(state.hive_url.clone()),
        );
        assert_ne!(new_alpha_lease.lease_id, alpha_lease.lease_id);
        let (status, response) = crate::persistence::request(
            State(state.clone()),
            Path(alpha_id),
            runtime_headers(&new_alpha_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.0.ok, "fresh runtime lease can resume durable data");
        assert_eq!(response.0.result["content"], "Alpha only");
        let (status, response) = crate::persistence::request(
            State(state),
            Path(beta_id),
            runtime_headers(&beta_lease.lease_id),
            Json(select_record()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "closing Alpha must not revoke Beta");
        assert!(response.0.ok);
        assert!(response.0.result.is_null());
    }

    #[tokio::test]
    async fn runtime_bootstrap_is_scoped_single_use_and_diagnostics_require_own_lease() {
        let (state, entity_id) = build_state();
        let (other_id, _) = state.identity_manager.create_agent("Other Entity").unwrap();
        let request = |id: &str| RuntimeSessionRequest {
            entity_id: id.to_string(),
            runtime_id: None,
        };
        let (status, response) = issue_runtime_session(
            State(state.clone()),
            HeaderMap::new(),
            Json(request(&entity_id)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(response.0.data.is_none());

        let bootstrap = crate::supervisor::issue_runtime_bootstrap(&entity_id);
        let (status, _) = issue_runtime_session(
            State(state.clone()),
            runtime_headers(&bootstrap),
            Json(request(&other_id)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "another Entity cannot use a launch capability"
        );
        let (status, response) = issue_runtime_session(
            State(state.clone()),
            runtime_headers(&bootstrap),
            Json(request(&entity_id)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let lease = response.0.data.expect("authorized runtime lease");
        let (status, _) = issue_runtime_session(
            State(state.clone()),
            runtime_headers(&bootstrap),
            Json(request(&entity_id)),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "launch capability must be consumed"
        );

        let other_bootstrap = crate::supervisor::issue_runtime_bootstrap(&other_id);
        let (_, response) = issue_runtime_session(
            State(state.clone()),
            runtime_headers(&other_bootstrap),
            Json(request(&other_id)),
        )
        .await;
        let other_lease = response.0.data.unwrap();
        for headers in [HeaderMap::new(), runtime_headers(&other_lease.lease_id)] {
            let (status, response) = get_runtime_session(
                State(state.clone()),
                Path(lease.lease_id.clone()),
                headers.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(response.0.data.is_none());
            let (status, response) =
                get_execution_receipts(State(state.clone()), Path(entity_id.clone()), headers)
                    .await;
            assert_eq!(status, StatusCode::FORBIDDEN);
            assert!(response.0.data.is_none());
        }
        let (status, response) = get_runtime_session(
            State(state.clone()),
            Path(lease.lease_id.clone()),
            runtime_headers(&lease.lease_id),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.0.ok);
        let (status, response) = get_execution_receipts(
            State(state),
            Path(entity_id),
            runtime_headers(&lease.lease_id),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(response.0.ok);
    }

    #[tokio::test]
    async fn close_entity_preserves_the_immortal_hive_helper_session() {
        let (state, _) = build_state();
        let hive_id = state.identity_manager.hive_agent_id().unwrap();
        let lease = state.runtime_control.lock().unwrap().issue_session(
            RuntimeSessionRequest {
                entity_id: hive_id.clone(),
                runtime_id: None,
            },
            Some("Abigail Hive".into()),
            Some(state.hive_url.clone()),
        );
        let response = close_entity(State(state.clone()), Path(hive_id)).await;
        assert!(
            !response.0.ok,
            "family close route cannot close the Hive helper"
        );
        assert!(state
            .runtime_control
            .lock()
            .unwrap()
            .session_status(&lease.lease_id)
            .is_some());
    }

    #[tokio::test]
    async fn update_entity_config_persists_provider_preferences() {
        let (state, entity_id) = build_state();
        let resp = update_entity_config(
            State(state.clone()),
            Path(entity_id.clone()),
            Json(UpdateEntityConfigRequest {
                active_provider_preference: Some("openai".to_string()),
                ego_model: Some("gpt-4.1".to_string()),
                local_llm_base_url: Some("http://localhost:11434".to_string()),
                routing_mode: Some("cli_orchestrator".to_string()),
                cli_permission_mode: Some("interactive".to_string()),
            }),
        )
        .await
        .0;

        assert!(resp.ok, "response error: {:?}", resp.error);
        // The chosen model is surfaced back on the resolved provider config.
        assert_eq!(
            resp.data
                .as_ref()
                .and_then(|d| d.provider_config.ego_model.as_deref()),
            Some("gpt-4.1")
        );
        let config = state
            .identity_manager
            .load_agent(&entity_id)
            .expect("load updated config");
        assert_eq!(config.active_provider_preference.as_deref(), Some("openai"));
        assert_eq!(config.ego_model.as_deref(), Some("gpt-4.1"));
        assert_eq!(
            config.local_llm_base_url.as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(format!("{:?}", config.routing_mode), "CliOrchestrator");
    }
}
