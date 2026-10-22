//! Owner-scoped durable Genesis sessions in the existing Hive database.
//! A lock serializes compare-and-save inside Hive (the sole embedded-store
//! process); clients must supply their last observed revision.

use abigail_persistence::{EntityScope, PersistenceHandle, QueryBinding};
use anyhow::{anyhow, bail, Context, Result};
use hive_core::genesis::GenesisSession;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const SESSION_TABLE: &str = "genesis_session";
const MAX_OWNER_SESSIONS: usize = 100;

pub struct GenesisStore {
    handle: PersistenceHandle,
    writes: Mutex<()>,
}

/// Store serialized JSON as text so the embedded engine does not reinterpret
/// nested timestamps, nulls, maps, or entity IDs. Indexed ownership and revision
/// are still explicit and cross-checked after deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    id: String,
    owner_id: String,
    revision: u64,
    updated_at: String,
    payload: String,
}

impl SessionRecord {
    fn from_session(session: &GenesisSession) -> Result<Self> {
        session.validate().map_err(|error| anyhow!(error))?;
        Ok(Self {
            id: session.id.clone(),
            owner_id: session.owner_id.clone(),
            revision: session.revision,
            updated_at: session.updated_at.clone(),
            payload: serde_json::to_string(session)?,
        })
    }

    fn into_session(self) -> Result<GenesisSession> {
        let session: GenesisSession =
            serde_json::from_str(&self.payload).context("Invalid saved Genesis session")?;
        if session.id != self.id
            || session.owner_id != self.owner_id
            || session.revision != self.revision
        {
            bail!("Genesis session metadata does not match its saved definition");
        }
        session.validate().map_err(|error| anyhow!(error))?;
        Ok(session)
    }
}

impl GenesisStore {
    pub fn new(handle: PersistenceHandle) -> Result<Self> {
        if handle.scope() != &EntityScope::Hive {
            bail!("Genesis sessions must use the Hive persistence scope");
        }
        handle.upsert("migration_log", "genesis_sessions_v1", &serde_json::json!({
            "schema_version": 1,
            "description": "Versioned owner-scoped Genesis sessions, sources, proposals and identity revisions"
        }))?;
        Ok(Self {
            handle,
            writes: Mutex::new(()),
        })
    }

    pub fn create(&self, session: &GenesisSession) -> Result<()> {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Genesis store lock unavailable"))?;
        self.create_locked(session)
    }

    pub fn create_or_resume(&self, proposed: &GenesisSession) -> Result<GenesisSession> {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Genesis store lock unavailable"))?;
        let parsed = uuid::Uuid::parse_str(&proposed.id)
            .map_err(|_| anyhow!("Creation key must be a canonical UUID"))?;
        if parsed.to_string() != proposed.id {
            bail!("Creation key must be a canonical UUID");
        }
        if proposed.creation_seed.is_none() {
            bail!("Keyed creation requires reviewed identity input");
        }
        if let Some(existing) = self.load(&proposed.owner_id, &proposed.id)? {
            if existing.creation_seed != proposed.creation_seed {
                bail!(
                    "This creation key belongs to different original identity input. Resume the saved draft."
                );
            }
            return Ok(existing);
        }
        self.create_locked(proposed)?;
        Ok(proposed.clone())
    }

    fn create_locked(&self, session: &GenesisSession) -> Result<()> {
        validate_id(&session.id)?;
        let record = SessionRecord::from_session(session)?;
        if self.handle.record_exists(SESSION_TABLE, &session.id)? {
            bail!("Genesis session already exists");
        }
        if self.list(&session.owner_id)?.len() >= MAX_OWNER_SESSIONS {
            bail!("Genesis session limit reached; delete an unused draft to continue");
        }
        self.handle.create(SESSION_TABLE, &session.id, &record)?;
        Ok(())
    }

    pub fn load(&self, owner_id: &str, id: &str) -> Result<Option<GenesisSession>> {
        validate_id(id)?;
        let Some(record) = self
            .handle
            .select_record::<SessionRecord>(SESSION_TABLE, id)?
        else {
            return Ok(None);
        };
        // Wrong owner and absent records have identical visibility.
        if record.owner_id != owner_id {
            return Ok(None);
        }
        Ok(Some(record.into_session()?))
    }

