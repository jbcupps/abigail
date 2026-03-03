//! HTTP client for entity-daemon.

use entity_core::{ChatRequest, ChatResponse, SkillInfo, ToolExecRequest, ToolExecResponse};
use futures_util::StreamExt;
use hive_core::ApiEnvelope;

/// SSE event received from the entity-daemon streaming chat endpoint.
#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    Token(String),
    Done(Box<ChatResponse>),
    Error(String),
}

/// HTTP client wrapping all entity-daemon REST endpoints.
#[derive(Clone)]
pub struct EntityClient {
    base_url: String,
    client: reqwest::Client,
}

impl EntityClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn health(&self) -> anyhow::Result<bool> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn status(&self) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!("{}/v1/status", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    // ── Chat ────────────────────────────────────────────────────────

    pub async fn chat(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let resp: ApiEnvelope<ChatResponse> = self
            .client
            .post(format!("{}/v1/chat", self.base_url))
            .json(request)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    /// Open an SSE stream to `POST /v1/chat/stream`.
    /// Returns a channel receiver that yields `ChatStreamEvent`s.
    pub async fn chat_stream(
        &self,
        request: &ChatRequest,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<ChatStreamEvent>> {
        let resp = self
            .client
            .post(format!("{}/v1/chat/stream", self.base_url))
            .json(request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("chat_stream failed: {} {}", status, body);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let mut stream = resp.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(ChatStreamEvent::Error(e.to_string())).await;
                        break;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buffer.find("\n\n") {
                    let block = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();

                    let event = parse_sse_block(&block);
                    if let Some(ev) = event {
                        if tx.send(ev).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    pub async fn cancel_chat_stream(&self) -> anyhow::Result<bool> {
        let resp: serde_json::Value = self
            .client
            .post(format!("{}/v1/chat/cancel", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp["cancelled"].as_bool().unwrap_or(false))
    }

    // ── Skills ──────────────────────────────────────────────────────

    pub async fn list_skills(&self) -> anyhow::Result<Vec<SkillInfo>> {
        let resp: ApiEnvelope<Vec<SkillInfo>> = self
            .client
            .get(format!("{}/v1/skills", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn execute_tool(
        &self,
        request: &ToolExecRequest,
    ) -> anyhow::Result<ToolExecResponse> {
        let resp: ApiEnvelope<ToolExecResponse> = self
            .client
            .post(format!("{}/v1/tools/execute", self.base_url))
            .json(request)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    // ── Memory ──────────────────────────────────────────────────────

    pub async fn memory_stats(&self) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!("{}/v1/memory/stats", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn memory_search(&self, query: &str) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .post(format!("{}/v1/memory/search", self.base_url))
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn memory_recent(&self, limit: u32) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!(
                "{}/v1/memory/recent?limit={}",
                self.base_url, limit
            ))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn memory_insert(&self, content: &str, weight: &str) -> anyhow::Result<()> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .post(format!("{}/v1/memory/insert", self.base_url))
            .json(&serde_json::json!({ "content": content, "weight": weight }))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)?;
        Ok(())
    }

    // ── Jobs ────────────────────────────────────────────────────────

    pub async fn list_jobs(&self) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!("{}/v1/jobs", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn submit_job(&self, body: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .post(format!("{}/v1/jobs/submit", self.base_url))
            .json(body)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    // ── Governance ──────────────────────────────────────────────────

    pub async fn get_constraints(&self) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!("{}/v1/governance/constraints", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn clear_constraints(&self) -> anyhow::Result<()> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .delete(format!("{}/v1/governance/constraints", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)?;
        Ok(())
    }

    // ── Routing ─────────────────────────────────────────────────────

    pub async fn diagnose_routing(&self) -> anyhow::Result<serde_json::Value> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .get(format!("{}/v1/routing/diagnose", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }
}

fn unwrap_envelope<T>(resp: ApiEnvelope<T>) -> anyhow::Result<T> {
    if resp.ok {
        resp.data
            .ok_or_else(|| anyhow::anyhow!("Empty data in entity response"))
    } else {
        Err(anyhow::anyhow!(
            "Entity error: {}",
            resp.error.unwrap_or_default()
        ))
    }
}

/// Parse a raw SSE block into a ChatStreamEvent.
/// SSE format: `event: <type>\ndata: <payload>\n\n`
fn parse_sse_block(block: &str) -> Option<ChatStreamEvent> {
    let mut event_type = None;
    let mut data = None;

    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event: ") {
            event_type = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data: ") {
            data = Some(rest.to_string());
        }
    }

    match (event_type.as_deref(), data) {
        (Some("token"), Some(d)) => Some(ChatStreamEvent::Token(d)),
        (Some("done"), Some(d)) => match serde_json::from_str::<ChatResponse>(&d) {
            Ok(resp) => Some(ChatStreamEvent::Done(Box::new(resp))),
            Err(e) => Some(ChatStreamEvent::Error(format!(
                "Failed to parse done event: {}",
                e
            ))),
        },
        (Some("error"), Some(d)) => Some(ChatStreamEvent::Error(d)),
        _ => None,
    }
}
