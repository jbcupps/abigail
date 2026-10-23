//! Entity journeys and Presence cards. Both are presentation: they never feed
//! prompts, permissions, providers, or the signed constitutional documents.
//!
//! Journeys live in the Hive-owned store (`entity_journey`) so badges and
//! mentoring milestones outlive an expired or deleted Genesis record.
//! Presence cards live beside the Entity as `presence.json`, outside the
//! `docs/` folder that runtimes read for their system prompt.

use crate::{genesis_http::OWNER, state::HiveDaemonState};
use abigail_persistence::{EntityScope, PersistenceHandle, QueryBinding};
use anyhow::{anyhow, bail, Context, Result};
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Datelike;
use hive_core::genesis::{EntityDefinition, GenesisSession};
use hive_core::genesis_quest::*;
use hive_core::{ApiEnvelope, BirthCertificate, BirthCertificateSummary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::sync::Mutex;

const JOURNEY_TABLE: &str = "entity_journey";
const JOURNEY_EVENT_TABLE: &str = "entity_journey_event";
const LEGACY_MIGRATION_TABLE: &str = "genesis_legacy_migration";
const JOURNEY_EVENT_SELECT: &str =
    "record_id, owner_id, entity_id, kind, occurred_at, session_id, skill_id, tool_name, successful_skill_count, memory_id, memory_weight";
const PRESENCE_FILE: &str = "presence.json";
const MAX_PRESENCE_BYTES: usize = 32 * 1024;
const MAX_EVENT_IDENTIFIER_BYTES: usize = 160;

pub struct JourneyStore {
    handle: PersistenceHandle,
    writes: Mutex<()>,
    legacy_migrations: Mutex<()>,
    #[cfg(test)]
    test_faults: Mutex<JourneyTestFaults>,
}

#[cfg(test)]
#[derive(Default)]
struct JourneyTestFaults {
    fail_event_write_after: Option<usize>,
    fail_projection_writes: usize,
    fail_legacy_migration_after_presence: bool,
}

/// Serialized as text like Genesis sessions, with ownership cross-checked.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JourneyRecord {
    entity_id: String,
    owner_id: String,
    payload: String,
}

/// Closed persistence shape constructed only after Runtime metadata validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct JourneyEventRecord {
    record_id: String,
    owner_id: String,
    entity_id: String,
    kind: String,
    occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skill_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    successful_skill_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_weight: Option<String>,
}