    pub fn list(&self, owner_id: &str) -> Result<Vec<GenesisSession>> {
        let records = self.handle.query_vec::<SessionRecord>(
            "SELECT * FROM genesis_session WHERE owner_id = $owner_id ORDER BY updated_at DESC",
            &[QueryBinding::new("owner_id", owner_id)?],
        )?;
        records
            .into_iter()
            .map(SessionRecord::into_session)
            .collect()
    }

    pub fn save(&self, session: &GenesisSession, expected_revision: u64) -> Result<()> {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Genesis store lock unavailable"))?;
        self.save_locked(session, expected_revision)
    }

    fn save_locked(&self, session: &GenesisSession, expected_revision: u64) -> Result<()> {
        let Some(existing) = self.load(&session.owner_id, &session.id)? else {
            bail!("Genesis session not found")
        };
        if existing.revision != expected_revision {
            bail!("Genesis revision conflict: reload the current draft before applying changes");
        }
        if session.revision <= expected_revision {
            bail!("A saved Genesis change must advance the revision");
        }
        if session.created_at != existing.created_at
            || session.definition.created_at != existing.definition.created_at
        {
            bail!("Genesis creation timestamps are immutable");
        }
        self.handle.upsert(
            SESSION_TABLE,
            &session.id,
            &SessionRecord::from_session(session)?,
        )?;
        Ok(())
    }

    /// Atomic in-process mutation with optimistic revision checking. Expensive
    /// model/media operations must happen outside this closure; their resulting
    /// changes are then committed against the revision they started from.
    #[cfg(test)]
    pub fn mutate<F>(
        &self,
        owner_id: &str,
        id: &str,
        expected_revision: u64,
        change: F,
    ) -> Result<GenesisSession>
    where
        F: FnOnce(&mut GenesisSession) -> std::result::Result<(), String>,
    {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Genesis store lock unavailable"))?;
        let mut session = self
            .load(owner_id, id)?
            .ok_or_else(|| anyhow!("Genesis session not found"))?;
        if session.revision != expected_revision {
            bail!("Genesis revision conflict: reload the current draft before applying changes");
        }
        change(&mut session).map_err(|error| anyhow!(error))?;
        if session.id != id || session.owner_id != owner_id {
            bail!("Genesis ownership cannot change");
        }
        if session.revision == expected_revision {
            session.touch(&chrono::Utc::now().to_rfc3339());
        }
        self.save_locked(&session, expected_revision)?;
        Ok(session)
    }

    pub fn delete(&self, owner_id: &str, id: &str) -> Result<bool> {
        let _lock = self
            .writes
            .lock()
            .map_err(|_| anyhow!("Genesis store lock unavailable"))?;
        if self.load(owner_id, id)?.is_none() {
            return Ok(false);
        }
        self.handle.delete_record(SESSION_TABLE, id)?;
        Ok(true)
    }
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        bail!("Invalid Genesis session ID");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::genesis::{FieldUpdate, GenesisCreationSeed, GenesisField, GenesisMode};
    use hive_core::genesis_quest::PresenceChoice;
    use serde_json::json;

    fn store() -> GenesisStore {
        GenesisStore::new(PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap()).unwrap()
    }
    fn session(owner: &str) -> GenesisSession {
        GenesisSession::new(
            "draft-a".into(),
            owner.into(),
            GenesisMode::Guided,
            "2026-10-17T12:00:00Z".into(),
        )
    }

    fn keyed(owner: &str) -> GenesisSession {
        let mut session = GenesisSession::new(
            uuid::Uuid::new_v4().to_string(),
            owner.into(),
            GenesisMode::Quick,
            "2026-10-17T12:00:00Z".into(),
        );
        let seed = GenesisCreationSeed {
            mode: GenesisMode::Quick,
            name: "Signal".into(),
            purpose: "Explore tiny worlds".into(),
            presence: PresenceChoice::default(),
        };
        session.definition.name = seed.name.clone();
        session.definition.purpose = seed.purpose.clone();
        session.creation_seed = Some(seed);
        session
    }

