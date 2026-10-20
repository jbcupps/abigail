//! Scoped persistence over the local Hive HTTP boundary. Only Hive opens the
//! embedded database; Entity processes use their runtime lease to access their
//! own database in that store.

use crate::{EntityScope, PersistenceError, PersistenceHandle, QueryBinding};
use abigail_core::profile::{ProcessIdentity, ProfileContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum RemoteOperation {
    Execute {
        sql: String,
        bindings: Vec<QueryBinding>,
    },
    QueryVec {
        sql: String,
        bindings: Vec<QueryBinding>,
    },
    QueryOne {
        sql: String,
        bindings: Vec<QueryBinding>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteResponse {
    pub ok: bool,
    pub result: Value,
    pub error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct HiveTransport {
    pub base_url: String,
    pub entity_id: String,
    pub lease_id: String,
    client: reqwest::Client,
    identity: Option<ProcessIdentity>,
}

static HIVE_TRANSPORT: OnceLock<HiveTransport> = OnceLock::new();

pub fn configure_hive_transport(
    base_url: &str,
    entity_id: &str,
    lease_id: &str,
) -> crate::client::Result<()> {
    let transport = HiveTransport::new(base_url, entity_id, lease_id)?;
    HIVE_TRANSPORT.set(transport).map_err(|_| {
        PersistenceError::Remote("Hive persistence is already configured for this process".into())
    })
}

/// Configure an Entity's persistence boundary using the already verified,
/// exact live Hive identity. Preview never adopts an unpinned endpoint.
pub fn configure_hive_transport_with_identity(
    base_url: &str,
    entity_id: &str,
    lease_id: &str,
    identity: &ProcessIdentity,
) -> crate::client::Result<()> {
    let profile = abigail_core::profile::current()
        .map_err(|error| PersistenceError::Remote(error.to_string()))?
        .ok_or_else(|| {
            PersistenceError::Remote(
                "Pinned Hive persistence requires a validated preview profile".into(),
            )
        })?;
    let transport =
        HiveTransport::new_with_profile(base_url, entity_id, lease_id, &profile, identity)?;
    HIVE_TRANSPORT.set(transport).map_err(|_| {
        PersistenceError::Remote("Hive persistence is already configured for this process".into())
    })
}

pub(crate) fn configured_transport() -> Option<HiveTransport> {
    HIVE_TRANSPORT.get().cloned()
}

impl HiveTransport {
    pub(crate) fn new(
        base_url: &str,
        entity_id: &str,
        lease_id: &str,
    ) -> crate::client::Result<Self> {
        if abigail_core::profile::current()
            .map_err(|error| PersistenceError::Remote(error.to_string()))?
            .is_some()
        {
            return Err(PersistenceError::Remote(
                "Preview Hive persistence requires an explicit process identity".into(),
            ));
        }
        Self::build(base_url, entity_id, lease_id, None)
    }

    fn new_with_profile(
        base_url: &str,
        entity_id: &str,
        lease_id: &str,
        profile: &ProfileContext,
        identity: &ProcessIdentity,
    ) -> crate::client::Result<Self> {
        profile
            .validate_identity(identity)
            .map_err(|error| PersistenceError::Remote(error.to_string()))?;
        if identity.instance_id != identity.hive_instance_id {
            return Err(PersistenceError::Remote(
                "Persistence must be pinned to the Hive process instance".into(),
            ));
        }
        Self::build(base_url, entity_id, lease_id, Some(identity.clone()))
    }

    fn build(
        base_url: &str,
        entity_id: &str,
        lease_id: &str,
        identity: Option<ProcessIdentity>,
    ) -> crate::client::Result<Self> {
        let url =
            reqwest::Url::parse(base_url).map_err(|e| PersistenceError::Remote(e.to_string()))?;
        if url.scheme() != "http"
            || !matches!(
                url.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
            )
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(PersistenceError::Remote(
                "Hive persistence requires a local HTTP endpoint".into(),
            ));
        }
        if identity.is_some() {
            let port = url.port_or_known_default().filter(|port| *port != 0);
            if port.is_none_or(|port| {
                (43141..=43150).contains(&port) || base_url != format!("http://127.0.0.1:{port}")
            }) {
                return Err(PersistenceError::Remote(
                    "Preview Hive persistence requires a canonical loopback endpoint".into(),
                ));
            }
        }
        uuid::Uuid::parse_str(entity_id).map_err(|e| PersistenceError::Remote(e.to_string()))?;
        if lease_id.is_empty() {
            return Err(PersistenceError::Remote(
                "Hive persistence requires a runtime lease".into(),
            ));
        }
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
        if identity.is_some() {
            builder = builder
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none());
        }
        let client = builder
            .build()
            .map_err(|error| PersistenceError::Remote(error.to_string()))?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            entity_id: entity_id.to_string(),
            lease_id: lease_id.to_string(),
            client,
            identity,
        })
    }

    pub(crate) fn validate_scope(&self, scope: &EntityScope) -> crate::client::Result<()> {
        if *scope == EntityScope::Entity(self.entity_id.clone()) {
            Ok(())
        } else {
            Err(PersistenceError::Remote(
                "An Entity runtime can only access its own persistence scope".into(),
            ))
        }
    }

    fn request(&self, operation: &RemoteOperation) -> reqwest::RequestBuilder {
        let request = self
            .client
            .post(format!(
                "{}/v1/entities/{}/persistence",
                self.base_url, self.entity_id
            ))
            .bearer_auth(&self.lease_id)
            .json(operation);
        match &self.identity {
            // Ownership IDs do not replace the independent runtime lease.
            Some(identity) => request
                .header("x-abigail-profile-id", &identity.profile_id)
                .header("x-abigail-process-instance-id", &identity.instance_id),
            None => request,
        }
    }

    pub(crate) async fn call(&self, operation: RemoteOperation) -> crate::client::Result<Value> {
        let response = self
            .request(&operation)
            .send()
            .await
            .map_err(|e| PersistenceError::Remote(e.to_string()))?
            .error_for_status()
            .map_err(|e| PersistenceError::Remote(e.to_string()))?
            .json::<RemoteResponse>()
            .await
            .map_err(|e| PersistenceError::Remote(e.to_string()))?;
        if response.ok {
            Ok(response.result)
        } else {
            Err(PersistenceError::Remote(response.error.unwrap_or_else(
                || "Hive persistence request failed".into(),
            )))
        }
    }
}

