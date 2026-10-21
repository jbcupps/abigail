//! HTTP conventions for the local household's private Genesis records.
//! Access is enforced for every Hive route by [`crate::access`].
use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hive_core::ApiEnvelope;

pub const OWNER: &str = "local-household";

pub async fn harden_responses(request: Request, next: Next) -> Response {
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
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
