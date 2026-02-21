use crate::ollama_manager::{
    OllamaDetection, OllamaInstallProgress, OllamaManager, OllamaModelProgress, OllamaStatus,
    RecommendedModel,
};
use crate::state::AppState;
use tauri::{Emitter, State};

#[tauri::command]
pub async fn detect_ollama() -> OllamaDetection {
    OllamaManager::detect_ollama().await
}

#[tauri::command]
pub fn list_recommended_models() -> Vec<RecommendedModel> {
    OllamaManager::list_recommended_models()
}

#[tauri::command]
pub async fn install_ollama(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    OllamaManager::download_and_install(|progress: OllamaInstallProgress| {
        let _ = app.emit("ollama-install-progress", progress);
    })
    .await
}

#[tauri::command]
pub async fn pull_ollama_model(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    OllamaManager::pull_model_streaming(
        "http://localhost:11434",
        &model,
        |progress: OllamaModelProgress| {
            let _ = app.emit("ollama-model-progress", progress);
        },
    )
    .await
}

#[tauri::command]
pub async fn get_ollama_status(state: State<'_, AppState>) -> Result<OllamaStatus, String> {
    let mgr_guard = state.ollama.lock().await;
    if let Some(mgr) = &*mgr_guard {
        Ok(mgr.status())
    } else {
        // If not managed, just probe the default port
        let running = OllamaManager::detect_ollama().await.status == "running";
        Ok(OllamaStatus {
            managed: false,
            running,
            port: 11434,
            model_ready: running, // Assume model ready if running for now, or we could probe deeper
        })
    }
}

#[tauri::command]
pub async fn probe_local_llm() -> Result<serde_json::Value, String> {
    let ports = [1234, 11434, 8080, 8000];
    let mut detected = Vec::new();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .map_err(|e| e.to_string())?;

    for port in ports {
        let url = format!("http://localhost:{}", port);
        let tags_url = format!("{}/api/tags", url); // Ollama
        let models_url = format!("{}/v1/models", url); // OpenAI-compatible (LM Studio, etc.)

        if let Ok(resp) = client.get(&tags_url).send().await {
            if resp.status().is_success() {
                detected.push(serde_json::json!({
                    "name": "Ollama",
                    "url": url,
                    "reachable": true
                }));
                continue;
            }
        }

        if let Ok(resp) = client.get(&models_url).send().await {
            if resp.status().is_success() {
                detected.push(serde_json::json!({
                    "name": if port == 1234 { "LM Studio" } else { "Local LLM" },
                    "url": url,
                    "reachable": true
                }));
            }
        }
    }

    Ok(serde_json::json!({ "detected": detected }))
}

#[tauri::command]
pub async fn set_local_llm_during_birth(
    state: State<'_, AppState>,
    url: String,
    skip_health_check: bool,
) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;

    // Robust health check: try multiple common endpoints
    let url_trimmed = url.trim_end_matches('/');
    let endpoints = [
        format!("{}/api/tags", url_trimmed),  // Ollama
        format!("{}/v1/models", url_trimmed), // OpenAI-compatible
        url_trimmed.to_string(),              // Root
    ];

    let mut reachable = false;
    if !skip_health_check {
        for ep in endpoints {
            if let Ok(resp) = client.get(&ep).send().await {
                // Any response (even 404 for root) suggests the server is there
                if resp.status().is_success() || resp.status().as_u16() == 404 {
                    reachable = true;
                    break;
                }
            }
        }
    } else {
        reachable = true;
    }

    if reachable {
        {
            let mut config = state.config.write().map_err(|e| e.to_string())?;
            config.local_llm_base_url = Some(url);
            config
                .save(&config.config_path())
                .map_err(|e| e.to_string())?;
        }

        // Rebuild router with new URL
        if let Err(e) = crate::rebuild_router_with_superego(&state).await {
            tracing::warn!("Failed to rebuild router after setting local LLM: {}", e);
            // Even if rebuild fails (e.g. model not loaded), we saved the URL
        }
        Ok(true)
    } else {
        // Log failure for troubleshooting
        tracing::warn!("Local LLM health check failed for URL: {}", url);
        Ok(false)
    }
}
