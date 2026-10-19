//! UI presence profile routes. No runtime configuration is modified here.

use crate::presence_profile::{load_profile, save_profile, PresenceProfile, MAX_PROFILE_BYTES};
use abigail_identity::IdentityManager;
use axum::extract::{rejection::JsonRejection, DefaultBodyLimit, Path, State};
use axum::routing::get;
use axum::{Json, Router};
use hive_core::ApiEnvelope;
use std::path::PathBuf;
use std::sync::Arc;

/// The profile routes need only the registered entity's directory and name.
/// Tests can supply an isolated registry without initializing the daemon vault.
pub trait PresenceIdentityResolver: Send + Sync {
    fn registered_entity(&self, id: &str) -> Result<(PathBuf, String), String>;
}

impl PresenceIdentityResolver for IdentityManager {
    fn registered_entity(&self, id: &str) -> Result<(PathBuf, String), String> {
        let entity = self
            .list_agents()?
            .into_iter()
            .find(|entity| entity.id == id)
            .ok_or_else(|| "Entity is not registered".to_string())?;
        // Use the guarded registry directory rather than trusting data_dir read
        // from a config file. The request ID is never joined into a path.
        let directory = self.agent_dir(id)?;
        Ok((directory, entity.name))
    }
}

type PresenceState = Arc<dyn PresenceIdentityResolver>;

/// Shared production/test route wiring, including Axum JSON body limits.
pub fn router<S: Clone + Send + Sync + 'static>(identities: PresenceState) -> Router<S> {
    Router::new()
        .route(
            "/v1/entities/:id/presence",
            get(get_presence)
                .put(put_presence)
                .layer(DefaultBodyLimit::max(MAX_PROFILE_BYTES)),
        )
        .with_state(identities)
}

fn entity_directory(state: &PresenceState, id: &str) -> Result<(PathBuf, String), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid entity ID".to_string())?;
    state.registered_entity(id)
}

async fn get_presence(
    State(state): State<PresenceState>,
    Path(id): Path<String>,
) -> Json<ApiEnvelope<PresenceProfile>> {
    let result =
        entity_directory(&state, &id).and_then(|(directory, name)| load_profile(&directory, &name));
    Json(match result {
        Ok(profile) => ApiEnvelope::success(profile),
        Err(error) => ApiEnvelope::error(error),
    })
}

async fn put_presence(
    State(state): State<PresenceState>,
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
