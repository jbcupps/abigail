//! Manages the lifecycle of a bundled or system-installed Ollama instance.
//!
//! On Windows, Abigail bundles `ollama.exe` as a Tauri resource. This module
//! discovers the binary (bundled or system PATH), starts it as a child process,
//! ensures a default model is available, and shuts it down on app exit.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Manages a bundled or system Ollama process.
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
    /// Discover an Ollama binary and start it.
    ///
    /// Looks in `resource_dir` first (bundled), then system PATH.
    /// `data_dir` is used for model storage.
    pub async fn discover_and_start(resource_dir: &Path, data_dir: &Path) -> Result<Self, String> {
        let models_dir = data_dir.join("ollama_models");

        // 1. Check bundled location
        let bundled = resource_dir.join("ollama").join("ollama.exe");
        if bundled.exists() {
            tracing::info!("Found bundled Ollama at {}", bundled.display());
            let mut mgr = Self {
                child: None,
                port: 11434,
                ollama_exe: bundled,
                models_dir,
                model_ready: false,
            };
            mgr.start().await?;
            return Ok(mgr);
        }

        // 2. Check resource_dir root (CI puts it there)
        let resource_root = resource_dir.join("ollama.exe");
        if resource_root.exists() {
            tracing::info!("Found Ollama at resource root: {}", resource_root.display());
            let mut mgr = Self {
                child: None,
                port: 11434,
                ollama_exe: resource_root,
                models_dir,
                model_ready: false,
            };
            mgr.start().await?;
            return Ok(mgr);
        }

        // 3. Check system PATH
        if let Ok(which) = which::which("ollama") {
            tracing::info!("Found system Ollama at {}", which.display());
            let mut mgr = Self {
                child: None,
                port: 11434,
                ollama_exe: which,
                models_dir,
                model_ready: false,
            };
            mgr.start().await?;
            return Ok(mgr);
        }

        Err("Ollama binary not found (not bundled, not in PATH)".into())
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
