use entity_core::{EntityOutboxRecord, EntityOutboxStatus, JourneyQuarantineDiagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(test)]
use {std::path::Path, std::sync::Arc};

const OUTBOX_FILE_NAME: &str = "runtime_outbox.json";
const JOURNEY_QUARANTINE_REASON: &str =
    "No matching durable fact was found after the reservation recovery threshold";

#[derive(Clone, Debug)]
pub struct OutboxRecordDraft {
    pub record_id: String,
    pub kind: String,
    pub payload: serde_json::Value,
    soul_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxReservation {
    pub record_id: String,
    pub entity_id: String,
    pub kind: String,
    pub created_at_utc: String,
    #[serde(default)]
    pub soul_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantinedOutboxReservation {
    pub reservation: OutboxReservation,
    pub quarantined_at_utc: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OutboxLedgerMetadata {
    pub record_id: String,
    pub entity_id: String,
    pub kind: String,
    pub session_id: Option<String>,
    pub payload_digest: String,
    pub soul_ref: String,
}

impl OutboxLedgerMetadata {
    fn from_payload(
        record_id: &str,
        entity_id: &str,
        kind: &str,
        payload: &serde_json::Value,
        soul_ref: &str,
    ) -> Result<Self, String> {
        Ok(Self {
            record_id: record_id.to_string(),
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            session_id: payload
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            payload_digest: crate::execution_ledger::execution_payload_digest(payload)?,
            soul_ref: soul_ref.to_string(),
        })
    }

    pub(crate) fn from_execution_event(event: &hive_core::ExecutionEvent) -> Self {
        Self {
            record_id: event.event_id.clone(),
            entity_id: event.entity_id.clone(),
            kind: event.event_kind.clone(),
            session_id: event.session_id.clone(),
            payload_digest: event.payload_digest.clone(),
            soul_ref: event.soul_ref.clone(),
        }
    }

    pub(crate) fn validate_source(
        &self,
        record_id: &str,
        entity_id: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        let expected = Self::from_payload(record_id, entity_id, kind, payload, &self.soul_ref)?;
        if self.same_source(&expected) {
            Ok(())
        } else {
            Err(format!(
                "Runtime outbox ledger metadata for record {record_id} conflicts with durable activity"
            ))
        }
    }

    fn same_source(&self, other: &Self) -> bool {
        self.record_id == other.record_id
            && self.entity_id == other.entity_id
            && self.kind == other.kind
            && self.session_id == other.session_id
            && self.payload_digest == other.payload_digest
    }
}

#[derive(Clone, Debug)]
pub enum ReservationRecovery {
    All,
    StaleBefore(chrono::DateTime<chrono::Utc>),
}

#[derive(Debug)]
pub enum DurableRecordReconcile {
    Queued(EntityOutboxRecord),
    Acknowledged,
    Deferred,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedOutbox {
    #[serde(default)]
    records: Vec<EntityOutboxRecord>,
    #[serde(default)]
    reservations: Vec<OutboxReservation>,
    #[serde(default)]
    quarantined_reservations: Vec<QuarantinedOutboxReservation>,
    #[serde(default)]
    acknowledged_record_ids: Vec<String>,
    #[serde(default)]
    record_ledger_metadata: Vec<OutboxLedgerMetadata>,
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
    pub(crate) fn load_with_saver(
        root_dir: impl AsRef<std::path::Path>,
        max_records: usize,
        save: impl Fn(&Path, &[u8]) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let mut outbox = Self::load(root_dir, max_records)?;
        outbox.test_save = Some(Arc::new(save));
        Ok(outbox)
    }

    #[cfg(test)]
    pub fn enqueue(
        &self,
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<EntityOutboxRecord, String> {
        self.enqueue_with_id(uuid::Uuid::new_v4().to_string(), entity_id, kind, payload)
    }

    #[cfg(test)]
    pub fn enqueue_with_id(
        &self,
        record_id: String,
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<EntityOutboxRecord, String> {
        self.enqueue_batch(
            entity_id,
            vec![OutboxRecordDraft {
                record_id,
                kind: kind.to_string(),
                payload,
                soul_ref: None,
            }],
        )?
        .into_iter()
        .next()
        .ok_or_else(|| "Runtime outbox did not stage the requested record".to_string())
    }

    pub(crate) fn enqueue_with_ledger_metadata(
        &self,
        record_id: String,
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
        soul_ref: &str,
    ) -> Result<EntityOutboxRecord, String> {
        self.enqueue_batch(
            entity_id,
            vec![OutboxRecordDraft {
                record_id,
                kind: kind.to_string(),
                payload,
                soul_ref: Some(soul_ref.to_string()),
            }],
        )?
        .into_iter()
        .next()
        .ok_or_else(|| "Runtime outbox did not stage the requested record".to_string())
    }

    pub fn enqueue_batch(
        &self,
        entity_id: &str,
        drafts: Vec<OutboxRecordDraft>,
    ) -> Result<Vec<EntityOutboxRecord>, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut staged = Vec::with_capacity(drafts.len());
        let mut additions = Vec::new();
        let mut metadata_additions = Vec::new();
        for draft in drafts {
            validate_record_id(&draft.record_id)?;
            let ledger_metadata = draft
                .soul_ref
                .as_deref()
                .map(|soul_ref| {
                    OutboxLedgerMetadata::from_payload(
                        &draft.record_id,
                        entity_id,
                        &draft.kind,
                        &draft.payload,
                        soul_ref,
                    )
                })
                .transpose()?;
            if inner
                .quarantined_reservations
                .iter()
                .any(|quarantined| quarantined.reservation.record_id == draft.record_id)
            {
                return Err(format!(
                    "Runtime outbox record ID {} is quarantined for manual recovery",
                    draft.record_id
                ));
            }
            if inner
                .acknowledged_record_ids
                .iter()
                .any(|record_id| record_id == &draft.record_id)
            {
                return Err(format!(
                    "Runtime outbox record ID {} was already acknowledged",
                    draft.record_id
                ));
            }
            if inner
                .reservations
                .iter()
                .any(|reservation| reservation.record_id == draft.record_id)
            {
                return Err(format!(
                    "Runtime outbox record ID {} is reserved; commit its reservation",
                    draft.record_id
                ));
            }
            if let Some(existing) = inner
                .records
                .iter()
                .chain(additions.iter())
                .find(|record| record.record_id == draft.record_id)
            {
                if existing.entity_id != entity_id
                    || existing.kind != draft.kind
                    || existing.payload != draft.payload
                {
                    return Err(format!(
                        "Runtime outbox record ID {} conflicts with different event data",
                        draft.record_id
                    ));
                }
                staged.push(existing.clone());
                if let Some(metadata) = ledger_metadata {
                    stage_record_ledger_metadata(
                        &inner.record_ledger_metadata,
                        &mut metadata_additions,
                        metadata,
                    )?;
                }
                continue;
            }
            let record = EntityOutboxRecord {
                record_id: draft.record_id,
                entity_id: entity_id.to_string(),
                kind: draft.kind,
                created_at_utc: chrono::Utc::now().to_rfc3339(),
                payload: draft.payload,
            };
            staged.push(record.clone());
            additions.push(record);
            if let Some(metadata) = ledger_metadata {
                stage_record_ledger_metadata(
                    &inner.record_ledger_metadata,
                    &mut metadata_additions,
                    metadata,
                )?;
            }
        }
        if inner.records.len() + inner.reservations.len() + additions.len() > self.max_records {
            return Err(format!(
                "Runtime outbox backpressure: capacity {} reached; retry after Hive acknowledges queued records.",
                self.max_records
            ));
        }
        if additions.is_empty() && metadata_additions.is_empty() {
            return Ok(staged);
        }
        let mut next = inner.clone();
        next.records.extend(additions);
        next.record_ledger_metadata.extend(metadata_additions);
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(staged)
    }

    #[cfg(test)]
    pub fn reserve_slot(
        &self,
        record_id: String,
        entity_id: &str,
        kind: &str,
    ) -> Result<OutboxReservation, String> {
        self.reserve_slot_internal(record_id, entity_id, kind, "")
    }

    pub(crate) fn reserve_slot_with_soul_ref(
        &self,
        record_id: String,
        entity_id: &str,
        kind: &str,
        soul_ref: &str,
    ) -> Result<OutboxReservation, String> {
        if soul_ref.is_empty() {
            return Err(
                "Runtime outbox reservations require an original soul reference".to_string(),
            );
        }
        self.reserve_slot_internal(record_id, entity_id, kind, soul_ref)
    }

    fn reserve_slot_internal(
        &self,
        record_id: String,
        entity_id: &str,
        kind: &str,
        soul_ref: &str,
    ) -> Result<OutboxReservation, String> {
        validate_record_id(&record_id)?;
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if inner
            .quarantined_reservations
            .iter()
            .any(|quarantined| quarantined.reservation.record_id == record_id)
        {
            return Err(format!(
                "Runtime outbox record ID {record_id} is quarantined for manual recovery"
            ));
        }
        if inner
            .acknowledged_record_ids
            .iter()
            .any(|acknowledged| acknowledged == &record_id)
        {
            return Err(format!(
                "Runtime outbox record ID {record_id} was already acknowledged"
            ));
        }
        if let Some(existing) = inner
            .records
            .iter()
            .find(|record| record.record_id == record_id)
        {
            if existing.entity_id != entity_id || existing.kind != kind {
                return Err(format!(
                    "Runtime outbox record ID {record_id} conflicts with different event data"
                ));
            }
            return Err(format!(
                "Runtime outbox record ID {record_id} is already queued"
            ));
        }
        if let Some(existing) = inner
            .reservations
            .iter()
            .find(|reservation| reservation.record_id == record_id)
        {
            if existing.entity_id != entity_id || existing.kind != kind {
                return Err(format!(
                    "Runtime outbox reservation {record_id} conflicts with different event data"
                ));
            }
            return Ok(existing.clone());
        }
        if inner.records.len() + inner.reservations.len() >= self.max_records {
            return Err(format!(
                "Runtime outbox backpressure: capacity {} reached; retry after Hive acknowledges queued records.",
                self.max_records
            ));
        }

        let reservation = OutboxReservation {
            record_id,
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            created_at_utc: chrono::Utc::now().to_rfc3339(),
            soul_ref: soul_ref.to_string(),
        };
        let mut next = inner.clone();
        next.reservations.push(reservation.clone());
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(reservation)
    }

    #[cfg(test)]
    pub fn commit_reservation(
        &self,
        record_id: &str,
        payload: serde_json::Value,
    ) -> Result<EntityOutboxRecord, String> {
        self.commit_reservation_internal(record_id, payload, None)
    }

    pub(crate) fn commit_reservation_with_ledger_metadata(
        &self,
        record_id: &str,
        payload: serde_json::Value,
        soul_ref: &str,
    ) -> Result<EntityOutboxRecord, String> {
        self.commit_reservation_internal(record_id, payload, Some(soul_ref))
    }

    fn commit_reservation_internal(
        &self,
        record_id: &str,
        payload: serde_json::Value,
        soul_ref: Option<&str>,
    ) -> Result<EntityOutboxRecord, String> {
        validate_record_id(record_id)?;
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if inner
            .acknowledged_record_ids
            .iter()
            .any(|acknowledged| acknowledged == record_id)
        {
            return Err(format!(
                "Runtime outbox record ID {record_id} was already acknowledged"
            ));
        }
        if let Some(existing) = inner
            .records
            .iter()
            .find(|record| record.record_id == record_id)
            .cloned()
        {
            if existing.payload == payload {
                if let Some(soul_ref) = soul_ref {
                    let metadata = OutboxLedgerMetadata::from_payload(
                        record_id,
                        &existing.entity_id,
                        &existing.kind,
                        &payload,
                        soul_ref,
                    )?;
                    let mut next = inner.clone();
                    if merge_record_ledger_metadata(
                        &mut next.record_ledger_metadata,
                        metadata,
                        false,
                    )? {
                        self.save_locked(&next).map_err(|e| e.to_string())?;
                        *inner = next;
                    }
                }
                return Ok(existing);
            }
            return Err(format!(
                "Runtime outbox record ID {record_id} conflicts with different event data"
            ));
        }
        let reservation = inner
            .reservations
            .iter()
            .find(|reservation| reservation.record_id == record_id)
            .cloned()
            .ok_or_else(|| format!("Runtime outbox reservation {record_id} does not exist"))?;
        let authoritative_soul_ref = if reservation.soul_ref.is_empty() {
            soul_ref
        } else {
            Some(reservation.soul_ref.as_str())
        };
        let ledger_metadata = authoritative_soul_ref
            .map(|soul_ref| {
                OutboxLedgerMetadata::from_payload(
                    record_id,
                    &reservation.entity_id,
                    &reservation.kind,
                    &payload,
                    soul_ref,
                )
            })
            .transpose()?;
        let record = EntityOutboxRecord {
            record_id: reservation.record_id.clone(),
            entity_id: reservation.entity_id,
            kind: reservation.kind,
            created_at_utc: chrono::Utc::now().to_rfc3339(),
            payload,
        };
        let mut next = inner.clone();
        next.reservations
            .retain(|reservation| reservation.record_id != record_id);
        next.records.push(record.clone());
        if let Some(metadata) = ledger_metadata {
            merge_record_ledger_metadata(&mut next.record_ledger_metadata, metadata, true)?;
        }
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(record)
    }

    pub fn release_reservation(&self, record_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if !inner
            .reservations
            .iter()
            .any(|reservation| reservation.record_id == record_id)
        {
            return Ok(());
        }
        let mut next = inner.clone();
        next.reservations
            .retain(|reservation| reservation.record_id != record_id);
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(())
    }

    pub fn reconcile_durable_record(
        &self,
        record_id: &str,
        entity_id: &str,
        kind: &str,
        created_at_utc: &str,
        payload: serde_json::Value,
        recovery: ReservationRecovery,
    ) -> Result<DurableRecordReconcile, String> {
        validate_record_id(record_id)?;
        chrono::DateTime::parse_from_rfc3339(created_at_utc)
            .map_err(|_| "Durable activity fact has an invalid timestamp".to_string())?;
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        if inner
            .quarantined_reservations
            .iter()
            .any(|quarantined| quarantined.reservation.record_id == record_id)
        {
            return Err(format!(
                "Runtime outbox record ID {record_id} is quarantined for manual recovery"
            ));
        }
        let reservation = inner
            .reservations
            .iter()
            .find(|reservation| reservation.record_id == record_id)
            .cloned();
        if let Some(reservation) = reservation.as_ref() {
            if reservation.entity_id != entity_id || reservation.kind != kind {
                return Err(format!(
                    "Runtime outbox reservation {record_id} conflicts with durable activity"
                ));
            }
        }
        let recover_reservation = reservation
            .as_ref()
            .map(|reservation| reservation_is_recoverable(reservation, &recovery))
            .transpose()?
            .unwrap_or(false);
        let reservation_metadata = reservation
            .as_ref()
            .filter(|reservation| !reservation.soul_ref.is_empty())
            .map(|reservation| {
                OutboxLedgerMetadata::from_payload(
                    record_id,
                    entity_id,
                    kind,
                    &payload,
                    &reservation.soul_ref,
                )
            })
            .transpose()?;

        if inner
            .acknowledged_record_ids
            .iter()
            .any(|acknowledged| acknowledged == record_id)
        {
            if recover_reservation {
                let mut next = inner.clone();
                next.reservations
                    .retain(|reservation| reservation.record_id != record_id);
                if let Some(metadata) = reservation_metadata {
                    merge_record_ledger_metadata(&mut next.record_ledger_metadata, metadata, true)?;
                }
                self.save_locked(&next).map_err(|e| e.to_string())?;
                *inner = next;
            }
            return Ok(DurableRecordReconcile::Acknowledged);
        }

        if let Some(existing) = inner
            .records
            .iter()
            .find(|record| record.record_id == record_id)
            .cloned()
        {
            if existing.entity_id != entity_id
                || existing.kind != kind
                || existing.payload != payload
            {
                return Err(format!(
                    "Runtime outbox record ID {record_id} conflicts with durable activity"
                ));
            }
            if recover_reservation {
                let mut next = inner.clone();
                next.reservations
                    .retain(|reservation| reservation.record_id != record_id);
                if let Some(metadata) = reservation_metadata {
                    merge_record_ledger_metadata(&mut next.record_ledger_metadata, metadata, true)?;
                }
                self.save_locked(&next).map_err(|e| e.to_string())?;
                *inner = next;
            }
            return Ok(DurableRecordReconcile::Queued(existing));
        }

        if reservation.is_some() && !recover_reservation {
            return Ok(DurableRecordReconcile::Deferred);
        }
        if reservation.is_none()
            && inner.records.len() + inner.reservations.len() >= self.max_records
        {
            return Err(format!(
                "Runtime outbox backpressure: capacity {} reached; durable activity remains available for recovery.",
                self.max_records
            ));
        }

        let record = EntityOutboxRecord {
            record_id: record_id.to_string(),
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            created_at_utc: created_at_utc.to_string(),
            payload,
        };
        let mut next = inner.clone();
        next.reservations
            .retain(|reservation| reservation.record_id != record_id);
        next.records.push(record.clone());
        if let Some(metadata) = reservation_metadata {
            merge_record_ledger_metadata(&mut next.record_ledger_metadata, metadata, true)?;
        }
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(DurableRecordReconcile::Queued(record))
    }

    #[cfg(test)]
    pub fn reservations(&self) -> Result<Vec<OutboxReservation>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner.reservations.clone())
    }

    #[cfg(test)]
    pub fn quarantined_reservations(&self) -> Result<Vec<QuarantinedOutboxReservation>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner.quarantined_reservations.clone())
    }

    pub fn quarantine_diagnostics(&self) -> Result<Vec<JourneyQuarantineDiagnostic>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut diagnostics = inner
            .quarantined_reservations
            .iter()
            .map(|quarantined| JourneyQuarantineDiagnostic {
                record_id: quarantined.reservation.record_id.clone(),
                kind: quarantined.reservation.kind.clone(),
                quarantined_at_utc: quarantined.quarantined_at_utc.clone(),
                reason: JOURNEY_QUARANTINE_REASON.to_string(),
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| {
            left.quarantined_at_utc
                .cmp(&right.quarantined_at_utc)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });
        Ok(diagnostics)
    }

    pub fn quarantine_stale_unmatched_reservations(
        &self,
        durable_record_ids: &HashSet<String>,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut stale = Vec::new();
        for reservation in &inner.reservations {
            if durable_record_ids.contains(&reservation.record_id) {
                continue;
            }
            let created_at = chrono::DateTime::parse_from_rfc3339(&reservation.created_at_utc)
                .map_err(|_| "Runtime outbox reservation has an invalid timestamp".to_string())?
                .with_timezone(&chrono::Utc);
            if created_at <= stale_before {
                stale.push(reservation.clone());
            }
        }
        if stale.is_empty() {
            return Ok(0);
        }

        let stale_ids = stale
            .iter()
            .map(|reservation| reservation.record_id.clone())
            .collect::<HashSet<_>>();
        let quarantined = stale.len();
        let quarantined_at_utc = chrono::Utc::now().to_rfc3339();
        let mut next = inner.clone();
        next.reservations
            .retain(|reservation| !stale_ids.contains(reservation.record_id.as_str()));
        next.quarantined_reservations
            .extend(
                stale
                    .into_iter()
                    .map(|reservation| QuarantinedOutboxReservation {
                        reservation,
                        quarantined_at_utc: quarantined_at_utc.clone(),
                        reason: JOURNEY_QUARANTINE_REASON.to_string(),
                    }),
            );
        self.save_locked(&next).map_err(|e| e.to_string())?;
        *inner = next;
        Ok(quarantined)
    }

    pub(crate) fn record_ledger_metadata(
        &self,
        record_id: &str,
    ) -> Result<Option<OutboxLedgerMetadata>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner
            .record_ledger_metadata
            .iter()
            .find(|metadata| metadata.record_id == record_id)
            .cloned())
    }

    pub(crate) fn ledger_reconciliation_metadata(
        &self,
        legacy_soul_ref: &str,
    ) -> Result<Vec<OutboxLedgerMetadata>, String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = inner.clone();
        let mut metadata_to_reconcile = Vec::new();
        let mut seen = HashSet::new();

        for record in &inner.records {
            let existing = next
                .record_ledger_metadata
                .iter()
                .find(|metadata| metadata.record_id == record.record_id)
                .cloned();
            let metadata = match existing {
                Some(metadata) => {
                    metadata.validate_source(
                        &record.record_id,
                        &record.entity_id,
                        &record.kind,
                        &record.payload,
                    )?;
                    if metadata.soul_ref.is_empty() {
                        let repaired = OutboxLedgerMetadata::from_payload(
                            &record.record_id,
                            &record.entity_id,
                            &record.kind,
                            &record.payload,
                            legacy_soul_ref,
                        )?;
                        merge_record_ledger_metadata(
                            &mut next.record_ledger_metadata,
                            repaired.clone(),
                            true,
                        )?;
                        repaired
                    } else {
                        metadata
                    }
                }
                None => {
                    let recovered = OutboxLedgerMetadata::from_payload(
                        &record.record_id,
                        &record.entity_id,
                        &record.kind,
                        &record.payload,
                        legacy_soul_ref,
                    )?;
                    merge_record_ledger_metadata(
                        &mut next.record_ledger_metadata,
                        recovered.clone(),
                        false,
                    )?;
                    recovered
                }
            };
            seen.insert(metadata.record_id.clone());
            metadata_to_reconcile.push(metadata);
        }

        for record_id in &inner.acknowledged_record_ids {
            if seen.contains(record_id) {
                continue;
            }
            if let Some(metadata) = next
                .record_ledger_metadata
                .iter()
                .find(|metadata| metadata.record_id == *record_id)
                .cloned()
            {
                if metadata.soul_ref.is_empty() {
                    return Err(format!(
                        "Acknowledged Runtime outbox record {record_id} is missing its original soul reference"
                    ));
                }
                seen.insert(record_id.clone());
                metadata_to_reconcile.push(metadata);
            }
        }

        if next.record_ledger_metadata != inner.record_ledger_metadata {
            self.save_locked(&next).map_err(|error| error.to_string())?;
            *inner = next;
        }
        Ok(metadata_to_reconcile)
    }

    pub(crate) fn persist_authoritative_record_ledger_metadata(
        &self,
        metadata: OutboxLedgerMetadata,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = inner.clone();
        if !merge_record_ledger_metadata(&mut next.record_ledger_metadata, metadata, true)? {
            return Ok(());
        }
        self.save_locked(&next).map_err(|error| error.to_string())?;
        *inner = next;
        Ok(())
    }

    #[cfg(test)]
    pub fn is_acknowledged(&self, record_id: &str) -> Result<bool, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner
            .acknowledged_record_ids
            .iter()
            .any(|acknowledged| acknowledged == record_id))
    }

    pub fn snapshot(&self) -> Result<Vec<EntityOutboxRecord>, String> {
        let inner = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(inner.records.clone())
    }

    pub fn acknowledge(&self, accepted_record_ids: &[String]) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = inner.clone();
        next.records
            .retain(|record| !accepted_record_ids.iter().any(|id| id == &record.record_id));
        for record_id in accepted_record_ids {
            if !next
                .acknowledged_record_ids
                .iter()
                .any(|acknowledged| acknowledged == record_id)
            {
                next.acknowledged_record_ids.push(record_id.clone());
            }
        }
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
            quarantined_records: inner.quarantined_reservations.len(),
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

