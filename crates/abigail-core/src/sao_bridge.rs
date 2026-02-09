//! SAO Bridge Client — optional connection to the SAO orchestrator.
//!
//! When `sao_endpoint` is configured in [`AppConfig`], Abigail can register
//! with a SAO instance and send periodic status updates. The connection is
//! entirely optional: if no endpoint is set every method is a silent no-op,
//! allowing Abigail to work fully standalone.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur when talking to SAO.
#[derive(Debug, Error)]
pub enum SaoBridgeError {
    #[error("HTTP request failed: {0}")]
    Http(String),
    #[error("SAO rejected registration: {0}")]
    RegistrationRejected(String),
    #[error("not connected to SAO")]
    NotConnected,
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Agent lifecycle states reported to SAO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Online,
    Busy,
    Idle,
    Offline,
}

/// A lightweight HTTP client that talks to the SAO orchestrator.
///
/// All methods are no-ops when `endpoint` is `None`, so callers never
/// need to gate on whether SAO is configured.
#[derive(Debug, Clone)]
pub struct SaoBridgeClient {
    /// Base URL of the SAO instance (e.g. `http://localhost:3030`).
    endpoint: Option<String>,
    /// This agent's UUID (hex-encoded).
    agent_id: String,
    /// Whether we have successfully registered with SAO.
    connected: bool,
}

/// Registration payload sent to `POST /api/agents/register`.
#[derive(Serialize)]
struct RegisterRequest<'a> {
    agent_id: &'a str,
    /// Base64-encoded Ed25519 public key.
    pubkey: String,
    name: Option<String>,
}

/// Status update payload sent to `POST /api/agents/{id}/status`.
#[derive(Serialize)]
struct StatusUpdate {
    state: AgentState,
}

impl SaoBridgeClient {
    /// Create a new bridge client.
    ///
    /// If `endpoint` is `None` the client is inert and all methods return `Ok(())`.
    pub fn new(endpoint: Option<String>, agent_id: &str) -> Self {
        Self {
            endpoint,
            agent_id: agent_id.to_string(),
            connected: false,
        }
    }

    /// Register this agent with the SAO orchestrator.
    ///
    /// Sends the agent's Ed25519 public key so SAO can verify future messages.
    /// No-op if no endpoint is configured.
    pub async fn register(
        &mut self,
        pubkey: &[u8],
        name: Option<&str>,
    ) -> Result<(), SaoBridgeError> {
        let endpoint = match &self.endpoint {
            Some(e) => e.clone(),
            None => return Ok(()),
        };

        let body = RegisterRequest {
            agent_id: &self.agent_id,
            pubkey: STANDARD.encode(pubkey),
            name: name.map(|n| n.to_string()),
        };

        let url = format!("{}/api/agents/register", endpoint.trim_end_matches('/'));

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| SaoBridgeError::Http(e.to_string()))?;

        if resp.status().is_success() {
            self.connected = true;
            tracing::info!("Registered with SAO at {}", endpoint);
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| "no body".to_string());
            Err(SaoBridgeError::RegistrationRejected(format!(
                "{}: {}",
                status, text
            )))
        }
    }

    /// Send a status heartbeat to SAO.
    ///
    /// No-op if not connected or no endpoint configured.
    pub async fn send_status(&self, state: AgentState) -> Result<(), SaoBridgeError> {
        let endpoint = match &self.endpoint {
            Some(e) => e.clone(),
            None => return Ok(()),
        };

        if !self.connected {
            return Ok(());
        }

        let url = format!(
            "{}/api/agents/{}/status",
            endpoint.trim_end_matches('/'),
            self.agent_id
        );

        let client = reqwest::Client::new();
        client
            .post(&url)
            .json(&StatusUpdate { state })
            .send()
            .await
            .map_err(|e| SaoBridgeError::Http(e.to_string()))?;

        Ok(())
    }

    /// Notify SAO that this agent is going offline, then mark disconnected.
    ///
    /// No-op if not connected or no endpoint configured.
    pub async fn disconnect(&mut self) -> Result<(), SaoBridgeError> {
        if !self.connected {
            return Ok(());
        }

        // Best-effort send offline status; ignore errors on shutdown.
        let _ = self.send_status(AgentState::Offline).await;
        self.connected = false;
        tracing::info!("Disconnected from SAO");
        Ok(())
    }

    /// Whether we have an active registration with SAO.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// The configured SAO endpoint, if any.
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_when_no_endpoint() {
        let client = SaoBridgeClient::new(None, "test-agent");
        assert!(!client.is_connected());
        assert!(client.endpoint().is_none());
    }

    #[test]
    fn stores_endpoint_and_agent_id() {
        let client = SaoBridgeClient::new(Some("http://localhost:3030".into()), "abc-123");
        assert!(!client.is_connected());
        assert_eq!(client.endpoint(), Some("http://localhost:3030"));
    }

    #[tokio::test]
    async fn register_noop_without_endpoint() {
        let mut client = SaoBridgeClient::new(None, "test");
        let result = client.register(b"fake-pubkey", Some("TestAgent")).await;
        assert!(result.is_ok());
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn send_status_noop_without_endpoint() {
        let client = SaoBridgeClient::new(None, "test");
        let result = client.send_status(AgentState::Online).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn disconnect_noop_when_not_connected() {
        let mut client = SaoBridgeClient::new(Some("http://localhost:9999".into()), "test");
        let result = client.disconnect().await;
        assert!(result.is_ok());
        assert!(!client.is_connected());
    }
}
