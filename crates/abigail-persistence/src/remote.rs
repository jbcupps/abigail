//! Scoped persistence over the local Hive HTTP boundary. Only Hive opens the
//! embedded database; Entity processes use their runtime lease to access their
//! own database in that store.

use crate::{EntityScope, PersistenceError, PersistenceHandle, QueryBinding};
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

pub(crate) fn configured_transport() -> Option<HiveTransport> {
    HIVE_TRANSPORT.get().cloned()
}

impl HiveTransport {
    pub(crate) fn new(
        base_url: &str,
        entity_id: &str,
        lease_id: &str,
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
        uuid::Uuid::parse_str(entity_id).map_err(|e| PersistenceError::Remote(e.to_string()))?;
        if lease_id.is_empty() {
            return Err(PersistenceError::Remote(
                "Hive persistence requires a runtime lease".into(),
            ));
        }
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            entity_id: entity_id.to_string(),
            lease_id: lease_id.to_string(),
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

    pub(crate) async fn call(&self, operation: RemoteOperation) -> crate::client::Result<Value> {
        let response = reqwest::Client::new()
            .post(format!(
                "{}/v1/entities/{}/persistence",
                self.base_url, self.entity_id
            ))
            .bearer_auth(&self.lease_id)
            .timeout(std::time::Duration::from_secs(30))
            .json(&operation)
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
