//! Remote persistence transport.
//!
//! The shared embedded SurrealKv store takes an **exclusive OS file lock**, so
//! exactly one process may open it. Abigail Hive owns that store (see
//! CLAUDE.md); each family `entity-daemon` reaches it across the authenticated local
//! HTTP control plane instead of opening the file itself.
//!
//! This module defines the wire contract shared by both ends and the client
//! half of the transport. `hive-daemon` implements the server half.

use crate::client::{EntityScope, PersistenceError, QueryBinding, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Env var naming the Hive control-plane base URL to route persistence through.
/// When set, `PersistenceHandle::open` returns a remote handle instead of
/// opening the on-disk store. The Hive supervisor sets this on every child it
/// spawns; `hive-daemon` itself never sets it, so it remains the sole owner.
pub const PERSISTENCE_URL_ENV: &str = "ABIGAIL_PERSISTENCE_URL";

/// Env var carrying the per-launch local control-plane Bearer token. Mirrors
/// `hive_core::LOCAL_AUTH_ENV` — restated as a literal so this low-level crate
/// does not depend on the control-plane crate.
pub const PERSISTENCE_AUTH_ENV: &str = "ABIGAIL_LOCAL_AUTH_TOKEN";

/// Path of the persistence operation endpoint on the Hive control plane.
pub const PERSISTENCE_OP_PATH: &str = "/v1/persistence/op";

/// Wire form of [`EntityScope`]. Kept as its own type rather than deriving on
/// `EntityScope` so the on-the-wire shape stays stable if the enum grows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScopeWire {
    Hive,
    Entity { id: String },
}

impl From<&EntityScope> for ScopeWire {
    fn from(scope: &EntityScope) -> Self {
        match scope {
            EntityScope::Hive => Self::Hive,
            EntityScope::Entity(id) => Self::Entity { id: id.clone() },
        }
    }
}

impl From<ScopeWire> for EntityScope {
    fn from(wire: ScopeWire) -> Self {
        match wire {
            ScopeWire::Hive => Self::Hive,
            ScopeWire::Entity { id } => Self::Entity(id),
        }
    }
}

/// A query binding in wire form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingWire {
    pub key: String,
    pub value: Value,
}

impl From<&QueryBinding> for BindingWire {
    fn from(binding: &QueryBinding) -> Self {
        Self {
            key: binding.key.clone(),
            value: binding.value.clone(),
        }
    }
}

impl From<BindingWire> for QueryBinding {
    fn from(wire: BindingWire) -> Self {
        QueryBinding {
            key: wire.key,
            value: wire.value,
        }
    }
}

/// One persistence operation. Mirrors the data methods on `PersistenceHandle`.
///
/// `record_exists` is deliberately absent: it is derived from `Select` on the
/// client side, so it needs no wire shape of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PersistenceOp {
    Execute {
        sql: String,
        bindings: Vec<BindingWire>,
    },
    QueryVec {
        sql: String,
        bindings: Vec<BindingWire>,
    },
    QueryOne {
        sql: String,
        bindings: Vec<BindingWire>,
    },
    Upsert {
        table: String,
        id: String,
        value: Value,
    },
    Create {
        table: String,
        id: String,
        value: Value,
    },
    Delete {
        table: String,
        id: String,
    },
    Select {
        table: String,
        id: String,
    },
}

/// Request body for `POST /v1/persistence/op`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceRequest {
    pub scope: ScopeWire,
    #[serde(flatten)]
    pub op: PersistenceOp,
}

/// Result payload of a persistence operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PersistenceOutcome {
    /// Write-style op that returns nothing.
    Unit,
    /// Row set from `QueryVec`.
    Many { rows: Vec<Value> },
    /// Optional single row from `QueryOne` / `Select`.
    Maybe { row: Option<Value> },
}

