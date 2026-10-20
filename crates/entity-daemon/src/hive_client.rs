//! HTTP client for communicating with the Hive daemon.

use abigail_skills::{HiveAgentInfo, HiveOperations};
use async_trait::async_trait;
use hive_core::{
    ApiEnvelope, CreateEntityResponse, EntityInfo, ForgeApprovalJobsResponse, OutboxSyncRequest,
    OutboxSyncResponse, ProviderConfig, RuntimeHeartbeatRequest, RuntimeHeartbeatResponse,
    RuntimeRegistrationRequest, RuntimeSessionLease, RuntimeSessionRequest, RuntimeSessionStatus,
    SecretListResponse, SecretValueResponse, SkillAssignmentsResponse,
};

pub(crate) fn validate_preview_hive_url(value: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| anyhow::anyhow!("Preview Hive URL must be a canonical loopback address"))?;
    let port = url
        .port_or_known_default()
        .filter(|port| *port != 0)
        .ok_or_else(|| anyhow::anyhow!("Preview Hive URL requires an explicit nonzero port"))?;
    if (43141..=43150).contains(&port) {
        anyhow::bail!("Preview Hive URL cannot use a reserved stable port");
    }
    if value != format!("http://127.0.0.1:{port}") {
        anyhow::bail!("Preview Hive URL must be a canonical loopback address");
    }
    Ok(())
}

fn verify_hive_identity(
    actual: Option<&abigail_core::profile::ProcessIdentity>,
    expected: &abigail_core::profile::ProcessIdentity,
) -> anyhow::Result<()> {
    if actual != Some(expected) {
        anyhow::bail!("Hive process identity does not match this preview launch");
    }
    Ok(())
}

fn preview_headers(
    expected_identity: &abigail_core::profile::ProcessIdentity,
) -> anyhow::Result<reqwest::header::HeaderMap> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-abigail-profile-id",
        expected_identity.profile_id.parse()?,
    );
    headers.insert(
        "x-abigail-process-instance-id",
        expected_identity.instance_id.parse()?,
    );
    Ok(headers)
}

fn parent_hive_identity(
    runtime_identity: &abigail_core::profile::ProcessIdentity,
) -> abigail_core::profile::ProcessIdentity {
    let mut hive_identity = runtime_identity.clone();
    hive_identity.instance_id = runtime_identity.hive_instance_id.clone();
    hive_identity
}

/// HTTP client for fetching data from the Hive daemon.
#[derive(Clone)]
pub struct HiveClient {
    base_url: String,
    client: reqwest::Client,
    expected_identity: Option<abigail_core::profile::ProcessIdentity>,
}

