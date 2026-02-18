//! Manages the lifecycle of a system-installed Ollama instance.
//!
//! On Windows, Abigail discovers Ollama from standard install paths or PATH,
//! starts it as a child process when needed, ensures a default model is
//! available, and shuts it down on app exit.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Status of the managed Ollama instance, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaStatus {
    /// Whether Abigail spawned/manages the Ollama process
    pub managed: bool,
    /// Whether Ollama is currently responding
    pub running: bool,
    /// Port Ollama is listening on
    pub port: u16,
    /// Whether the target model is available
    pub model_ready: bool,
}

/// Detection result for Ollama install/run status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaDetection {
    /// "running", "installed", or "not_found"
    pub status: String,
    /// Full path to the discovered ollama executable, when available.
    pub path: Option<String>,
}

/// Progress payload for install steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaInstallProgress {
    pub step: String,
    pub written: Option<u64>,
    pub total: Option<u64>,
    pub message: String,
}

/// Curated model options for first-run UX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedModel {
    pub name: String,
    pub label: String,
    pub size_bytes: u64,
    pub description: String,
    pub recommended: bool,
}

/// Progress payload from streaming `/api/pull`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModelProgress {
    pub model: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub status: String,
}

/// Manages a local Ollama process.
pub struct OllamaManager {
    /// Child process handle (Some if we spawned it)
    child: Option<tokio::process::Child>,
    /// Port Ollama is listening on
    port: u16,
    /// Path to the ollama binary
    ollama_exe: PathBuf,
    /// Directory for Ollama model storage
    models_dir: PathBuf,
    /// Whether the desired model is ready
    model_ready: bool,
}

impl OllamaManager {
    /// Discover a system Ollama binary and start it.
    ///
    /// `data_dir` is used for model storage.
    pub async fn discover_and_start(data_dir: &Path) -> Result<Self, String> {
        let models_dir = data_dir.join("ollama_models");
        let ollama_exe = Self::find_ollama_binary()
            .ok_or_else(|| "Ollama binary not found (not installed and not in PATH)".to_string())?;
        tracing::info!("Found Ollama at {}", ollama_exe.display());
        let mut mgr = Self {
            child: None,
            port: 11434,
            ollama_exe,
            models_dir,
            model_ready: false,
        };
        mgr.start().await?;
        Ok(mgr)
    }

    /// Detect whether Ollama is running, installed, or missing.
    pub async fn detect_ollama() -> OllamaDetection {
        if Self::probe_health_static(11434).await {
            return OllamaDetection {
                status: "running".to_string(),
                path: Self::find_ollama_binary().map(|p| p.display().to_string()),
            };
        }

        if let Some(path) = Self::find_ollama_binary() {
            return OllamaDetection {
                status: "installed".to_string(),
                path: Some(path.display().to_string()),
            };
        }

        OllamaDetection {
            status: "not_found".to_string(),
            path: None,
        }
    }

    /// Download and silently run the official Ollama installer.
    pub async fn download_and_install<F>(mut on_progress: F) -> Result<(), String>
    where
        F: FnMut(OllamaInstallProgress),
    {
        #[cfg(not(windows))]
        {
            let _ = &on_progress;
            return Err(
                "Automatic Ollama install is currently supported on Windows only".to_string(),
            );
        }

        #[cfg(windows)]
        {
            let installer_url = "https://ollama.com/download/OllamaSetup.exe";
            let installer_path = std::env::temp_dir().join("OllamaSetup.exe");

            on_progress(OllamaInstallProgress {
                step: "downloading".to_string(),
                written: Some(0),
                total: None,
                message: "Downloading Ollama installer...".to_string(),
            });

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30 * 60))
                .build()
                .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

            let mut response = client
                .get(installer_url)
                .send()
                .await
                .map_err(|e| format!("Failed to download installer: {}", e))?;

            if !response.status().is_success() {
                return Err(format!(
                    "Failed to download installer: HTTP {}",
                    response.status()
                ));
            }

