//! UI presence profile routes. No runtime configuration is modified here.

use crate::presence_profile::{load_profile, save_profile, PresenceProfile};
use crate::state::HiveDaemonState;
use axum::extract::{rejection::JsonRejection, Path, State};
use axum::Json;
use hive_core::ApiEnvelope;
use std::path::PathBuf;

fn entity_directory(state: &HiveDaemonState, id: &str) -> Result<(PathBuf, String), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid entity ID".to_string())?;
    let entity = state
        .identity_manager
        .list_agents()?
        .into_iter()
        .find(|entity| entity.id == id)
        .ok_or_else(|| "Entity is not registered".to_string())?;
    // Use the guarded registry directory rather than trusting data_dir read
    // from a config file. The request ID is never joined into a path.
    let directory = state.identity_manager.agent_dir(id)?;
    Ok((directory, entity.name))
}

pub async fn get_presence(
    State(state): State<HiveDaemonState>,
    Path(id): Path<String>,
) -> Json<ApiEnvelope<PresenceProfile>> {
    let result =
        entity_directory(&state, &id).and_then(|(directory, name)| load_profile(&directory, &name));
    Json(match result {
        Ok(profile) => ApiEnvelope::success(profile),
        Err(error) => ApiEnvelope::error(error),
    })
}

pub async fn put_presence(
    State(state): State<HiveDaemonState>,
    Path(id): Path<String>,
    body: Result<Json<PresenceProfile>, JsonRejection>,
) -> Json<ApiEnvelope<PresenceProfile>> {
    let result: Result<PresenceProfile, String> = (|| {
        let (directory, _) = entity_directory(&state, &id)?;
        let Json(profile) =
            body.map_err(|error| format!("Invalid presence: {}", error.body_text()))?;
        save_profile(&directory, &profile)?;
        Ok(profile)
    })();
    Json(match result {
        Ok(profile) => ApiEnvelope::success(profile),
        Err(error) => ApiEnvelope::error(error),
    })
}