fn stage_record_ledger_metadata(
    existing: &[OutboxLedgerMetadata],
    additions: &mut Vec<OutboxLedgerMetadata>,
    candidate: OutboxLedgerMetadata,
) -> Result<(), String> {
    if let Some(metadata) = existing
        .iter()
        .chain(additions.iter())
        .find(|metadata| metadata.record_id == candidate.record_id)
    {
        if metadata.same_source(&candidate) {
            return Ok(());
        }
        return Err(format!(
            "Runtime outbox ledger metadata for record {} conflicts with different event data",
            candidate.record_id
        ));
    }
    additions.push(candidate);
    Ok(())
}

fn merge_record_ledger_metadata(
    metadata: &mut Vec<OutboxLedgerMetadata>,
    candidate: OutboxLedgerMetadata,
    candidate_is_authoritative: bool,
) -> Result<bool, String> {
    if let Some(existing) = metadata
        .iter_mut()
        .find(|existing| existing.record_id == candidate.record_id)
    {
        if !existing.same_source(&candidate) {
            return Err(format!(
                "Runtime outbox ledger metadata for record {} conflicts with different event data",
                candidate.record_id
            ));
        }
        if candidate_is_authoritative && existing.soul_ref != candidate.soul_ref {
            *existing = candidate;
            return Ok(true);
        }
        return Ok(false);
    }
    metadata.push(candidate);
    Ok(true)
}

