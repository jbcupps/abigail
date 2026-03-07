//! REST API server for the Abigail CLI (`serve` subcommand).
//!
//! Uses axum with Bearer token auth middleware. The token is generated on
//! startup, printed to stdout, and can be rotated via `/rotate-key`.

use crate::auth::{auth_middleware, AuthState};
use abigail_core::{ops::is_reserved_provider_key, AppConfig, SecretsVault};
use abigail_router::IdEgoRouter;
use abigail_runtime::validate_secret_namespace_from_manifests;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use axum::{
    extract::Path,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use entity_core::ChatResponse;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};

/// Shared application state for the REST server.
#[derive(Clone)]
pub struct AppServerState {
    pub auth: AuthState,
    pub config_path: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
    pub vault: Arc<Mutex<SecretsVault>>,
    /// Skills vault for operational secrets (Jira, GitHub, Browser fallback, etc.)
    pub skills_vault: Option<Arc<Mutex<SecretsVault>>>,
    /// Router for handling chat requests (optional, provided when run from Tauri)
    pub router: Option<Arc<tokio::sync::RwLock<IdEgoRouter>>>,
    /// Skill registry (optional, provided when run from Tauri)
    pub registry: Option<Arc<SkillRegistry>>,
    /// Skill executor (optional, provided when run from Tauri)
    pub executor: Option<Arc<SkillExecutor>>,
    /// Skill instruction registry (optional, provided when run from Tauri)
    pub instruction_registry: Option<Arc<InstructionRegistry>>,
    /// Path to constitutional documents directory
    pub docs_dir: Option<PathBuf>,
    /// Agent name for system prompt
    pub agent_name: Option<String>,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub birth_complete: bool,
    pub agent_name: Option<String>,
    pub routing_mode: String,
    pub local_llm_url: Option<String>,
    pub ego_provider: Option<String>,
    pub ego_key_set: bool,
    pub has_ego: bool,
    pub email_configured: bool,
    pub email_accounts: usize,
    pub mcp_servers: usize,
    pub secrets_count: usize,
    pub skills_count: usize,
}

#[derive(Serialize)]
pub struct IntegrationStatusItem {
    pub service_id: String,
    pub name: String,
    pub configured: bool,
    pub missing_secrets: Vec<String>,
    pub setup_url: String,
}

#[derive(Serialize)]
pub struct RouterStatusResponse {
    pub routing_mode: String,
    pub id_url: Option<String>,
    pub ego_provider: Option<String>,
    pub ego_key_set: bool,
    pub has_ego: bool,
    pub has_local_llm: bool,
}

#[derive(Deserialize)]
pub struct StoreSecretRequest {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct ConfigureEmailRequest {
    pub address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub password: String,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    /// Optional target: "EGO" (default), "ID", or "AUTO"
    #[allow(dead_code)]
    pub target: Option<String>,
    /// Optional prior messages for multi-turn context.
    pub session_messages: Option<Vec<entity_core::SessionMessage>>,
    /// Conversation session ID (matches entity-core).
    pub session_id: Option<String>,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Serialize)]
pub struct SecretCheckResponse {
    pub key: String,
    pub exists: bool,
}

