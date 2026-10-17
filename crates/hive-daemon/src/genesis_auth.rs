//! Private desktop capability for the local household's Genesis records.
use crate::state::HiveDaemonState;
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hive_core::ApiEnvelope;
use std::path::Path;

pub const OWNER: &str = "local-household";

pub fn load_or_create_token(data_root: &Path) -> anyhow::Result<String> {
    let path = data_root.join("genesis.token");
    if path.exists() {
        let token = std::fs::read_to_string(&path)?;
        let token = token.trim();
        anyhow::ensure!(
            token.len() == 64 && token.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid Genesis desktop capability"
        );
        return Ok(token.to_string());
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    abigail_core::secure_fs::write_string_atomic(&path, &token)?;
    Ok(token)
}

fn authorized(value: Option<&str>, expected: &str) -> bool {
    let Some(supplied) = value.and_then(|v| v.strip_prefix("Bearer ")) else {
        return false;
    };
    if supplied.len() != expected.len() {
        return false;
    }
    supplied
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

pub async fn require_desktop(
    State(state): State<HiveDaemonState>,
    request: Request,
    next: Next,
) -> Response {
    if !authorized(
        request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok()),
        &state.genesis_token,
    ) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiEnvelope::<String>::error(
                "Open Genesis from Abigail Hive to access this household's drafts.",
            )),
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    if response.status().is_client_error()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.starts_with("application/json"))
    {
        let status = response.status();
        let message = if status == StatusCode::PAYLOAD_TOO_LARGE {
            "This request is too large. Markdown files must be at most 256 KiB."
        } else {
            "This Genesis request is invalid. Check the field values and try again."
        };
        response = (status, Json(ApiEnvelope::<String>::error(message))).into_response();
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_absent_wrong_and_partial_capabilities() {
        assert!(!authorized(None, "secret"));
        assert!(!authorized(Some("secret"), "secret"));
        assert!(!authorized(Some("Bearer secre"), "secret"));
        assert!(!authorized(Some("Bearer secrex"), "secret"));
        assert!(authorized(Some("Bearer secret"), "secret"));
    }
}
