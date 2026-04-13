use entity_core::{EntityOutboxRecord, EntityOutboxStatus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const OUTBOX_FILE_NAME: &str = "runtime_outbox.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedOutbox {
    #[serde(default)]
    records: Vec<EntityOutboxRecord>,
    #[serde(default)]
    last_sync_at_utc: Option<String>,
    #[serde(default)]
    last_sync_error: Option<String>,
}

pub struct RuntimeOutbox {
    path: PathBuf,
    max_records: usize,
    inner: Mutex<PersistedOutbox>,
}

impl RuntimeOutbox {
    pub fn load(root_dir: impl AsRef<std::path::Path>, max_records: usize) -> anyhow::Result<Self> {
        let path =
            abigail_core::path_guard::trusted_file_path(root_dir.as_ref(), OUTBOX_FILE_NAME)?;
        let inner = if path.exists() {
            let bytes = fs::read_to_string(&path)?;
            serde_json::from_str(&bytes)?
        } else {
            PersistedOutbox::default()
        };

        Ok(Self {
            path,
            max_records,
            inner: Mutex::new(inner),
        })
    }

    pub fn enqueue(
        &self,
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<EntityOutboxRecord, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let record = EntityOutboxRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            created_at_utc: chrono::Utc::now().to_rfc3339(),
            payload,
        };
        inner.records.push(record.clone());
        if inner.records.len() > self.max_records {
            let overflow = inner.records.len() - self.max_records;
            inner.records.drain(0..overflow);
        }
        self.save_locked(&inner).map_err(|e| e.to_string())?;
        Ok(record)
    }

    pub fn snapshot(&self) -> Result<Vec<EntityOutboxRecord>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner.records.clone())
    }

    pub fn acknowledge(&self, accepted_record_ids: &[String]) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner
            .records
            .retain(|record| !accepted_record_ids.iter().any(|id| id == &record.record_id));
        inner.last_sync_at_utc = Some(chrono::Utc::now().to_rfc3339());
        inner.last_sync_error = None;
        self.save_locked(&inner).map_err(|e| e.to_string())
    }

    pub fn mark_sync_error(&self, error: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        inner.last_sync_error = Some(error.to_string());
        self.save_locked(&inner).map_err(|e| e.to_string())
    }

    pub fn status(&self) -> Result<EntityOutboxStatus, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(EntityOutboxStatus {
            queued_records: inner.records.len(),
            oldest_record_at_utc: inner
                .records
                .first()
                .map(|record| record.created_at_utc.clone()),
            last_sync_at_utc: inner.last_sync_at_utc.clone(),
            last_sync_error: inner.last_sync_error.clone(),
        })
    }

    fn save_locked(&self, inner: &PersistedOutbox) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(inner)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeOutbox;

    #[test]
    fn outbox_round_trips_and_acknowledges() {
        let root = std::env::temp_dir().join(format!("abigail-outbox-{}", uuid::Uuid::new_v4()));
        let outbox = RuntimeOutbox::load(&root, 4).unwrap();

        let first = outbox
            .enqueue(
                "entity-1",
                "chat_turn",
                serde_json::json!({ "message": "hi" }),
            )
            .unwrap();
        let second = outbox
            .enqueue(
                "entity-1",
                "memory_insert",
                serde_json::json!({ "content": "remember this" }),
            )
            .unwrap();

        let snapshot = outbox.snapshot().unwrap();
        assert_eq!(snapshot.len(), 2);

        outbox
            .acknowledge(std::slice::from_ref(&first.record_id))
            .unwrap();
        let snapshot = outbox.snapshot().unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].record_id, second.record_id);
    }
}