/// Start the REST API server.
pub async fn serve(port: u16) -> anyhow::Result<()> {
    let defaults = AppConfig::default_paths();
    let config_path = defaults.config_path();
    let data_dir = defaults.data_dir.clone();

    let vault = if data_dir.exists() {
        SecretsVault::load(data_dir.clone())?
    } else {
        SecretsVault::new(data_dir.clone())
    };
    let skills_vault = if data_dir.exists() {
        SecretsVault::load_custom(data_dir.clone(), "skills.bin")?
    } else {
        SecretsVault::new_custom(data_dir.clone(), "skills.bin")
    };

    let auth = AuthState::new();
    let token = auth.token.read().await.clone();

    let state = AppServerState {
        auth: auth.clone(),
        config_path,
        data_dir,
        vault: Arc::new(Mutex::new(vault)),
        skills_vault: Some(Arc::new(Mutex::new(skills_vault))),
        router: None,
        registry: None,
        executor: None,
        instruction_registry: None,
        docs_dir: None,
        agent_name: None,
    };

    let app = build_router(state);

    println!("=== Abigail REST API ===");
    println!("Listening on: http://127.0.0.1:{}", port);
    println!("Bearer token: {}", token);
    println!();
    println!("Example:");
    println!(
        "  curl -H \"Authorization: Bearer {}\" http://127.0.0.1:{}/status",
        token, port
    );

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the axum Router with all routes and middleware.
pub fn build_router(state: AppServerState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/status", get(get_status))
        .route("/secrets/check/:key", get(check_secret))
        .route("/secrets/store", post(store_secret))
        .route("/secrets/:key", delete(remove_secret))
        .route("/integrations", get(get_integrations))
        .route("/email/configure", post(configure_email_endpoint))
        .route("/router/status", get(get_router_status))
        .route("/rotate-key", post(rotate_key))
        .route("/chat", post(chat_endpoint))
        .layer(middleware::from_fn_with_state(
            state.auth.clone(),
            auth_middleware,
        ))
        .layer(cors)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn get_status(
    State(state): State<AppServerState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let config = load_config(&state).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let vault = state
        .vault
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let secrets_count = vault.list_providers().len();

    // Prefer live router state over stale config.trinity
    let (ego_provider, ego_key_set, has_ego) = if let Some(ref router_lock) = state.router {
        if let Ok(router) = router_lock.try_read() {
            let rs = router.status();
            (rs.ego_provider, rs.has_ego, rs.has_ego)
        } else {
            provider_from_config(&config)
        }
    } else {
        provider_from_config(&config)
    };

    let skills_count = state
        .registry
        .as_ref()
        .and_then(|r| r.list().ok())
        .map(|s| s.len())
        .unwrap_or(0);
    let (email_configured, email_accounts) = email_status(&config);

    Ok(Json(StatusResponse {
        birth_complete: config.birth_complete,
        agent_name: config.agent_name,
        routing_mode: format!("{:?}", config.routing_mode),
        local_llm_url: config.local_llm_base_url,
        ego_provider,
        ego_key_set,
        has_ego,
        email_configured,
        email_accounts,
        mcp_servers: config.mcp_servers.len(),
        secrets_count,
        skills_count,
    }))
}

fn email_status(config: &AppConfig) -> (bool, usize) {
    let _ = config;
    (false, 0)
}

fn target_vault_for_key(state: &AppServerState, key: &str) -> Arc<Mutex<SecretsVault>> {
    if is_reserved_provider_key(key) {
        state.vault.clone()
    } else if let Some(skills_vault) = &state.skills_vault {
        skills_vault.clone()
    } else {
        state.vault.clone()
    }
}

fn secret_exists_anywhere(state: &AppServerState, key: &str) -> Result<bool, StatusCode> {
    let provider_exists = state
        .vault
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .exists(key);
    if provider_exists {
        return Ok(true);
    }
    let skills_exists = state
        .skills_vault
        .as_ref()
        .map(|vault| {
            vault
                .lock()
                .map(|v| v.exists(key))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .transpose()?
        .unwrap_or(false);
    Ok(skills_exists)
}

/// Extract provider info from config.trinity (fallback when router is unavailable).
fn provider_from_config(config: &AppConfig) -> (Option<String>, bool, bool) {
    config
        .trinity
        .as_ref()
        .map(|t| {
            let has = t.ego_api_key.is_some();
            (t.ego_provider.clone(), has, has)
        })
        .unwrap_or((None, false, false))
}

async fn check_secret(
    State(state): State<AppServerState>,
    Path(key): Path<String>,
) -> Result<Json<SecretCheckResponse>, StatusCode> {
    Ok(Json(SecretCheckResponse {
        exists: secret_exists_anywhere(&state, &key)?,
        key,
    }))
}

async fn store_secret(
    State(state): State<AppServerState>,
    Json(body): Json<StoreSecretRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    validate_secret_namespace_from_manifests(&[], &[state.data_dir.join("skills")], &body.key)
        .map_err(|message| (StatusCode::BAD_REQUEST, Json(MessageResponse { message })))?;
    let target_vault = target_vault_for_key(&state, &body.key);
    let mut vault = target_vault.lock().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: "Lock error".to_string(),
            }),
        )
    })?;
    abigail_core::ops::store_vault_secret(&mut vault, &body.key, &body.value).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: e.to_string(),
            }),
        )
    })?;
    Ok(Json(MessageResponse {
        message: format!("Secret '{}' stored successfully", body.key),
    }))
}

