//! Loopback access control for every Hive route.
//!
//! Loopback is not a trust boundary: other local accounts, browser pages via
//! DNS rebinding, and unrelated local processes can all reach `127.0.0.1`. A
//! request must name a loopback host and, unless its route is public, present
//! either the per-install control token (the desktop Hive app) or an Entity
//! runtime lease issued through the supervisor's one-use bootstrap.

use crate::state::HiveDaemonState;
use axum::{
    extract::{FromRequestParts, MatchedPath, RawPathParams, Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use hive_core::access::{self as contract, AdoptionChallenge, AdoptionProof};
use hive_core::ApiEnvelope;
use std::path::Path;

const LEGACY_GENESIS_TOKEN_FILE: &str = "genesis.token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Liveness, the adoption challenge, and bootstrap-authorized lease issuance.
    Public,
    /// Only the desktop Hive app.
    Desktop,
    /// The desktop app or the immortal Abigail Hive helper Entity.
    HiveAdmin,
    /// The desktop app or any Entity runtime.
    Household,
    /// The desktop app or the runtime of the Entity named by `:id`.
    Entity,
    /// Only the runtime of the Entity named by `:id`.
    EntityRuntime,
    /// Any Entity runtime; the handler matches the lease named by the request.
    Runtime,
}

/// The access each route requires. Routes not listed are desktop-only.
pub fn policy(method: &Method, route: &str) -> Access {
    use Access::*;
    match (method.as_str(), route) {
        ("GET", "/health") | ("POST", contract::ADOPTION_PATH | "/v1/runtime/sessions") => Public,
        ("GET" | "POST", "/v1/entities")
        | ("GET", "/v1/entity-creations/:creation_key")
        | ("POST", "/v1/secrets") => HiveAdmin,
        ("GET", "/v1/secrets/list" | "/v1/secrets/:key" | "/v1/providers/profiles/:name") => {
            Household
        }
        (
            "GET",
            "/v1/entities/:id"
            | "/v1/entities/:id/provider-config"
            | "/v1/entities/:id/assignments"
            | "/v1/entities/:id/forge-approvals",
        ) => Entity,
        ("POST", "/v1/entities/:id/persistence")
        | ("GET", "/v1/entities/:id/birth" | "/v1/entities/:id/execution/receipts") => {
            EntityRuntime
        }
        ("GET", "/v1/runtime/sessions/:lease_id")
        | ("POST", "/v1/runtime/register" | "/v1/runtime/heartbeat" | "/v1/runtime/outbox/sync") => {
            Runtime
        }
        ("POST", "/v1/entities/:id/journey/legacy-first-signal") => Desktop,
        _ => Desktop,
    }
}

/// Who made an authorized request. Inserted into request extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    Desktop,
    Runtime { lease_id: String, entity_id: String },
}

impl Principal {
    pub fn lease_id(&self) -> Option<&str> {
        match self {
            Self::Runtime { lease_id, .. } => Some(lease_id),
            Self::Desktop => None,
        }
    }
}

fn resolve(state: &HiveDaemonState, bearer: &str) -> Option<Principal> {
    if contract::tokens_match(bearer, &state.control_token) {
        return Some(Principal::Desktop);
    }
    let session = state.runtime_control.lock().ok()?.session_status(bearer)?;
    Some(Principal::Runtime {
        lease_id: session.lease.lease_id,
        entity_id: session.lease.entity_id,
    })
}

fn permits(
    state: &HiveDaemonState,
    access: Access,
    principal: &Principal,
    path_entity: Option<&str>,
) -> bool {
    let runtime_entity = match principal {
        Principal::Desktop => {
            return !matches!(access, Access::EntityRuntime | Access::Runtime);
        }
        Principal::Runtime { entity_id, .. } => entity_id.as_str(),
    };
    match access {
        Access::Public | Access::Household | Access::Runtime => true,
        Access::Desktop => false,
        Access::HiveAdmin => state
            .identity_manager
            .hive_agent_id()
            .is_ok_and(|hive_id| hive_id == runtime_entity),
        Access::Entity | Access::EntityRuntime => path_entity == Some(runtime_entity),
    }
}

fn bearer(request: &Request) -> Option<&str> {
    request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
}