struct PreparedJourneyEvent {
    event: JourneyEventRecord,
    needs_write: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LegacyMigrationState {
    Pending,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LegacyMigrationRecord {
    owner_id: String,
    entity_id: String,
    request_digest: String,
    state: LegacyMigrationState,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LegacyFirstSignalMigrationRequest {
    pub attempt_key: String,
    pub original_name: String,
    pub purpose: String,
    pub boundaries: String,
    pub presence: PresenceChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LegacyFirstSignalMigrationResult {
    pub entity_id: String,
    pub presence: PresenceCard,
    pub badges: Vec<Badge>,
    pub migrated_at: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LegacyPresenceCard {
    name: String,
    purpose: String,
    temperament: Temperament,
    boundaries: String,
    identity: Palette,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySkillUsedMetadata {
    session_id: String,
    skill_id: String,
    tool_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AggregateSkillUsedMetadata {
    session_id: String,
    successful_skill_count: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SkillUsedMetadata {
    Aggregate(AggregateSkillUsedMetadata),
    Legacy(LegacySkillUsedMetadata),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryConfirmedMetadata {
    memory_id: String,
    memory_weight: String,
}

impl JourneyStore {
    pub fn new(handle: PersistenceHandle) -> Result<Self> {
        if handle.scope() != &EntityScope::Hive {
            bail!("Entity journeys must use the Hive persistence scope");
        }
        handle.upsert(
            "migration_log",
            "entity_journey_v1",
            &serde_json::json!({
                "schema_version": JOURNEY_SCHEMA_VERSION,
                "description": "Owner-scoped Entity journeys: earned badges and mentoring milestones"
            }),
        )?;
        handle.upsert(
            "migration_log",
            "entity_journey_event_v1",
            &serde_json::json!({
                "schema_version": 1,
                "description": "Owner-scoped redacted journey activity metadata"
            }),
        )?;
        handle.upsert(
            "migration_log",
            "genesis_legacy_first_signal_v1",
            &serde_json::json!({
                "schema_version": 1,
                "description": "Replay-safe binding for legacy First Signal migration"
            }),
        )?;
        Ok(Self {
            handle,
            writes: Mutex::new(()),
            legacy_migrations: Mutex::new(()),
            #[cfg(test)]
            test_faults: Mutex::new(JourneyTestFaults::default()),
        })
    }

    #[cfg(test)]
    pub(crate) fn fail_event_writes_after(&self, successful_writes: usize) {
        self.test_faults.lock().unwrap().fail_event_write_after = Some(successful_writes);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_projection_write(&self) {
        self.test_faults.lock().unwrap().fail_projection_writes += 1;
    }

    #[cfg(test)]
    pub(crate) fn fail_next_legacy_migration_after_presence(&self) {
        self.test_faults
            .lock()
            .unwrap()
            .fail_legacy_migration_after_presence = true;
    }

    pub fn load(&self, owner_id: &str, entity_id: &str) -> Result<Option<EntityJourney>> {
        validate_entity_id(entity_id)?;
        let Some(record) = self
            .handle
            .select_record::<JourneyRecord>(JOURNEY_TABLE, entity_id)?
        else {
            return Ok(None);
        };
        if record.owner_id != owner_id {
            return Ok(None);
        }
        let journey: EntityJourney =
            serde_json::from_str(&record.payload).context("Invalid saved Entity journey")?;
        if journey.entity_id != record.entity_id || journey.owner_id != record.owner_id {
            bail!("Entity journey metadata does not match its saved record");
        }
        journey.validate().map_err(|e| anyhow!(e))?;
        Ok(Some(journey))
    }

    fn save(&self, journey: &EntityJourney) -> Result<()> {
        self.handle.upsert(
            JOURNEY_TABLE,
            &journey.entity_id,
            &JourneyRecord {
                entity_id: journey.entity_id.clone(),
                owner_id: journey.owner_id.clone(),
                payload: serde_json::to_string(journey)?,
            },
        )?;
        Ok(())
    }

    fn load_legacy_migration(&self, record_id: &str) -> Result<Option<LegacyMigrationRecord>> {
        let Some(mut value) = self
            .handle
            .select_record::<serde_json::Value>(LEGACY_MIGRATION_TABLE, record_id)?
        else {
            return Ok(None);
        };
        let object = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("Invalid legacy migration ledger row"))?;
        let saved_id = object
            .remove("id")
            .and_then(|id| id.as_str().map(str::to_owned));
        if saved_id.as_deref().is_some_and(|id| id != record_id) {
            bail!("Legacy migration ledger ID does not match its record");
        }
        serde_json::from_value(value)
            .context("Invalid legacy migration ledger row")
            .map(Some)
    }

    /// Loads (or starts) a journey, applies `change`, and writes only when it
    /// reports a change. Serialized so concurrent awards cannot lose a badge.
    pub fn update<F>(
        &self,
        owner_id: &str,
        entity_id: &str,
        now: &str,
        change: F,
    ) -> Result<EntityJourney>
    where
        F: FnOnce(&mut EntityJourney) -> bool,
    {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Entity journey lock unavailable"))?;
        let existing = self.load(owner_id, entity_id)?;
        let existed = self.handle.record_exists(JOURNEY_TABLE, entity_id)?;
        if existed && existing.is_none() {
            bail!("This Entity journey belongs to another household");
        }
        let mut journey = existing.unwrap_or_else(|| EntityJourney::new(entity_id, owner_id, now));
        if change(&mut journey) || !existed {
            journey.validate().map_err(|e| anyhow!(e))?;
            self.save(&journey)?;
        }
        Ok(journey)
    }

    /// Persist recognized metadata-only Runtime events. The outbox record ID
    /// is the durable table key, so a retried sync cannot increment twice.
    #[cfg(test)]
    pub fn record_runtime_events(
        &self,
        owner_id: &str,
        entity_id: &str,
        records: &[hive_core::EntityOutboxRecord],
    ) -> Result<EntityJourney> {
        let recognized = self.persist_runtime_events(owner_id, entity_id, records)?;
        if recognized {
            self.reconcile_runtime_event_projection(owner_id, entity_id)
        } else {
            Ok(self
                .load(owner_id, entity_id)?
                .unwrap_or_else(|| EntityJourney::new(entity_id, owner_id, &now_utc())))
        }
    }

    /// Validate every recognized event and every existing record-ID collision
    /// before the daemon accepts the Runtime batch.
    pub fn prevalidate_runtime_events(
        &self,
        owner_id: &str,
        entity_id: &str,
        records: &[hive_core::EntityOutboxRecord],
    ) -> Result<()> {
        validate_entity_id(entity_id)?;
        validate_event_identifier(owner_id, "owner ID")?;
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Entity journey lock unavailable"))?;
        self.prepare_runtime_events_unlocked(owner_id, entity_id, records)
            .map(|_| ())
    }

    /// Persist only validated metadata rows. A storage error can leave an
    /// initial subset durable; a retry skips matching rows and completes the
    /// remainder without double counting.
    pub fn persist_runtime_events(
        &self,
        owner_id: &str,
        entity_id: &str,
        records: &[hive_core::EntityOutboxRecord],
    ) -> Result<bool> {
        validate_entity_id(entity_id)?;
        validate_event_identifier(owner_id, "owner ID")?;
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Entity journey lock unavailable"))?;
        let prepared = self.prepare_runtime_events_unlocked(owner_id, entity_id, records)?;
        if prepared.is_empty() {
            return Ok(false);
        }

        for (successful_writes, prepared_event) in prepared
            .into_iter()
            .filter(|prepared_event| prepared_event.needs_write)
            .enumerate()
        {
            #[cfg(not(test))]
            let _ = successful_writes;
            #[cfg(test)]
            {
                let mut faults = self.test_faults.lock().unwrap();
                if faults.fail_event_write_after == Some(successful_writes) {
                    faults.fail_event_write_after = None;
                    bail!("Injected journey event storage failure");
                }
            }
            self.handle.upsert(
                JOURNEY_EVENT_TABLE,
                &prepared_event.event.record_id,
                &prepared_event.event,
            )?;
        }
        Ok(true)
    }

    fn prepare_runtime_events_unlocked(
        &self,
        owner_id: &str,
        entity_id: &str,
        records: &[hive_core::EntityOutboxRecord],
    ) -> Result<Vec<PreparedJourneyEvent>> {
        let existing = self.load(owner_id, entity_id)?;
        let existed = self.handle.record_exists(JOURNEY_TABLE, entity_id)?;
        if existed && existing.is_none() {
            bail!("This Entity journey belongs to another household");
        }

        let mut indexes = std::collections::HashMap::<String, usize>::new();
        let mut events = Vec::<JourneyEventRecord>::new();
        for record in records {
            if record.entity_id != entity_id {
                bail!("Runtime outbox record belongs to another Entity");
            }
            let Some(event) = journey_event(owner_id, entity_id, record)? else {
                continue;
            };
            if let Some(index) = indexes.get(&event.record_id) {
                if events[*index] != event {
                    bail!("Runtime outbox record ID conflicts within this journey batch");
                }
                continue;
            }
            indexes.insert(event.record_id.clone(), events.len());
            events.push(event);
        }

        let mut prepared = Vec::with_capacity(events.len());
        for event in events {
            let saved = self.handle.query_one::<JourneyEventRecord>(
                &format!(
                    "SELECT {JOURNEY_EVENT_SELECT} FROM entity_journey_event WHERE record_id = $record_id LIMIT 1"
                ),
                &[QueryBinding::new("record_id", &event.record_id)?],
            )?;
            match saved {
                Some(saved) if saved == event => prepared.push(PreparedJourneyEvent {
                    event,
                    needs_write: false,
                }),
                Some(_) => bail!("Runtime outbox record ID conflicts with a saved journey event"),
                None => {
                    if self
                        .handle
                        .record_exists(JOURNEY_EVENT_TABLE, &event.record_id)?
                    {
                        bail!("Runtime outbox record ID conflicts with a saved journey event");
                    }
                    prepared.push(PreparedJourneyEvent {
                        event,
                        needs_write: true,
                    });
                }
            }
        }
        Ok(prepared)
    }

    /// Rebuild the summary only from durable redacted rows. Callers may retry
    /// this independently from event persistence, so a summary write failure
    /// cannot make an already durable Runtime event unacknowledgeable.
    pub fn reconcile_runtime_event_projection(
        &self,
        owner_id: &str,
        entity_id: &str,
    ) -> Result<EntityJourney> {
        validate_entity_id(entity_id)?;
        validate_event_identifier(owner_id, "owner ID")?;
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Entity journey lock unavailable"))?;
        let existing = self.load(owner_id, entity_id)?;
        let existed = self.handle.record_exists(JOURNEY_TABLE, entity_id)?;
        if existed && existing.is_none() {
            bail!("This Entity journey belongs to another household");
        }
        let mut journey =
            existing.unwrap_or_else(|| EntityJourney::new(entity_id, owner_id, &now_utc()));
        let activity = self.event_activity_unlocked(owner_id, entity_id)?;
        let now = now_utc();
        let changed = journey.observe(&[], activity, &now);
        if changed || !existed {
            #[cfg(test)]
            {
                let mut faults = self.test_faults.lock().unwrap();
                if faults.fail_projection_writes > 0 {
                    faults.fail_projection_writes -= 1;
                    bail!("Injected journey projection storage failure");
                }
            }
            journey.validate().map_err(|e| anyhow!(e))?;
            self.save(&journey)?;
        }
        Ok(journey)
    }

    pub fn event_activity(&self, owner_id: &str, entity_id: &str) -> Result<JourneyActivity> {
        validate_entity_id(entity_id)?;
        validate_event_identifier(owner_id, "owner ID")?;
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Entity journey lock unavailable"))?;
        self.event_activity_unlocked(owner_id, entity_id)
    }

    fn event_activity_unlocked(&self, owner_id: &str, entity_id: &str) -> Result<JourneyActivity> {
        let rows = self.handle.query_vec::<JourneyEventRecord>(
            &format!(
                "SELECT {JOURNEY_EVENT_SELECT} FROM entity_journey_event WHERE owner_id = $owner_id AND entity_id = $entity_id"
            ),
            &[
                QueryBinding::new("owner_id", owner_id)?,
                QueryBinding::new("entity_id", entity_id)?,
            ],
        )?;
        let mut activity = JourneyActivity::default();
        for row in rows {
            if row.owner_id != owner_id || row.entity_id != entity_id {
                bail!("Saved journey event ownership does not match its scope");
            }
            match row.kind.as_str() {
                "skill_used" => {
                    let count = match (
                        row.successful_skill_count,
                        row.skill_id.as_deref(),
                        row.tool_name.as_deref(),
                    ) {
                        (Some(count), None, None)
                            if (1..=hive_core::MAX_SUCCESSFUL_SKILL_COUNT_PER_TURN)
                                .contains(&count) =>
                        {
                            u64::from(count)
                        }
                        (None, Some(_), Some(_)) => 1,
                        _ => bail!("Saved skill journey event metadata is invalid"),
                    };
                    activity.successful_skill_uses = activity
                        .successful_skill_uses
                        .checked_add(count)
                        .ok_or_else(|| anyhow!("Saved skill journey activity count overflowed"))?;
                }
                "memory_confirmed" => activity.confirmed_memories += 1,
                _ => bail!("Saved journey event has an unsupported kind"),
            }
        }
        Ok(activity)
    }
}

fn validate_entity_id(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id).map_err(|_| anyhow!("Invalid Entity ID"))?;
    Ok(())
}

fn validate_event_identifier(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_EVENT_IDENTIFIER_BYTES
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
    {
        bail!("Invalid {label}");
    }
    Ok(())
}

fn journey_event(
    owner_id: &str,
    entity_id: &str,
    record: &hive_core::EntityOutboxRecord,
) -> Result<Option<JourneyEventRecord>> {
    if !matches!(record.kind.as_str(), "skill_used" | "memory_confirmed") {
        return Ok(None);
    }
    uuid::Uuid::parse_str(&record.record_id)
        .map_err(|_| anyhow!("Invalid Runtime outbox record ID"))?;
    if record.created_at_utc.len() > 64
        || chrono::DateTime::parse_from_rfc3339(&record.created_at_utc).is_err()
    {
        bail!("Invalid Runtime outbox timestamp");
    }
    let mut event = JourneyEventRecord {
        record_id: record.record_id.clone(),
        owner_id: owner_id.to_string(),
        entity_id: entity_id.to_string(),
        kind: record.kind.clone(),
        occurred_at: record.created_at_utc.clone(),
        session_id: None,
        skill_id: None,
        tool_name: None,
        successful_skill_count: None,
        memory_id: None,
        memory_weight: None,
    };
    match record.kind.as_str() {
        "skill_used" => {
            let metadata: SkillUsedMetadata = serde_json::from_value(record.payload.clone())
                .context("Invalid skill journey metadata")?;
            match metadata {
                SkillUsedMetadata::Aggregate(metadata) => {
                    validate_event_identifier(&metadata.session_id, "session ID")?;
                    if !(1..=hive_core::MAX_SUCCESSFUL_SKILL_COUNT_PER_TURN)
                        .contains(&metadata.successful_skill_count)
                    {
                        bail!("Invalid successful skill count");
                    }
                    event.session_id = Some(metadata.session_id);
                    event.successful_skill_count = Some(metadata.successful_skill_count);
                }
                SkillUsedMetadata::Legacy(metadata) => {
                    validate_event_identifier(&metadata.session_id, "session ID")?;
                    validate_event_identifier(&metadata.skill_id, "skill ID")?;
                    validate_event_identifier(&metadata.tool_name, "tool name")?;
                    event.session_id = Some(metadata.session_id);
                    event.skill_id = Some(metadata.skill_id);
                    event.tool_name = Some(metadata.tool_name);
                }
            }
        }
        "memory_confirmed" => {
            let metadata: MemoryConfirmedMetadata = serde_json::from_value(record.payload.clone())
                .context("Invalid memory journey metadata")?;
            validate_event_identifier(&metadata.memory_id, "memory ID")?;
            if !matches!(
                metadata.memory_weight.as_str(),
                "distilled" | "crystallized"
            ) {
                bail!("Invalid confirmed memory weight");
            }
            event.memory_id = Some(metadata.memory_id);
            event.memory_weight = Some(metadata.memory_weight);
        }
        _ => unreachable!("unknown kinds return before metadata parsing"),
    }
    Ok(Some(event))
}

fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Called after a successful Genesis awakening: copy creation badges into the
/// Entity's durable journey and link its Genesis record.
pub fn record_awakening(state: &HiveDaemonState, session: &GenesisSession) -> Result<(), String> {
    let Some(entity_id) = &session.definition.entity_id else {
        return Ok(());
    };
    let now = chrono::Utc::now().to_rfc3339();
    state
        .journeys
        .update(OWNER, entity_id, &now, |journey| {
            let linked = journey.genesis_session_id.as_deref() != Some(&session.id);
            journey.genesis_session_id = Some(session.id.clone());
            journey.observe(&session.quest.earned, JourneyActivity::default(), &now) || linked
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Full durable conversation facts for this Entity. Only identifiers and UTC
/// calendar buckets leave this function; conversation text is never copied.
fn conversation_activity(
    state: &HiveDaemonState,
    entity_id: &str,
) -> Result<JourneyActivity, String> {
    let config = state.identity_manager.load_agent(entity_id)?;
    if !config.birth_complete {
        return Ok(JourneyActivity::default());
    }
    let memory =
        abigail_memory::MemoryStore::open_with_config(&config).map_err(|e| e.to_string())?;
    durable_entity_activity(&memory)
}

fn durable_entity_activity(
    memory: &abigail_memory::MemoryStore,
) -> Result<JourneyActivity, String> {
    let turns = memory.all_turns().map_err(|e| e.to_string())?;
    let mut activity = summarize_conversations(&turns);
    let mut assistant_turn_ids = HashSet::new();
    for turn in turns.iter().filter(|turn| turn.role == "assistant") {
        if assistant_turn_ids.insert(turn.id.as_str()) {
            activity.successful_skill_uses += turn.successful_tool_uses.len() as u64;
        }
    }

    let memories = memory.all_memories().map_err(|e| e.to_string())?;
    let mut memory_ids = HashSet::new();
    activity.confirmed_memories = memories
        .iter()
        .filter(|memory| {
            memory.explicitly_confirmed
                && memory.weight != abigail_memory::MemoryWeight::Ephemeral
                && memory_ids.insert(memory.id.as_str())
        })
        .count() as u64;
    Ok(activity)
}

fn summarize_conversations(turns: &[abigail_memory::ConversationTurn]) -> JourneyActivity {
    let mut turn_ids = HashSet::new();
    let mut sessions = HashSet::new();
    let mut days = HashSet::new();
    let mut weeks = HashSet::new();
    for turn in turns.iter().filter(|turn| turn.role == "user") {
        if !turn_ids.insert(turn.id.as_str()) {
            continue;
        }
        sessions.insert(turn.session_id.as_str());
        days.insert(turn.created_at.date_naive());
        let week = turn.created_at.iso_week();
        weeks.insert((week.year(), week.week()));
    }
    JourneyActivity {
        mentor_messages: turn_ids.len() as u64,
        conversation_sessions: sessions.len() as u64,
        active_days: days.len() as u64,
        active_weeks: weeks.len() as u64,
        ..Default::default()
    }
}

fn merge_activity_sources(
    durable: JourneyActivity,
    event_rows: JourneyActivity,
) -> JourneyActivity {
    JourneyActivity {
        mentor_messages: durable.mentor_messages.max(event_rows.mentor_messages),
        conversation_sessions: durable
            .conversation_sessions
            .max(event_rows.conversation_sessions),
        active_days: durable.active_days.max(event_rows.active_days),
        active_weeks: durable.active_weeks.max(event_rows.active_weeks),
        successful_skill_uses: durable
            .successful_skill_uses
            .max(event_rows.successful_skill_uses),
        confirmed_memories: durable
            .confirmed_memories
            .max(event_rows.confirmed_memories),
    }
}

fn registered_name(state: &HiveDaemonState, entity_id: &str) -> Result<String, String> {
    state
        .identity_manager
        .list_agents()?
        .into_iter()
        .find(|agent| agent.id == entity_id && !agent.is_hive)
        .map(|agent| agent.name)
        .ok_or_else(|| "Entity not found".to_string())
}

/// The approved, signed-at-birth narrative, if this Entity came from Genesis.
pub(crate) fn approved_definition(
    state: &HiveDaemonState,
    entity_id: &str,
) -> Option<EntityDefinition> {
    let config = state.identity_manager.load_agent(entity_id).ok()?;
    let text = std::fs::read_to_string(config.docs_dir.join("genesis.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn presence_path(state: &HiveDaemonState, entity_id: &str) -> Result<std::path::PathBuf, String> {
    validate_entity_id(entity_id).map_err(|e| e.to_string())?;
    registered_name(state, entity_id)?;
    Ok(state
        .identity_manager
        .agent_dir(entity_id)?
        .join(PRESENCE_FILE))
}

/// A missing card is `None`; a damaged one is an error, never a silent reset.
pub fn load_presence(
    state: &HiveDaemonState,
    entity_id: &str,
) -> Result<Option<PresenceCard>, String> {
    let path = presence_path(state, entity_id)?;
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the Presence card: {error}")),
    };
    let mut bytes = Vec::new();
    file.take((MAX_PRESENCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the Presence card: {error}"))?;
    if bytes.len() > MAX_PRESENCE_BYTES {
        return Err("The saved Presence card is too large".into());
    }
    let card: PresenceCard = serde_json::from_slice(&bytes)
        .map_err(|error| format!("The saved Presence card is invalid: {error}"))?;
    card.validate()?;
    Ok(Some(card))
}

pub fn save_presence(
    state: &HiveDaemonState,
    entity_id: &str,
    card: &PresenceCard,
) -> Result<(), String> {
    card.validate()?;
    let path = presence_path(state, entity_id)?;
    let bytes = serde_json::to_vec_pretty(card).map_err(|e| e.to_string())?;
    save_presence_bytes(&path, &bytes)
}

fn save_presence_bytes(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_PRESENCE_BYTES {
        return Err("The Presence card is too large to save".into());
    }
    abigail_core::secure_fs::write_bytes_atomic(path, bytes)
        .map_err(|error| format!("Could not save the Presence card: {error}"))
}

impl LegacyFirstSignalMigrationRequest {
    fn validate(&self) -> Result<(), String> {
        if self.attempt_key.is_empty()
            || self.attempt_key != self.attempt_key.trim()
            || self.attempt_key.len() > 512
            || self.attempt_key.chars().any(char::is_control)
        {
            return Err("The legacy First Signal attempt key is invalid".into());
        }
        if self.original_name.trim().is_empty()
            || self.original_name != self.original_name.trim()
            || self.original_name.chars().count() > PRESENCE_NAME_CHARS
            || self.original_name.chars().any(char::is_control)
        {
            return Err("The legacy First Signal registered name is invalid".into());
        }
        for text in [&self.purpose, &self.boundaries] {
            if text.chars().count() > PRESENCE_TEXT_CHARS
                || text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            {
                return Err("The legacy First Signal brief is invalid".into());
            }
        }
        if !matches!(
            self.presence.temperament,
            Temperament::Gentle | Temperament::Direct | Temperament::Curious
        ) || !matches!(
            self.presence.identity,
            Palette::Aether | Palette::Ember | Palette::Lumen
        ) || self.presence.crest != Crest::Orbit
            || self.presence.motion != Motion::Calm
        {
            return Err("The legacy First Signal Presence is invalid".into());
        }
        Ok(())
    }
}

fn digest_parts(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    Sha256::finalize(hasher)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn legacy_migration_record_id(owner_id: &str, attempt_key: &str) -> String {
    digest_parts(
        b"abigail.genesis.legacy-first-signal.record.v1\0",
        &[owner_id.as_bytes(), attempt_key.as_bytes()],
    )
}

fn legacy_migration_request_digest(
    request: &LegacyFirstSignalMigrationRequest,
) -> Result<String, String> {
    let bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    Ok(digest_parts(
        b"abigail.genesis.legacy-first-signal.request.v1\0",
        &[&bytes],
    ))
}

fn legacy_badges(request: &LegacyFirstSignalMigrationRequest) -> Vec<Badge> {
    let mut badges = vec![Badge::FirstSpark, Badge::Named];
    if !request.purpose.trim().is_empty() {
        badges.push(Badge::Purposeful);
    }
    badges.push(Badge::PresenceChosen);
    badges
}

fn legacy_presence(request: &LegacyFirstSignalMigrationRequest) -> PresenceCard {
    let mut definition = EntityDefinition::new(OWNER.into(), String::new());
    definition.name = request.original_name.clone();
    definition.purpose = request.purpose.clone();
    if !request.boundaries.is_empty() {
        definition.boundaries = vec![request.boundaries.clone()];
    }
    PresenceCard::compose(request.presence, &definition)
}

fn read_bounded_presence(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read the Presence card: {error}")),
    };
    let mut bytes = Vec::new();
    file.take((MAX_PRESENCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read the Presence card: {error}"))?;
    if bytes.len() > MAX_PRESENCE_BYTES {
        return Err("The saved Presence card is too large".into());
    }
    Ok(Some(bytes))
}

fn ensure_legacy_presence(
    state: &HiveDaemonState,
    entity_id: &str,
    request: &LegacyFirstSignalMigrationRequest,
    expected: &PresenceCard,
) -> Result<(), String> {
    let path = presence_path(state, entity_id)?;
    let Some(bytes) = read_bounded_presence(&path)? else {
        return save_presence(state, entity_id, expected);
    };
    let raw: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("The saved Presence card is invalid: {error}"))?;
    let is_old_five_field = raw.as_object().is_some_and(|object| {
        object.len() == 5
            && ["name", "purpose", "temperament", "boundaries", "identity"]
                .iter()
                .all(|key| object.contains_key(*key))
    });
    if is_old_five_field {
        let old: LegacyPresenceCard = serde_json::from_value(raw)
            .map_err(|error| format!("The saved legacy Presence card is invalid: {error}"))?;
        let reviewed = LegacyPresenceCard {
            name: request.original_name.clone(),
            purpose: request.purpose.clone(),
            temperament: request.presence.temperament,
            boundaries: request.boundaries.clone(),
            identity: request.presence.identity,
        };
        if old != reviewed {
            return Err(
                "The saved legacy Presence differs from the reviewed First Signal brief".into(),
            );
        }
        return save_presence(state, entity_id, expected);
    }

    let current: PresenceCard = serde_json::from_slice(&bytes)
        .map_err(|error| format!("The saved Presence card is invalid: {error}"))?;
    current.validate()?;
    if &current != expected {
        return Err(
            "This Entity already has a different canonical Presence; it was not overwritten".into(),
        );
    }
    Ok(())
}

fn legacy_migration_result(
    entity_id: &str,
    request: &LegacyFirstSignalMigrationRequest,
    migrated_at: &str,
) -> LegacyFirstSignalMigrationResult {
    LegacyFirstSignalMigrationResult {
        entity_id: entity_id.into(),
        presence: legacy_presence(request),
        badges: legacy_badges(request),
        migrated_at: migrated_at.into(),
    }
}

fn migrate_legacy_first_signal(
    state: &HiveDaemonState,
    entity_id: &str,
    request: &LegacyFirstSignalMigrationRequest,
) -> Result<LegacyFirstSignalMigrationResult, String> {
    request.validate()?;
    validate_entity_id(entity_id).map_err(|error| error.to_string())?;
    let registered = registered_name(state, entity_id)?;
    if registered != request.original_name {
        return Err(
            "The selected Entity does not match the legacy First Signal registered name".into(),
        );
    }
    let expected = legacy_presence(request);
    expected.validate()?;
    let record_id = legacy_migration_record_id(OWNER, &request.attempt_key);
    let request_digest = legacy_migration_request_digest(request)?;
    let _lock = state
        .journeys
        .legacy_migrations
        .lock()
        .map_err(|_| "Legacy First Signal migration lock unavailable".to_string())?;

    let saved = state
        .journeys
        .load_legacy_migration(&record_id)
        .map_err(|error| error.to_string())?;
    let mut ledger = match saved {
        Some(saved) => {
            if saved.owner_id != OWNER
                || saved.entity_id != entity_id
                || saved.request_digest != request_digest
            {
                return Err(
                    "This legacy First Signal attempt is already bound to a different migration"
                        .into(),
                );
            }
            if saved.state == LegacyMigrationState::Complete {
                return Ok(legacy_migration_result(
                    entity_id,
                    request,
                    &saved.updated_at,
                ));
            }
            saved
        }
        None => {
            let now = now_utc();
            let pending = LegacyMigrationRecord {
                owner_id: OWNER.into(),
                entity_id: entity_id.into(),
                request_digest,
                state: LegacyMigrationState::Pending,
                created_at: now.clone(),
                updated_at: now,
            };
            state
                .journeys
                .handle
                .upsert(LEGACY_MIGRATION_TABLE, &record_id, &pending)
                .map_err(|error| error.to_string())?;
            pending
        }
    };

    ensure_legacy_presence(state, entity_id, request, &expected)?;
    #[cfg(test)]
    {
        let mut faults = state.journeys.test_faults.lock().unwrap();
        if faults.fail_legacy_migration_after_presence {
            faults.fail_legacy_migration_after_presence = false;
            return Err("Injected legacy First Signal response loss".into());
        }
    }

    let badges = legacy_badges(request);
    state
        .journeys
        .update(OWNER, entity_id, &ledger.created_at, |journey| {
            let changed = !award(&mut journey.earned, &badges, &ledger.created_at).is_empty();
            if changed {
                journey.updated_at = ledger.created_at.clone();
            }
            changed
        })
        .map_err(|error| error.to_string())?;

    ledger.state = LegacyMigrationState::Complete;
    ledger.updated_at = now_utc();
    state
        .journeys
        .handle
        .upsert(LEGACY_MIGRATION_TABLE, &record_id, &ledger)
        .map_err(|error| error.to_string())?;
    Ok(legacy_migration_result(
        entity_id,
        request,
        &ledger.updated_at,
    ))
}

fn certificate(state: &HiveDaemonState, entity_id: &str) -> Option<BirthCertificateSummary> {
    let config = state.identity_manager.load_agent(entity_id).ok()?;
    let text = std::fs::read_to_string(config.docs_dir.join("birth_certificate.json")).ok()?;
    let cert: BirthCertificate = serde_json::from_str(&text).ok()?;
    let signature_verified = crate::birth::verified_certificate(state, &text, entity_id).is_ok();
    Some(cert.summary(signature_verified))
}

#[derive(Serialize)]
struct Milestone {
    badge: Badge,
    title: &'static str,
    metric: JourneyMetric,
    target: u64,
    progress: u64,
}

#[derive(Serialize)]
struct JourneyView {
    entity_id: String,
    name: String,
    level: BondLevel,
    badges: Vec<BadgeView>,
    mentor_messages: u64,
    conversation_sessions: u64,
    active_days: u64,
    active_weeks: u64,
    successful_skill_uses: u64,
    confirmed_memories: u64,
    next_milestone: Option<Milestone>,
    unlocked: Unlocks,
    first_quests: Vec<FirstQuest>,
    presence: Option<PresenceCard>,
    certificate: Option<BirthCertificateSummary>,
}

/// GET /v1/entities/:id/journey
pub async fn get_journey(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<serde_json::Value>> {
    Json(match journey_view(&state, &entity_id) {
        Ok(view) => ApiEnvelope::success(view),
        Err(error) => ApiEnvelope::error(error),
    })
}

fn journey_view(state: &HiveDaemonState, entity_id: &str) -> Result<serde_json::Value, String> {
    validate_entity_id(entity_id).map_err(|e| e.to_string())?;
    let name = registered_name(state, entity_id)?;
    let born = state.identity_manager.load_agent(entity_id)?.birth_complete;
    let durable_activity = conversation_activity(state, entity_id)?;
    let event_activity = state
        .journeys
        .event_activity(OWNER, entity_id)
        .map_err(|e| e.to_string())?;
    let activity = merge_activity_sources(durable_activity, event_activity);
    let now = chrono::Utc::now().to_rfc3339();
    let journey = if born {
        // Self-heals a journey whose awakening record could not be written.
        let linked = state
            .genesis
            .list(OWNER)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|s| {
                s.awakening.is_some() && s.definition.entity_id.as_deref() == Some(entity_id)
            });
        let creation = linked
            .as_ref()
            .map(|s| s.quest.earned.clone())
            .unwrap_or_default();
        state
            .journeys
            .update(OWNER, entity_id, &now, |journey| {
                let relink = linked
                    .as_ref()
                    .is_some_and(|s| journey.genesis_session_id.as_deref() != Some(s.id.as_str()));
                if let Some(session) = &linked {
                    journey.genesis_session_id = Some(session.id.clone());
                }
                journey.observe(&creation, activity, &now) || relink
            })
            .map_err(|e| e.to_string())?
    } else {
        state
            .journeys
            .load(OWNER, entity_id)
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| EntityJourney::new(entity_id, OWNER, &now))
    };
    let earned = journey.earned_badges();
    let presence = load_presence(state, entity_id)?;
    let journey_activity = journey.activity();
    let next_milestone = BADGE_DEFINITIONS.iter().find_map(|definition| {
        let requirement = definition.requirement?;
        let progress = journey_activity.metric(requirement.metric);
        (progress < requirement.target).then_some(Milestone {
            badge: definition.badge,
            title: definition.title,
            metric: requirement.metric,
            target: requirement.target,
            progress,
        })
    });
    let view = JourneyView {
        entity_id: entity_id.into(),
        first_quests: first_quests(
            &name,
            presence.as_ref().map(|card| card.temperament),
            journey.mentor_messages,
        ),
        name,
        level: bond_level(xp_for(&earned)),
        badges: badge_views(&journey.earned),
        mentor_messages: journey.mentor_messages,
        conversation_sessions: journey.conversation_sessions,
        active_days: journey.active_days,
        active_weeks: journey.active_weeks,
        successful_skill_uses: journey.successful_skill_uses,
        confirmed_memories: journey.confirmed_memories,
        next_milestone,
        unlocked: unlocks(&earned),
        presence,
        certificate: certificate(state, entity_id),
    };
    serde_json::to_value(view).map_err(|e| e.to_string())
}

/// POST /v1/entities/:id/journey/legacy-first-signal
///
/// Imports only a reviewed brief into an existing Entity. It never allocates,
/// retries, or otherwise creates an Entity.
pub async fn post_legacy_first_signal(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(request): Json<LegacyFirstSignalMigrationRequest>,
) -> Json<ApiEnvelope<LegacyFirstSignalMigrationResult>> {
    Json(
        match migrate_legacy_first_signal(&state, &entity_id, &request) {
            Ok(result) => ApiEnvelope::success(result),
            Err(error) => ApiEnvelope::error(error),
        },
    )
}

/// GET /v1/entities/:id/presence
pub async fn get_presence(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
) -> Json<ApiEnvelope<Option<PresenceCard>>> {
    Json(match load_presence(&state, &entity_id) {
        Ok(card) => ApiEnvelope::success(card),
        Err(error) => ApiEnvelope::error(error),
    })
}

/// PUT /v1/entities/:id/presence. The body carries only cosmetic choices;
/// display text is taken from the approved definition (or registered name).
pub async fn put_presence(
    State(state): State<HiveDaemonState>,
    Path(entity_id): Path<String>,
    Json(choice): Json<PresenceChoice>,
) -> Json<ApiEnvelope<PresenceCard>> {
    let result = (|| -> Result<PresenceCard, String> {
        let name = registered_name(&state, &entity_id)?;
        let journey = state
            .journeys
            .load(OWNER, &entity_id)
            .map_err(|e| e.to_string())?;
        let earned = journey.map(|j| j.earned_badges()).unwrap_or_default();
        check_unlocked(choice, &earned)?;
        let definition = approved_definition(&state, &entity_id).unwrap_or_else(|| {
            let mut fallback = EntityDefinition::new(OWNER.into(), String::new());
            fallback.name = name;
            fallback
        });
        let card = PresenceCard::compose(choice, &definition);
        save_presence(&state, &entity_id, &card)?;
        Ok(card)
    })();
    Json(match result {
        Ok(card) => ApiEnvelope::success(card),
        Err(error) => ApiEnvelope::error(error),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn view(state: &HiveDaemonState, entity_id: &str) -> serde_json::Value {
        journey_view(state, entity_id).unwrap()
    }

    fn legacy_request() -> LegacyFirstSignalMigrationRequest {
        LegacyFirstSignalMigrationRequest {
            attempt_key: "opaque-old-attempt".into(),
            original_name: "Route Test".into(),
            purpose: "Help our family notice small wonders.".into(),
            boundaries: "Ask before changing shared plans.".into(),
            presence: PresenceChoice {
                temperament: Temperament::Curious,
                identity: Palette::Ember,
                crest: Crest::Orbit,
                motion: Motion::Calm,
            },
        }
    }

    async fn import_legacy(
        state: &HiveDaemonState,
        entity_id: &str,
        request: LegacyFirstSignalMigrationRequest,
    ) -> hive_core::ApiEnvelope<LegacyFirstSignalMigrationResult> {
        let Json(response) = post_legacy_first_signal(
            State(state.clone()),
            Path(entity_id.to_string()),
            Json(request),
        )
        .await;
        response
    }

    fn forbidden_public_keys(value: &serde_json::Value) -> Vec<String> {
        const FORBIDDEN: [&str; 16] = [
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
            "message",
            "reply",
            "content",
            "prompt",
            "model",
            "model_output",
            "memory",
            "memory_content",
            "tool_argument",
            "tool_result",
            "payload",
        ];
        fn visit(value: &serde_json::Value, path: &str, found: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(object) => {
                    for (key, child) in object {
                        let child_path = format!("{path}.{key}");
                        if FORBIDDEN.contains(&key.as_str()) {
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
    async fn legacy_first_signal_first_import_creates_canonical_presence_badges_and_closed_ledger()
    {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();

        let response = import_legacy(&state, &entity_id, request.clone()).await;

        assert!(response.ok, "{:?}", response.error);
        let result = response.data.unwrap();
        assert_eq!(result.entity_id, entity_id);
        assert_eq!(
            result.presence,
            PresenceCard {
                name: "Route Test".into(),
                purpose: request.purpose.clone(),
                temperament: Temperament::Curious,
                boundaries: request.boundaries.clone(),
                identity: Palette::Ember,
                crest: Crest::Orbit,
                motion: Motion::Calm,
            }
        );
        assert_eq!(
            result.badges,
            vec![
                Badge::FirstSpark,
                Badge::Named,
                Badge::Purposeful,
                Badge::PresenceChosen,
            ]
        );
        assert_eq!(
            load_presence(&state, &entity_id).unwrap(),
            Some(result.presence.clone())
        );
        let journey = state.journeys.load(OWNER, &entity_id).unwrap().unwrap();
        for badge in &result.badges {
            assert!(journey.earned_badges().contains(badge));
        }
        let record_id = legacy_migration_record_id(OWNER, &request.attempt_key);
        let ledger = state
            .journeys
            .handle
            .select_record::<serde_json::Value>(LEGACY_MIGRATION_TABLE, &record_id)
            .unwrap()
            .unwrap();
        let keys = ledger
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            keys.into_iter().collect::<std::collections::BTreeSet<_>>(),
            [
                "created_at",
                "entity_id",
                "id",
                "owner_id",
                "request_digest",
                "state",
                "updated_at",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        );
        let encoded = ledger.to_string();
        assert!(!encoded.contains(&request.attempt_key));
        assert!(!encoded.contains(&request.original_name));
        assert!(!encoded.contains(&request.purpose));
        assert!(!encoded.contains(&request.boundaries));
    }

    #[tokio::test]
    async fn legacy_first_signal_exact_replay_returns_same_result_without_timestamp_changes() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        let first = import_legacy(&state, &entity_id, request.clone())
            .await
            .data
            .unwrap();
        let record_id = legacy_migration_record_id(OWNER, &request.attempt_key);
        let first_ledger = state
            .journeys
            .load_legacy_migration(&record_id)
            .unwrap()
            .unwrap();
        let first_journey = state.journeys.load(OWNER, &entity_id).unwrap().unwrap();
        let first_presence = std::fs::read(presence_path(&state, &entity_id).unwrap()).unwrap();

        let replay = import_legacy(&state, &entity_id, request)
            .await
            .data
            .unwrap();

        assert_eq!(replay, first);
        assert_eq!(
            state
                .journeys
                .load_legacy_migration(&record_id)
                .unwrap()
                .unwrap(),
            first_ledger
        );
        assert_eq!(
            state.journeys.load(OWNER, &entity_id).unwrap().unwrap(),
            first_journey
        );
        assert_eq!(
            std::fs::read(presence_path(&state, &entity_id).unwrap()).unwrap(),
            first_presence
        );
    }

    #[tokio::test]
    async fn legacy_first_signal_response_loss_replay_completes_a_partial_import() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        state.journeys.fail_next_legacy_migration_after_presence();

        let interrupted = import_legacy(&state, &entity_id, request.clone()).await;

        assert!(!interrupted.ok);
        assert_eq!(
            load_presence(&state, &entity_id).unwrap().unwrap().purpose,
            request.purpose
        );
        assert!(state.journeys.load(OWNER, &entity_id).unwrap().is_none());
        let record_id = legacy_migration_record_id(OWNER, &request.attempt_key);
        assert_eq!(
            state
                .journeys
                .load_legacy_migration(&record_id)
                .unwrap()
                .unwrap()
                .state,
            LegacyMigrationState::Pending
        );

        let replay = import_legacy(&state, &entity_id, request).await;

        assert!(replay.ok, "{:?}", replay.error);
        assert!(state.journeys.load(OWNER, &entity_id).unwrap().is_some());
        assert_eq!(
            state
                .journeys
                .load_legacy_migration(&record_id)
                .unwrap()
                .unwrap()
                .state,
            LegacyMigrationState::Complete
        );
    }

    #[tokio::test]
    async fn legacy_first_signal_attempt_cannot_bind_another_entity_or_request_digest() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        assert!(import_legacy(&state, &entity_id, request.clone()).await.ok);

        let (other_id, _) = state
            .identity_manager
            .create_agent("Route Test")
            .expect("create same-name recovery candidate");
        let other = import_legacy(&state, &other_id, request.clone()).await;
        assert!(!other.ok, "one attempt was rebound to another Entity");
        assert!(load_presence(&state, &other_id).unwrap().is_none());

        let mut changed = request;
        changed.purpose.push_str(" Changed");
        let changed = import_legacy(&state, &entity_id, changed).await;
        assert!(
            !changed.ok,
            "one attempt accepted a different import digest"
        );
    }

    #[tokio::test]
    async fn legacy_first_signal_replaces_only_a_matching_old_five_field_presence() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        let path = presence_path(&state, &entity_id).unwrap();
        let matching = serde_json::json!({
            "name": request.original_name,
            "purpose": request.purpose,
            "temperament": request.presence.temperament,
            "boundaries": request.boundaries,
            "identity": request.presence.identity,
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&matching).unwrap()).unwrap();

        assert!(import_legacy(&state, &entity_id, request.clone()).await.ok);
        let canonical: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(canonical["crest"], "orbit");
        assert_eq!(canonical["motion"], "calm");

        let (other_state, other_id) = crate::routes::tests::build_state();
        let other_path = presence_path(&other_state, &other_id).unwrap();
        let conflicting = serde_json::json!({
            "name": request.original_name,
            "purpose": "A different saved purpose",
            "temperament": request.presence.temperament,
            "boundaries": request.boundaries,
            "identity": request.presence.identity,
        });
        let conflicting_bytes = serde_json::to_vec_pretty(&conflicting).unwrap();
        std::fs::write(&other_path, &conflicting_bytes).unwrap();

        let rejected = import_legacy(&other_state, &other_id, request).await;
        assert!(!rejected.ok);
        assert_eq!(std::fs::read(other_path).unwrap(), conflicting_bytes);
    }

    #[tokio::test]
    async fn legacy_first_signal_completed_replay_never_overwrites_a_later_canonical_edit() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        let first = import_legacy(&state, &entity_id, request.clone())
            .await
            .data
            .unwrap();
        let mut edited = first.presence.clone();
        edited.motion = Motion::Lively;
        save_presence(&state, &entity_id, &edited).unwrap();

        let replay = import_legacy(&state, &entity_id, request)
            .await
            .data
            .unwrap();

        assert_eq!(replay, first);
        assert_eq!(load_presence(&state, &entity_id).unwrap(), Some(edited));
    }

    #[tokio::test]
    async fn legacy_first_signal_serialized_request_and_response_are_scoring_and_event_free() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let request = legacy_request();
        let request_json = serde_json::to_value(&request).unwrap();
        assert!(
            forbidden_public_keys(&request_json).is_empty(),
            "{request_json}"
        );
        for forbidden in [
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
            "message",
            "reply",
            "content",
            "prompt",
            "model_output",
            "memory",
            "payload",
        ] {
            let mut injected = request_json.clone();
            injected[forbidden] = serde_json::json!("private");
            assert!(
                serde_json::from_value::<LegacyFirstSignalMigrationRequest>(injected).is_err(),
                "migration request accepted forbidden key {forbidden}"
            );
        }

        let response = import_legacy(&state, &entity_id, request)
            .await
            .data
            .unwrap();
        let response_json = serde_json::to_value(response).unwrap();
        assert!(
            forbidden_public_keys(&response_json).is_empty(),
            "{response_json}"
        );
    }

    #[tokio::test]
    async fn legacy_first_signal_import_adds_only_its_applicable_presentation_badges() {
        let (state, entity_id) = crate::routes::tests::build_state();
        state
            .journeys
            .update(OWNER, &entity_id, "before", |journey| {
                journey.mentor_messages = 5;
                true
            })
            .unwrap();

        let response = import_legacy(&state, &entity_id, legacy_request()).await;

        assert!(response.ok, "{:?}", response.error);
        let earned = state
            .journeys
            .load(OWNER, &entity_id)
            .unwrap()
            .unwrap()
            .earned_badges();
        assert_eq!(
            earned,
            vec![
                Badge::FirstSpark,
                Badge::Named,
                Badge::Purposeful,
                Badge::PresenceChosen,
            ]
        );
    }

    #[tokio::test]
    async fn presence_route_composes_display_text_and_respects_unlocks() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let put = |identity: Palette, crest: Crest| {
            put_presence(
                State(state.clone()),
                Path(entity_id.clone()),
                Json(PresenceChoice {
                    temperament: Temperament::Steady,
                    identity,
                    crest,
                    motion: Motion::Still,
                }),
            )
        };
        let Json(locked) = put(Palette::Hearth, Crest::Orbit).await;
        assert!(!locked.ok);
        let Json(saved) = put(Palette::Ember, Crest::Wave).await;
        let card = saved.data.unwrap();
        assert_eq!(card.name, "Route Test");
        let Json(loaded) = get_presence(State(state.clone()), Path(entity_id.clone())).await;
        assert_eq!(loaded.data.unwrap(), Some(card));
        let Json(missing) = get_presence(State(state.clone()), Path("../x".into())).await;
        assert!(!missing.ok);

        // Mentoring conversations unlock the hearth palette.
        state
            .journeys
            .update(OWNER, &entity_id, "t", |j| {
                j.observe(
                    &[],
                    JourneyActivity {
                        mentor_messages: 1,
                        ..Default::default()
                    },
                    "t",
                ) && j.earned_badges().contains(&Badge::FirstWords)
            })
            .unwrap();
        let Json(unlocked) = put(Palette::Hearth, Crest::Orbit).await;
        assert!(unlocked.ok, "{:?}", unlocked.error);

        let path = presence_path(&state, &entity_id).unwrap();
        std::fs::write(&path, b"{\"name\":1}").unwrap();
        let Json(damaged) = get_presence(State(state.clone()), Path(entity_id.clone())).await;
        assert!(!damaged.ok, "damaged cards are reported, never reset");
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"name\":1}");

        let oversized = vec![b' '; MAX_PRESENCE_BYTES + 1];
        std::fs::write(&path, &oversized).unwrap();
        let Json(too_large) = get_presence(State(state), Path(entity_id)).await;
        assert_eq!(
            too_large.error.as_deref(),
            Some("The saved Presence card is too large")
        );
        assert_eq!(std::fs::read(path).unwrap(), oversized);
    }

    #[tokio::test]
    async fn generic_birth_does_not_award_genesis_first_light() {
        let (state, entity_id) = crate::routes::tests::build_state();
        let Json(birth) = crate::birth::perform_birth(
            State(state.clone()),
            Path(entity_id.clone()),
            Json(hive_core::BirthRiteRequest {
                path: "quickstart".into(),
                choices: vec![],
                purpose: None,
            }),
        )
        .await;
        assert!(birth.ok, "{:?}", birth.error);

        let journey = journey_view(&state, &entity_id).unwrap();
        let first_light = journey["badges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|badge| badge["badge"] == "first_light")
            .unwrap();
        assert!(
            first_light["earned_at"].is_null(),
            "generic birth is not a confirmed Genesis awakening"
        );
        assert!(state
            .journeys
            .load(OWNER, &entity_id)
            .unwrap()
            .unwrap()
            .genesis_session_id
            .is_none());
    }

    #[test]
    fn presence_write_cap_preserves_an_existing_card() {
        let directory =
            std::env::temp_dir().join(format!("abigail-presence-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(PRESENCE_FILE);
        let existing = br#"{"name":"Existing"}"#;
        std::fs::write(&path, existing).unwrap();

        let oversized = vec![b'x'; MAX_PRESENCE_BYTES + 1];
        assert_eq!(
            save_presence_bytes(&path, &oversized).unwrap_err(),
            "The Presence card is too large to save"
        );
        assert_eq!(std::fs::read(path).unwrap(), existing);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn store() -> JourneyStore {
        JourneyStore::new(PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap()).unwrap()
    }

    #[test]
    fn journeys_are_owner_scoped_durable_and_monotonic() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let first = JourneyStore::new(handle.clone()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        first
            .update(OWNER, &id, "t1", |j| {
                j.observe(
                    &[],
                    JourneyActivity {
                        mentor_messages: 5,
                        ..Default::default()
                    },
                    "t1",
                )
            })
            .unwrap();
        drop(first);
        let reopened = JourneyStore::new(handle).unwrap();
        let journey = reopened.load(OWNER, &id).unwrap().unwrap();
        assert!(journey.earned_badges().contains(&Badge::GettingAcquainted));
        assert!(reopened.load("another-household", &id).unwrap().is_none());
        let after = reopened
            .update(OWNER, &id, "t2", |j| {
                j.observe(&[], JourneyActivity::default(), "t2")
            })
            .unwrap();
        assert_eq!(after.mentor_messages, 5);
        assert_eq!(after.updated_at, "t1");
    }

    #[test]
    fn wrong_owner_cannot_overwrite_an_existing_journey() {
        let store = store();
        let id = uuid::Uuid::new_v4().to_string();
        let other = EntityJourney::new(&id, "another-household", "t1");
        store
            .handle
            .upsert(
                JOURNEY_TABLE,
                &id,
                &JourneyRecord {
                    entity_id: id.clone(),
                    owner_id: "another-household".into(),
                    payload: serde_json::to_string(&other).unwrap(),
                },
            )
            .unwrap();

        assert!(store.update(OWNER, &id, "t2", |_| true).is_err());
        assert!(store.load(OWNER, &id).unwrap().is_none());
        assert_eq!(
            store
                .load("another-household", &id)
                .unwrap()
                .unwrap()
                .owner_id,
            "another-household"
        );
    }

    #[test]
    fn concurrent_updates_preserve_every_earned_badge() {
        let store = std::sync::Arc::new(store());
        let id = uuid::Uuid::new_v4().to_string();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for badge in [Badge::Named, Badge::Purposeful] {
            let store = store.clone();
            let id = id.clone();
            let start = start.clone();
            workers.push(std::thread::spawn(move || {
                start.wait();
                store
                    .update(OWNER, &id, "t", |journey| {
                        journey.observe(
                            &[EarnedBadge {
                                badge,
                                earned_at: "t".into(),
                            }],
                            JourneyActivity::default(),
                            "t",
                        )
                    })
                    .unwrap();
            }));
        }
        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }

        let earned = store.load(OWNER, &id).unwrap().unwrap().earned_badges();
        assert!(earned.contains(&Badge::Named));
        assert!(earned.contains(&Badge::Purposeful));
    }

    #[test]
    fn journey_ids_must_be_entity_uuids() {
        let store = store();
        assert!(store.load(OWNER, "../escape").is_err());
        assert!(store.update(OWNER, "x", "t", |_| true).is_err());
    }

    fn outbox_event(
        entity_id: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> hive_core::EntityOutboxRecord {
        hive_core::EntityOutboxRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            entity_id: entity_id.to_string(),
            kind: kind.to_string(),
            created_at_utc: "2026-10-22T12:00:00Z".to_string(),
            payload,
        }
    }

    #[test]
    fn runtime_events_are_idempotent_owner_scoped_and_redacted() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let skill = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        let first = store
            .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&skill))
            .unwrap();
        assert_eq!(first.successful_skill_uses, 1);
        let duplicate = store
            .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&skill))
            .unwrap();
        assert_eq!(duplicate.successful_skill_uses, 1);

        let unknown = outbox_event(
            &entity_id,
            "chat_assistant_turn",
            serde_json::json!({
                "message": "private prompt",
                "reply": "private model output",
                "content": "private memory",
                "payload": {"arbitrary": true},
            }),
        );
        let unchanged = store
            .record_runtime_events(OWNER, &entity_id, &[unknown])
            .unwrap();
        assert_eq!(unchanged.successful_skill_uses, 1);
        assert_eq!(unchanged.confirmed_memories, 0);

        let rows = store
            .handle
            .query_vec::<serde_json::Value>("SELECT * FROM entity_journey_event", &[])
            .unwrap();
        assert_eq!(rows.len(), 1, "unknown event kinds are not persisted");
        let row = serde_json::to_string(&rows[0]).unwrap();
        for forbidden in ["message", "reply", "content", "prompt", "payload", "model"] {
            assert!(
                !row.contains(forbidden),
                "journey event leaked forbidden field {forbidden}: {row}"
            );
        }

        let wrong_entity = uuid::Uuid::new_v4().to_string();
        assert!(store
            .record_runtime_events(OWNER, &wrong_entity, std::slice::from_ref(&skill))
            .is_err());
        assert!(store
            .record_runtime_events(
                "another-household",
                &entity_id,
                std::slice::from_ref(&skill),
            )
            .is_err());
    }

    #[test]
    fn aggregate_skill_events_project_the_exact_count_and_keep_legacy_compatibility() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let aggregate = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-aggregate",
                "successful_skill_count": 3,
            }),
        );
        let legacy = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-legacy",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );

        let projected = store
            .record_runtime_events(OWNER, &entity_id, &[aggregate.clone(), legacy])
            .unwrap();
        assert_eq!(projected.successful_skill_uses, 4);

        let duplicate = store
            .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&aggregate))
            .unwrap();
        assert_eq!(duplicate.successful_skill_uses, 4);

        let saved = store
            .handle
            .select_record::<serde_json::Value>(JOURNEY_EVENT_TABLE, &aggregate.record_id)
            .unwrap()
            .unwrap();
        assert_eq!(saved["successful_skill_count"], 3);
        assert!(saved.get("skill_id").is_none() || saved["skill_id"].is_null());
        assert!(saved.get("tool_name").is_none() || saved["tool_name"].is_null());
        let encoded = saved.to_string();
        for forbidden in ["message", "reply", "content", "prompt", "payload", "model"] {
            assert!(
                !encoded.contains(forbidden),
                "aggregate journey event leaked forbidden field {forbidden}: {encoded}"
            );
        }
    }

    #[test]
    fn aggregate_skill_count_is_positive_bounded_and_rejects_private_fields() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        for payload in [
            serde_json::json!({
                "session_id": "session-zero",
                "successful_skill_count": 0,
            }),
            serde_json::json!({
                "session_id": "session-too-large",
                "successful_skill_count": 257,
            }),
            serde_json::json!({
                "session_id": "session-private",
                "successful_skill_count": 1,
                "content": "private tool output",
            }),
        ] {
            let event = outbox_event(&entity_id, "skill_used", payload);
            assert!(
                store
                    .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&event))
                    .is_err(),
                "invalid aggregate metadata was accepted: {}",
                event.payload
            );
            assert!(
                !store
                    .handle
                    .record_exists(JOURNEY_EVENT_TABLE, &event.record_id)
                    .unwrap(),
                "invalid aggregate metadata reached durable journey storage"
            );
        }
        assert!(store.load(OWNER, &entity_id).unwrap().is_none());
    }

    #[test]
    fn stored_journey_event_decoder_rejects_injected_private_fields() {
        let base = serde_json::json!({
            "record_id": uuid::Uuid::new_v4().to_string(),
            "owner_id": OWNER,
            "entity_id": uuid::Uuid::new_v4().to_string(),
            "kind": "skill_used",
            "occurred_at": "2026-10-22T12:00:00Z",
            "session_id": "session-1",
            "skill_id": "com.abigail.calendar",
            "tool_name": "list_events"
        });
        assert!(serde_json::from_value::<JourneyEventRecord>(base.clone()).is_ok());
        for field in [
            "payload",
            "message",
            "content",
            "reply",
            "prompt",
            "model_output",
            "arbitrary",
        ] {
            let mut injected = base.clone();
            injected[field] = serde_json::json!("private value");
            assert!(
                serde_json::from_value::<JourneyEventRecord>(injected).is_err(),
                "stored journey decoder accepted forbidden field {field}"
            );
        }
    }

    #[test]
    fn unknown_runtime_event_does_not_create_or_advance_a_journey() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let unknown = outbox_event(
            &entity_id,
            "chat_assistant_turn",
            serde_json::json!({
                "reply": "private model output",
                "payload": {"arbitrary": true},
            }),
        );
        let projected = store
            .record_runtime_events(OWNER, &entity_id, &[unknown])
            .unwrap();
        assert!(projected.earned.is_empty());
        assert_eq!(projected.activity(), JourneyActivity::default());
        assert!(
            store.load(OWNER, &entity_id).unwrap().is_none(),
            "unknown events must not create progression state"
        );
    }

    #[test]
    fn runtime_events_survive_reopen_and_concurrent_kinds_do_not_lose_counts() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let store = std::sync::Arc::new(JourneyStore::new(handle.clone()).unwrap());
        let entity_id = uuid::Uuid::new_v4().to_string();
        let start = std::sync::Arc::new(std::sync::Barrier::new(9));
        let mut workers = Vec::new();
        for index in 0..8 {
            let store = store.clone();
            let entity_id = entity_id.clone();
            let start = start.clone();
            workers.push(std::thread::spawn(move || {
                let event = if index % 2 == 0 {
                    outbox_event(
                        &entity_id,
                        "skill_used",
                        serde_json::json!({
                            "session_id": format!("session-{index}"),
                            "skill_id": "com.abigail.calendar",
                            "tool_name": "list_events",
                        }),
                    )
                } else {
                    outbox_event(
                        &entity_id,
                        "memory_confirmed",
                        serde_json::json!({
                            "memory_id": format!("memory-{index}"),
                            "memory_weight": "distilled",
                        }),
                    )
                };
                start.wait();
                store
                    .record_runtime_events(OWNER, &entity_id, &[event])
                    .unwrap();
            }));
        }
        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        drop(store);

        let reopened = JourneyStore::new(handle).unwrap();
        let activity = reopened.event_activity(OWNER, &entity_id).unwrap();
        assert_eq!(activity.successful_skill_uses, 4);
        assert_eq!(activity.confirmed_memories, 4);
        let journey = reopened.load(OWNER, &entity_id).unwrap().unwrap();
        assert_eq!(journey.successful_skill_uses, 4);
        assert_eq!(journey.confirmed_memories, 4);
    }

