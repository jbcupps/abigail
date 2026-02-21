//! Bearer token authentication middleware for the REST API.

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared auth state holding the current bearer token.
#[derive(Clone)]
pub struct AuthState {
    pub token: Arc<RwLock<String>>,
}

impl AuthState {
    /// Generate a new random bearer token.
    pub fn new() -> Self {
        Self {
            token: Arc::new(RwLock::new(generate_token())),
        }
    }

    /// Rotate the token: generate a new one and return it.
    pub async fn rotate(&self) -> String {
        let new_token = generate_token();
        let mut token = self.token.write().await;
        *token = new_token.clone();
        new_token
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a cryptographically random bearer token (base64-encoded 32 bytes).
fn generate_token() -> String {
    use base64::Engine as _;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Axum middleware that validates the Bearer token on every request except /health.
pub async fn auth_middleware(
    axum::extract::State(auth): axum::extract::State<AuthState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Skip auth for /health
    if request.uri().path() == "/health" {
        return Ok(next.run(request).await);
    }

    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let expected = auth.token.read().await;

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let provided = &header[7..];
            if provided == expected.as_str() {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