/// Response envelope. Field-compatible with `hive_core::ApiEnvelope`, restated
/// here so this crate stays independent of the control-plane crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceEnvelope {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<PersistenceOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl PersistenceEnvelope {
    pub fn success(outcome: PersistenceOutcome) -> Self {
        Self {
            ok: true,
            data: Some(outcome),
            error: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Client half of the transport: speaks to the Hive-owned store over HTTP.
#[derive(Clone)]
pub struct RemoteBackend {
    endpoint: String,
    token: Option<String>,
    client: reqwest::Client,
}

/// Cap on a single persistence round trip.
///
/// Operations are local embedded-store reads and writes, so they complete in
/// milliseconds. Bounding them matters because the calling thread blocks on the
/// result: an unbounded wait would let a wedged Hive pin an entity-daemon worker
/// thread forever instead of surfacing an error the caller can report.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl RemoteBackend {
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        Self {
            endpoint: format!("{}{}", base_url.trim_end_matches('/'), PERSISTENCE_OP_PATH),
            token,
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Resolve a remote backend from the environment, if persistence has been
    /// delegated to a Hive control plane.
    pub fn from_env() -> Option<Self> {
        let base = std::env::var(PERSISTENCE_URL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        let token = std::env::var(PERSISTENCE_AUTH_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());
        if token.is_none() {
            // Every Hive route but /health requires the token, so this is not a
            // degraded mode — it is a guaranteed 401 on the first read or write.
            // Say so now, in the log, rather than leaving a daemon that reports
            // healthy and then fails everything it is asked to do.
            tracing::error!(
                "{PERSISTENCE_URL_ENV} is set but {PERSISTENCE_AUTH_ENV} is not; \
                 every persistence operation will be rejected by the Hive"
            );
        }
        Some(Self::new(base.trim(), token))
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Send one operation and return its outcome.
    pub async fn send(&self, request: PersistenceRequest) -> Result<PersistenceOutcome> {
        let mut builder = self.client.post(&self.endpoint).json(&request);
        if let Some(token) = &self.token {
            builder = builder.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
        }

        let response = builder
            .send()
            .await
            .map_err(|error| PersistenceError::Remote(format!("request failed: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(PersistenceError::Remote(format!(
                "hive persistence returned {status}: {}",
                body.trim()
            )));
        }

        let envelope: PersistenceEnvelope = response
            .json()
            .await
            .map_err(|error| PersistenceError::Remote(format!("malformed response: {error}")))?;

        if !envelope.ok {
            return Err(PersistenceError::Remote(envelope.error.unwrap_or_else(
                || "hive persistence reported failure".to_string(),
            )));
        }

        envelope
            .data
            .ok_or_else(|| PersistenceError::Remote("response carried no payload".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_wire_round_trips() {
        for scope in [
            EntityScope::Hive,
            EntityScope::Entity("abc-123".to_string()),
        ] {
            let wire = ScopeWire::from(&scope);
            let json = serde_json::to_string(&wire).unwrap();
            let back: ScopeWire = serde_json::from_str(&json).unwrap();
            assert_eq!(wire, back);
            assert_eq!(EntityScope::from(back), scope);
        }
    }

    #[test]
    fn request_serializes_scope_alongside_op() {
        let request = PersistenceRequest {
            scope: ScopeWire::Entity {
                id: "e1".to_string(),
            },
            op: PersistenceOp::Select {
                table: "birth".to_string(),
                id: "primary".to_string(),
            },
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["op"], "select");
        assert_eq!(json["scope"]["kind"], "entity");
        assert_eq!(json["scope"]["id"], "e1");

        let back: PersistenceRequest = serde_json::from_value(json).unwrap();
        assert!(matches!(back.op, PersistenceOp::Select { .. }));
    }

    #[test]
    fn outcome_variants_round_trip() {
        let cases = vec![
            PersistenceOutcome::Unit,
            PersistenceOutcome::Many {
                rows: vec![serde_json::json!({"a": 1})],
            },
            PersistenceOutcome::Maybe { row: None },
        ];
        for case in cases {
            let json = serde_json::to_string(&case).unwrap();
            let back: PersistenceOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_string(&back).unwrap(),
                serde_json::to_string(&case).unwrap()
            );
        }
    }
}