    #[test]
    fn malformed_later_event_rejects_the_whole_batch_before_writes() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let first = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        let malformed = outbox_event(
            &entity_id,
            "memory_confirmed",
            serde_json::json!({
                "memory_id": "memory-1",
                "memory_weight": "ephemeral",
            }),
        );

        assert!(store
            .record_runtime_events(OWNER, &entity_id, &[first.clone(), malformed])
            .is_err());
        assert!(!store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &first.record_id)
            .unwrap());
        assert!(store.load(OWNER, &entity_id).unwrap().is_none());
    }

    #[test]
    fn later_saved_id_collision_rejects_the_whole_batch_before_writes() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let saved = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-saved",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        store
            .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&saved))
            .unwrap();

        let first = outbox_event(
            &entity_id,
            "memory_confirmed",
            serde_json::json!({
                "memory_id": "memory-new",
                "memory_weight": "distilled",
            }),
        );
        let mut collision = saved;
        collision.payload["tool_name"] = serde_json::json!("delete_event");

        assert!(store
            .record_runtime_events(OWNER, &entity_id, &[first.clone(), collision])
            .is_err());
        assert!(!store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &first.record_id)
            .unwrap());
        assert_eq!(
            store.event_activity(OWNER, &entity_id).unwrap(),
            JourneyActivity {
                successful_skill_uses: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn conflicting_ids_inside_one_batch_are_rejected_before_writes() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let first = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        let mut collision = first.clone();
        collision.payload["tool_name"] = serde_json::json!("delete_event");

        assert!(store
            .record_runtime_events(OWNER, &entity_id, &[first.clone(), collision])
            .is_err());
        assert!(!store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &first.record_id)
            .unwrap());
    }

    #[test]
    fn partial_event_storage_failure_retries_missing_rows_without_double_counting() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let skill = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        let memory = outbox_event(
            &entity_id,
            "memory_confirmed",
            serde_json::json!({
                "memory_id": "memory-1",
                "memory_weight": "distilled",
            }),
        );
        store.fail_event_writes_after(1);

        assert!(store
            .record_runtime_events(OWNER, &entity_id, &[skill.clone(), memory.clone()])
            .is_err());
        assert!(store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &skill.record_id)
            .unwrap());
        assert!(!store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &memory.record_id)
            .unwrap());

        let projection = store
            .record_runtime_events(OWNER, &entity_id, &[skill, memory])
            .unwrap();
        assert_eq!(projection.successful_skill_uses, 1);
        assert_eq!(projection.confirmed_memories, 1);
        let rows = store
            .handle
            .query_vec::<serde_json::Value>("SELECT * FROM entity_journey_event", &[])
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn durable_event_rows_self_heal_after_projection_write_failure() {
        let store = store();
        let entity_id = uuid::Uuid::new_v4().to_string();
        let event = outbox_event(
            &entity_id,
            "skill_used",
            serde_json::json!({
                "session_id": "session-1",
                "skill_id": "com.abigail.calendar",
                "tool_name": "list_events",
            }),
        );
        store.fail_next_projection_write();

        assert!(store
            .record_runtime_events(OWNER, &entity_id, std::slice::from_ref(&event))
            .is_err());
        assert!(store
            .handle
            .record_exists(JOURNEY_EVENT_TABLE, &event.record_id)
            .unwrap());
        assert!(store.load(OWNER, &entity_id).unwrap().is_none());

        let healed = store
            .record_runtime_events(OWNER, &entity_id, &[event])
            .unwrap();
        assert_eq!(healed.successful_skill_uses, 1);
        assert_eq!(healed.confirmed_memories, 0);
    }

    #[test]
    fn conversation_summary_counts_all_distinct_user_activity() {
        let turn = |id: &str, session: &str, role: &str, at: &str| {
            let mut turn = abigail_memory::ConversationTurn::new(session, role, "private text");
            turn.id = id.to_string();
            turn.created_at = chrono::DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&chrono::Utc);
            turn
        };
        let turns = vec![
            turn("a", "session-1", "user", "2026-01-01T10:00:00Z"),
            turn("a", "session-1", "user", "2026-01-01T10:00:00Z"),
            turn("b", "session-1", "user", "2026-01-02T10:00:00Z"),
            turn("c", "session-2", "user", "2026-01-08T10:00:00Z"),
            turn("d", "session-2", "assistant", "2026-01-08T10:01:00Z"),
        ];
        let activity = summarize_conversations(&turns);
        assert_eq!(activity.mentor_messages, 3);
        assert_eq!(activity.conversation_sessions, 2);
        assert_eq!(activity.active_days, 3);
        assert_eq!(activity.active_weeks, 2);
    }

    #[test]
    fn durable_entity_facts_scan_more_than_five_hundred_turns_and_merge_by_max() {
        let memory = abigail_memory::MemoryStore::open_in_memory().unwrap();
        for index in 0..525 {
            let mut turn = abigail_memory::ConversationTurn::new(
                &format!("session-{}", index % 7),
                "user",
                "private mentor message",
            );
            turn.id = format!("user-turn-{index:04}");
            memory.insert_turn(&turn).unwrap();
        }
        let assistant =
            abigail_memory::ConversationTurn::new("session-0", "assistant", "private model output")
                .with_successful_tool_uses(vec![
                    abigail_memory::SuccessfulToolUse {
                        skill_id: "com.abigail.calendar".to_string(),
                        tool_name: "list_events".to_string(),
                    },
                    abigail_memory::SuccessfulToolUse {
                        skill_id: "com.abigail.weather".to_string(),
                        tool_name: "forecast".to_string(),
                    },
                ]);
        memory.insert_turn(&assistant).unwrap();
        memory
            .insert_memory(
                &abigail_memory::Memory::distilled("explicit private memory".to_string())
                    .with_explicit_confirmation(),
            )
            .unwrap();
        memory
            .insert_memory(&abigail_memory::Memory::crystallized(
                "system-generated private memory".to_string(),
            ))
            .unwrap();
        memory
            .insert_memory(
                &abigail_memory::Memory::ephemeral("temporary private memory".to_string())
                    .with_explicit_confirmation(),
            )
            .unwrap();

        let durable = durable_entity_activity(&memory).unwrap();
        assert_eq!(durable.mentor_messages, 525);
        assert_eq!(durable.conversation_sessions, 7);
        assert_eq!(durable.successful_skill_uses, 2);
        assert_eq!(durable.confirmed_memories, 1);

        let merged = merge_activity_sources(
            durable,
            JourneyActivity {
                successful_skill_uses: 1,
                confirmed_memories: 3,
                ..Default::default()
            },
        );
        assert_eq!(merged.mentor_messages, 525);
        assert_eq!(merged.successful_skill_uses, 2);
        assert_eq!(merged.confirmed_memories, 3);
    }
}
