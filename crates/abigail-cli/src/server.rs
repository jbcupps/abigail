//! REST API server for the Abigail CLI (`serve` subcommand).
//!
//! Uses axum with Bearer token auth middleware. The token is generated on
//! startup, printed to stdout, and can be rotated via `/rotate-key`.

use crate::auth::{auth_middleware, AuthState};
use abigail_core::{AppConfig, SecretsVault};
use abigail_router::IdEgoRouter;
use axum::{
    extract::Path,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tower_http::cors::{Any, CorsLayer};

/// Shared application state for the REST server.
#[derive(Clone)]
pub struct AppServerState {
    pub auth: AuthState,
    pub config_path: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
    pub vault: Arc<Mutex<SecretsVault>>,
    /// Router for handling chat requests (optional, provided when run from Tauri)
    pub router: Option<Arc<tokio::sync::RwLock<IdEgoRouter>>>,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub birth_complete: bool,
    pub agent_name: Option<String>,
    pub routing_mode: String,
    pub superego_l2_mode: String,
    pub local_llm_url: Option<String>,
    pub ego_provider: Option<String>,
    pub ego_key_set: bool,
    pub email_configured: bool,
    pub email_accounts: usize,
    pub mcp_servers: usize,
    pub secrets_count: usize,
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
    pub superego_l2_mode: String,
    pub id_url: Option<String>,
    pub ego_provider: Option<String>,
    pub ego_key_set: bool,
    pub superego_provider: Option<String>,
    pub superego_key_set: bool,
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
    /// Optional target: "EGO" (default) or "ID"
    pub target: Option<String>,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub reply: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub token: String,
}

#[derive(Serialize)]
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

    let auth = AuthState::new();
    let token = auth.token.read().await.clone();

    let state = AppServerState {
        auth: auth.clone(),
        config_path,
        data_dir,
        vault: Arc::new(Mutex::new(vault)),
        router: None,
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
    let vault = state.vault.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let secrets_count = vault.list_providers().len();

    let (ego_provider, ego_key_set) = config
        .trinity
        .as_ref()
        .map(|t| (t.ego_provider.clone(), t.ego_api_key.is_some()))
        .unwrap_or((None, false));

    Ok(Json(StatusResponse {
        birth_complete: config.birth_complete,
        agent_name: config.agent_name,
        routing_mode: format!("{:?}", config.routing_mode),
        superego_l2_mode: format!("{:?}", config.superego_l2_mode),
        local_llm_url: config.local_llm_base_url,
        ego_provider,
        ego_key_set,
        email_configured: config.email.is_some(),
        email_accounts: config.email_accounts.len(),
        mcp_servers: config.mcp_servers.len(),
        secrets_count,
    }))
}

async fn check_secret(
    State(state): State<AppServerState>,
    Path(key): Path<String>,
) -> Result<Json<SecretCheckResponse>, StatusCode> {
    let vault = state.vault.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(SecretCheckResponse {
        exists: vault.exists(&key),
        key,
    }))
}

async fn store_secret(
    State(state): State<AppServerState>,
    Json(body): Json<StoreSecretRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let mut vault = state.vault.lock().map_err(|_| {
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
    let mut vault = state.vault.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    let vault = state.vault.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let integrations = abigail_skills::preloaded_integration_skills();

    let items: Vec<IntegrationStatusItem> = integrations
        .iter()
        .map(|(config, auth)| {
            let secret_keys = abigail_skills::dynamic::extract_secret_keys(config);
            let missing: Vec<String> = secret_keys
                .into_iter()
                .filter(|k| vault.get_secret(k).is_none())
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
            StatusCode::INTERNAL_SERVER_ERROR,
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
    let config = load_config(&state).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (id_url, ego_provider, ego_key_set, superego_provider, superego_key_set) = config
        .trinity
        .as_ref()
        .map(|t| {
            (
                t.id_url.clone(),
                t.ego_provider.clone(),
                t.ego_api_key.is_some(),
                t.superego_provider.clone(),
                t.superego_api_key.is_some(),
            )
        })
        .unwrap_or((None, None, false, None, false));

    Ok(Json(RouterStatusResponse {
        routing_mode: format!("{:?}", config.routing_mode),
        superego_l2_mode: format!("{:?}", config.superego_l2_mode),
        id_url,
        ego_provider,
        ego_key_set,
        superego_provider,
        superego_key_set,
    }))
}

async fn chat_endpoint(
    State(state): State<AppServerState>,
    Json(body): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let router_lock = state.router.as_ref().ok_or(StatusCode::NOT_IMPLEMENTED)?;
    let router = router_lock.read().await.clone();

    let target_mode = body.target.as_deref().unwrap_or("EGO");
    let messages = vec![abigail_capabilities::cognitive::Message::new(
        "user",
        &body.message,
    )];

    let response = if target_mode == "ID" {
        router
            .id_only(messages)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        router
            .route(messages)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    Ok(Json(ChatResponse {
        reply: response.content,
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
