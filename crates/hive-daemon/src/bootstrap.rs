//! Offline, narrowly scoped Abigail setup assistant. No shell or model-driven mutations.
use crate::state::HiveDaemonState;
use abigail_capabilities::cognitive::Message;
use abigail_persistence::{EntityScope, PersistenceHandle};
use axum::{extract::State, Json};
use hive_core::ApiEnvelope;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};

const CONNECTION_KEY: &str = "abigail_setup_connection";
const SYSTEM: &str = "You are Abigail, the assistant in this application. Help with initial setup only. Reply in plain text with at most three short sentences. Use the verified examples below. Do not invent steps or claim you changed anything. An API account is an account on a provider's developer website; an API key is a secret created inside that account. These are different things. Abigail does not create provider accounts. The person creates their account and key on the provider's website, then enters the key and a model ID in Abigail's Connect a model form. Never ask for secrets in chat. The form tests the chosen model before saving its key encrypted. You have no tools and cannot change settings yourself. Local chat works offline without any account. Cloud replies send recent context to the selected provider. The same Abigail identity and conversation continue when the model changes. If unsure, say so.";

fn setup_examples() -> Vec<Message> {
    vec![
        Message::new("user", "What is an API account?"),
        Message::new("assistant", "It is a developer account you create on Anthropic's or OpenAI's website so an app like Abigail can use their models. API usage usually has its own billing. It is separate from the secret API key you create within that account."),
        Message::new("user", "How do I connect a stronger model?"),
        Message::new("assistant", "First create an API key on your provider's developer website. Then choose Connect a model here, enter that key in the secure form, and choose a model. Test and connect checks a real reply before saving the connection."),
        Message::new("user", "Do I need to pay or make an account to talk here now?"),
        Message::new("assistant", "No. We can talk with the packaged local model offline, without an account. Connecting a cloud API is optional and may have separate usage charges."),
    ]
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Connection {
    pub provider: String,
    pub model: String,
    api_key: String,
    #[cfg(test)]
    #[serde(skip)]
    endpoint_override: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct Conversation {
    messages: Vec<Message>,
}

#[derive(Clone, Serialize)]
pub struct SetupStatus {
    pub phase: String,
    pub message: String,
    pub model: String,
    pub active_provider: String,
    pub active_model: String,
}

#[derive(Deserialize)]
struct Bundle {
    model: String,
    // Relative paths only. The installer carries the runtime, model, and licenses.
    runtime: String,
    models: String,
}

pub struct Bootstrap {
    status: RwLock<SetupStatus>,
    local_url: RwLock<Option<String>>,
    active: RwLock<Option<Connection>>,
    conversation: Mutex<Conversation>,
    store: PersistenceHandle,
    bundle_dir: PathBuf,
    // Serializes restart/cancel and keeps the managed runtime alive.
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    runtime: Mutex<Option<tokio::process::Child>>,
    // Serializes activation with chat, so a turn has one unambiguous provider.
    turn: Mutex<()>,
}

impl Bootstrap {
    pub fn open(
        state_root: &std::path::Path,
        entity_id: &str,
        active: Option<Connection>,
    ) -> anyhow::Result<Arc<Self>> {
        let store = PersistenceHandle::open_local(
            abigail_identity::HiveEntity::memory_db_path(state_root),
            EntityScope::Entity(entity_id.to_string()),
        )?;
        let conversation = store
            .select_record("birth", "setup_conversation")?
            .unwrap_or_default();
        let bundle_dir = std::env::var_os("ABIGAIL_BOOTSTRAP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap_or_default()
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("bootstrap")
            });
        Ok(Arc::new(Self {
            status: RwLock::new(SetupStatus {
                phase: "pending".into(),
                message: "Preparing Abigail…".into(),
                model: String::new(),
                active_provider: "local".into(),
                active_model: String::new(),
            }),
            local_url: RwLock::new(None),
            active: RwLock::new(active),
            conversation: Mutex::new(conversation),
            store,
            bundle_dir,
            worker: Mutex::new(None),
            runtime: Mutex::new(None),
            turn: Mutex::new(()),
        }))
    }

    pub fn saved_connection(
        vault: &abigail_core::SecretsVault,
    ) -> anyhow::Result<Option<Connection>> {
        vault
            .get_secret(CONNECTION_KEY)
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }

    pub async fn snapshot(&self) -> SetupStatus {
        let mut status = self.status.read().await.clone();
        if let Some(active) = self.active.read().await.as_ref() {
            status.active_provider = active.provider.clone();
            status.active_model = active.model.clone();
        } else {
            status.active_model = status.model.clone();
        }
        status
    }

    async fn phase(&self, phase: &str, message: &str) {
        let mut status = self.status.write().await;
        status.phase = phase.into();
        status.message = message.into();
    }

    pub async fn start(self: &Arc<Self>) {
        let mut worker = self.worker.lock().await;
        if worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return;
        }
        if self.status.read().await.phase == "ready" {
            return;
        }
        self.phase(
            "loading",
            "Loading your local Abigail. This works without internet…",
        )
        .await;
        let this = self.clone();
        *worker = Some(tokio::spawn(async move {
            if let Err(error) = this.load().await {
                this.phase("error", &error.to_string()).await;
                if let Some(mut child) = this.runtime.lock().await.take() {
                    let _ = child.kill().await;
                }
                *this.local_url.write().await = None;
            }
        }));
    }

    async fn load(&self) -> anyhow::Result<()> {
        let bundle: Bundle = serde_json::from_slice(&tokio::fs::read(self.bundle_dir.join("bundle.json")).await
            .map_err(|_| anyhow::anyhow!("The offline model package is missing. Install the full Abigail installer, then Retry."))?)?;
        anyhow::ensure!(
            !bundle.model.is_empty() && !bundle.model.contains("cloud"),
            "The packaged model must run locally."
        );
        let runtime = bundled_path(&self.bundle_dir, &bundle.runtime)?;
        let models = bundled_path(&self.bundle_dir, &bundle.models)?;
        anyhow::ensure!(
            runtime.is_file() && models.is_dir(),
            "The offline model package is incomplete. Reinstall Abigail, then Retry."
        );
        self.status.write().await.model = bundle.model.clone();
        // A dedicated port/model directory avoids adopting a stranger's unauthenticated
        // localhost Ollama, and never changes the user's independently installed Ollama.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        drop(listener);
        let url = format!("http://{address}");
        let mut command = tokio::process::Command::new(runtime);
        command
            .arg("serve")
            .env("OLLAMA_HOST", address.to_string())
            .env("OLLAMA_MODELS", models)
            .env("OLLAMA_NO_CLOUD", "1")
            .env("OLLAMA_ORIGINS", "")
            .env("OLLAMA_CONTEXT_LENGTH", "4096")
            .env("OLLAMA_KEEP_ALIVE", "-1")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        *self.runtime.lock().await = Some(command.spawn().map_err(|_| {
            anyhow::anyhow!(
                "The packaged local runtime could not start. Retry or reinstall Abigail."
            )
        })?);
        let client = local_client()?;
        let mut listening = false;
        for _ in 0..100 {
            if self
                .runtime
                .lock()
                .await
                .as_mut()
                .unwrap()
                .try_wait()?
                .is_some()
            {
                anyhow::bail!(
                    "The local runtime stopped during startup. Retry or reinstall Abigail."
                );
            }
            if client
                .get(format!("{url}/api/tags"))
                .timeout(Duration::from_secs(1))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                listening = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        anyhow::ensure!(
            listening,
            "The local runtime did not start in time. Retry to start it again."
        );
        // A real nonempty generated completion, not /health, makes chat ready.
        local_completion(
            &url,
            &bundle.model,
            &[Message::new("user", "Say hello in one short sentence.")],
        )
        .await?;
        *self.local_url.write().await = Some(url);
        self.phase("ready", "Abigail is ready on this computer.")
            .await;
        Ok(())
    }

    pub async fn cancel(&self) {
        let mut worker = self.worker.lock().await;
        if self.status.read().await.phase == "ready" {
            return;
        }
        if let Some(task) = worker.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(mut child) = self.runtime.lock().await.take() {
            let _ = child.kill().await;
        }
        *self.local_url.write().await = None;
        self.phase("cancelled", "Startup paused. Retry whenever you are ready.")
            .await;
    }

    pub async fn provider_config(&self) -> Option<hive_core::ProviderConfig> {
        let active = self.active.read().await.clone();
        let url = self.local_url.read().await.clone();
        if active.is_none() && url.is_none() {
            return None;
        }
        Some(hive_core::ProviderConfig {
            local_llm_base_url: url,
            ego_provider_name: active.as_ref().map(|c| c.provider.clone()),
            ego_api_key: active.as_ref().map(|c| c.api_key.clone()),
            ego_model: active.map(|c| c.model),
            routing_mode: "EgoPrimary".into(),
            cli_permission_mode: Some("allowlist_only".into()),
        })
    }
}

fn bundled_path(root: &std::path::Path, relative: &str) -> anyhow::Result<PathBuf> {
    // Bundle manifests use portable forward-slash paths. Reject Windows drive
    // and separator syntax on every host, including Linux/macOS validation.
    anyhow::ensure!(
        !relative.contains('\\') && !relative.contains(':'),
        "Invalid offline package path"
    );
    let relative = std::path::Path::new(relative);
    anyhow::ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Invalid offline package path"
    );
    Ok(root.join(relative))
}

