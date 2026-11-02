//! HTTP client for communicating with the Hive daemon.

use abigail_skills::{HiveAgentInfo, HiveOperations};
use async_trait::async_trait;
use hive_core::{
    genesis_quest::PresenceCard, ApiEnvelope, CreateEntityResponse, EntityInfo,
    ForgeApprovalJobsResponse, OutboxSyncRequest, OutboxSyncResponse, ProviderConfig,
    RuntimeHeartbeatRequest, RuntimeHeartbeatResponse, RuntimeRegistrationRequest,
    RuntimeSessionLease, RuntimeSessionRequest, RuntimeSessionStatus, SecretListResponse,
    SecretValueResponse, SkillAssignmentsResponse,
};

/// HTTP client for fetching data from the Hive daemon.
#[derive(Clone)]
pub struct HiveClient {
    base_url: String,
    client: reqwest::Client,
    lease_id: Option<std::sync::Arc<str>>,
}

impl HiveClient {
    /// Without a lease, the Hive only answers the bootstrap exchange in
    /// [`Self::issue_runtime_session`].
    pub fn new(base_url: &str) -> Self {
        // Control-plane calls share the supervision loop with heartbeats and
        // outbox sync; an unbounded request (e.g. a half-open connection
        // across a hive restart) would stop heartbeats permanently. Requests
        // carry the lease, so they never go through a proxy or a redirect.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            lease_id: None,
        }
    }

    pub fn with_lease(mut self, lease_id: &str) -> Self {
        self.lease_id = Some(lease_id.into());
        self
    }

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.lease_id {
            Some(lease_id) => request.bearer_auth(lease_id),
            None => request,
        }
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.authorized(self.client.get(url))
    }

    fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.authorized(self.client.post(url))
    }

    /// Fetch provider configuration for an entity.
    pub async fn get_provider_config(&self, entity_id: &str) -> anyhow::Result<ProviderConfig> {
        let url = format!(
            "{}/v1/entities/{}/provider-config",
            self.base_url, entity_id
        );
        let resp: ApiEnvelope<ProviderConfig> = self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in provider-config response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    /// Fetch the entity's full birth document (idempotent runtime identity).
    /// Called on every launch so hive-side changes apply on restart.
    pub async fn get_birth_document(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<hive_core::EntityBirthDocument> {
        let url = format!("{}/v1/entities/{}/birth", self.base_url, entity_id);
        let resp: ApiEnvelope<hive_core::EntityBirthDocument> =
            self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in birth document response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    /// Resolve a named provider profile for sub-agent delegation.
    pub async fn get_provider_profile(
        &self,
        name: &str,
    ) -> anyhow::Result<hive_core::ProviderProfileResponse> {
        let url = format!("{}/v1/providers/profiles/{}", self.base_url, name);
        let resp: ApiEnvelope<hive_core::ProviderProfileResponse> =
            self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in provider-profile response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    /// Fetch a secret value from Hive by key. Returns None if not found.
    pub async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>> {
        let url = format!("{}/v1/secrets/{}", self.base_url, key);
        let resp: ApiEnvelope<SecretValueResponse> = self.get(&url).send().await?.json().await?;
        if resp.ok {
            Ok(resp.data.map(|d| d.value))
        } else {
            Ok(None)
        }
    }

    /// Fetch entity info.
    pub async fn get_entity(&self, entity_id: &str) -> anyhow::Result<EntityInfo> {
        let url = format!("{}/v1/entities/{}", self.base_url, entity_id);
        let resp: ApiEnvelope<EntityInfo> = self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in entity response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    /// Fetch the canonical, cosmetic Presence for this leased Entity.
    pub async fn get_presence(&self, entity_id: &str) -> anyhow::Result<Option<PresenceCard>> {
        let url = format!("{}/v1/entities/{}/presence", self.base_url, entity_id);
        let resp: ApiEnvelope<Option<PresenceCard>> = self.get(&url).send().await?.json().await?;
        if resp.ok {
            // A successful `null` is the canonical "Presence not chosen" state.
            Ok(resp.data.flatten())
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn issue_runtime_session(
        &self,
        entity_id: &str,
        runtime_id: Option<String>,
    ) -> anyhow::Result<RuntimeSessionLease> {
        let bootstrap = std::env::var("ABIGAIL_RUNTIME_BOOTSTRAP")
            .ok()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Open this Entity through the Abigail Hive to authorize its runtime"
                )
            })?;
        let url = format!("{}/v1/runtime/sessions", self.base_url);
        let resp: ApiEnvelope<RuntimeSessionLease> = self
            .client
            .post(&url)
            .bearer_auth(bootstrap)
            .json(&RuntimeSessionRequest {
                entity_id: entity_id.to_string(),
                runtime_id,
            })
            .send()
            .await?
            .json()
            .await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in runtime session response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn register_runtime(
        &self,
        request: &RuntimeRegistrationRequest,
    ) -> anyhow::Result<RuntimeSessionStatus> {
        let url = format!("{}/v1/runtime/register", self.base_url);
        let resp: ApiEnvelope<RuntimeSessionStatus> =
            self.post(&url).json(request).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in runtime register response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn heartbeat(
        &self,
        request: &RuntimeHeartbeatRequest,
    ) -> anyhow::Result<RuntimeHeartbeatResponse> {
        let url = format!("{}/v1/runtime/heartbeat", self.base_url);
        let resp: ApiEnvelope<RuntimeHeartbeatResponse> =
            self.post(&url).json(request).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in runtime heartbeat response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn get_skill_assignments(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<SkillAssignmentsResponse> {
        let url = format!("{}/v1/entities/{}/assignments", self.base_url, entity_id);
        let resp: ApiEnvelope<SkillAssignmentsResponse> =
            self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in assignments response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn get_forge_approval_jobs(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<ForgeApprovalJobsResponse> {
        let url = format!(
            "{}/v1/entities/{}/forge-approvals",
            self.base_url, entity_id
        );
        let resp: ApiEnvelope<ForgeApprovalJobsResponse> =
            self.get(&url).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in forge approvals response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }

    pub async fn sync_outbox(
        &self,
        request: &OutboxSyncRequest,
    ) -> anyhow::Result<OutboxSyncResponse> {
        let url = format!("{}/v1/runtime/outbox/sync", self.base_url);
        let resp: ApiEnvelope<OutboxSyncResponse> =
            self.post(&url).json(request).send().await?.json().await?;
        if resp.ok {
            resp.data
                .ok_or_else(|| anyhow::anyhow!("Empty data in outbox sync response"))
        } else {
            Err(anyhow::anyhow!(
                "Hive error: {}",
                resp.error.unwrap_or_default()
            ))
        }
    }
}

/// Implementation of `HiveOperations` that calls hive-daemon over HTTP.
///
/// Drop-in replacement for `TauriHiveOps` — no changes needed to the
/// `HiveManagementSkill` or `HiveOperations` trait.
pub struct HttpHiveOps {
    client: HiveClient,
}

impl HttpHiveOps {
    pub fn new(client: HiveClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl HiveOperations for HttpHiveOps {
    async fn list_agents(&self) -> Result<Vec<HiveAgentInfo>, String> {
        let url = format!("{}/v1/entities", self.client.base_url);
        let resp: ApiEnvelope<Vec<EntityInfo>> = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        if resp.ok {
            Ok(resp
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|e| HiveAgentInfo {
                    id: e.id,
                    name: e.name,
                })
                .collect())
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    }

    async fn load_agent(&self, _agent_id: &str) -> Result<(), String> {
        // In the daemon model, entities are independent processes.
        // "Loading" is a no-op since the entity is already running.
        Ok(())
    }

    async fn create_agent(&self, name: &str) -> Result<String, String> {
        let url = format!("{}/v1/entities", self.client.base_url);
        let body = hive_core::CreateEntityRequest {
            name: name.to_string(),
            creation_key: None,
        };
        let resp: ApiEnvelope<CreateEntityResponse> = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        if resp.ok {
            Ok(resp.data.map(|d| d.id).unwrap_or_default())
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    }

    async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
        // Not applicable in daemon model — each entity-daemon IS an entity.
        Ok(None)
    }

    async fn get_config_value(&self, _key: &str) -> Result<serde_json::Value, String> {
        // Config reading would need a dedicated Hive endpoint.
        // For now, return null.
        Ok(serde_json::Value::Null)
    }

    async fn set_config_value(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
        // Config writing would need a dedicated Hive endpoint.
        Err("Config writes not yet supported over HTTP".to_string())
    }

    async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String> {
        let url = format!("{}/v1/secrets", self.client.base_url);
        let body = hive_core::StoreSecretRequest {
            key: key.to_string(),
            value: value.to_string(),
        };
        let resp: ApiEnvelope<String> = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        if resp.ok {
            Ok(())
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    }

    async fn get_skill_secret_names(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/v1/secrets/list", self.client.base_url);
        let resp: ApiEnvelope<SecretListResponse> = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        if resp.ok {
            Ok(resp.data.map(|d| d.keys).unwrap_or_default())
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    }
}
