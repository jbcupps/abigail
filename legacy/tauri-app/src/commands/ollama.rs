use crate::ollama_manager::{
    InstalledModel, OllamaDetection, OllamaInstallProgress, OllamaLifecycleState, OllamaManager,
    OllamaModelProgress, OllamaStatus, RecommendedModel,
};
use crate::state::AppState;
use tauri::{Emitter, Manager, State};

#[tauri::command]
pub async fn detect_ollama() -> OllamaDetection {
    OllamaManager::detect_ollama().await
}

#[tauri::command]
pub fn list_recommended_models() -> Vec<RecommendedModel> {
    OllamaManager::list_recommended_models()
}

#[tauri::command]
pub async fn list_ollama_models(base_url: Option<String>) -> Result<Vec<InstalledModel>, String> {
    let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
    OllamaManager::list_installed_models(&url).await
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
    base_url: Option<String>,
) -> Result<(), String> {
    let url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
    OllamaManager::pull_model_streaming(&url, &model, |progress: OllamaModelProgress| {
        let _ = app.emit("ollama-model-progress", progress);
    })
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
        if let Err(e) = crate::rebuild_router(&state).await {
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

/// Start the managed Ollama instance, pull model if needed, and configure the
/// local LLM URL for the router.  Returns `true` if background work (install
/// and/or model pull) is happening, `false` if everything was already ready or
/// Ollama management is disabled.
///
/// When Ollama is not installed, this command **auto-installs** it before
/// starting the server and pulling the model.  All heavy work runs in a
/// background tokio task; the command returns immediately so the frontend can
/// show the loading screen.
///
/// Emits `ollama-lifecycle` events so the frontend can track progress.
#[tauri::command]
pub async fn start_managed_ollama(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    // 1. Read config
    let (bundled_ollama, bundled_model, first_pull_done) = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        (
            config.bundled_ollama,
            config.bundled_model.clone(),
            config.first_model_pull_complete,
        )
    };

    if !bundled_ollama {
        let _ = app.emit("ollama-lifecycle", OllamaLifecycleState::NotStarted);
        return Ok(false);
    }

    let model_name = bundled_model.unwrap_or_else(|| "llama3.2:3b".to_string());

    // 2. Emit Starting
    let _ = app.emit("ollama-lifecycle", OllamaLifecycleState::Starting);

    // 3. Resolve bundled binary path from Tauri resource directory
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to resolve resource dir: {}", e))?;
    let bundled_bin = OllamaManager::bundled_binary_path(&resource_dir);
    let bundled_path = if bundled_bin.exists() {
        Some(bundled_bin)
    } else {
        tracing::info!("Bundled Ollama not found at resource dir, trying system install");
        None
    };

    // 4. Read data_dir for model storage
    let data_dir = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config.data_dir.clone()
    };

    // 5. Try to start Ollama (bundled or system).  If not found, auto-install.
    let start_result = OllamaManager::discover_and_start_bundled(&data_dir, bundled_path).await;

    let needs_install = start_result.is_err();

    if needs_install {
        // Ollama binary not found — run install + start + model-pull in background.
        tracing::info!("Ollama not found; launching background auto-install");
        let app_bg = app.clone();
        let data_dir_bg = data_dir.clone();

        tokio::spawn(async move {
            let state_ref = app_bg.state::<AppState>();

            // ── Phase 1: Install Ollama ──────────────────────────────
            let _ = app_bg.emit(
                "ollama-lifecycle",
                OllamaLifecycleState::InstallingOllama { progress_pct: 0.0 },
            );

            let app_install = app_bg.clone();
            let install_result =
                OllamaManager::download_and_install(move |progress: OllamaInstallProgress| {
                    let pct = match (progress.written, progress.total) {
                        (Some(w), Some(t)) if t > 0 => (w as f32 / t as f32) * 100.0,
                        _ => 0.0,
                    };
                    let _ = app_install.emit(
                        "ollama-lifecycle",
                        OllamaLifecycleState::InstallingOllama { progress_pct: pct },
                    );
                })
                .await;

            if let Err(e) = install_result {
                tracing::error!("Auto-install of Ollama failed: {}", e);
                let _ = app_bg.emit(
                    "ollama-lifecycle",
                    OllamaLifecycleState::Error(format!("Ollama install failed: {e}")),
                );
                return;
            }

            tracing::info!("Ollama auto-install complete, starting server");

            // ── Phase 2: Start the freshly installed Ollama ──────────
            let _ = app_bg.emit("ollama-lifecycle", OllamaLifecycleState::Starting);

            let mgr = match OllamaManager::discover_and_start(&data_dir_bg).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!("Failed to start Ollama after install: {}", e);
                    let _ = app_bg.emit(
                        "ollama-lifecycle",
                        OllamaLifecycleState::Error(format!(
                            "Ollama installed but failed to start: {e}"
                        )),
                    );
                    return;
                }
            };

            let base_url = mgr.base_url();
            {
                let mut guard = state_ref.ollama.lock().await;
                *guard = Some(mgr);
            }
            let _ = app_bg.emit("ollama-lifecycle", OllamaLifecycleState::Running);

            // ── Phase 3: Pull model ──────────────────────────────────
            pull_model_and_finalize(&app_bg, &state_ref, &base_url, &model_name, first_pull_done)
                .await;
        });

        return Ok(true);
    }

    // Ollama was already installed — normal synchronous start succeeded.
    let mgr = start_result.unwrap();
    let base_url = mgr.base_url();

    // 6. Store manager in state
    {
        let mut guard = state.ollama.lock().await;
        *guard = Some(mgr);
    }

    let _ = app.emit("ollama-lifecycle", OllamaLifecycleState::Running);

    // 7. Check if model already exists
    let model_exists = check_model_exists_on_server(&base_url, &model_name).await;

    let needs_pull = !model_exists;

    if needs_pull {
        let app_bg = app.clone();
        tokio::spawn(async move {
            let state_ref = app_bg.state::<AppState>();
            pull_model_and_finalize(&app_bg, &state_ref, &base_url, &model_name, first_pull_done)
                .await;
        });

        return Ok(true);
    }

    // Model already exists — finalize synchronously
    // 9. Mark model ready
    {
        let mut guard = state.ollama.lock().await;
        if let Some(ref mut mgr) = *guard {
            mgr.mark_model_ready();
        }
    }

    // 10. Auto-configure local_llm_base_url if not already set
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        let should_set = config.local_llm_base_url.is_none()
            || config
                .local_llm_base_url
                .as_deref()
                .is_none_or(|u| u.is_empty());
        if should_set {
            config.local_llm_base_url = Some(base_url);
        }
        if !first_pull_done {
            config.first_model_pull_complete = true;
        }
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }

    // 11. Rebuild router to pick up the local LLM URL
    if let Err(e) = crate::rebuild_router(&state).await {
        tracing::warn!("Failed to rebuild router after Ollama start: {}", e);
    }

    let _ = app.emit("ollama-lifecycle", OllamaLifecycleState::ModelReady);
    Ok(false)
}

