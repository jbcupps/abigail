//! Per-launch Bearer auth for the local Hive control plane.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared auth material for middleware and supervised children.
#[derive(Clone)]
pub struct LocalAuth {
    token: Arc<String>,
    entities: Arc<Mutex<HashMap<String, String>>>,
}

impl LocalAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: Arc::new(token.into()),
            entities: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn grant_entity(&self, entity_id: &str) -> String {
        let token = generate_token();
        self.entities
            .lock()
            .unwrap()
            .insert(token.clone(), entity_id.into());
        token
    }
    pub fn revoke(&self, token: &str) {
        self.entities.lock().unwrap().remove(token);
    }

    pub fn token(&self) -> &str {
        self.token.as_str()
    }

    #[cfg(test)]
    pub fn generate() -> Self {
        Self::new(generate_token())
    }
}

fn generate_token() -> String {
    // Two random UUIDs as a compact hex string (no padding issues).
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Paths that remain reachable without a Bearer token.
fn is_public_path(path: &str) -> bool {
    path == "/health"
}

/// Axum middleware: require `Authorization: Bearer <token>` on non-public routes.
pub async fn require_local_auth(
    State(state): State<crate::state::HiveDaemonState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if is_public_path(request.uri().path()) {
        return Ok(next.run(request).await);
    }
    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?
        .to_string();
    if provided.len() >= 32
        && constant_time_eq(provided.as_bytes(), state.local_auth.token().as_bytes())
    {
        return Ok(next.run(request).await);
    }
    let entity_id = state
        .local_auth
        .entities
        .lock()
        .unwrap()
        .iter()
        .find(|(token, _)| constant_time_eq(provided.as_bytes(), token.as_bytes()))
        .map(|(_, id)| id.clone())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let own_prefix = format!("/v1/entities/{entity_id}");
    let allowed_get = method == axum::http::Method::GET
        && [
            "",
            "/birth",
            "/provider-config",
            "/assignments",
            "/forge-approvals",
        ]
        .iter()
        .any(|suffix| path == format!("{own_prefix}{suffix}"));
    let allowed_post = method == axum::http::Method::POST
        && [
            "/v1/persistence/op",
            "/v1/runtime/sessions",
            "/v1/runtime/register",
            "/v1/runtime/heartbeat",
            "/v1/runtime/outbox/sync",
        ]
        .contains(&path.as_str());
    if !allowed_get && !allowed_post {
        return Err(StatusCode::FORBIDDEN);
    }
    if allowed_post {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 64 * 1024 * 1024)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
        let owns = if path == "/v1/persistence/op" {
            value["scope"]["kind"] == "entity" && value["scope"]["id"] == entity_id
        } else if path == "/v1/runtime/sessions" {
            value["entity_id"] == entity_id
        } else {
            let lease = value["lease_id"].as_str().ok_or(StatusCode::FORBIDDEN)?;
            state
                .runtime_control
                .lock()
                .unwrap()
                .session_status(lease)
                .is_some_and(|s| {
                    s.lease.entity_id == entity_id && value["runtime_id"] == s.lease.runtime_id
                })
        };
        if !owns {
            return Err(StatusCode::FORBIDDEN);
        }
        request = Request::from_parts(parts, axum::body::Body::from(bytes));
    }
    Ok(next.run(request).await)
}

/// Constant-time equality for equal-length slices; unequal lengths always fail.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_nonempty_and_distinct() {
        let a = generate_token();
        let b = generate_token();
        assert!(!a.is_empty());
        assert_ne!(a, b);
    }

    #[test]
    fn constant_time_eq_rejects_mismatched_lengths() {
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[tokio::test]
    async fn caller_tokens_enforce_entity_scope_and_setup_authority() {
        use axum::{
            routing::{get, post},
            Router,
        };
        let (state, id) = crate::routes::tests::build_state();
        let token = state.local_auth.grant_entity(&id);
        let app = Router::new()
            .route("/v1/setup", get(|| async { "setup" }))
            .route("/v1/persistence/op", post(|| async { "storage" }))
            .route("/v1/secrets/test", get(|| async { "secret" }))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_local_auth,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .get(format!("{url}/v1/setup"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            client
                .get(format!("{url}/v1/setup"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{url}/v1/setup"))
                .bearer_auth(state.local_auth.token())
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .get(format!("{url}/v1/secrets/test"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        for (scope, expected) in [
            (serde_json::json!({"kind":"entity","id":id}), 200),
            (
                serde_json::json!({"kind":"entity","id":uuid::Uuid::new_v4().to_string()}),
                403,
            ),
            (serde_json::json!({"kind":"hive"}), 403),
        ] {
            assert_eq!(
                client
                    .post(format!("{url}/v1/persistence/op"))
                    .bearer_auth(&token)
                    .json(&serde_json::json!({"scope":scope}))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        state.local_auth.revoke(&token);
        assert_eq!(
            client
                .get(format!("{url}/v1/setup"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        task.abort();
    }
}