fn validate_record_id(record_id: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(record_id).is_ok_and(|id| id.to_string() == record_id) {
        Ok(())
    } else {
        Err("Runtime outbox record ID must be a canonical UUID".to_string())
    }
}

fn reservation_is_recoverable(
    reservation: &OutboxReservation,
    recovery: &ReservationRecovery,
) -> Result<bool, String> {
    match recovery {
        ReservationRecovery::All => Ok(true),
        ReservationRecovery::StaleBefore(cutoff) => {
            let created_at = chrono::DateTime::parse_from_rfc3339(&reservation.created_at_utc)
                .map_err(|_| "Runtime outbox reservation has an invalid timestamp".to_string())?
                .with_timezone(&chrono::Utc);
            Ok(created_at <= *cutoff)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DurableRecordReconcile, ReservationRecovery, RuntimeOutbox};
    use hive_core::OutboxSyncResponse;
    use std::collections::HashSet;
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

    fn age_reservation(root: &std::path::Path, record_id: &str, age: chrono::Duration) {
        let path = root.join(super::OUTBOX_FILE_NAME);
        let mut document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let reservation = document["reservations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|reservation| reservation["record_id"] == record_id)
            .expect("reservation to age");
        reservation["created_at_utc"] =
            serde_json::Value::String((chrono::Utc::now() - age).to_rfc3339());
        std::fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    fn assert_no_private_quarantine_keys(value: &serde_json::Value) {
        const FORBIDDEN: &[&str] = &[
            "reservation",
            "entity_id",
            "session_id",
            "soul_ref",
            "payload_digest",
            "payload",
            "message",
            "reply",
            "content",
            "prompt",
            "model_output",
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
        ];

        match value {
            serde_json::Value::Object(object) => {
                for (key, nested) in object {
                    assert!(
                        !FORBIDDEN.contains(&key.as_str()),
                        "quarantine diagnostic leaked private key {key}: {value}"
                    );
                    assert_no_private_quarantine_keys(nested);
                }
            }
            serde_json::Value::Array(values) => {
                for nested in values {
                    assert_no_private_quarantine_keys(nested);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn quarantine_diagnostic_serialization_is_fixed_redacted_and_deterministic() {
        const REASON: &str =
            "No matching durable fact was found after the reservation recovery threshold";
        let root = root("abigail-outbox-quarantine-diagnostic");
        let earlier_first = "00000000-0000-4000-8000-000000000001";
        let earlier_second = "00000000-0000-4000-8000-000000000002";
        let later = "00000000-0000-4000-8000-000000000003";
        let outbox = RuntimeOutbox::load(&root, 3).unwrap();
        for record_id in [later, earlier_second, earlier_first] {
            outbox
                .reserve_slot_with_soul_ref(
                    record_id.to_string(),
                    "private-entity-id",
                    "skill_used",
                    "private-soul-ref",
                )
                .unwrap();
        }
        assert_eq!(
            outbox
                .quarantine_stale_unmatched_reservations(
                    &HashSet::new(),
                    chrono::Utc::now() + chrono::Duration::hours(1),
                )
                .unwrap(),
            3
        );
        assert_eq!(outbox.status().unwrap().quarantined_records, 3);
        drop(outbox);

        let path = root.join(super::OUTBOX_FILE_NAME);
        let mut persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for row in persisted["quarantined_reservations"]
            .as_array_mut()
            .unwrap()
        {
            let record_id = row["reservation"]["record_id"].as_str().unwrap();
            row["quarantined_at_utc"] = serde_json::json!(if record_id == later {
                "2026-10-23T00:00:02Z"
            } else {
                "2026-10-23T00:00:01Z"
            });
            row["reason"] =
                serde_json::json!("private prompt, model output, payload, and scoring material");
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();

        let outbox = RuntimeOutbox::load(&root, 3).unwrap();
        let serialized = serde_json::to_value(outbox.quarantine_diagnostics().unwrap()).unwrap();
        let rows = serialized.as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| row["record_id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![earlier_first, earlier_second, later]
        );
        for row in rows {
            assert_eq!(
                row.as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>(),
                std::collections::BTreeSet::from([
                    "kind",
                    "quarantined_at_utc",
                    "reason",
                    "record_id",
                ])
            );
            assert_eq!(row["reason"], REASON);
        }
        assert_no_private_quarantine_keys(&serialized);
        let _ = std::fs::remove_dir_all(root);
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
        assert!(
            error.to_string().contains("key must be a string"),
            "{error}"
        );
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

    #[test]
    fn reservations_count_toward_capacity_and_survive_commit_restart() {
        let root = root("abigail-outbox-reservation-capacity");
        let outbox = RuntimeOutbox::load(&root, 2).unwrap();
        let reserved_id = uuid::Uuid::new_v4().to_string();
        let existing = outbox
            .enqueue(
                "entity-1",
                "memory_confirmed",
                serde_json::json!({"memory_id": "existing"}),
            )
            .unwrap();

        let reservation = outbox
            .reserve_slot(reserved_id.clone(), "entity-1", "skill_used")
            .unwrap();
        assert_eq!(reservation.record_id, reserved_id);
        let full_error = outbox
            .reserve_slot(uuid::Uuid::new_v4().to_string(), "entity-1", "skill_used")
            .unwrap_err();
        assert!(full_error.contains("backpressure"), "{full_error}");
        assert_eq!(outbox.snapshot().unwrap()[0].record_id, existing.record_id);
        assert_eq!(outbox.reservations().unwrap().len(), 1);

        let record = outbox
            .commit_reservation(
                &reserved_id,
                serde_json::json!({
                    "session_id": "session-1",
                    "successful_skill_count": 2
                }),
            )
            .unwrap();
        assert_eq!(record.record_id, reserved_id);
        assert!(outbox.reservations().unwrap().is_empty());
        assert_eq!(outbox.snapshot().unwrap().len(), 2);
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 2).unwrap();
        assert!(restarted.reservations().unwrap().is_empty());
        assert_eq!(restarted.snapshot().unwrap().len(), 2);
        assert_eq!(restarted.snapshot().unwrap()[1].record_id, reserved_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reservation_mutations_publish_only_after_successful_save() {
        let reserve_root = root("abigail-outbox-reserve-save");
        let reserve_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load_with_saver(&reserve_root, 2, fail_save_on(1)).unwrap();
        let error = outbox
            .reserve_slot(reserve_id, "entity-1", "skill_used")
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert!(outbox.reservations().unwrap().is_empty());
        assert!(RuntimeOutbox::load(&reserve_root, 2)
            .unwrap()
            .reservations()
            .unwrap()
            .is_empty());

        let commit_root = root("abigail-outbox-commit-save");
        let commit_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load_with_saver(&commit_root, 2, fail_save_on(2)).unwrap();
        outbox
            .reserve_slot(commit_id.clone(), "entity-1", "skill_used")
            .unwrap();
        let error = outbox
            .commit_reservation(
                &commit_id,
                serde_json::json!({
                    "session_id": "session-1",
                    "successful_skill_count": 1
                }),
            )
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert_eq!(outbox.reservations().unwrap()[0].record_id, commit_id);
        assert!(outbox.snapshot().unwrap().is_empty());
        let restarted = RuntimeOutbox::load(&commit_root, 2).unwrap();
        assert_eq!(restarted.reservations().unwrap()[0].record_id, commit_id);
        assert!(restarted.snapshot().unwrap().is_empty());

        let release_root = root("abigail-outbox-release-save");
        let release_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load_with_saver(&release_root, 2, fail_save_on(2)).unwrap();
        outbox
            .reserve_slot(release_id.clone(), "entity-1", "memory_confirmed")
            .unwrap();
        let error = outbox.release_reservation(&release_id).unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert_eq!(outbox.reservations().unwrap()[0].record_id, release_id);
        drop(outbox);
        assert_eq!(
            RuntimeOutbox::load(&release_root, 2)
                .unwrap()
                .reservations()
                .unwrap()[0]
                .record_id,
            release_id
        );

        let _ = std::fs::remove_dir_all(reserve_root);
        let _ = std::fs::remove_dir_all(commit_root);
        let _ = std::fs::remove_dir_all(release_root);
    }

    #[test]
    fn released_reservations_restore_capacity_across_restart() {
        let root = root("abigail-outbox-release");
        let first_id = uuid::Uuid::new_v4().to_string();
        let second_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(first_id.clone(), "entity-1", "skill_used")
            .unwrap();
        outbox.release_reservation(&first_id).unwrap();
        assert!(outbox.reservations().unwrap().is_empty());
        outbox
            .reserve_slot(second_id.clone(), "entity-1", "memory_confirmed")
            .unwrap();
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 1).unwrap();
        assert_eq!(restarted.reservations().unwrap()[0].record_id, second_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acknowledged_ids_survive_restart_and_block_duplicate_reservations() {
        let root = root("abigail-outbox-acknowledged");
        let record_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(record_id.clone(), "entity-1", "memory_confirmed")
            .unwrap();
        outbox
            .commit_reservation(
                &record_id,
                serde_json::json!({
                    "memory_id": "memory-1",
                    "memory_weight": "distilled"
                }),
            )
            .unwrap();
        outbox
            .acknowledge(std::slice::from_ref(&record_id))
            .unwrap();
        assert!(outbox.is_acknowledged(&record_id).unwrap());
        assert!(outbox.snapshot().unwrap().is_empty());
        assert!(outbox.reservations().unwrap().is_empty());
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 1).unwrap();
        assert!(restarted.is_acknowledged(&record_id).unwrap());
        let error = restarted
            .reserve_slot(record_id, "entity-1", "memory_confirmed")
            .unwrap_err();
        assert!(error.contains("acknowledged"), "{error}");
        assert!(restarted.snapshot().unwrap().is_empty());
        assert!(restarted.reservations().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_outbox_files_default_reservations_and_acknowledged_ids() {
        let root = root("abigail-outbox-legacy-defaults");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(super::OUTBOX_FILE_NAME),
            br#"{
                "records": [],
                "reservations": [{
                    "record_id": "00000000-0000-4000-8000-000000000001",
                    "entity_id": "legacy-entity",
                    "kind": "skill_used",
                    "created_at_utc": "2026-10-22T12:00:00Z"
                }],
                "last_sync_at_utc": null,
                "last_sync_error": null
            }"#,
        )
        .unwrap();

        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        let reservations = outbox.reservations().unwrap();
        assert_eq!(reservations.len(), 1);
        assert!(
            reservations[0].soul_ref.is_empty(),
            "legacy reservations must load with a defaulted soul reference"
        );
        assert!(!outbox
            .is_acknowledged(&uuid::Uuid::new_v4().to_string())
            .unwrap());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn durable_reconciliation_waits_for_active_reservations_then_replaces_them_atomically() {
        let root = root("abigail-outbox-reconcile-active");
        let record_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(record_id.clone(), "entity-1", "skill_used")
            .unwrap();
        let payload = serde_json::json!({
            "session_id": "session-1",
            "successful_skill_count": 2,
        });

        let active = outbox
            .reconcile_durable_record(
                &record_id,
                "entity-1",
                "skill_used",
                "2026-10-22T12:00:00Z",
                payload.clone(),
                ReservationRecovery::StaleBefore(chrono::Utc::now() - chrono::Duration::minutes(1)),
            )
            .unwrap();
        assert!(matches!(active, DurableRecordReconcile::Deferred));
        assert!(outbox.snapshot().unwrap().is_empty());
        assert_eq!(outbox.reservations().unwrap().len(), 1);

        let recovered = outbox
            .reconcile_durable_record(
                &record_id,
                "entity-1",
                "skill_used",
                "2026-10-22T12:00:00Z",
                payload,
                ReservationRecovery::All,
            )
            .unwrap();
        let DurableRecordReconcile::Queued(recovered) = recovered else {
            panic!("startup reconciliation should recover the durable fact");
        };
        assert_eq!(recovered.record_id, record_id);
        assert!(outbox.reservations().unwrap().is_empty());
        assert_eq!(outbox.snapshot().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reconciliation_save_failure_keeps_the_reservation_in_memory_and_on_disk() {
        let root = root("abigail-outbox-reconcile-save");
        let record_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load_with_saver(&root, 1, fail_save_on(2)).unwrap();
        outbox
            .reserve_slot(record_id.clone(), "entity-1", "memory_confirmed")
            .unwrap();

        let error = outbox
            .reconcile_durable_record(
                &record_id,
                "entity-1",
                "memory_confirmed",
                "2026-10-22T12:00:00Z",
                serde_json::json!({
                    "memory_id": "memory-1",
                    "memory_weight": "distilled",
                }),
                ReservationRecovery::All,
            )
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert!(outbox.snapshot().unwrap().is_empty());
        assert_eq!(outbox.reservations().unwrap()[0].record_id, record_id);

        let restarted = RuntimeOutbox::load(&root, 1).unwrap();
        assert!(restarted.snapshot().unwrap().is_empty());
        assert_eq!(restarted.reservations().unwrap()[0].record_id, record_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_unmatched_reservation_is_quarantined_without_inventing_activity_and_restores_capacity()
    {
        let root = root("abigail-outbox-quarantine");
        let stale_id = uuid::Uuid::new_v4().to_string();
        let replacement_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(stale_id.clone(), "entity-1", "skill_used")
            .unwrap();
        drop(outbox);
        age_reservation(&root, &stale_id, chrono::Duration::hours(25));

        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        let quarantined = outbox
            .quarantine_stale_unmatched_reservations(
                &HashSet::new(),
                chrono::Utc::now() - chrono::Duration::hours(24),
            )
            .unwrap();

        assert_eq!(quarantined, 1);
        assert!(outbox.reservations().unwrap().is_empty());
        assert!(
            outbox.snapshot().unwrap().is_empty(),
            "quarantine must never invent an activity record"
        );
        let diagnostic = outbox.quarantined_reservations().unwrap();
        assert_eq!(diagnostic.len(), 1);
        assert_eq!(diagnostic[0].reservation.record_id, stale_id);
        assert!(!diagnostic[0].quarantined_at_utc.is_empty());
        assert!(diagnostic[0].reason.contains("durable fact"));

        outbox
            .reserve_slot(replacement_id.clone(), "entity-1", "memory_confirmed")
            .expect("quarantine must restore active capacity");
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 1).unwrap();
        assert_eq!(
            restarted.quarantined_reservations().unwrap(),
            diagnostic,
            "quarantine diagnostics must survive restart"
        );
        assert_eq!(
            restarted.reservations().unwrap()[0].record_id,
            replacement_id
        );
        let error = restarted
            .reserve_slot(stale_id, "entity-1", "skill_used")
            .unwrap_err();
        assert!(error.contains("quarantined"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fresh_unmatched_and_stale_matching_reservations_remain_active_and_consume_capacity() {
        let root = root("abigail-outbox-quarantine-protected");
        let fresh_id = uuid::Uuid::new_v4().to_string();
        let matching_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 2).unwrap();
        outbox
            .reserve_slot(fresh_id.clone(), "entity-1", "skill_used")
            .unwrap();
        outbox
            .reserve_slot(matching_id.clone(), "entity-1", "memory_confirmed")
            .unwrap();
        drop(outbox);
        age_reservation(&root, &matching_id, chrono::Duration::hours(25));

        let outbox = RuntimeOutbox::load(&root, 2).unwrap();
        let quarantined = outbox
            .quarantine_stale_unmatched_reservations(
                &HashSet::from([matching_id.clone()]),
                chrono::Utc::now() - chrono::Duration::hours(24),
            )
            .unwrap();

        assert_eq!(quarantined, 0);
        assert!(outbox.quarantined_reservations().unwrap().is_empty());
        let active_ids = outbox
            .reservations()
            .unwrap()
            .into_iter()
            .map(|reservation| reservation.record_id)
            .collect::<HashSet<_>>();
        assert_eq!(active_ids, HashSet::from([fresh_id, matching_id]));
        let error = outbox
            .reserve_slot(uuid::Uuid::new_v4().to_string(), "entity-1", "skill_used")
            .unwrap_err();
        assert!(error.contains("backpressure"), "{error}");
        assert!(outbox.snapshot().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quarantine_save_failure_leaves_active_and_diagnostic_state_unchanged() {
        let root = root("abigail-outbox-quarantine-save");
        let stale_id = uuid::Uuid::new_v4().to_string();
        let outbox = RuntimeOutbox::load(&root, 1).unwrap();
        outbox
            .reserve_slot(stale_id.clone(), "entity-1", "skill_used")
            .unwrap();
        drop(outbox);
        age_reservation(&root, &stale_id, chrono::Duration::hours(25));

        let outbox = RuntimeOutbox::load_with_saver(&root, 1, fail_save_on(1)).unwrap();
        let error = outbox
            .quarantine_stale_unmatched_reservations(
                &HashSet::new(),
                chrono::Utc::now() - chrono::Duration::hours(24),
            )
            .unwrap_err();
        assert!(error.contains("injected outbox save failure"), "{error}");
        assert_eq!(outbox.reservations().unwrap()[0].record_id, stale_id);
        assert!(outbox.quarantined_reservations().unwrap().is_empty());
        assert!(outbox.snapshot().unwrap().is_empty());
        drop(outbox);

        let restarted = RuntimeOutbox::load(&root, 1).unwrap();
        assert_eq!(restarted.reservations().unwrap()[0].record_id, stale_id);
        assert!(restarted.quarantined_reservations().unwrap().is_empty());
        assert!(restarted.snapshot().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