/// Check whether a model is already available on a running Ollama server.
async fn check_model_exists_on_server(base_url: &str, model_name: &str) -> bool {
    let client = reqwest::Client::new();
    let tags_url = format!("{}/api/tags", base_url);
    match client.get(&tags_url).send().await {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                body.get("models")
                    .and_then(|m| m.as_array())
                    .map(|models| {
                        models.iter().any(|m| {
                            m.get("name").and_then(|n| n.as_str()).is_some_and(|name| {
                                name == model_name
                                    || name == format!("{}:latest", model_name)
                                    || name.starts_with(&format!("{}:", model_name))
                            })
                        })
                    })
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Background helper: pull a model, configure `local_llm_base_url`, rebuild
/// the router, and emit `ModelReady`.  Used by both the fresh-install path and
/// the normal "Ollama exists but model is missing" path.
async fn pull_model_and_finalize(
    app: &tauri::AppHandle,
    state: &AppState,
    base_url: &str,
    model_name: &str,
    first_pull_done: bool,
) {
    let base_url = base_url.to_string();
    let model_name = model_name.to_string();
    let app_bg = app.clone();

    let pull_result = OllamaManager::pull_model_streaming(&base_url, &model_name, |progress| {
        let pct = match (progress.completed, progress.total) {
            (Some(c), Some(t)) if t > 0 => (c as f32 / t as f32) * 100.0,
            _ => 0.0,
        };
        let _ = app_bg.emit(
            "ollama-lifecycle",
            OllamaLifecycleState::PullingModel { progress_pct: pct },
        );
        let _ = app_bg.emit(
            "ollama-model-progress",
            OllamaModelProgress {
                model: model_name.clone(),
                completed: progress.completed,
                total: progress.total,
                status: progress.status.clone(),
            },
        );
    })
    .await;

    if let Err(e) = pull_result {
        tracing::error!("Background model pull failed: {}", e);
        let _ = app.emit(
            "ollama-lifecycle",
            OllamaLifecycleState::Error(e.to_string()),
        );
        return;
    }

    {
        let mut guard = state.ollama.lock().await;
        if let Some(ref mut mgr) = *guard {
            mgr.mark_model_ready();
        }
    }

    if let Ok(mut config) = state.config.write() {
        let should_set = config.local_llm_base_url.is_none()
            || config
                .local_llm_base_url
                .as_deref()
                .is_none_or(|u| u.is_empty());
        if should_set {
            config.local_llm_base_url = Some(base_url.clone());
        }
        if !first_pull_done {
            config.first_model_pull_complete = true;
        }
        let _ = config.save(&config.config_path());
    }

    if let Err(e) = crate::rebuild_router_from_handle(app).await {
        tracing::warn!("Failed to rebuild router after Ollama start: {}", e);
    }

    let _ = app.emit("ollama-lifecycle", OllamaLifecycleState::ModelReady);
}

/// Send a lightweight generate request to Ollama so the model loads into memory.
/// This avoids a cold-start delay on the first real chat request.
#[tauri::command]
pub async fn warmup_ollama_model(state: State<'_, AppState>) -> Result<(), String> {
    let base_url = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config
            .local_llm_base_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string())
    };
    let model_name = {
        let config = state.config.read().map_err(|e| e.to_string())?;
        config
            .bundled_model
            .clone()
            .unwrap_or_else(|| "llama3.2:3b".to_string())
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // Use /api/generate with a trivial prompt and num_predict=1 to load the model
    // into memory without generating a full response.
    let resp = client
        .post(format!("{}/api/generate", base_url))
        .json(&serde_json::json!({
            "model": model_name,
            "prompt": "hi",
            "stream": false,
            "options": { "num_predict": 1 }
        }))
        .send()
        .await
        .map_err(|e| format!("Warmup request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Warmup returned status {}", resp.status()));
    }

    tracing::info!("Ollama model {} warmed up successfully", model_name);
    Ok(())
}

/// Return a snapshot of local_llm_base_url and bundled_model for the Hive agent panel.
#[tauri::command]
pub fn get_config_snapshot(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "local_llm_base_url": config.local_llm_base_url,
        "bundled_model": config.bundled_model,
    }))
}

/// Switch the active bundled model used by the Hive agent.
#[tauri::command]
pub async fn set_bundled_model(
    model_name: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        config.bundled_model = Some(model_name.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
    }
    tracing::info!("Hive agent model set to {}", model_name);
    Ok(())
}