impl RemoteOperation {
    /// Execute on a Hive-local scoped handle, never on another remote handle.
    pub fn execute(self, handle: &PersistenceHandle) -> crate::client::Result<Value> {
        match self {
            Self::Execute { sql, bindings } => {
                validate_sql(&sql)?;
                handle.execute(&sql, &bindings)?;
                Ok(Value::Null)
            }
            Self::QueryVec { sql, bindings } => {
                validate_sql(&sql)?;
                Ok(serde_json::to_value(
                    handle.query_vec::<Value>(&sql, &bindings)?,
                )?)
            }
            Self::QueryOne { sql, bindings } => {
                validate_sql(&sql)?;
                Ok(serde_json::to_value(
                    handle.query_one::<Value>(&sql, &bindings)?,
                )?)
            }
            Self::Upsert { table, id, value } => {
                validate_table(&table)?;
                handle.upsert(&table, &id, &value)?;
                Ok(Value::Null)
            }
            Self::Create { table, id, value } => {
                validate_table(&table)?;
                handle.create(&table, &id, &value)?;
                Ok(Value::Null)
            }
            Self::Delete { table, id } => {
                validate_table(&table)?;
                handle.delete_record(&table, &id)?;
                Ok(Value::Null)
            }
            Self::Select { table, id } => {
                validate_table(&table)?;
                Ok(serde_json::to_value(
                    handle.select_record::<Value>(&table, &id)?,
                )?)
            }
        }
    }
}

fn validate_table(table: &str) -> crate::client::Result<()> {
    if !table.is_empty()
        && table
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Ok(())
    } else {
        Err(PersistenceError::Remote("Invalid persistence table".into()))
    }
}

