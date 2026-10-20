//! Bridge Genesis activation to the existing durable Entity creation receipts.
use abigail_identity::{AgentCreationStatus, IdentityManager};
use hive_core::genesis::GenesisSession;

pub fn ensure_entity_link(
    identities: &IdentityManager,
    draft: &mut GenesisSession,
    save: impl FnOnce(&GenesisSession, u64) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(id) = &draft.definition.entity_id {
        // Legacy Genesis links remain authoritative. Never allocate on return.
        identities.load_agent(id).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let original_name = match identities
        .entity_creation_status(&draft.id)
        .map_err(|e| e.to_string())?
    {
        AgentCreationStatus::Absent => draft.definition.name.clone(),
        AgentCreationStatus::Reserved { name } | AgentCreationStatus::Complete { name, .. } => name,
        AgentCreationStatus::Unavailable { .. } => {
            return Err(
                "The Entity reserved for this creation is unavailable. No replacement was created."
                    .into(),
            )
        }
    };
    let (entity_id, _) = identities
        .create_agent_idempotent(&original_name, &draft.id)
        .map_err(|e| e.to_string())?;
    // If this save fails, the registry still owns the exact key -> UUID mapping.
    // Do not advance the caller's in-memory session until persistence confirms it.
    let mut linked = draft.clone();
    linked.definition.entity_id = Some(entity_id);
    linked.touch(&chrono::Utc::now().to_rfc3339());
    save(&linked, draft.revision)?;
    *draft = linked;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hive_core::genesis::GenesisMode;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(
                std::env::temp_dir().join(format!("abigail-genesis-link-{}", uuid::Uuid::new_v4())),
            )
        }
        fn manager(&self) -> IdentityManager {
            IdentityManager::isolated_creation_test_manager(self.0.clone()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn draft() -> GenesisSession {
        let mut value = GenesisSession::new(
            uuid::Uuid::new_v4().to_string(),
            "local-household".into(),
            GenesisMode::Quick,
            "2026-10-20T00:00:00Z".into(),
        );
        value.definition.name = "Vesper".into();
        value.definition.purpose = "Explore tiny worlds".into();
        value
    }
    #[test]
    fn interrupted_link_recovers_the_same_uuid_after_restart_and_duplicate_retry() {
        let root = Fixture::new();
        let mut draft = draft();
        let mut allocated = String::new();
        let failure = ensure_entity_link(&root.manager(), &mut draft, |linked, _| {
            allocated = linked.definition.entity_id.clone().unwrap();
            Err("fixture interrupted before link save".into())
        });
        assert!(failure.is_err());
        assert!(draft.definition.entity_id.is_none());
        draft.definition.name = "Vesper revised".into();
        ensure_entity_link(&root.manager(), &mut draft, |linked, _| {
            assert_eq!(
                linked.definition.entity_id.as_deref(),
                Some(allocated.as_str())
            );
            Ok(())
        })
        .unwrap();
        let revision = draft.revision;
        ensure_entity_link(&root.manager(), &mut draft, |_, _| {
            panic!("linked return must not save or allocate")
        })
        .unwrap();
        assert_eq!(draft.revision, revision);
        assert_eq!(root.manager().list_agents().unwrap().len(), 1);
    }
    #[test]
    fn missing_allocated_entity_never_gets_a_replacement() {
        let root = Fixture::new();
        let mut draft = draft();
        let (_, directory) = root
            .manager()
            .create_agent_idempotent("Vesper", &draft.id)
            .unwrap();
        std::fs::remove_file(directory.join("config.json")).unwrap();
        assert!(
            ensure_entity_link(&root.manager(), &mut draft, |_, _| panic!("must not save"))
                .is_err()
        );
        assert_eq!(root.manager().list_agents().unwrap().len(), 1);
    }
    #[test]
    fn legacy_link_and_presentation_data_are_preserved_without_a_new_receipt() {
        let root = Fixture::new();
        let mut draft = draft();
        let old_key = uuid::Uuid::new_v4().to_string();
        let (id, directory) = root
            .manager()
            .create_agent_idempotent("Legacy", &old_key)
            .unwrap();
        let brief = br#"{"name":"Display alias","purpose":"presentation only"}"#;
        std::fs::write(directory.join("presence.json"), brief).unwrap();
        draft.definition.entity_id = Some(id.clone());
        ensure_entity_link(&root.manager(), &mut draft, |_, _| {
            panic!("legacy return must not save")
        })
        .unwrap();
        assert_eq!(draft.definition.entity_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            root.manager().entity_creation_status(&draft.id).unwrap(),
            AgentCreationStatus::Absent
        );
        assert_eq!(
            std::fs::read(directory.join("presence.json")).unwrap(),
            brief
        );
    }
}