fn host(request: &Request) -> Option<&str> {
    match request.headers().get(header::HOST) {
        Some(value) => value.to_str().ok(),
        None => request
            .uri()
            .authority()
            .map(|authority| authority.as_str()),
    }
}

fn deny(status: StatusCode, message: &str) -> Response {
    let mut response = (status, Json(ApiEnvelope::<String>::error(message))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub async fn authorize(
    State(state): State<HiveDaemonState>,
    request: Request,
    next: Next,
) -> Response {
    if !host(&request).is_some_and(contract::is_loopback_host) {
        return deny(
            StatusCode::FORBIDDEN,
            "Abigail Hive only accepts requests from this computer.",
        );
    }
    let access = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(Access::Desktop, |route| {
            policy(request.method(), route.as_str())
        });
    if access == Access::Public {
        return next.run(request).await;
    }
    let Some(principal) = bearer(&request).and_then(|token| resolve(&state, token)) else {
        return deny(
            StatusCode::UNAUTHORIZED,
            "Abigail Hive did not recognize this request. Reopen Abigail and try again.",
        );
    };
    let (mut parts, body) = request.into_parts();
    let path_entity = RawPathParams::from_request_parts(&mut parts, &state)
        .await
        .ok()
        .and_then(|params| {
            params
                .iter()
                .find(|(name, _)| *name == "id")
                .map(|(_, value)| value.to_string())
        });
    if !permits(&state, access, &principal, path_entity.as_deref()) {
        return deny(
            StatusCode::FORBIDDEN,
            "This part of Abigail is not allowed to make that request.",
        );
    }
    parts.extensions.insert(principal);
    next.run(Request::from_parts(parts, body)).await
}

/// Lets a desktop shell confirm a daemon holds this install's control token
/// before it ever sends that token.
pub async fn adoption(
    State(state): State<HiveDaemonState>,
    Json(challenge): Json<AdoptionChallenge>,
) -> (StatusCode, Json<ApiEnvelope<AdoptionProof>>) {
    if !contract::is_valid_nonce(&challenge.nonce) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiEnvelope::error("Invalid adoption challenge")),
        );
    }
    let channel = abigail_core::channel::current().name();
    (
        StatusCode::OK,
        Json(ApiEnvelope::success(AdoptionProof {
            channel: channel.to_string(),
            proof: contract::adoption_proof(
                &state.control_token,
                channel,
                &state.hive_url,
                &challenge.nonce,
            ),
        })),
    )
}

