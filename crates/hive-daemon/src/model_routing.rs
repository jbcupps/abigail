//! Hive-owned model consent. These endpoints contain no account credentials.
use crate::state::HiveDaemonState;
use abigail_core::model_routing::{effective_policy, ModelConnection, ModelRoutingPolicy};
use axum::{extract::State, Json};
use hive_core::ApiEnvelope;

pub(crate) static POLICY_EDIT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct HiveApprovalGate(pub HiveDaemonState);
#[async_trait::async_trait]
impl abigail_hive::approved_dispatch::ApprovalGate for HiveApprovalGate {
    async fn still_approved(&self, connection: &ModelConnection) -> anyhow::Result<bool> {
        let policy = current(&self.0).map_err(anyhow::Error::msg)?;
        Ok(self
            .0
            .hive
            .resolve_approved_connections(&policy)
            .iter()
            .any(|route| route.ready && &route.connection == connection))
    }
}

#[derive(serde::Serialize)]
pub struct RoutingChoices {
    pub policy: ModelRoutingPolicy,
    pub readiness: Vec<ConnectionReadiness>,
}
#[derive(serde::Serialize)]
pub struct ConnectionReadiness {
    connection: ModelConnection,
    ready: bool,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChooseModels {
    pub allowed: Vec<ModelConnection>,
    pub expected: ModelRoutingPolicy,
}

fn choices(state: &HiveDaemonState, policy: ModelRoutingPolicy) -> RoutingChoices {
    let all = ModelRoutingPolicy {
        allowed: policy.connections.clone(),
        connections: policy.connections.clone(),
    };
    let readiness = state
        .hive
        .resolve_approved_connections(&all)
        .into_iter()
        .map(|r| ConnectionReadiness {
            connection: r.connection,
            ready: r.ready,
        })
        .collect();
    RoutingChoices { policy, readiness }
}

pub async fn get(State(state): State<HiveDaemonState>) -> Json<ApiEnvelope<RoutingChoices>> {
    let result = state
        .identity_manager
        .hive_agent_id()
        .and_then(|id| state.identity_manager.load_agent(&id))
        .and_then(|config| effective_policy(&config));
    match result {
        Ok(policy) => Json(ApiEnvelope::success(choices(&state, policy))),
        Err(error) => Json(ApiEnvelope::error(error)),
    }
}

pub async fn choose(
    State(state): State<HiveDaemonState>,
    Json(body): Json<ChooseModels>,
) -> Json<ApiEnvelope<RoutingChoices>> {
    let _edit = POLICY_EDIT.lock().await;
    let result: Result<ModelRoutingPolicy, String> = (|| {
        let id = state.identity_manager.hive_agent_id()?;
        let mut config = state.identity_manager.load_agent(&id)?;
        let mut policy = effective_policy(&config)?;
        if policy != body.expected {
            return Err("Model choices changed. Refresh Settings and choose again.".into());
        }
        policy.choose(body.allowed)?;
        config.model_routing = Some(policy.clone());
        // Keep compatibility fields descriptive only; an empty policy remains empty.
        config.active_provider_preference = policy.allowed.first().map(|c| c.provider.clone());
        config.ego_model = policy.allowed.first().and_then(|c| c.model.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        Ok(policy)
    })();
    match result {
        Ok(policy) => Json(ApiEnvelope::success(choices(&state, policy))),
        Err(error) => Json(ApiEnvelope::error(error)),
    }
}

/// Persist a successful user-initiated model check without losing a concurrent edit.
pub async fn save_checked(
    state: &HiveDaemonState,
    route: ModelConnection,
    expected: ModelRoutingPolicy,
) -> Result<(), String> {
    let _edit = POLICY_EDIT.lock().await;
    let id = state.identity_manager.hive_agent_id()?;
    let mut config = state.identity_manager.load_agent(&id)?;
    let mut policy = effective_policy(&config)?;
    if policy != expected {
        return Err(
            "Model choices changed during the connection check. Review Settings and try again."
                .into(),
        );
    }
    policy.approve_checked(route.clone())?;
    config.model_routing = Some(policy);
    config.active_provider_preference = Some(route.provider.clone());
    config.ego_model = route.model;
    if route.provider == "local" {
        config.local_llm_base_url = route.local_url;
    }
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())
}

pub fn current(state: &HiveDaemonState) -> Result<ModelRoutingPolicy, String> {
    let id = state.identity_manager.hive_agent_id()?;
    effective_policy(&state.identity_manager.load_agent(&id)?)
}