    #[test]
    fn keyed_replay_returns_saved_session_and_rejects_changed_seed() {
        let store = store();
        let proposed = keyed("owner-a");
        let mut saved = store.create_or_resume(&proposed).unwrap();
        saved.definition.identity = "Reviewed identity".into();
        saved.touch("later");
        store.save(&saved, 0).unwrap();

        let resumed = store.create_or_resume(&proposed).unwrap();
        assert_eq!(resumed.definition.identity, "Reviewed identity");
        assert_eq!(resumed.revision, 1);

        let mut changed = proposed;
        changed.creation_seed.as_mut().unwrap().purpose = "Different".into();
        assert!(store.create_or_resume(&changed).is_err());
        assert_eq!(store.list("owner-a").unwrap().len(), 1);
    }

    #[test]
    fn keyed_creation_requires_a_canonical_uuid_and_seed() {
        let store = store();
        let mut proposed = keyed("owner-a");
        proposed.id = "not-a-uuid".into();
        assert!(store.create_or_resume(&proposed).is_err());
        proposed.id = uuid::Uuid::new_v4().to_string();
        proposed.creation_seed = None;
        assert!(store.create_or_resume(&proposed).is_err());
    }

    #[test]
    fn owner_is_required_for_load_list_mutation_and_delete() {
        let store = store();
        let session = session("owner-a");
        store.create(&session).unwrap();
        assert!(store.load("owner-b", &session.id).unwrap().is_none());
        assert!(store.list("owner-b").unwrap().is_empty());
        assert!(store.mutate("owner-b", &session.id, 0, |_| Ok(())).is_err());
        assert!(!store.delete("owner-b", &session.id).unwrap());
        assert!(store.load("owner-a", &session.id).unwrap().is_some());
    }

    #[test]
    fn stale_revision_cannot_overwrite_a_newer_edit() {
        let store = store();
        let s = session("owner-a");
        store.create(&s).unwrap();
        let changed = store
            .mutate("owner-a", &s.id, 0, |draft| {
                draft.apply_updates(
                    vec![FieldUpdate {
                        field: GenesisField::Name,
                        value: json!("Nova"),
                        source_ref: None,
                    }],
                    "User named Entity",
                    "later",
                )
            })
            .unwrap();
        assert_eq!(changed.definition.name, "Nova");
        assert!(store
            .mutate("owner-a", &s.id, 0, |draft| {
                draft.definition.name = "Wrong".into();
                Ok(())
            })
            .is_err());
        assert_eq!(
            store
                .load("owner-a", &s.id)
                .unwrap()
                .unwrap()
                .definition
                .name,
            "Nova"
        );
    }

    #[test]
    fn session_survives_new_store_wrapper_and_delete_removes_it() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let first = GenesisStore::new(handle.clone()).unwrap();
        let mut draft = session("owner-a");
        draft
            .import_markdown(
                "source-one".into(),
                "lore.md".into(),
                "## Unknown\nPreserve me".into(),
                "later",
            )
            .unwrap();
        first.create(&draft).unwrap();
        drop(first);
        let reopened = GenesisStore::new(handle).unwrap();
        let restored = reopened.load("owner-a", &draft.id).unwrap().unwrap();
        assert_eq!(restored.sources[0].content, "## Unknown\nPreserve me");
        assert_eq!(restored.revision, draft.revision);
        assert!(reopened.delete("owner-a", &draft.id).unwrap());
        assert!(reopened.load("owner-a", &draft.id).unwrap().is_none());
    }

    #[test]
    fn simultaneous_edits_from_one_revision_have_one_winner() {
        use std::sync::{Arc, Barrier};
        let store = Arc::new(store());
        let draft = session("owner-a");
        store.create(&draft).unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = vec![];
        for name in ["Nova", "Aelira"] {
            let store = store.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store
                    .mutate("owner-a", "draft-a", 0, |draft| {
                        draft.apply_updates(
                            vec![FieldUpdate {
                                field: GenesisField::Name,
                                value: json!(name),
                                source_ref: None,
                            }],
                            "simultaneous edit",
                            "later",
                        )
                    })
                    .is_ok()
            }));
        }
        barrier.wait();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| usize::from(worker.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert_eq!(
            store.load("owner-a", "draft-a").unwrap().unwrap().revision,
            1
        );
    }
}
