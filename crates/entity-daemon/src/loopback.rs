//! Browser pages can reach loopback through DNS rebinding, but they cannot
//! make the request name a loopback host.

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hive_core::ApiEnvelope;

pub async fn require_loopback_host(request: Request, next: Next) -> Response {
    let host = match request.headers().get(header::HOST) {
        Some(value) => value.to_str().ok(),
        None => request
            .uri()
            .authority()
            .map(|authority| authority.as_str()),
    };
    if host.is_some_and(hive_core::access::is_loopback_host) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            Json(ApiEnvelope::<String>::error(
                "This Entity only accepts requests from this computer.",
            )),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use tower::ServiceExt;

    async fn status_for(host: &str) -> StatusCode {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(require_loopback_host));
        let request = Request::builder()
            .uri("/health")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        app.oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn only_requests_naming_a_loopback_host_are_served() {
        for host in ["127.0.0.1:43180", "localhost:43180", "[::1]:43180"] {
            assert_eq!(status_for(host).await, StatusCode::OK, "{host}");
        }
        for host in [
            "rebound.example:43180",
            "127.0.0.1.rebound.example",
            "192.168.1.5:43180",
        ] {
            assert_eq!(status_for(host).await, StatusCode::FORBIDDEN, "{host}");
        }
    }
}
