use entity_core::{EntityOutboxRecord, EntityOutboxStatus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(test)]
use {
    std::path::Path,
    std::sync::Arc,
};

const OUTBOX_FILE_NAME: &str = "runtime_outbox.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
    #[cfg(test)]
    test_save: Option<Arc<TestSave>>,
}

#[cfg(test)]
type TestSave = dyn Fn(&Path, &[u8]) -> anyhow::Result<()> + Send + Sync;

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
            #[cfg(test)]
            test_save: None,
        })
    }

    #[cfg(test)]
    fn load_with_saver(
        root_dir: impl AsRef<std::path::Path>,
        max_records: usize,
        save: impl Fn(&Path, &[u8]) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let mut outbox = Self::load(root_dir, max_records)?;
        outbox.test_save = Some(Arc::new(save));
        Ok(outbox)
    }

    pub fn enqueue(
        &self,
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<EntityOutboxRecord, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if inner.records.len() >= self.max_records {
            return Err(format!(
                "Runtime outbox backpressure: capacity {} reached; retry after Hive acknowledges queued records.",
                self.max_records
            ));
        }
        let record = EntityOutboxRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            created_at_utc: chrono::Utc::now().to_rfc3339(),
            payload,
        };
        let mut next = inner.clone();
        next.records.push(record.clone());
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(record)
    }

    pub fn snapshot(&self) -> Result<Vec<EntityOutboxRecord>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner.records.clone())
    }

    pub fn acknowledge(&self, accepted_record_ids: &[String]) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = inner.clone();
        next
            .records
            .retain(|record| !accepted_record_ids.iter().any(|id| id == &record.record_id));
        next.last_sync_at_utc = Some(chrono::Utc::now().to_rfc3339());
        next.last_sync_error = None;
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(())
    }

    pub fn mark_sync_error(&self, error: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = inner.clone();
        next.last_sync_error = Some(error.to_string());
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(())
    }

    pub fn finish_sync(
        &self,
        result: Result<hive_core::OutboxSyncResponse, String>,
    ) -> Result<(), String> {
        match result {
            Ok(response) => self.acknowledge(&response.accepted_record_ids),
            Err(error) => {
                self.mark_sync_error(&error)?;
                Err(error)
            }
        }
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
        let bytes = serde_json::to_vec_pretty(inner)?;
        #[cfg(test)]
        if let Some(save) = &self.test_save {
            return save(&self.path, &bytes);
        }
        abigail_core::secure_fs::write_bytes_atomic(&self.path, &bytes).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeOutbox;
    use hive_core::OutboxSyncResponse;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{label}-{}", uuid::Uuid::new_v4()))
    }

    fn fail_save_on(
        failed_call: usize,
    ) -> impl Fn(&std::path::Path, &[u8]) -> anyhow::Result<()> + Send + Sync {
        let calls = Arc::new(AtomicUsize::new(0));
        move |path, bytes| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == failed_call {
                anyhow::bail!("injected outbox save failure");
            }
            abigail_core::secure_fs::write_bytes_atomic(path, bytes)?;
            Ok(())
        }
    }

    fn assert_records_equal(
        actual: &[hive_core::EntityOutboxRecord],
        expected: &[hive_core::EntityOutboxRecord],
    ) {
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn normal_acknowledgement_survives_restart() {
        let root = root("abigail-outbox");
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
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 4).unwrap();
        let restarted_snapshot = restarted.snapshot().unwrap();
        assert_eq!(restarted_snapshot.len(), 1);
        assert_eq!(restarted_snapshot[0].record_id, second.record_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn capacity_applies_backpressure_without_evicting_unacknowledged_records() {
        let root = root("abigail-outbox-capacity");
        let outbox = RuntimeOutbox::load(&root, 2).unwrap();
        let first = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 1}))
            .unwrap();
        let second = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 2}))
            .unwrap();
        let before = outbox.snapshot().unwrap();

        let error = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 3}))
            .unwrap_err();
        assert!(error.contains("backpressure"), "{error}");
        assert!(error.contains("capacity 2"), "{error}");
        assert_records_equal(&outbox.snapshot().unwrap(), &before);
        assert_eq!(
            before
                .iter()
                .map(|record| record.record_id.as_str())
                .collect::<Vec<_>>(),
            vec![first.record_id.as_str(), second.record_id.as_str()]
        );

        drop(outbox);
        let restarted = RuntimeOutbox::load(&root, 2).unwrap();
        assert_records_equal(&restarted.snapshot().unwrap(), &before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn enqueue_save_failure_keeps_memory_and_disk_at_the_prior_state() {
        let root = root("abigail-outbox-enqueue-save");
        let outbox = RuntimeOutbox::load_with_saver(&root, 4, fail_save_on(2)).unwrap();
        let first = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 1}))
            .unwrap();

        let error = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 2}))
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert_records_equal(&outbox.snapshot().unwrap(), std::slice::from_ref(&first));

        let restarted = RuntimeOutbox::load(&root, 4).unwrap();
        assert_records_equal(&restarted.snapshot().unwrap(), std::slice::from_ref(&first));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acknowledge_save_failure_keeps_records_unacknowledged_in_memory_and_on_disk() {
        let root = root("abigail-outbox-ack-save");
        let outbox = RuntimeOutbox::load_with_saver(&root, 4, fail_save_on(3)).unwrap();
        let first = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 1}))
            .unwrap();
        let second = outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 2}))
            .unwrap();
        let before = vec![first.clone(), second];

        let error = outbox
            .acknowledge(std::slice::from_ref(&first.record_id))
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert_records_equal(&outbox.snapshot().unwrap(), &before);

        let restarted = RuntimeOutbox::load(&root, 4).unwrap();
        assert_records_equal(&restarted.snapshot().unwrap(), &before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restart_rejects_a_corrupt_outbox_instead_of_silently_losing_records() {
        let root = root("abigail-outbox-corrupt");
        let outbox = RuntimeOutbox::load(&root, 4).unwrap();
        outbox
            .enqueue("entity-1", "skill_used", serde_json::json!({"index": 1}))
            .unwrap();
        drop(outbox);
        std::fs::write(root.join(super::OUTBOX_FILE_NAME), b"{not-json").unwrap();

        let error = RuntimeOutbox::load(&root, 4)
            .err()
            .expect("corrupt outbox must fail closed");
        assert!(error.to_string().contains("key must be a string"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_sync_retains_exact_records_until_a_successful_acknowledgement() {
        let root =
            std::env::temp_dir().join(format!("abigail-outbox-retry-{}", uuid::Uuid::new_v4()));
        let outbox = RuntimeOutbox::load(&root, 4).unwrap();
        let record = outbox
            .enqueue(
                "entity-1",
                "skill_used",
                serde_json::json!({
                    "session_id": "session-1",
                    "skill_id": "com.abigail.calendar",
                    "tool_name": "list_events"
                }),
            )
            .unwrap();

        assert!(outbox
            .finish_sync(Err("Hive error: journey projection failed".to_string()))
            .is_err());
        let failed_status = outbox.status().unwrap();
        assert_eq!(failed_status.queued_records, 1);
        assert_eq!(
            outbox.snapshot().unwrap()[0].record_id,
            record.record_id,
            "an error envelope must retain the exact record ID for retry"
        );

        outbox
            .finish_sync(Ok(OutboxSyncResponse {
                accepted_record_ids: vec![record.record_id],
                pending_records: 0,
                server_time_utc: "2026-10-22T12:00:01Z".into(),
                execution_receipts: vec![],
            }))
            .unwrap();
        assert_eq!(outbox.status().unwrap().queued_records, 0);
    }
}