fn local_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(180))
        .build()?)
}

async fn local_completion(url: &str, model: &str, messages: &[Message]) -> anyhow::Result<String> {
    let response = local_client()?
        .post(format!("{url}/api/chat"))
        .json(&serde_json::json!({
            "model": model, "messages": messages, "stream": false, "think": false,
            "keep_alive": -1, "options": { "num_ctx": 4096, "num_predict": 512 }
        }))
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("The local model is unavailable or took too long. Retry startup.")
        })?;
    anyhow::ensure!(response.status().is_success(), "The packaged model could not load. Check available memory or reinstall Abigail, then Retry.");
    let value: serde_json::Value = response.json().await?;
    let content = value["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .trim();
    anyhow::ensure!(
        !content.is_empty() && value["done"] == true,
        "The local model did not finish a reply. Retry startup."
    );
    Ok(content.to_string())
}

// Fixed provider endpoints; credentials never enter messages, logs, or URLs.
async fn cloud_completion(connection: &Connection, messages: &[Message]) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(90))
        .build()?;
    let endpoint = match connection.provider.as_str() {
        "openai" => "https://api.openai.com/v1/chat/completions",
        "anthropic" => "https://api.anthropic.com/v1/messages",
        _ => anyhow::bail!("Choose OpenAI or Anthropic for initial setup."),
    };
    #[cfg(test)]
    let endpoint = connection.endpoint_override.as_deref().unwrap_or(endpoint);
    let response = match connection.provider.as_str() {
        "openai" => client.post(endpoint)
            .bearer_auth(&connection.api_key).json(&serde_json::json!({"model":connection.model,"messages":messages,"max_completion_tokens":1024})),
        "anthropic" => client.post(endpoint)
            .header("x-api-key", &connection.api_key).header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({"model":connection.model,"max_tokens":1024,
                "system":messages.iter().filter(|m| m.role=="system").map(|m|m.content.as_str()).collect::<Vec<_>>().join("\n"),
                "messages":messages.iter().filter(|m|m.role!="system").collect::<Vec<_>>()})),
        _ => anyhow::bail!("Choose OpenAI or Anthropic for initial setup."),
    }.send().await.map_err(|_| anyhow::anyhow!("Could not reach the provider. Check your connection and retry, or continue locally."))?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "Provider returned HTTP {}. Check the API key, model access, and API billing, then retry. Your previous connection is still available.", status.as_u16());
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| anyhow::anyhow!("The provider returned an unreadable response."))?;
    let reply = if connection.provider == "anthropic" {
        value["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    } else {
        value["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    anyhow::ensure!(
        !reply.trim().is_empty(),
        "The model returned no text. Choose a chat model and retry."
    );
    Ok(reply)
}

pub async fn discover_models(
    provider: &str,
    key: &str,
) -> anyhow::Result<Vec<hive_core::ProviderModelInfo>> {
    anyhow::ensure!(
        !key.trim().is_empty() && key.len() <= 4096,
        "Enter an API key."
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    let request = match provider {
        "anthropic" => client
            .get("https://api.anthropic.com/v1/models?limit=1000")
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        "openai" => client
            .get("https://api.openai.com/v1/models")
            .bearer_auth(key),
        _ => anyhow::bail!("Choose OpenAI or Anthropic for initial setup."),
    };
    let response = request.send().await.map_err(|_| {
        anyhow::anyhow!("Could not reach the provider. Check your connection and retry.")
    })?;
    anyhow::ensure!(
        response.status().is_success(),
        "Model discovery returned HTTP {}. Check your API key and model access.",
        response.status().as_u16()
    );
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| anyhow::anyhow!("Unreadable model list"))?;
    let mut models = value["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("The provider returned no model list"))?
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?;
            if !abigail_capabilities::cognitive::validation::is_model_compatible_with_provider(
                provider, id,
            ) {
                return None;
            }
            Some(hive_core::ProviderModelInfo {
                model_id: id.into(),
                display_name: entry["display_name"].as_str().map(str::to_string),
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    Ok(models)
}

#[derive(Serialize)]
pub struct SetupConversation {
    pub messages: Vec<Message>,
}
#[derive(Deserialize)]
pub struct ChatInput {
    message: String,
}
#[derive(Deserialize)]
pub struct ActivateInput {
    provider: String,
    model: String,
    api_key: String,
}

pub async fn status(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<SetupStatus>> {
    Json(ApiEnvelope::success(state.bootstrap.snapshot().await))
}
pub async fn retry(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<SetupStatus>> {
    state.bootstrap.start().await;
    status(State(state)).await
}
pub async fn cancel(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<SetupStatus>> {
    state.bootstrap.cancel().await;
    status(State(state)).await
}
pub async fn history(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<SetupConversation>> {
    Json(ApiEnvelope::success(SetupConversation {
        messages: state.bootstrap.conversation.lock().await.messages.clone(),
    }))
}

fn looks_like_secret(text: &str) -> bool {
    ["sk-", "AIza", "xai-", "pplx-"].iter().any(|prefix| {
        text.split_whitespace()
            .any(|word| word.contains(prefix) && word.len() > 18)
    })
}

pub async fn chat(
    State(state): State<HiveDaemonState>,
    Json(body): Json<ChatInput>,
) -> Json<ApiEnvelope<SetupConversation>> {
    let setup = &state.bootstrap;
    let result = async {
        let _turn = setup.turn.try_lock().map_err(|_| anyhow::anyhow!("Abigail is finishing another request. Try again in a moment."))?;
        anyhow::ensure!(setup.status.read().await.phase == "ready", "Abigail's local model is not ready yet. Retry startup.");
        let text = body.message.trim();
        anyhow::ensure!(!text.is_empty() && text.len() <= 4000, "Write a message between 1 and 4,000 bytes.");
        let active = setup.active.read().await.clone();
        anyhow::ensure!(!looks_like_secret(text) && !active.as_ref().is_some_and(|c|text.contains(&c.api_key)), "Please put API keys in Connect a model, never in chat. This message was not saved or sent.");
        let mut conversation = setup.conversation.lock().await;
        let mut messages = vec![Message::new("system", format!("{SYSTEM}\nApplication status: {}.", if active.is_some() {"a cloud API connection is active"} else {"using the offline local model; no cloud connection is active"}))];
        messages.extend(setup_examples());
        // Bound the small model's context; the complete transcript stays on disk.
        let mut tail: Vec<Message> = Vec::new();
        let mut chars = 0;
        for message in conversation.messages.iter().rev() {
            chars += message.content.len();
            if chars > 6000 { break; }
            tail.push(message.clone());
        }
        tail.reverse();
        if tail.first().is_some_and(|m| m.role == "assistant") { tail.remove(0); }
        messages.extend(tail);
        messages.push(Message::new("user", text));
        let reply = match active {
            Some(connection) => cloud_completion(&connection, &messages).await?,
            None => {
                let url = setup.local_url.read().await.clone().ok_or_else(||anyhow::anyhow!("Local model is unavailable. Retry startup."))?;
                let model = setup.status.read().await.model.clone();
                match local_completion(&url, &model, &messages).await {
                    Ok(reply) => reply,
                    Err(error) => { setup.phase("error", &error.to_string()).await; return Err(error); }
                }
            },
        };
        let mut next = conversation.clone();
        next.messages.push(Message::new("user", text));
        next.messages.push(Message::new("assistant", reply));
        let store = setup.store.clone(); let persisted = next.clone();
        tokio::task::spawn_blocking(move ||store.upsert("birth", "setup_conversation", &persisted)).await??;
        *conversation = next;
        Ok::<_, anyhow::Error>(SetupConversation { messages: conversation.messages.clone() })
    }.await;
    Json(match result {
        Ok(value) => ApiEnvelope::success(value),
        Err(e) => ApiEnvelope::error(e.to_string()),
    })
}

pub async fn activate(
    State(state): State<HiveDaemonState>,
    Json(body): Json<ActivateInput>,
) -> Json<ApiEnvelope<SetupStatus>> {
    let result = async {
        let _turn =
            state.bootstrap.turn.try_lock().map_err(|_| {
                anyhow::anyhow!("Wait for Abigail's current reply before connecting.")
            })?;
        anyhow::ensure!(
            matches!(body.provider.as_str(), "openai" | "anthropic"),
            "Choose OpenAI or Anthropic."
        );
        anyhow::ensure!(
            !body.model.trim().is_empty()
                && body.model.len() <= 200
                && !body.api_key.trim().is_empty()
                && body.api_key.len() <= 4096,
            "Enter an API key and model."
        );
        let connection = Connection {
            provider: body.provider,
            model: body.model.trim().into(),
            api_key: body.api_key.trim().into(),
            #[cfg(test)]
            endpoint_override: None,
        };
        commit_connection(&state, connection).await?;
        Ok::<_, anyhow::Error>(state.bootstrap.snapshot().await)
    }
    .await;
    Json(match result {
        Ok(status) => ApiEnvelope::success(status),
        Err(e) => ApiEnvelope::error(e.to_string()),
    })
}

async fn commit_connection(state: &HiveDaemonState, connection: Connection) -> anyhow::Result<()> {
    // A real selected-model completion must pass before any durable mutation.
    cloud_completion(
        &connection,
        &[Message::new("user", "Reply with the word ready.")],
    )
    .await?;
    let encoded = serde_json::to_string(&connection)?;
    {
        let mut vault = state
            .hive_secrets
            .lock()
            .map_err(|_| anyhow::anyhow!("Secure storage is unavailable."))?;
        let previous = vault.get_secret(CONNECTION_KEY).map(str::to_string);
        vault.set_secret(CONNECTION_KEY, &encoded);
        if let Err(error) = vault.save() {
            if let Some(previous) = previous {
                vault.set_secret(CONNECTION_KEY, &previous);
            } else {
                vault.remove_secret(CONNECTION_KEY);
            }
            return Err(anyhow::anyhow!(
                "Could not save the connection securely: {error}"
            ));
        }
    }
    *state.bootstrap.active.write().await = Some(connection);
    Ok(())
}

pub async fn use_local(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<SetupStatus>> {
    let result = async {
        let _turn = state
            .bootstrap
            .turn
            .try_lock()
            .map_err(|_| anyhow::anyhow!("Wait for Abigail's current reply."))?;
        {
            let mut vault = state
                .hive_secrets
                .lock()
                .map_err(|_| anyhow::anyhow!("Secure storage is unavailable."))?;
            let previous = vault.get_secret(CONNECTION_KEY).map(str::to_string);
            vault.remove_secret(CONNECTION_KEY);
            if vault.save().is_err() {
                if let Some(previous) = previous {
                    vault.set_secret(CONNECTION_KEY, &previous);
                }
                anyhow::bail!("Could not save the local model selection.");
            }
        }
        *state.bootstrap.active.write().await = None;
        Ok::<_, anyhow::Error>(state.bootstrap.snapshot().await)
    }
    .await;
    Json(match result {
        Ok(status) => ApiEnvelope::success(status),
        Err(e) => ApiEnvelope::error(e.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    async fn mock_provider() -> (
        String,
        Arc<AtomicBool>,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let healthy = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let h = healthy.clone();
        let c = calls.clone();
        let app = axum::Router::new()
            .route("/api/chat", axum::routing::post(||async { Json(serde_json::json!({"done":true,"message":{"content":"Hello from your local Abigail"}})) }))
            .route("/cloud", axum::routing::post(move |Json(body): Json<serde_json::Value>| {
                let h = h.clone(); let c = c.clone();
                async move {
                    c.lock().await.push(body);
                    if h.load(Ordering::SeqCst) { (axum::http::StatusCode::OK, Json(serde_json::json!({"choices":[{"message":{"content":"Still Abigail, with your earlier context"}}]}))) }
                    else { (axum::http::StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"sk-test-private-credential-must-not-appear"}))) }
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, healthy, calls, task)
    }

    #[tokio::test]
    async fn validated_handoff_preserves_conversation_and_restarts_from_encrypted_connection() {
        let (state, id) = crate::routes::tests::build_state();
        let (url, healthy, calls, task) = mock_provider().await;
        *state.bootstrap.local_url.write().await = Some(url.clone());
        state
            .bootstrap
            .phase("ready", "test local completion ready")
            .await;
        let first = chat(
            State(state.clone()),
            Json(ChatInput {
                message: "My name is Taylor. Help me set up.".into(),
            }),
        )
        .await
        .0;
        assert!(first.ok);
        let connection = Connection {
            provider: "openai".into(),
            model: "test-chat".into(),
            api_key: "sk-test-private-credential-must-not-appear".into(),
            endpoint_override: Some(format!("{url}/cloud")),
        };
        commit_connection(&state, connection.clone()).await.unwrap();
        let next = chat(
            State(state.clone()),
            Json(ChatInput {
                message: "Do you still know my name?".into(),
            }),
        )
        .await
        .0;
        assert_eq!(next.data.unwrap().messages.len(), 4);
        let calls = calls.lock().await;
        assert!(calls[1].to_string().contains("Taylor"));
        assert!(!calls
            .iter()
            .any(|call| call.to_string().contains(&connection.api_key)));
        drop(calls);
        let root = state.bootstrap.store.path().parent().unwrap();
        let vault = abigail_core::SecretsVault::load(root.join("hive_secrets")).unwrap();
        let saved = Bootstrap::saved_connection(&vault).unwrap().unwrap();
        assert_eq!(saved.model, "test-chat");
        let encrypted = std::fs::read(root.join("hive_secrets/secrets.vault")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains(&connection.api_key));
        let restarted = Bootstrap::open(root, &id, Some(saved)).unwrap();
        assert_eq!(restarted.conversation.lock().await.messages.len(), 4);
        assert_eq!(restarted.snapshot().await.active_provider, "openai");
        healthy.store(false, Ordering::SeqCst);
        let mut rejected = connection;
        rejected.model = "rejected-model".into();
        let error = commit_connection(&state, rejected)
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("sk-test"));
        assert_eq!(state.bootstrap.snapshot().await.active_model, "test-chat");
        assert_eq!(state.bootstrap.conversation.lock().await.messages.len(), 4);
        assert!(use_local(State(state.clone())).await.0.ok);
        assert_eq!(state.bootstrap.snapshot().await.active_provider, "local");
        let vault = abigail_core::SecretsVault::load(root.join("hive_secrets")).unwrap();
        assert!(Bootstrap::saved_connection(&vault).unwrap().is_none());
        task.abort();
    }

    #[tokio::test]
    async fn anthropic_validation_uses_separate_system_and_key_header() {
        let captured = Arc::new(Mutex::new(None));
        let observer = captured.clone();
        let app = axum::Router::new().route("/messages", axum::routing::post(move |headers: axum::http::HeaderMap, Json(body): Json<serde_json::Value>| {
            let observer = observer.clone();
            async move {
                *observer.lock().await = Some((headers, body));
                Json(serde_json::json!({"content":[{"type":"text","text":"Hello from Claude"}]}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/messages", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let connection = Connection {
            provider: "anthropic".into(),
            model: "claude-test".into(),
            api_key: "fake-claude-key".into(),
            endpoint_override: Some(endpoint),
        };
        let result = cloud_completion(
            &connection,
            &[
                Message::new("system", "Setup guidance"),
                Message::new("user", "Hello"),
            ],
        )
        .await
        .unwrap();
        assert_eq!(result, "Hello from Claude");
        let captured = captured.lock().await;
        let (headers, body) = captured.as_ref().unwrap();
        assert_eq!(headers["x-api-key"], "fake-claude-key");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert_eq!(body["model"], "claude-test");
        assert_eq!(body["system"], "Setup guidance");
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(!body.to_string().contains("fake-claude-key"));
        server.abort();
    }

    #[tokio::test]
    async fn missing_bundle_cancel_and_retry_are_recoverable() {
        let (state, _) = crate::routes::tests::build_state();
        state.bootstrap.start().await;
        for _ in 0..50 {
            if state.bootstrap.snapshot().await.phase == "error" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(state.bootstrap.snapshot().await.phase, "error");
        state.bootstrap.cancel().await;
        assert_eq!(state.bootstrap.snapshot().await.phase, "cancelled");
        state.bootstrap.start().await;
        assert_eq!(state.bootstrap.snapshot().await.phase, "loading");
        state.bootstrap.cancel().await;
        assert_eq!(state.bootstrap.snapshot().await.phase, "cancelled");
        assert!(state.bootstrap.local_url.read().await.is_none());
    }
    #[test]
    fn bundle_paths_cannot_escape_installation() {
        for invalid in [
            "../ollama.exe",
            "/ollama.exe",
            "C:\\ollama.exe",
            "C:/ollama.exe",
            "C:ollama.exe",
            "..\\ollama.exe",
            "ollama/../../ollama.exe",
            "",
        ] {
            assert!(
                bundled_path(std::path::Path::new("package"), invalid).is_err(),
                "accepted invalid package path: {invalid}"
            );
        }
        assert!(bundled_path(std::path::Path::new("package"), "ollama/ollama.exe").is_ok());
    }
    #[test]
    fn common_credentials_are_rejected_from_chat() {
        assert!(looks_like_secret("here is sk-ant-api03-privatecredential"));
        assert!(!looks_like_secret("Where do I get an API key?"));
    }
    #[tokio::test]
    async fn health_alone_cannot_make_model_ready() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(|| async {
                Json(serde_json::json!({"done":true,"message":{"content":""}}))
            }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(local_completion(&url, "test", &[]).await.is_err());
        task.abort();
    }
}