/// The token is only an access credential, so a damaged file is replaced
/// rather than keeping the household's Hive from starting.
pub fn load_or_create_control_token(data_root: &Path) -> anyhow::Result<String> {
    let path = data_root.join(contract::CONTROL_TOKEN_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(contents) => contract::parse_control_token(&contents).map(str::to_owned),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let token = match existing {
        Some(token) => token,
        None => {
            if path.exists() {
                tracing::warn!("Replacing a damaged Hive control token");
            }
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            abigail_core::secure_fs::write_string_atomic(&path, &token)?;
            token
        }
    };
    let _ = std::fs::remove_file(data_root.join(LEGACY_GENESIS_TOKEN_FILE));
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::tests::build_state;
    use axum::body::Body;
    use hive_core::{RuntimeSessionLease, RuntimeSessionRequest};
    use tower::ServiceExt;

    struct Fixture {
        state: HiveDaemonState,
        app: axum::Router,
        entity_id: String,
    }

    fn fixture() -> Fixture {
        let (state, entity_id) = build_state();
        Fixture {
            app: crate::app::router(state.clone()),
            state,
            entity_id,
        }
    }

    impl Fixture {
        fn lease_for(&self, entity_id: &str) -> RuntimeSessionLease {
            self.state.runtime_control.lock().unwrap().issue_session(
                RuntimeSessionRequest {
                    entity_id: entity_id.to_string(),
                    runtime_id: None,
                },
                None,
                Some(self.state.hive_url.clone()),
            )
        }

        fn desktop(&self) -> String {
            self.state.control_token.to_string()
        }

        async fn send(
            &self,
            method: Method,
            path: &str,
            bearer: Option<&str>,
            body: Option<serde_json::Value>,
        ) -> (StatusCode, serde_json::Value) {
            self.send_with_host(method, path, bearer, body, "127.0.0.1:43141")
                .await
        }

        async fn send_with_host(
            &self,
            method: Method,
            path: &str,
            bearer: Option<&str>,
            body: Option<serde_json::Value>,
            host: &str,
        ) -> (StatusCode, serde_json::Value) {
            let mut request = Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, host);
            if let Some(token) = bearer {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let request = match body {
                Some(body) => request
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string())),
                None => request.body(Body::empty()),
            }
            .unwrap();
            let response = self.app.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap();
            let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into())
            });
            (status, value)
        }
    }

    fn hidden_weight_paths(value: &serde_json::Value) -> Vec<String> {
        const HIDDEN: [&str; 5] = [
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
        ];
        fn visit(value: &serde_json::Value, path: &str, found: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    for (key, child) in object {
                        let child_path = format!("{path}.{key}");
                        if HIDDEN.contains(&key.as_str()) {
                            found.push(child_path.clone());
                        }
                        visit(child, &child_path, found);
                    }
                }
                serde_json::Value::Array(array) => {
                    for (index, child) in array.iter().enumerate() {
                        visit(child, &format!("{path}[{index}]"), found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        visit(value, "$", &mut found);
        found
    }

    #[tokio::test]
    async fn router_user_surfaces_hide_weights_and_full_birth_requires_matching_runtime() {
        let f = fixture();
        let desktop = f.desktop();
        let mut surfaces = Vec::new();

        let (status, created) = f
            .send(
                Method::POST,
                "/v1/genesis/sessions",
                Some(&desktop),
                Some(serde_json::json!({})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(created["ok"], true, "{created}");
        let genesis_id = created["data"]["id"].as_str().unwrap();
        let revision = created["data"]["revision"].as_u64().unwrap();
        surfaces.push(("Genesis create", created.clone()));

        let (status, loaded) = f
            .send(
                Method::GET,
                &format!("/v1/genesis/sessions/{genesis_id}"),
                Some(&desktop),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(loaded["ok"], true, "{loaded}");
        surfaces.push(("Genesis load", loaded));

        let (status, action) = f
            .send(
                Method::POST,
                &format!("/v1/genesis/sessions/{genesis_id}/actions"),
                Some(&desktop),
                Some(serde_json::json!({
                    "action": "save_draft",
                    "expected_revision": revision,
                    "text": "A private draft"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(action["ok"], true, "{action}");
        surfaces.push(("Genesis action", action));

        let (status, catalog) = f
            .send(
                Method::GET,
                "/v1/genesis/quest/catalog",
                Some(&desktop),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(catalog["ok"], true, "{catalog}");
        surfaces.push(("quest catalog", catalog));

        let (status, scenarios) = f
            .send(Method::GET, "/v1/birth/scenarios", Some(&desktop), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(scenarios["ok"], true, "{scenarios}");
        surfaces.push(("birth scenarios", scenarios));

        let birth_path = format!("/v1/entities/{}/birth", f.entity_id);
        let (status, birth) = f
            .send(
                Method::POST,
                &birth_path,
                Some(&desktop),
                Some(serde_json::json!({
                    "path": "quickstart",
                    "choices": [],
                    "purpose": "Help the family"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(birth["ok"], true, "{birth}");
        surfaces.push(("generic birth POST", birth));

        let (status, journey) = f
            .send(
                Method::GET,
                &format!("/v1/entities/{}/journey", f.entity_id),
                Some(&desktop),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(journey["ok"], true, "{journey}");
        surfaces.push(("journey", journey));

        let presence_path = format!("/v1/entities/{}/presence", f.entity_id);
        let (status, presence_put) = f
            .send(
                Method::PUT,
                &presence_path,
                Some(&desktop),
                Some(serde_json::json!({
                    "temperament": "gentle",
                    "identity": "aether",
                    "crest": "orbit",
                    "motion": "still"
                })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(presence_put["ok"], true, "{presence_put}");
        surfaces.push(("Presence PUT", presence_put));
        let (status, presence_get) = f
            .send(Method::GET, &presence_path, Some(&desktop), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(presence_get["ok"], true, "{presence_get}");
        surfaces.push(("Presence GET", presence_get));

        let leaks: Vec<_> = surfaces
            .iter()
            .filter_map(|(surface, value)| {
                let paths = hidden_weight_paths(value);
                (!paths.is_empty()).then(|| format!("{surface}: {}", paths.join(", ")))
            })
            .collect();
        assert!(
            leaks.is_empty(),
            "hidden ethical scoring leaked through HTTP routes:\n{}",
            leaks.join("\n")
        );

        let (status, denied) = f.send(Method::GET, &birth_path, Some(&desktop), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(denied["ok"], false);

        let runtime = f.lease_for(&f.entity_id).lease_id;
        let (status, full) = f.send(Method::GET, &birth_path, Some(&runtime), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(full["ok"], true, "{full}");
        let certificate = &full["data"]["certificate"];
        assert_eq!(certificate["scoring_version"], hive_core::SCORING_VERSION);
        for axis in ["deontology", "teleology", "areteology", "welfare"] {
            assert!(
                certificate["weights"][axis].is_number(),
                "runtime certificate lacks {axis}: {certificate}"
            );
        }
    }

    #[test]
    fn every_route_has_the_intended_access() {
        use Access::*;
        let cases = [
            ("GET", "/health", Public),
            ("POST", "/v1/hive/adoption", Public),
            ("POST", "/v1/runtime/sessions", Public),
            ("GET", "/v1/status", Desktop),
            ("GET", "/v1/entities", HiveAdmin),
            ("POST", "/v1/entities", HiveAdmin),
            ("GET", "/v1/entity-creations/:creation_key", HiveAdmin),
            ("GET", "/v1/entities/:id", Entity),
            ("POST", "/v1/entities/:id/open", Desktop),
            ("POST", "/v1/entities/:id/close", Desktop),
            ("POST", "/v1/entities/:id/persistence", EntityRuntime),
            ("GET", "/v1/birth/scenarios", Desktop),
            ("GET", "/v1/entities/:id/birth", EntityRuntime),
            ("POST", "/v1/entities/:id/birth", Desktop),
            ("PATCH", "/v1/entities/:id/config", Desktop),
            ("GET", "/v1/entities/:id/provider-config", Entity),
            ("POST", "/v1/entities/:id/sign", Desktop),
            ("GET", "/v1/entities/:id/assignments", Entity),
            ("POST", "/v1/entities/:id/assignments", Desktop),
            ("GET", "/v1/entities/:id/forge-approvals", Entity),
            ("POST", "/v1/entities/:id/forge-approvals", Desktop),
            ("GET", "/v1/entities/:id/journey", Desktop),
            (
                "POST",
                "/v1/entities/:id/journey/legacy-first-signal",
                Desktop,
            ),
            ("GET", "/v1/entities/:id/presence", Desktop),
            ("PUT", "/v1/entities/:id/presence", Desktop),
            ("GET", "/v1/genesis/quest/catalog", Desktop),
            ("POST", "/v1/secrets", HiveAdmin),
            ("GET", "/v1/secrets/list", Household),
            ("GET", "/v1/secrets/:key", Household),
            ("POST", "/v1/providers/models", Desktop),
            ("GET", "/v1/providers/best", Desktop),
            ("GET", "/v1/providers/detect", Desktop),
            ("POST", "/v1/providers/hive-default", Desktop),
            ("POST", "/v1/providers/local", Desktop),
            ("GET", "/v1/providers/profiles/:name", Household),
            ("GET", "/v1/runtime/sessions/:lease_id", Runtime),
            ("POST", "/v1/runtime/register", Runtime),
            ("POST", "/v1/runtime/heartbeat", Runtime),
            ("POST", "/v1/runtime/outbox/sync", Runtime),
            ("GET", "/v1/entities/:id/execution/receipts", EntityRuntime),
            ("GET", "/v1/genesis/sessions", Desktop),
            ("POST", "/v1/genesis/media/credentials", Desktop),
            ("DELETE", "/v1/genesis/sessions/:id", Desktop),
            ("GET", "/v1/unlisted-future-route", Desktop),
            ("DELETE", "/v1/secrets/:key", Desktop),
        ];
        for (method, route, expected) in cases {
            let method = Method::from_bytes(method.as_bytes()).unwrap();
            assert_eq!(policy(&method, route), expected, "{method} {route}");
        }
    }

    #[tokio::test]
    async fn control_plane_routes_reject_requests_without_a_credential() {
        let f = fixture();
        for (method, route, body) in [
            (Method::GET, "/v1/status", None),
            (Method::GET, "/v1/entities", None),
            (
                Method::GET,
                "/v1/entity-creations/00000000-0000-4000-8000-000000000000",
                None,
            ),
            (
                Method::POST,
                "/v1/entities",
                Some(serde_json::json!({"name": "Intruder"})),
            ),
            (Method::GET, "/v1/secrets/list", None),
            (Method::GET, "/v1/secrets/openai", None),
            (
                Method::POST,
                "/v1/secrets",
                Some(serde_json::json!({"key": "openai", "value": "sk-x"})),
            ),
            (Method::GET, "/v1/entities/:id/provider-config", None),
            (Method::POST, "/v1/entities/:id/open", None),
            (Method::GET, "/v1/providers/profiles/openai", None),
            (Method::GET, "/v1/genesis/sessions", None),
        ] {
            let path = route.replace(":id", &f.entity_id);
            for bearer in [None, Some("not-a-real-credential")] {
                let (status, body) = f.send(method.clone(), &path, bearer, body.clone()).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {route}");
                assert_eq!(body["ok"], false);
            }
        }
        let (status, body) = f.send(Method::GET, "/health", None, None).await;
        assert_eq!((status, body), (StatusCode::OK, "ok".into()));
    }

    #[tokio::test]
    async fn requests_naming_a_non_loopback_host_are_refused_even_with_the_token() {
        let f = fixture();
        let desktop = f.desktop();
        for host in ["attacker.example:43141", "127.0.0.1.attacker.example"] {
            for (path, bearer) in [
                ("/health", None),
                ("/v1/secrets/list", Some(desktop.as_str())),
            ] {
                let (status, _) = f
                    .send_with_host(Method::GET, path, bearer, None, host)
                    .await;
                assert_eq!(status, StatusCode::FORBIDDEN, "{host} {path}");
            }
        }
        let (status, _) = f
            .send_with_host(Method::GET, "/health", None, None, "localhost:43141")
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn desktop_token_manages_secrets_and_entities() {
        let f = fixture();
        let desktop = f.desktop();
        let (status, body) = f
            .send(
                Method::POST,
                "/v1/secrets",
                Some(&desktop),
                Some(serde_json::json!({"key": "openai", "value": "sk-desktop-fixture"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
        let (status, body) = f
            .send(Method::GET, "/v1/secrets/openai", Some(&desktop), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["value"], "sk-desktop-fixture");
        let (status, body) = f
            .send(Method::GET, "/v1/entities", Some(&desktop), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        let (status, _) = f
            .send(Method::GET, "/v1/genesis/sessions", Some(&desktop), None)
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn family_entity_lease_reaches_only_its_own_runtime_routes() {
        let f = fixture();
        let (other_id, _) = f.state.identity_manager.create_agent("Other").unwrap();
        let lease = f.lease_for(&f.entity_id).lease_id;
        let concrete = |route: &str| {
            route
                .replace(":lease_id", &lease)
                .replace(":other", &other_id)
                .replace(":id", &f.entity_id)
        };

        for route in [
            "/v1/entities/:id",
            "/v1/entities/:id/birth",
            "/v1/entities/:id/provider-config",
            "/v1/entities/:id/assignments",
            "/v1/entities/:id/execution/receipts",
            "/v1/runtime/sessions/:lease_id",
            "/v1/secrets/list",
            "/v1/secrets/openai",
        ] {
            let (status, _) = f
                .send(Method::GET, &concrete(route), Some(&lease), None)
                .await;
            assert_eq!(status, StatusCode::OK, "{route}");
        }
        for (method, route, body) in [
            (Method::GET, "/v1/entities/:other", None),
            (Method::GET, "/v1/entities/:other/birth", None),
            (Method::GET, "/v1/entities/:other/provider-config", None),
            (Method::GET, "/v1/entities/:other/execution/receipts", None),
            (Method::GET, "/v1/entities", None),
            (
                Method::GET,
                "/v1/entity-creations/00000000-0000-4000-8000-000000000000",
                None,
            ),
            (
                Method::POST,
                "/v1/entities",
                Some(serde_json::json!({"name": "Spawned"})),
            ),
            (
                Method::POST,
                "/v1/secrets",
                Some(serde_json::json!({"key": "openai", "value": "sk-family"})),
            ),
            (
                Method::PATCH,
                "/v1/entities/:id/config",
                Some(serde_json::json!({"ego_model": "other"})),
            ),
            (Method::POST, "/v1/entities/:id/open", None),
            (Method::GET, "/v1/status", None),
            (Method::GET, "/v1/genesis/sessions", None),
            (Method::GET, "/v1/entities/:id/journey", None),
            (
                Method::POST,
                "/v1/entities/:id/journey/legacy-first-signal",
                Some(serde_json::json!({
                    "attempt_key": "opaque-old-attempt",
                    "original_name": "Route Test",
                    "purpose": "Help the family",
                    "boundaries": "Ask first",
                    "presence": {
                        "temperament": "gentle",
                        "identity": "aether",
                        "crest": "orbit",
                        "motion": "calm"
                    }
                })),
            ),
            (Method::GET, "/v1/entities/:id/presence", None),
            (
                Method::PUT,
                "/v1/entities/:id/presence",
                Some(serde_json::json!({
                    "temperament": "gentle",
                    "identity": "aether",
                    "crest": "orbit",
                    "motion": "still"
                })),
            ),
            (Method::GET, "/v1/genesis/quest/catalog", None),
        ] {
            let (status, body) = f
                .send(method.clone(), &concrete(route), Some(&lease), body)
                .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {route}");
            assert_eq!(body["ok"], false);
        }
    }

    #[tokio::test]
    async fn hive_helper_lease_keeps_household_management_but_not_desktop_routes() {
        let f = fixture();
        let hive_id = f.state.identity_manager.hive_agent_id().unwrap();
        let lease = f.lease_for(&hive_id).lease_id;
        let (status, body) = f
            .send(
                Method::POST,
                "/v1/secrets",
                Some(&lease),
                Some(serde_json::json!({"key": "openai", "value": "sk-helper-fixture"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
        let (status, _) = f
            .send(Method::GET, "/v1/entities", Some(&lease), None)
            .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = f
            .send(
                Method::GET,
                "/v1/entity-creations/00000000-0000-4000-8000-000000000000",
                Some(&lease),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        for (method, route) in [
            (Method::GET, "/v1/status"),
            (Method::POST, "/v1/entities/:id/open"),
            (Method::GET, "/v1/entities/:id/provider-config"),
        ] {
            let path = route.replace(":id", &f.entity_id);
            let (status, _) = f.send(method.clone(), &path, Some(&lease), None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {route}");
        }
    }

    #[tokio::test]
    async fn desktop_token_cannot_impersonate_an_entity_runtime() {
        let f = fixture();
        let desktop = f.desktop();
        let (status, _) = f
            .send(
                Method::POST,
                &format!("/v1/entities/{}/persistence", f.entity_id),
                Some(&desktop),
                Some(serde_json::json!({"op": "select", "table": "memory_entry", "id": "x"})),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = f
            .send(
                Method::POST,
                "/v1/runtime/heartbeat",
                Some(&desktop),
                Some(serde_json::json!({"lease_id": "x", "runtime_id": "y", "outbox_depth": 0})),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn runtime_reports_must_name_the_presented_lease() {
        let f = fixture();
        let (other_id, _) = f.state.identity_manager.create_agent("Other").unwrap();
        let own = f.lease_for(&f.entity_id);
        let other = f.lease_for(&other_id);
        let heartbeat = |lease: &RuntimeSessionLease| {
            serde_json::json!({
                "lease_id": lease.lease_id,
                "runtime_id": lease.runtime_id,
                "outbox_depth": 0,
            })
        };
        let (status, _) = f
            .send(
                Method::POST,
                "/v1/runtime/heartbeat",
                Some(&own.lease_id),
                Some(heartbeat(&other)),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, body) = f
            .send(
                Method::POST,
                "/v1/runtime/heartbeat",
                Some(&own.lease_id),
                Some(heartbeat(&own)),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true, "{body}");
    }

    #[tokio::test]
    async fn closing_an_entity_revokes_its_lease_for_every_route() {
        let f = fixture();
        let lease = f.lease_for(&f.entity_id).lease_id;
        let path = format!("/v1/entities/{}/provider-config", f.entity_id);
        assert_eq!(
            f.send(Method::GET, &path, Some(&lease), None).await.0,
            StatusCode::OK
        );
        let close = format!("/v1/entities/{}/close", f.entity_id);
        let (status, _) = f.send(Method::POST, &close, Some(&f.desktop()), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            f.send(Method::GET, &path, Some(&lease), None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn adoption_proves_the_token_without_revealing_it() {
        let f = fixture();
        let nonce = "ab".repeat(32);
        let (status, body) = f
            .send(
                Method::POST,
                contract::ADOPTION_PATH,
                None,
                Some(serde_json::json!({ "nonce": nonce })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let channel = abigail_core::channel::current().name();
        assert_eq!(body["data"]["channel"], channel);
        let proof = body["data"]["proof"].as_str().unwrap();
        assert!(contract::verify_adoption_proof(
            &f.state.control_token,
            channel,
            &f.state.hive_url,
            &nonce,
            proof
        ));
        assert!(!body.to_string().contains(f.state.control_token.as_str()));

        let (status, _) = f
            .send(
                Method::POST,
                contract::ADOPTION_PATH,
                None,
                Some(serde_json::json!({ "nonce": "short" })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webview_preflight_may_send_the_authorization_header() {
        let f = fixture();
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/v1/secrets")
            .header(header::HOST, "127.0.0.1:43141")
            .header(header::ORIGIN, "http://tauri.localhost")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization,content-type",
            )
            .body(Body::empty())
            .unwrap();
        let response = f.app.clone().oneshot(request).await.unwrap();
        assert!(response.status().is_success());
        let allowed = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(allowed.contains("authorization"), "{allowed}");
        assert!(!allowed.contains('*'), "{allowed}");
    }

    #[tokio::test]
    async fn real_http_outbox_replay_is_acknowledged_without_duplicate_receipt() {
        let f = fixture();
        let lease = f.lease_for(&f.entity_id);
        let record_id = uuid::Uuid::new_v4().to_string();
        let request = serde_json::json!({
            "lease_id": lease.lease_id,
            "runtime_id": lease.runtime_id,
            "records": [{
                "record_id": record_id,
                "entity_id": f.entity_id,
                "kind": "skill_used",
                "created_at_utc": "2026-10-22T12:00:00Z",
                "payload": {
                    "session_id": "session-1",
                    "skill_id": "com.abigail.calendar",
                    "tool_name": "list_events"
                }
            }]
        });

        for _ in 0..2 {
            let (status, response) = f
                .send(
                    Method::POST,
                    "/v1/runtime/outbox/sync",
                    Some(&lease.lease_id),
                    Some(request.clone()),
                )
                .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(
                response["data"]["accepted_record_ids"],
                serde_json::json!([record_id])
            );
        }

        let (status, response) = f
            .send(
                Method::GET,
                &format!("/v1/entities/{}/execution/receipts", f.entity_id),
                Some(&lease.lease_id),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["data"]["receipts"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn real_http_journey_partial_failure_retries_and_acknowledges_once() {
        let f = fixture();
        let lease = f.lease_for(&f.entity_id);
        let skill_id = uuid::Uuid::new_v4().to_string();
        let memory_id = uuid::Uuid::new_v4().to_string();
        let request = serde_json::json!({
            "lease_id": lease.lease_id,
            "runtime_id": lease.runtime_id,
            "records": [
                {
                    "record_id": skill_id,
                    "entity_id": f.entity_id,
                    "kind": "skill_used",
                    "created_at_utc": "2026-10-22T12:00:00Z",
                    "payload": {
                        "session_id": "session-1",
                        "skill_id": "com.abigail.calendar",
                        "tool_name": "list_events"
                    }
                },
                {
                    "record_id": memory_id,
                    "entity_id": f.entity_id,
                    "kind": "memory_confirmed",
                    "created_at_utc": "2026-10-22T12:01:00Z",
                    "payload": {
                        "memory_id": "memory-1",
                        "memory_weight": "distilled"
                    }
                }
            ]
        });
        f.state.journeys.fail_event_writes_after(1);

        let (_, failed) = f
            .send(
                Method::POST,
                "/v1/runtime/outbox/sync",
                Some(&lease.lease_id),
                Some(request.clone()),
            )
            .await;
        assert_eq!(failed["ok"], false, "{failed}");

        let (_, retried) = f
            .send(
                Method::POST,
                "/v1/runtime/outbox/sync",
                Some(&lease.lease_id),
                Some(request),
            )
            .await;
        assert_eq!(retried["ok"], true, "{retried}");
        assert_eq!(
            retried["data"]["accepted_record_ids"],
            serde_json::json!([skill_id, memory_id])
        );
        let journey = f
            .state
            .journeys
            .load(crate::genesis_http::OWNER, &f.entity_id)
            .unwrap()
            .unwrap();
        assert_eq!(journey.successful_skill_uses, 1);
        assert_eq!(journey.confirmed_memories, 1);
        assert_eq!(
            f.state
                .runtime_control
                .lock()
                .unwrap()
                .execution_receipts(&f.entity_id, 500)
                .receipts
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn real_http_projection_failure_retries_the_exact_record_without_duplicates() {
        let f = fixture();
        let lease = f.lease_for(&f.entity_id);
        let record_id = uuid::Uuid::new_v4().to_string();
        let request = serde_json::json!({
            "lease_id": lease.lease_id,
            "runtime_id": lease.runtime_id,
            "records": [{
                "record_id": record_id,
                "entity_id": f.entity_id,
                "kind": "skill_used",
                "created_at_utc": "2026-10-22T12:00:00Z",
                "payload": {
                    "session_id": "session-1",
                    "skill_id": "com.abigail.calendar",
                    "tool_name": "list_events"
                }
            }]
        });
        f.state.journeys.fail_next_projection_write();

        let (_, failed) = f
            .send(
                Method::POST,
                "/v1/runtime/outbox/sync",
                Some(&lease.lease_id),
                Some(request.clone()),
            )
            .await;
        assert_eq!(failed["ok"], false, "{failed}");
        assert!(
            f.state
                .journeys
                .load(crate::genesis_http::OWNER, &f.entity_id)
                .unwrap()
                .is_none(),
            "the injected projection failure must not erase the durable event"
        );
        assert_eq!(
            f.state
                .runtime_control
                .lock()
                .unwrap()
                .execution_receipts(&f.entity_id, 500)
                .receipts
                .len(),
            1,
            "the prior control-plane mutation must remain deduplicated for retry"
        );

        let (_, replay) = f
            .send(
                Method::POST,
                "/v1/runtime/outbox/sync",
                Some(&lease.lease_id),
                Some(request),
            )
            .await;
        assert_eq!(replay["ok"], true, "{replay}");
        assert_eq!(
            replay["data"]["accepted_record_ids"],
            serde_json::json!([record_id])
        );
        assert_eq!(
            replay["data"]["execution_receipts"],
            serde_json::json!([]),
            "an exact replay must not mint a second receipt"
        );
        assert_eq!(
            f.state
                .journeys
                .load(crate::genesis_http::OWNER, &f.entity_id)
                .unwrap()
                .unwrap()
                .successful_skill_uses,
            1
        );
        assert_eq!(
            f.state
                .journeys
                .event_activity(crate::genesis_http::OWNER, &f.entity_id)
                .unwrap()
                .successful_skill_uses,
            1,
            "the exact retry must not duplicate the durable event row"
        );
        assert_eq!(
            f.state
                .runtime_control
                .lock()
                .unwrap()
                .execution_receipts(&f.entity_id, 500)
                .receipts
                .len(),
            1
        );
    }

    #[test]
    fn control_token_is_created_once_replaces_damage_and_retires_the_genesis_token() {
        let root =
            std::env::temp_dir().join(format!("abigail-control-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(LEGACY_GENESIS_TOKEN_FILE), "a".repeat(64)).unwrap();

        let first = load_or_create_control_token(&root).unwrap();
        assert!(contract::parse_control_token(&first).is_some());
        assert!(!root.join(LEGACY_GENESIS_TOKEN_FILE).exists());
        assert_eq!(load_or_create_control_token(&root).unwrap(), first);

        std::fs::write(root.join(contract::CONTROL_TOKEN_FILE), "damaged").unwrap();
        let replaced = load_or_create_control_token(&root).unwrap();
        assert_ne!(replaced, first);
        assert!(contract::parse_control_token(&replaced).is_some());
        assert_eq!(
            std::fs::read_to_string(root.join(contract::CONTROL_TOKEN_FILE)).unwrap(),
            replaced
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