fn validate_sql(sql: &str) -> crate::client::Result<()> {
    // The runtime's reads use this small grammar; writes use structured record
    // operations. Do not execute arbitrary SurrealQL with the embedded engine's
    // authority. In particular, schema controls have short DB/NS aliases and
    // side effects may be nested in otherwise ordinary SELECT expressions.
    let invalid = || {
        PersistenceError::Remote("Only scoped table reads are allowed as persistence SQL".into())
    };
    if sql.len() > 64 * 1024 {
        return Err(invalid());
    }
    let normalized = sql.trim().trim_end_matches(';').replace(',', " , ");
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.len() < 4
        || !words[0].eq_ignore_ascii_case("SELECT")
        || words[1] != "*"
        || !words[2].eq_ignore_ascii_case("FROM")
    {
        return Err(invalid());
    }
    validate_table(words[3])?;
    let mut index = 4;
    if words
        .get(index)
        .is_some_and(|word| word.eq_ignore_ascii_case("WHERE"))
    {
        validate_table(words.get(index + 1).ok_or_else(invalid)?)?;
        if words.get(index + 2) != Some(&"=") {
            return Err(invalid());
        }
        let parameter = words
            .get(index + 3)
            .and_then(|word| word.strip_prefix('$'))
            .ok_or_else(invalid)?;
        validate_table(parameter)?;
        index += 4;
    }
    if words
        .get(index)
        .is_some_and(|word| word.eq_ignore_ascii_case("ORDER"))
    {
        if !words
            .get(index + 1)
            .is_some_and(|word| word.eq_ignore_ascii_case("BY"))
        {
            return Err(invalid());
        }
        index += 2;
        loop {
            validate_table(words.get(index).ok_or_else(invalid)?)?;
            if !words.get(index + 1).is_some_and(|word| {
                word.eq_ignore_ascii_case("ASC") || word.eq_ignore_ascii_case("DESC")
            }) {
                return Err(invalid());
            }
            index += 2;
            if words.get(index) != Some(&",") {
                break;
            }
            index += 1;
        }
    }
    if words
        .get(index)
        .is_some_and(|word| word.eq_ignore_ascii_case("LIMIT"))
    {
        words
            .get(index + 1)
            .ok_or_else(invalid)?
            .parse::<usize>()
            .map_err(|_| invalid())?;
        index += 2;
    }
    if index != words.len() {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PreviewFixture {
        root: std::path::PathBuf,
        profile: ProfileContext,
    }

    impl PreviewFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "abigail-preview-transport-{}",
                uuid::Uuid::new_v4()
            ));
            let profile = ProfileContext::preflight(&root, &[]).unwrap();
            Self { root, profile }
        }

        fn hive_identity(&self) -> ProcessIdentity {
            let instance = uuid::Uuid::new_v4().to_string();
            self.profile.process_identity(&instance, &instance).unwrap()
        }
    }

    impl Drop for PreviewFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn preview_transport_builds_a_pinned_request_without_opening_a_socket() {
        let fixture = PreviewFixture::new();
        let identity = fixture.hive_identity();
        let entity = uuid::Uuid::new_v4().to_string();
        let transport = HiveTransport::new_with_profile(
            "http://127.0.0.1:51111",
            &entity,
            "synthetic-lease",
            &fixture.profile,
            &identity,
        )
        .unwrap();
        let request = transport
            .request(&RemoteOperation::Select {
                table: "conversation_turn".into(),
                id: "synthetic-turn".into(),
            })
            .build()
            .unwrap();
        assert_eq!(
            request.url().as_str(),
            format!("http://127.0.0.1:51111/v1/entities/{entity}/persistence")
        );
        assert_eq!(
            request.headers()["x-abigail-profile-id"],
            identity.profile_id
        );
        assert_eq!(
            request.headers()["x-abigail-process-instance-id"],
            identity.instance_id
        );
        assert_eq!(
            request.headers()[reqwest::header::AUTHORIZATION],
            "Bearer synthetic-lease"
        );
        assert!(transport
            .validate_scope(&EntityScope::Entity(entity))
            .is_ok());
        assert!(transport.validate_scope(&EntityScope::Hive).is_err());
    }

    #[test]
    fn preview_transport_rejects_other_profile_root_channel_and_entity_instance() {
        let fixture = PreviewFixture::new();
        let expected = fixture.hive_identity();
        let entity = uuid::Uuid::new_v4().to_string();
        let mut differences = vec![expected.clone(); 5];
        differences[0].channel = "stable".into();
        differences[1].profile_id = uuid::Uuid::new_v4().to_string();
        differences[2].canonical_data_root = fixture.root.join("other");
        differences[3].instance_id = uuid::Uuid::new_v4().to_string();
        differences[4].hive_instance_id = "invalid-instance".into();
        for actual in differences {
            assert!(HiveTransport::new_with_profile(
                "http://127.0.0.1:51111",
                &entity,
                "synthetic-lease",
                &fixture.profile,
                &actual,
            )
            .is_err());
        }
    }

    #[test]
    fn preview_transport_requires_exact_endpoint_and_preserves_stable_url_compatibility() {
        let fixture = PreviewFixture::new();
        let identity = fixture.hive_identity();
        let entity = uuid::Uuid::new_v4().to_string();
        for url in [
            "http://127.0.0.1:43141",
            "http://127.0.0.1:43150",
            "http://localhost:51111",
            "http://127.0.0.1:51111/",
            "http://127.0.0.1:51111?other=profile",
            "http://127.0.0.1:0",
            "http://127.0.0.1",
            "http://127.0.0.1:051111",
        ] {
            assert!(
                HiveTransport::new_with_profile(
                    url,
                    &entity,
                    "synthetic-lease",
                    &fixture.profile,
                    &identity,
                )
                .is_err(),
                "{url}"
            );
        }
        let stable =
            HiveTransport::build("http://localhost:43141/", &entity, "synthetic-lease", None)
                .unwrap();
        assert_eq!(stable.base_url, "http://localhost:43141");
        let request = stable
            .request(&RemoteOperation::Select {
                table: "conversation_turn".into(),
                id: "synthetic-turn".into(),
            })
            .build()
            .unwrap();
        assert!(!request.headers().contains_key("x-abigail-profile-id"));
        assert!(!request
            .headers()
            .contains_key("x-abigail-process-instance-id"));
    }

    #[test]
    fn remote_transport_rejects_external_hive_and_cross_entity_scope() {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(HiveTransport::new("https://example.com", &id, "lease").is_err());
        let transport = HiveTransport::new("http://127.0.0.1:43141", &id, "lease").unwrap();
        assert!(transport.validate_scope(&EntityScope::Entity(id)).is_ok());
        assert!(transport.validate_scope(&EntityScope::Hive).is_err());
        assert!(transport
            .validate_scope(&EntityScope::Entity(uuid::Uuid::new_v4().to_string()))
            .is_err());
    }
    #[test]
    fn remote_queries_cannot_switch_database() {
        assert!(validate_sql("USE NS abigail DB other; SELECT * FROM conversation_turn").is_err());
        assert!(validate_sql("RETURN http::get('https://example.com')").is_err());
        assert!(validate_sql("REMOVE DB entity_other").is_err());
        assert!(validate_sql("REMOVE NS abigail").is_err());
        assert!(validate_sql("SELECT * FROM conversation_turn; REMOVE NS abigail").is_err());
        assert!(validate_sql("SELECT * FROM (REMOVE DB entity_other)").is_err());
        assert!(
            validate_sql("SELECT http::get('https://example.com') FROM conversation_turn").is_err()
        );
        assert!(
            validate_sql("SELECT * FROM conversation_turn WHERE session_id = $session_id").is_ok()
        );
        assert!(validate_sql("SELECT * FROM conversation_turn WHERE session_id = $session_id ORDER BY turn_number DESC, created_at DESC LIMIT 100").is_ok());
    }
}
