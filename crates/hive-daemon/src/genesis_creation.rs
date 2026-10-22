//! Bridge Genesis activation to durable Entity creation receipts.

use abigail_identity::{AgentCreationStatus, IdentityManager};
use hive_core::genesis::GenesisSession;

pub fn session_for_entity(
    state: &crate::state::HiveDaemonState,
    entity_id: &str,
) -> Result<Option<GenesisSession>, String> {
    for draft in state
        .genesis
        .list(crate::genesis_http::OWNER)
        .map_err(|error| error.to_string())?
    {
        if draft.definition.entity_id.as_deref() == Some(entity_id) {
            return Ok(Some(draft));
        }
        if draft.definition.entity_id.is_none()
            && uuid::Uuid::parse_str(&draft.id).is_ok_and(|id| id.to_string() == draft.id)
        {
            match state
                .identity_manager
                .entity_creation_status(&draft.id)
                .map_err(|error| error.to_string())?
            {
                AgentCreationStatus::Complete { id, .. }
                | AgentCreationStatus::Unavailable { id, .. }
                    if id == entity_id =>
                {
                    return Ok(Some(draft));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

pub fn receipt_entity_id(
    identities: &IdentityManager,
    draft: &GenesisSession,
) -> Result<Option<String>, String> {
    if draft.definition.entity_id.is_some()
        || !uuid::Uuid::parse_str(&draft.id).is_ok_and(|id| id.to_string() == draft.id)
    {
        return Ok(None);
    }
    match identities
        .entity_creation_status(&draft.id)
        .map_err(|error| error.to_string())?
    {
        AgentCreationStatus::Complete { id, .. } | AgentCreationStatus::Unavailable { id, .. } => {
            Ok(Some(id))
        }
        AgentCreationStatus::Absent | AgentCreationStatus::Reserved { .. } => Ok(None),
    }
}

pub fn ensure_entity_link(
    identities: &IdentityManager,
    draft: &mut GenesisSession,
    save: impl FnOnce(&GenesisSession, u64) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(entity_id) = &draft.definition.entity_id {
        identities.load_agent(entity_id)?;
        return Ok(());
    }
    let name = match identities
        .entity_creation_status(&draft.id)
        .map_err(|error| error.to_string())?
    {
        AgentCreationStatus::Absent => draft.definition.name.clone(),
        AgentCreationStatus::Reserved { name } | AgentCreationStatus::Complete { name, .. } => name,
        AgentCreationStatus::Unavailable { .. } => {
            return Err(
                "The Entity reserved for this creation is unavailable. No replacement was created."
                    .into(),
            );
        }
    };
    let fingerprint = draft
        .creation_seed
        .as_ref()
        .map(|seed| seed.fingerprint())
        .transpose()?;
    let (entity_id, _) = identities
        .create_agent_idempotent_with_fingerprint(&name, &draft.id, fingerprint.as_deref())
        .map_err(|error| error.to_string())?;
    let mut linked = draft.clone();
    linked.definition.entity_id = Some(entity_id);
    linked.touch(&chrono::Utc::now().to_rfc3339());
    save(&linked, draft.revision)?;
    *draft = linked;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_entity_link;
    use crate::genesis_http::OWNER;
    use hive_core::genesis::{GenesisMode, GenesisSession};

    #[test]
    fn interrupted_session_link_save_reuses_the_receipt_uuid() {
        let (state, _) = crate::routes::tests::build_state();
        let baseline = state.identity_manager.list_agents().unwrap().len();
        let mut draft = GenesisSession::new(
            uuid::Uuid::new_v4().to_string(),
            OWNER.into(),
            GenesisMode::Quick,
            "2026-10-20T00:00:00Z".into(),
        );
        draft.definition.name = "Vesper".into();
        draft.definition.purpose = "Explore tiny worlds".into();
        state.genesis.create(&draft).unwrap();

        let mut allocated = None;
        assert!(
            ensure_entity_link(&state.identity_manager, &mut draft, |linked, _| {
                allocated = linked.definition.entity_id.clone();
                Err("interrupted session save".into())
            })
            .is_err()
        );
        assert!(draft.definition.entity_id.is_none());
        let entity_id = allocated.clone().unwrap();
        assert!(crate::genesis::genesis_hold(&state, &entity_id)
            .unwrap()
            .is_some());
        assert!(crate::genesis::awaiting_awakening(&state, &entity_id).unwrap());

        let mut retry = state.genesis.load(OWNER, &draft.id).unwrap().unwrap();
        retry.definition.name = "Vesper reviewed".into();
        ensure_entity_link(&state.identity_manager, &mut retry, |linked, revision| {
            assert_eq!(revision, 0);
            assert_eq!(linked.definition.entity_id, allocated);
            state
                .genesis
                .save(linked, revision)
                .map_err(|error| error.to_string())
        })
        .unwrap();
        drop(retry);
        let reloaded = state.genesis.load(OWNER, &draft.id).unwrap().unwrap();
        assert_eq!(reloaded.definition.entity_id, allocated);
        assert_eq!(reloaded.revision, 1);
        assert_eq!(
            state.identity_manager.list_agents().unwrap().len(),
            baseline + 1
        );
    }
}