async fn remove_secret(
    State(state): State<AppServerState>,
    Path(key): Path<String>,
) -> Result<Json<MessageResponse>, StatusCode> {
    let target_vault = target_vault_for_key(&state, &key);
    let mut vault = target_vault
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let removed = vault.remove_secret(&key);
    if removed {
        vault
            .save()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(MessageResponse {
            message: format!("Secret '{}' removed", key),
        }))
    } else {
        Ok(Json(MessageResponse {
            message: format!("Secret '{}' not found", key),
        }))
    }
}

async fn get_integrations(
    State(state): State<AppServerState>,
) -> Result<Json<Vec<IntegrationStatusItem>>, StatusCode> {
    let integrations = abigail_skills::preloaded_integration_skills();

    let items: Vec<IntegrationStatusItem> = integrations
        .iter()
        .map(|(config, auth)| {
            let secret_keys = abigail_skills::dynamic::extract_secret_keys(config);
            let missing: Vec<String> = secret_keys
                .into_iter()
                .filter(|k| !secret_exists_anywhere(&state, k).unwrap_or(false))
                .collect();
            IntegrationStatusItem {
                service_id: auth.service_id.clone(),
                name: config.name.clone(),
                configured: missing.is_empty(),
                missing_secrets: missing,
                setup_url: auth.setup_url.clone(),
            }
        })
        .collect();

    Ok(Json(items))
}

async fn configure_email_endpoint(
    State(state): State<AppServerState>,
    Json(body): Json<ConfigureEmailRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let mut config = load_config(&state).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: e.to_string(),
            }),
        )
    })?;
    abigail_core::ops::set_email_config(
        &mut config,
        body.address.clone(),
        body.imap_host,
        body.imap_port,
        body.smtp_host,
        body.smtp_port,
        &body.password,
    )
    .map_err(|e| {
        (
            StatusCode::GONE,
            Json(MessageResponse {
                message: e.to_string(),
            }),
        )
    })?;
    Ok(Json(MessageResponse {
        message: format!("Email configured for {}", body.address),
    }))
}

async fn get_router_status(
    State(state): State<AppServerState>,
) -> Result<Json<RouterStatusResponse>, StatusCode> {
    // Prefer live router state over stale config.trinity
    if let Some(ref router_lock) = state.router {
        if let Ok(router) = router_lock.try_read() {
            let rs = router.status();
            return Ok(Json(RouterStatusResponse {
                routing_mode: format!("{:?}", rs.mode),
                id_url: None, // local HTTP URL not exposed via status()
                ego_provider: rs.ego_provider,
                ego_key_set: rs.has_ego,
                has_ego: rs.has_ego,
                has_local_llm: rs.has_local_http,
            }));
        }
    }

    let config = load_config(&state).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (id_url, ego_provider, ego_key_set) = config
        .trinity
        .as_ref()
        .map(|t| {
            (
                t.id_url.clone(),
                t.ego_provider.clone(),
                t.ego_api_key.is_some(),
            )
        })
        .unwrap_or((None, None, false));

    Ok(Json(RouterStatusResponse {
        routing_mode: format!("{:?}", config.routing_mode),
        id_url,
        ego_provider,
        ego_key_set,
        has_ego: ego_key_set,
        has_local_llm: config.local_llm_base_url.is_some(),
    }))
}

