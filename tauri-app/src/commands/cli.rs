use crate::state::{AppState, CliServerHandle};
use abigail_cli::auth::AuthState;
use abigail_cli::server::{build_router, AppServerState};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Serialize)]
pub struct CliServerStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub token: Option<String>,
}

#[tauri::command]
pub async fn get_cli_server_status(state: State<'_, AppState>) -> Result<CliServerStatus, String> {
    let handle = state.cli_server.lock().await;
    if let Some(h) = &*handle {
        Ok(CliServerStatus {
            running: true,
            port: Some(h.port),
            token: Some(h.token.clone()),
        })
    } else {
        Ok(CliServerStatus {
            running: false,
            port: None,
            token: None,
        })
    }
}

#[tauri::command]
pub async fn start_cli_server(
    state: State<'_, AppState>,
    port: u16,
) -> Result<CliServerStatus, String> {
    let mut handle_guard = state.cli_server.lock().await;
    if handle_guard.is_some() {
        return Err("CLI server is already running".to_string());
    }

    let auth = AuthState::new();
    let token: String = auth.token.read().await.clone();

    let (config_path, data_dir, agent_name, docs_dir) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        (
            config.config_path(),
            config.data_dir.clone(),
            config.agent_name.clone(),
            config.data_dir.join("docs"),
        )
    };

    let server_state = AppServerState {
        auth: auth.clone(),
        config_path,
        data_dir,
        vault: state.secrets.clone(),
        skills_vault: Some(state.skills_secrets.clone()),
        router: Some(Arc::new(tokio::sync::RwLock::new(
            state.router.read().map_err(|e| e.to_string())?.clone(),
        ))),
        registry: Some(state.registry.clone()),
        executor: Some(state.executor.clone()),
        instruction_registry: Some(state.instruction_registry.clone()),
        docs_dir: Some(docs_dir),
        agent_name,
    };

    let app = build_router(server_state);
    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind to port {}: {}", port, e))?;

    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("CLI Server error: {}", e);
        }
    });

    *handle_guard = Some(CliServerHandle {
        task,
        token: token.clone(),
        port,
    });

    Ok(CliServerStatus {
        running: true,
        port: Some(port),
        token: Some(token),
    })
}

#[tauri::command]
pub async fn stop_cli_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut handle_guard = state.cli_server.lock().await;
    if let Some(handle) = handle_guard.take() {
        handle.task.abort();
        Ok(())
    } else {
        Err("CLI server is not running".to_string())
    }
}