            let total = response.content_length();
            let mut written: u64 = 0;
            let mut file = tokio::fs::File::create(&installer_path)
                .await
                .map_err(|e| format!("Failed to create installer file: {}", e))?;

            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| format!("Failed while downloading installer: {}", e))?
            {
                file.write_all(&chunk)
                    .await
                    .map_err(|e| format!("Failed writing installer file: {}", e))?;
                written = written.saturating_add(chunk.len() as u64);
                on_progress(OllamaInstallProgress {
                    step: "downloading".to_string(),
                    written: Some(written),
                    total,
                    message: "Downloading Ollama installer...".to_string(),
                });
            }

            file.flush()
                .await
                .map_err(|e| format!("Failed to flush installer file: {}", e))?;

            on_progress(OllamaInstallProgress {
                step: "installing".to_string(),
                written: total,
                total,
                message: "Running Ollama installer...".to_string(),
            });

            let mut cmd = tokio::process::Command::new(&installer_path);
            cmd.arg("/S");

            // Hide installer console flashes.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);

            let status = cmd
                .status()
                .await
                .map_err(|e| format!("Failed to start Ollama installer: {}", e))?;

            if !status.success() {
                return Err(format!("Ollama installer exited with status: {}", status));
            }

            on_progress(OllamaInstallProgress {
                step: "waiting_for_service".to_string(),
                written: total,
                total,
                message: "Waiting for Ollama service to start...".to_string(),
            });

            let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
            loop {
                if Self::probe_health_static(11434).await {
                    on_progress(OllamaInstallProgress {
                        step: "complete".to_string(),
                        written: total,
                        total,
                        message: "Ollama is ready.".to_string(),
                    });
                    let _ = tokio::fs::remove_file(&installer_path).await;
                    return Ok(());
                }

                if tokio::time::Instant::now() > deadline {
                    return Err("Ollama installed, but service did not start in time".to_string());
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    /// Curated model list for first-run setup.
    pub fn list_recommended_models() -> Vec<RecommendedModel> {
        vec![
            RecommendedModel {
                name: "qwen2.5:0.5b".to_string(),
                label: "Small".to_string(),
                size_bytes: 400 * 1024 * 1024,
                description: "Fast and lightweight for basic local tasks.".to_string(),
                recommended: true,
            },
            RecommendedModel {
                name: "phi3:mini".to_string(),
                label: "Medium".to_string(),
                size_bytes: 2300 * 1024 * 1024,
                description: "Balanced quality and speed for most users.".to_string(),
                recommended: false,
            },
            RecommendedModel {
                name: "llama3.2:3b".to_string(),
                label: "Large".to_string(),
                size_bytes: 2000 * 1024 * 1024,
                description: "Higher quality responses with higher resource use.".to_string(),
                recommended: false,
            },
            RecommendedModel {
                name: "mistral:7b".to_string(),
                label: "XL".to_string(),
                size_bytes: 4100 * 1024 * 1024,
                description: "Stronger reasoning; requires more RAM and disk.".to_string(),
                recommended: false,
            },
        ]
    }

    /// Pull a model and emit incremental progress updates.
    pub async fn pull_model_streaming<F>(
        base_url: &str,
        model_name: &str,
        mut on_progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(OllamaModelProgress),
    {
        let base = base_url.trim_end_matches('/');
        let pull_url = format!("{}/api/pull", base);
        let body = serde_json::json!({
            "name": model_name,
            "stream": true,
        });

        on_progress(OllamaModelProgress {
            model: model_name.to_string(),
            completed: None,
            total: None,
            status: "starting".to_string(),
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60 * 60))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        let mut response = client
            .post(&pull_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to request model pull: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Failed to pull model '{}': HTTP {} - {}",
                model_name, status, body
            ));
        }

        let mut buffer = String::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Failed while reading model pull stream: {}", e))?
        {
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline_idx) = buffer.find('\n') {
                let line = buffer[..newline_idx].trim().to_string();
                buffer = buffer[newline_idx + 1..].to_string();
                if line.is_empty() {
                    continue;
                }
                Self::emit_pull_line(model_name, &line, &mut on_progress);
            }
        }

        if !buffer.trim().is_empty() {
            Self::emit_pull_line(model_name, buffer.trim(), &mut on_progress);
        }

        on_progress(OllamaModelProgress {
            model: model_name.to_string(),
            completed: None,
            total: None,
            status: "complete".to_string(),
        });
        Ok(())
    }

    fn emit_pull_line<F>(model_name: &str, line: &str, on_progress: &mut F)
    where
        F: FnMut(OllamaModelProgress),
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let status = v
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("downloading")
                .to_string();
            let completed = v.get("completed").and_then(|c| c.as_u64());
            let total = v.get("total").and_then(|t| t.as_u64());
            on_progress(OllamaModelProgress {
                model: model_name.to_string(),
                completed,
                total,
                status,
            });
            return;
        }

        on_progress(OllamaModelProgress {
            model: model_name.to_string(),
            completed: None,
            total: None,
            status: line.to_string(),
        });
    }

    fn find_ollama_binary() -> Option<PathBuf> {
        // Prefer common Windows install paths first.
        #[cfg(windows)]
        {
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                candidates.push(
                    PathBuf::from(local)
                        .join("Programs")
                        .join("Ollama")
                        .join("ollama.exe"),
                );
            }
            if let Some(program_files) = std::env::var_os("PROGRAMFILES") {
                candidates.push(
                    PathBuf::from(program_files)
                        .join("Ollama")
                        .join("ollama.exe"),
                );
            }
            if let Some(program_w6432) = std::env::var_os("ProgramW6432") {
                candidates.push(
                    PathBuf::from(program_w6432)
                        .join("Ollama")
                        .join("ollama.exe"),
                );
            }
            for candidate in candidates {
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        // Fallback to PATH lookup.
        which::which("ollama").ok()
    }

    async fn probe_health_static(port: u16) -> bool {
        let url = format!("http://127.0.0.1:{}/api/tags", port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_default();
        matches!(client.get(&url).send().await, Ok(r) if r.status().is_success())
    }

    /// Start the Ollama server, or attach to an existing one.
    async fn start(&mut self) -> Result<(), String> {
        // Check if port 11434 already has Ollama running
        if self.probe_health(11434).await {
            tracing::info!("Ollama already running on port 11434, reusing");
            self.port = 11434;
            // Don't spawn a child — we didn't start it
            return Ok(());
        }

        // Port 11434 might be taken by something else; try it first, then 11435
        let port = if Self::port_available(11434).await {
            11434
        } else if Self::port_available(11435).await {
            tracing::info!("Port 11434 taken by non-Ollama process, trying 11435");
            11435
        } else {
            return Err("Ports 11434 and 11435 are both in use".into());
        };

        self.port = port;

        // Ensure models directory exists
        std::fs::create_dir_all(&self.models_dir).map_err(|e| {
            format!(
                "Failed to create models dir {}: {}",
                self.models_dir.display(),
                e
            )
        })?;

        // Spawn ollama serve
        tracing::info!(
            "Starting Ollama: {} serve (port {}, models {})",
            self.ollama_exe.display(),
            port,
            self.models_dir.display()
        );

        let mut cmd = tokio::process::Command::new(&self.ollama_exe);
        cmd.arg("serve")
            .env("OLLAMA_HOST", format!("127.0.0.1:{}", port))
            .env("OLLAMA_MODELS", &self.models_dir);

        // Windows: hide the console window
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let child = cmd
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn Ollama: {}", e))?;

        self.child = Some(child);

        // Wait for health endpoint (up to 15 seconds)
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if tokio::time::Instant::now() > deadline {
                // Check if the child process died
                if let Some(ref mut child) = self.child {
                    if let Ok(Some(status)) = child.try_wait() {
                        return Err(format!("Ollama process exited with status: {}", status));
                    }
                }
                return Err("Ollama failed to start within 15 seconds".into());
            }

            if self.probe_health(port).await {
                tracing::info!("Bundled Ollama started on port {}", port);
                return Ok(());
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Check if a model is available; if not, pull it.
    pub async fn ensure_model(&mut self, model_name: &str) -> Result<(), String> {
        let base = self.base_url();

        // Check /api/tags for existing models
        let client = reqwest::Client::new();
        let tags_url = format!("{}/api/tags", base);

        match client.get(&tags_url).send().await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(models) = body.get("models").and_then(|m| m.as_array()) {
                        for m in models {
                            if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                                // Ollama returns names like "qwen2.5:0.5b" or "qwen2.5:0.5b:latest"
                                let matches = name == model_name
                                    || name.starts_with(&format!("{}-", model_name))
                                    || name == format!("{}:latest", model_name);
                                if matches {
                                    tracing::info!("Model '{}' already available", model_name);
                                    self.model_ready = true;
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                return Err(format!("Failed to query Ollama models: {}", e));
            }
        }

        // Model not found — pull it
        tracing::info!("Pulling model '{}' (this may take a while)...", model_name);
        let pull_url = format!("{}/api/pull", base);
        let pull_body = serde_json::json!({ "name": model_name, "stream": false });

        match client
            .post(&pull_url)
            .json(&pull_body)
            .timeout(Duration::from_secs(600)) // 10 min timeout for model download
            .send()
            .await
        {
            Ok(resp) => {
                if resp.status().is_success() {
                    tracing::info!("Model '{}' pulled successfully", model_name);
                    self.model_ready = true;
                    Ok(())
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    Err(format!(
                        "Failed to pull model '{}': HTTP {} — {}",
                        model_name, status, body
                    ))
                }
            }
            Err(e) => Err(format!("Failed to pull model '{}': {}", model_name, e)),
        }
    }

    /// Returns the Ollama base URL.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Returns the current status for the frontend.
    pub fn status(&self) -> OllamaStatus {
        OllamaStatus {
            managed: self.child.is_some(),
            running: true, // if we got here, Ollama responded to health check
            port: self.port,
            model_ready: self.model_ready,
        }
    }

    /// Mark model readiness after a successful pull via frontend command flow.
    pub fn mark_model_ready(&mut self) {
        self.model_ready = true;
    }

    /// Shut down the managed Ollama process (if we spawned it).
    pub fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            tracing::info!("Shutting down managed Ollama process");
            let _ = child.start_kill();
        }
    }

    /// Probe the health endpoint on the given port.
    async fn probe_health(&self, port: u16) -> bool {
        let url = format!("http://127.0.0.1:{}/api/tags", port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        matches!(client.get(&url).send().await, Ok(r) if r.status().is_success())
    }

    /// Check if a TCP port is available (not in use).
    async fn port_available(port: u16) -> bool {
        tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .is_err()
    }
}

impl Drop for OllamaManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