async fn chat_endpoint(
    State(state): State<AppServerState>,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let router_lock = state.router.as_ref().ok_or(StatusCode::NOT_IMPLEMENTED)?;
    let router = router_lock.read().await.clone();

    let has_pipeline = state.registry.is_some()
        && state.executor.is_some()
        && state.instruction_registry.is_some();

    if has_pipeline {
        chat_with_pipeline(&state, &router, body).await
    } else {
        chat_bare(&router, body).await
    }
}

/// Full chat pipeline: system prompt, tools, tool-use loop, metadata.
/// Mirrors entity-daemon's /v1/chat handler.
async fn chat_with_pipeline(
    state: &AppServerState,
    router: &IdEgoRouter,
    body: ChatRequest,
) -> Result<Json<ChatResponse>, StatusCode> {
    let registry = state.registry.as_ref().unwrap();
    let executor = state.executor.as_ref().unwrap();
    let instruction_registry = state.instruction_registry.as_ref().unwrap();

    let docs_dir = state
        .docs_dir
        .clone()
        .unwrap_or_else(|| state.data_dir.join("docs"));

    let base_prompt =
        abigail_core::system_prompt::build_system_prompt(&docs_dir, &state.agent_name);

    let status = router.status();

    let runtime_ctx = entity_chat::RuntimeContext {
        provider_name: status.ego_provider.clone(),
        model_id: None,
        routing_mode: Some(format!("{:?}", status.mode)),
        tier: None,
        complexity_score: None,
        entity_name: state.agent_name.clone(),
        entity_id: None,
        has_local_llm: status.has_local_http,
        last_provider_change_at: None,
    };

    let system_prompt = entity_chat::augment_system_prompt(
        &base_prompt,
        registry,
        instruction_registry,
        &body.message,
        &runtime_ctx,
        entity_chat::PromptMode::Full,
    );

    let messages = entity_chat::build_contextual_messages(
        &system_prompt,
        body.session_messages,
        &body.message,
    );

    let tools = entity_chat::build_tool_definitions(registry);

    // Chat never uses Id; always route (Ego when available).
    let result = if tools.is_empty() {
        let resp = router
            .route_unified(abigail_router::RoutingRequest::simple(messages))
            .await;
        resp.map(|r| entity_chat::ToolUseResult {
            content: r.completion.content,
            tool_calls_made: Vec::new(),
            execution_trace: r.trace,
        })
    } else {
        entity_chat::run_tool_use_loop(router, executor, messages, tools).await
    };

    match result {
        Ok(tool_result) => {
            let tier = tool_result.tier().map(|s| s.to_string());
            let model_used = tool_result.model_used().map(|s| s.to_string());
            let complexity_score = tool_result.complexity_score();
            let provider = tool_result
                .execution_trace
                .as_ref()
                .and_then(|t| t.final_provider())
                .map(|s| s.to_string())
                .or_else(|| Some(entity_chat::provider_label(router)));

            Ok(Json(ChatResponse {
                reply: tool_result.content,
                provider,
                tool_calls_made: tool_result.tool_calls_made,
                tier,
                model_used,
                complexity_score,
                execution_trace: tool_result.execution_trace,
                session_id: body.session_id.clone(),
            }))
        }
        Err(e) => {
            tracing::error!("Chat pipeline error: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Legacy bare chat: no system prompt, no tools, no metadata.
/// Used when the server runs standalone without Tauri state.
async fn chat_bare(
    router: &IdEgoRouter,
    body: ChatRequest,
) -> Result<Json<ChatResponse>, StatusCode> {
    let messages = vec![abigail_capabilities::cognitive::Message::new(
        "user",
        &body.message,
    )];

    // Chat never uses Id; always route (Ego when available).
    let response = router
        .route_unified(abigail_router::RoutingRequest::simple(messages))
        .await
        .map(|r| r.completion)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ChatResponse {
        reply: response.content,
        provider: None,
        tool_calls_made: Vec::new(),
        tier: None,
        model_used: None,
        complexity_score: None,
        execution_trace: None,
        session_id: None,
    }))
}

async fn rotate_key(State(state): State<AppServerState>) -> Json<TokenResponse> {
    let new_token = state.auth.rotate().await;
    tracing::info!("Bearer token rotated");
    Json(TokenResponse { token: new_token })
}

fn load_config(state: &AppServerState) -> anyhow::Result<AppConfig> {
    if state.config_path.exists() {
        AppConfig::load(&state.config_path)
    } else {
        Ok(AppConfig::default_paths())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_state(tmp: &std::path::Path) -> AppServerState {
        let data_dir = tmp.join("data");
        fs::create_dir_all(&data_dir).unwrap();
        AppServerState {
            auth: AuthState::new(),
            config_path: data_dir.join("config.json"),
            data_dir: data_dir.clone(),
            vault: Arc::new(Mutex::new(SecretsVault::new(data_dir.clone()))),
            skills_vault: Some(Arc::new(Mutex::new(SecretsVault::new_custom(
                data_dir,
                "skills.bin",
            )))),
            router: None,
            registry: None,
            executor: None,
            instruction_registry: None,
            docs_dir: None,
            agent_name: None,
        }
    }

    #[test]
    fn email_status_reports_removed_transport() {
        let tmp = std::env::temp_dir().join("abigail_cli_server_email_status");
        let _ = fs::remove_dir_all(&tmp);
        let mut config = AppConfig::default_paths();
        config.email = Some(abigail_core::EmailConfig {
            address: "mentor@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            password_encrypted: vec![],
        });

        let (configured, accounts) = email_status(&config);
        assert!(!configured);
        assert_eq!(accounts, 0);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn target_vault_routes_non_reserved_secret_to_skills_vault() {
        let tmp = std::env::temp_dir().join("abigail_cli_server_secret_route");
        let _ = fs::remove_dir_all(&tmp);
        let state = test_state(&tmp);

        let vault = target_vault_for_key(&state, "custom_service_token");
        {
            let mut guard = vault.lock().unwrap();
            guard.set_secret("custom_service_token", "secret");
        }

        assert!(state
            .skills_vault
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .exists("custom_service_token"));
        assert!(!state.vault.lock().unwrap().exists("custom_service_token"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn configure_email_endpoint_returns_gone() {
        let tmp = std::env::temp_dir().join("abigail_cli_server_configure_email_removed");
        let _ = fs::remove_dir_all(&tmp);
        let state = test_state(&tmp);

        let result = configure_email_endpoint(
            State(state),
            Json(ConfigureEmailRequest {
                address: "mentor@example.com".to_string(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                password: "secret".to_string(),
            }),
        )
        .await
        .expect_err("configure email should be tombstoned");

        assert_eq!(result.0, StatusCode::GONE);
        assert!(result
            .1
             .0
            .message
            .contains("removed from mainline Abigail"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn store_secret_rejects_removed_email_key() {
        let tmp = std::env::temp_dir().join("abigail_cli_server_removed_secret_key");
        let _ = fs::remove_dir_all(&tmp);
        let state = test_state(&tmp);

        let result = store_secret(
            State(state),
            Json(StoreSecretRequest {
                key: "imap_password".to_string(),
                value: "secret".to_string(),
            }),
        )
        .await
        .expect_err("removed email keys should be rejected");

        assert_eq!(result.0, StatusCode::BAD_REQUEST);
        assert!(result.1 .0.message.contains("email_transport"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
