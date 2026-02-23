use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use abigail_capabilities::cognitive::Message;
use abigail_core::{AppConfig, RoutingMode, SecretsVault};
use abigail_hive::Hive;
use abigail_router::IdEgoRouter;
use anyhow::Context;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use entity_core::{
    ApiEnvelope, ChatRequest, ChatResponse, EntityStatus, RunRequest, RunResponse,
    DEFAULT_ENTITY_ADDR, ENTITY_API_VERSION_PREFIX,
};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[derive(Clone)]
struct EntityState {
    router: Arc<IdEgoRouter>,
    history: Arc<RwLock<Vec<Message>>>,
    max_history_messages: usize,
}

impl EntityState {
    async fn build() -> anyhow::Result<Self> {
        let mut config = AppConfig::default_paths();
        let config_path = config.config_path();
        if config_path.exists() {
            match AppConfig::load(&config_path) {
                Ok(loaded) => {
                    config = loaded;
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %config_path.display(),
                        "failed to load config; using defaults"
                    );
                }
            }
        }

        // Allow local development/CI to provide the key through process env.
        if config
            .openai_api_key
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            if let Ok(key) = std::env::var("OPENAI_API_KEY") {
                let trimmed = key.trim();
                if !trimmed.is_empty() {
                    config.openai_api_key = Some(trimmed.to_string());
                }
            }
        }

        let entity_vault = SecretsVault::load(config.data_dir.clone()).unwrap_or_else(|err| {
            tracing::warn!(
                error = %err,
                "failed to load entity secrets vault; continuing with empty vault"
            );
            SecretsVault::new(config.data_dir.clone())
        });
        let hive_vault = SecretsVault::load_custom(config.data_dir.clone(), "hive_secrets.bin")
            .unwrap_or_else(|err| {
                tracing::warn!(
                    error = %err,
                    "failed to load hive secrets vault; continuing with empty vault"
                );
                SecretsVault::new_custom(config.data_dir.clone(), "hive_secrets.bin")
            });

        let hive = Hive::new(
            Arc::new(Mutex::new(entity_vault)),
            Arc::new(Mutex::new(hive_vault)),
        );
        let mut providers = hive
            .build_providers_from_config(&config)
            .await
            .map_err(anyhow::Error::msg)?;

        // If we only have cloud Ego and no local server configured, prefer Ego by default
        // so normal chat prompts return provider responses instead of Candle stub fallback.
        if providers.ego.is_some() && providers.local_http.is_none() {
            providers.routing_mode = RoutingMode::EgoPrimary;
        }

        let router = IdEgoRouter::from_built_providers(providers);
        let status = router.status();
        info!(
            mode = ?status.mode,
            has_ego = status.has_ego,
            has_superego = status.has_superego,
            has_local_http = status.has_local_http,
            ego_provider = ?status.ego_provider,
            "entity router initialized"
        );

        Ok(Self {
            router: Arc::new(router),
            history: Arc::new(RwLock::new(Vec::new())),
            max_history_messages: 24,
        })
    }

    async fn push_history(&self, user_message: Message, assistant_message: Message) {
        let mut history = self.history.write().await;
        history.push(user_message);
        history.push(assistant_message);
        trim_history(&mut history, self.max_history_messages);
    }

    async fn conversation_with(&self, user_message: Message) -> Vec<Message> {
        let history = self.history.read().await;
        let mut messages = history.clone();
        messages.push(user_message);
        messages
    }
}

fn trim_history(history: &mut Vec<Message>, max_messages: usize) {
    if history.len() > max_messages {
        let drop_count = history.len() - max_messages;
        history.drain(0..drop_count);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let state = EntityState::build()
        .await
        .context("failed to initialize entity runtime state")?;
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    let app = Router::new()
        .route("/v1/status", get(get_status))
        .route("/v1/run", post(run_task))
        .route("/v1/chat", post(chat))
        .route("/v1/logs", get(get_logs))
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = DEFAULT_ENTITY_ADDR.parse().context("invalid entity addr")?;
    info!("entity-daemon listening on http://{addr}{ENTITY_API_VERSION_PREFIX}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_status(State(_state): State<EntityState>) -> Json<ApiEnvelope<EntityStatus>> {
    Json(ApiEnvelope {
        ok: true,
        data: EntityStatus {
            service: "entity-daemon".to_string(),
            api_version: "v1".to_string(),
            mode: "daemon".to_string(),
        },
    })
}

async fn run_task(
    State(_state): State<EntityState>,
    Json(req): Json<RunRequest>,
) -> Json<ApiEnvelope<RunResponse>> {
    Json(ApiEnvelope {
        ok: true,
        data: RunResponse {
            accepted: true,
            task: req.task,
        },
    })
}

async fn chat(
    State(state): State<EntityState>,
    Json(req): Json<ChatRequest>,
) -> Json<ApiEnvelope<ChatResponse>> {
    let user_message = Message::new("user", req.message.clone());
    let request_messages = state.conversation_with(user_message.clone()).await;

    let reply = match state.router.route(request_messages).await {
        Ok(response) => {
            if response.content.trim().is_empty() {
                "Provider returned an empty response.".to_string()
            } else {
                response.content
            }
        }
        Err(err) => {
            tracing::error!(error = %err, "chat routing failed");
            format!(
                "Chat routing error: {}. Check provider key and connectivity.",
                err
            )
        }
    };
    state
        .push_history(user_message, Message::new("assistant", reply.clone()))
        .await;

    Json(ApiEnvelope {
        ok: true,
        data: ChatResponse { reply },
    })
}

async fn get_logs() -> Json<ApiEnvelope<Vec<String>>> {
    Json(ApiEnvelope {
        ok: true,
        data: vec!["local entity logs only (hive ingest deferred)".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_history_keeps_latest_messages() {
        let mut history = vec![
            Message::new("user", "m1"),
            Message::new("assistant", "m2"),
            Message::new("user", "m3"),
            Message::new("assistant", "m4"),
            Message::new("user", "m5"),
        ];
        trim_history(&mut history, 3);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].content, "m3");
        assert_eq!(history[2].content, "m5");
    }
}
