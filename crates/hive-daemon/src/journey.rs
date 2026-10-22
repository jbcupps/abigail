//! Entity journeys and Presence cards. Both are presentation: they never feed
//! prompts, permissions, providers, or the signed constitutional documents.
//!
//! Journeys live in the Hive-owned store (`entity_journey`) so badges and
//! mentoring milestones outlive an expired or deleted Genesis record.
//! Presence cards live beside the Entity as `presence.json`, outside the
//! `docs/` folder that runtimes read for their system prompt.

use crate::{genesis_http::OWNER, state::HiveDaemonState};
use abigail_persistence::{EntityScope, PersistenceHandle};
use anyhow::{anyhow, bail, Context, Result};
use axum::{
    extract::{Path, State},
    Json,
};
use hive_core::genesis::{EntityDefinition, GenesisSession};
use hive_core::genesis_quest::*;
use hive_core::{ApiEnvelope, BirthCertificate, BirthCertificateSummary};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::Mutex;

const JOURNEY_TABLE: &str = "entity_journey";
const PRESENCE_FILE: &str = "presence.json";
const MAX_PRESENCE_BYTES: usize = 32 * 1024;

pub struct JourneyStore {
    handle: PersistenceHandle,
    writes: Mutex<()>,
}

/// Serialized as text like Genesis sessions, with ownership cross-checked.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JourneyRecord {
    entity_id: String,
    owner_id: String,
    payload: String,
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
        Ok(Self {
            handle,
            writes: Mutex::new(()),
        })
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
            self.handle.upsert(
                JOURNEY_TABLE,
                entity_id,
                &JourneyRecord {
                    entity_id: entity_id.into(),
                    owner_id: owner_id.into(),
                    payload: serde_json::to_string(&journey)?,
                },
            )?;
        }
        Ok(journey)
    }
}

fn validate_entity_id(id: &str) -> Result<()> {
    uuid::Uuid::parse_str(id).map_err(|_| anyhow!("Invalid Entity ID"))?;
    Ok(())
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
            journey.observe(&session.quest.earned, 0, &now) || linked
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Recent turns scanned for milestones: far above the highest target, and the
/// journey keeps the highest count ever observed.
const MILESTONE_WINDOW: usize = 500;

/// Messages the mentor has sent this Entity. Hive owns the embedded store, so
/// this reads the Entity's own scoped conversation turns, never another's.
fn mentor_messages(state: &HiveDaemonState, entity_id: &str) -> Result<u64, String> {
    let config = state.identity_manager.load_agent(entity_id)?;
    if !config.birth_complete {
        return Ok(0);
    }
    let memory =
        abigail_memory::MemoryStore::open_with_config(&config).map_err(|e| e.to_string())?;
    Ok(memory
        .recent_turns_all_sessions(MILESTONE_WINDOW)
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|turn| turn.role == "user")
        .count() as u64)
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
    let messages = mentor_messages(state, entity_id)?;
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
                journey.observe(&creation, messages, &now) || relink
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
    let next_milestone = Badge::ALL.into_iter().find_map(|badge| {
        let target = badge.mentoring_target()?;
        (journey.mentor_messages < target).then_some(Milestone {
            badge,
            title: badge.title(),
            target,
            progress: journey.mentor_messages,
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
        next_milestone,
        unlocked: unlocks(&earned),
        presence,
        certificate: certificate(state, entity_id),
    };
    serde_json::to_value(view).map_err(|e| e.to_string())
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
}
