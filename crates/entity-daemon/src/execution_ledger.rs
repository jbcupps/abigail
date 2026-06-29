use hive_core::ExecutionEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct LedgerFile {
    events: Vec<ExecutionEvent>,
}

#[derive(Debug)]
pub struct ExecutionLedger {
    path: PathBuf,
    events: Mutex<Vec<ExecutionEvent>>,
}

impl ExecutionLedger {
    pub fn load(entity_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = entity_dir.as_ref().join("execution_ledger.json");
        let events = if path.exists() {
            let bytes = std::fs::read(&path)?;
            serde_json::from_slice::<LedgerFile>(&bytes)
                .map(|ledger| ledger.events)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            events: Mutex::new(events),
        })
    }

    pub fn append_outbox_event(
        &self,
        entity_id: &str,
        kind: &str,
        payload: &serde_json::Value,
        soul_ref: &str,
    ) -> Result<ExecutionEvent, String> {
        let session_id = payload
            .get("session_id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        self.append_event(entity_id, session_id, kind, payload, soul_ref)
    }

    pub fn append_event(
        &self,
        entity_id: &str,
        session_id: Option<String>,
        event_kind: &str,
        payload: &serde_json::Value,
        soul_ref: &str,
    ) -> Result<ExecutionEvent, String> {
        let mut events = self.events.lock().map_err(|e| e.to_string())?;
        let previous_hash = events.last().map(|event| event.event_hash.clone());
        let created_at_utc = chrono::Utc::now().to_rfc3339();
        let payload_digest = sha256_hex(&serde_json::to_vec(payload).map_err(|e| e.to_string())?);
        let event_id = uuid::Uuid::new_v4().to_string();
        let event_hash = event_hash(EventHashInput {
            event_id: &event_id,
            entity_id,
            session_id: session_id.as_deref(),
            event_kind,
            payload_digest: &payload_digest,
            previous_hash: previous_hash.as_deref(),
            created_at_utc: &created_at_utc,
            soul_ref,
        });
        let event = ExecutionEvent {
            schema_version: "execution_event_v1".to_string(),
            event_id,
            entity_id: entity_id.to_string(),
            session_id,
            event_kind: event_kind.to_string(),
            payload_digest,
            previous_hash,
            event_hash,
            created_at_utc,
            soul_ref: soul_ref.to_string(),
        };
        events.push(event.clone());
        self.persist(&events)?;
        Ok(event)
    }

    pub fn recent(&self, session_id: Option<&str>, limit: usize) -> Vec<ExecutionEvent> {
        let Ok(events) = self.events.lock() else {
            return Vec::new();
        };
        let mut filtered = events
            .iter()
            .filter(|event| {
                session_id
                    .map(|session_id| event.session_id.as_deref() == Some(session_id))
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        if filtered.len() > limit {
            filtered = filtered.split_off(filtered.len() - limit);
        }
        filtered
    }

    fn persist(&self, events: &[ExecutionEvent]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(&LedgerFile {
            events: events.to_vec(),
        })
        .map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub fn verify_chain(events: &[ExecutionEvent]) -> bool {
    let mut previous_hash: Option<String> = None;
    for event in events {
        if event.schema_version != "execution_event_v1" {
            return false;
        }
        if event.previous_hash != previous_hash {
            return false;
        }
        let expected = event_hash(EventHashInput {
            event_id: &event.event_id,
            entity_id: &event.entity_id,
            session_id: event.session_id.as_deref(),
            event_kind: &event.event_kind,
            payload_digest: &event.payload_digest,
            previous_hash: event.previous_hash.as_deref(),
            created_at_utc: &event.created_at_utc,
            soul_ref: &event.soul_ref,
        });
        if event.event_hash != expected {
            return false;
        }
        previous_hash = Some(event.event_hash.clone());
    }
    true
}

struct EventHashInput<'a> {
    event_id: &'a str,
    entity_id: &'a str,
    session_id: Option<&'a str>,
    event_kind: &'a str,
    payload_digest: &'a str,
    previous_hash: Option<&'a str>,
    created_at_utc: &'a str,
    soul_ref: &'a str,
}

fn event_hash(input: EventHashInput<'_>) -> String {
    let payload = format!(
        "execution_event_v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        input.event_id,
        input.entity_id,
        input.session_id.unwrap_or(""),
        input.event_kind,
        input.payload_digest,
        input.previous_hash.unwrap_or(""),
        input.created_at_utc,
        input.soul_ref
    );
    sha256_hex(payload.as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    to_hex(&digest)
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_chain_verifies_and_tamper_breaks_it() {
        let dir =
            std::env::temp_dir().join(format!("abigail-execution-ledger-{}", uuid::Uuid::new_v4()));
        let ledger = ExecutionLedger::load(&dir).unwrap();
        ledger
            .append_event(
                "entity-1",
                Some("session-1".to_string()),
                "chat_user_turn",
                &serde_json::json!({ "message": "hello" }),
                "soul-ref",
            )
            .unwrap();
        ledger
            .append_event(
                "entity-1",
                Some("session-1".to_string()),
                "chat_assistant_turn",
                &serde_json::json!({ "reply": "hello" }),
                "soul-ref",
            )
            .unwrap();

        let mut events = ledger.recent(None, 10);
        assert!(verify_chain(&events));
        events[1].payload_digest = "tampered".to_string();
        assert!(!verify_chain(&events));
        let _ = std::fs::remove_dir_all(dir);
    }
}