impl HiveClient {
    pub fn new(base_url: &str) -> anyhow::Result<Self> {
        if abigail_core::profile::current()?.is_some() {
            anyhow::bail!("Preview Hive requests require an explicit process identity");
        }
        // Control-plane calls share the supervision loop with heartbeats and
        // outbox sync; an unbounded request (e.g. a half-open connection
        // across a hive restart) would stop heartbeats permanently.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            expected_identity: None,
        })
    }

    pub fn for_runtime(
        base_url: &str,
        runtime_identity: Option<&abigail_core::profile::ProcessIdentity>,
    ) -> anyhow::Result<Self> {
        let Some(runtime_identity) = runtime_identity else {
            return Self::new(base_url);
        };
        let profile = abigail_core::profile::current()?
            .ok_or_else(|| anyhow::anyhow!("Preview Hive client requires profile preflight"))?;
        profile.validate_identity(runtime_identity)?;
        Self::new_preview(base_url, parent_hive_identity(runtime_identity))
    }

    fn new_preview(
        base_url: &str,
        expected_identity: abigail_core::profile::ProcessIdentity,
    ) -> anyhow::Result<Self> {
        validate_preview_hive_url(base_url)?;
        // These IDs bind ownership, not provider authentication. The runtime
        // bootstrap/session capability remains independently required.
        let client = reqwest::Client::builder()
            .default_headers(preview_headers(&expected_identity)?)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base_url: base_url.to_string(),
            client,
            expected_identity: Some(expected_identity),
        })
    }

    /// Establish exact profile/root/live-instance ownership before requesting
    /// any Entity metadata, provider configuration, or runtime session.
    pub async fn verify_identity(&self) -> anyhow::Result<()> {
        let Some(expected) = self.expected_identity.as_ref() else {
            return Ok(());
        };
        let mut response = self
            .client
            .get(format!("{}/v1/status", self.base_url))
            .send()
            .await?
            .error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > 64 * 1024 {
                anyhow::bail!("Preview Hive status exceeded its size limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: ApiEnvelope<hive_core::HiveStatus> = serde_json::from_slice(&bytes)?;
        let status = response
            .data
            .filter(|_| response.ok)
            .ok_or_else(|| anyhow::anyhow!("Preview Hive did not provide a usable identity"))?;
        verify_hive_identity(status.process_identity.as_ref(), expected)
    }

    /// Fetch provider configuration for an entity.
    pub async fn get_provider_config(&self, entity_id: &str) -> anyhow::Result<ProviderConfig> {
        let url = format!(
            "{}/v1/entities/{}/provider-config",
            self.base_url, entity_id
        );
        let resp: ApiEnvelope<ProviderConfig> = self.client.get(&url).send().await?.json().await?;
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
            self.client.get(&url).send().await?.json().await?;
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
            self.client.get(&url).send().await?.json().await?;
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
        let resp: ApiEnvelope<SecretValueResponse> =
            self.client.get(&url).send().await?.json().await?;
        if resp.ok {
            Ok(resp.data.map(|d| d.value))
        } else {
            Ok(None)
        }
    }

    /// Fetch entity info.
    pub async fn get_entity(&self, entity_id: &str) -> anyhow::Result<EntityInfo> {
        let url = format!("{}/v1/entities/{}", self.base_url, entity_id);
        let resp: ApiEnvelope<EntityInfo> = self.client.get(&url).send().await?.json().await?;
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
        let resp: ApiEnvelope<RuntimeSessionStatus> = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await?
            .json()
            .await?;
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
        let resp: ApiEnvelope<RuntimeHeartbeatResponse> = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await?
            .json()
            .await?;
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
            self.client.get(&url).send().await?.json().await?;
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
            self.client.get(&url).send().await?.json().await?;
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
        let resp: ApiEnvelope<OutboxSyncResponse> = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await?
            .json()
            .await?;
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
    pub fn from_client(client: HiveClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl HiveOperations for HttpHiveOps {
    async fn list_agents(&self) -> Result<Vec<HiveAgentInfo>, String> {
        let url = format!("{}/v1/entities", self.client.base_url);
        let resp: ApiEnvelope<Vec<EntityInfo>> = self
            .client
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

#[cfg(test)]
mod preview_client_tests {
    use super::*;
    use abigail_core::profile::ProcessIdentity;
    use std::path::PathBuf;

    fn identity() -> ProcessIdentity {
        ProcessIdentity {
            channel: "beta".into(),
            profile_id: "4fa59494-7704-4fae-bfbf-1e7a325a7664".into(),
            canonical_data_root: PathBuf::from("/synthetic/preview"),
            instance_id: "253b0b3d-31f6-41c2-afd2-cae7b5316376".into(),
            hive_instance_id: "253b0b3d-31f6-41c2-afd2-cae7b5316376".into(),
        }
    }

    #[test]
    fn preview_hive_url_requires_exact_local_endpoint() {
        for value in ["http://127.0.0.1:51111", "http://127.0.0.1:80"] {
            validate_preview_hive_url(value).unwrap();
        }
        for value in [
            "http://127.0.0.1",
            "http://127.0.0.1:0",
            "http://127.0.0.1:43141",
            "http://127.0.0.1:43150",
            "http://localhost:51111",
            "http://127.0.0.1:51111/",
            "http://127.0.0.1:51111?profile=preview",
            "https://127.0.0.1:51111",
            "http://user@127.0.0.1:51111",
            "http://127.0.0.1:051111",
            "http://example.test:51111",
        ] {
            assert!(validate_preview_hive_url(value).is_err(), "{value}");
        }
    }

    #[test]
    fn preview_identity_requires_all_fields_and_exact_live_instance() {
        let expected = identity();
        verify_hive_identity(Some(&expected), &expected).unwrap();
        assert!(verify_hive_identity(None, &expected).is_err());
        let mut differences = vec![expected.clone(); 5];
        differences[0].channel = "stable".into();
        differences[1].profile_id = uuid::Uuid::new_v4().to_string();
        differences[2].canonical_data_root = PathBuf::from("/synthetic/other");
        differences[3].instance_id = uuid::Uuid::new_v4().to_string();
        differences[4].hive_instance_id = uuid::Uuid::new_v4().to_string();
        for actual in differences {
            assert!(verify_hive_identity(Some(&actual), &expected).is_err());
        }
    }

    #[test]
    fn runtime_client_pins_parent_hive_instance_on_every_request() {
        let mut runtime = identity();
        runtime.instance_id = "57fbb3e7-e715-4c9a-a04f-31036f1c7f6c".into();
        let client =
            HiveClient::new_preview("http://127.0.0.1:51111", parent_hive_identity(&runtime))
                .unwrap();
        let headers = preview_headers(client.expected_identity.as_ref().unwrap()).unwrap();
        assert_eq!(headers["x-abigail-profile-id"], runtime.profile_id);
        assert_eq!(
            headers["x-abigail-process-instance-id"],
            runtime.hive_instance_id
        );
        assert_eq!(client.expected_identity.unwrap(), identity());
    }
}
