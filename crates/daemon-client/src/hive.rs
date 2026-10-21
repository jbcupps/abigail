//! HTTP client for hive-daemon.

use hive_core::access::{self as contract, AdoptionChallenge, AdoptionProof};
use hive_core::{
    ApiEnvelope, EntityInfo, ForgeApprovalJobsResponse, ProviderConfig, ProviderModelsRequest,
    ProviderModelsResponse, SecretListResponse, SecretValueResponse, SkillAssignmentsResponse,
    UpdateEntityConfigRequest, UpdateEntityConfigResponse,
};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::path::Path;

/// The control token in a Hive data root, if the daemon has written one.
pub fn read_control_token(data_root: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(data_root.join(contract::CONTROL_TOKEN_FILE)).ok()?;
    contract::parse_control_token(&contents).map(str::to_owned)
}

/// Requests carry a credential, so they never go through a proxy or follow a
/// redirect to another server.
fn loopback_http(control_token: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    if let Some(token) = control_token {
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))?;
        value.set_sensitive(true);
        builder = builder.default_headers(HeaderMap::from_iter([(AUTHORIZATION, value)]));
    }
    Ok(builder.build()?)
}

/// HTTP client wrapping all hive-daemon REST endpoints.
#[derive(Clone)]
pub struct HiveDaemonClient {
    base_url: String,
    client: reqwest::Client,
}

impl HiveDaemonClient {
    /// Without the control token the Hive only answers `/health` and the
    /// adoption challenge; use [`Self::adopt`] for everything else.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: loopback_http(None).unwrap_or_default(),
        }
    }

    /// An authorized client, but only once the daemon at `base_url` has
    /// proved it holds `control_token` for `channel`. The token is never sent
    /// to a daemon that fails the proof.
    pub async fn adopt(
        base_url: &str,
        control_token: &str,
        channel: &str,
    ) -> anyhow::Result<Option<Self>> {
        let unverified = Self::new(base_url);
        if !unverified
            .proves_control_token(control_token, channel)
            .await?
        {
            return Ok(None);
        }
        Ok(Some(Self {
            base_url: unverified.base_url,
            client: loopback_http(Some(control_token))?,
        }))
    }

    async fn proves_control_token(
        &self,
        control_token: &str,
        channel: &str,
    ) -> anyhow::Result<bool> {
        let nonce = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let response = self
            .client
            .post(format!("{}{}", self.base_url, contract::ADOPTION_PATH))
            .timeout(std::time::Duration::from_secs(2))
            .json(&AdoptionChallenge {
                nonce: nonce.clone(),
            })
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(false);
        }
        let Ok(envelope) = response.json::<ApiEnvelope<AdoptionProof>>().await else {
            return Ok(false);
        };
        Ok(envelope.data.is_some_and(|answer| {
            answer.channel == channel
                && contract::verify_adoption_proof(
                    control_token,
                    channel,
                    &self.base_url,
                    &nonce,
                    &answer.proof,
                )
        }))
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The underlying HTTP client, carrying this client's credential.
    pub fn http(&self) -> &reqwest::Client {
        &self.client
    }

    pub async fn health(&self) -> anyhow::Result<bool> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn list_entities(&self) -> anyhow::Result<Vec<EntityInfo>> {
        let resp: ApiEnvelope<Vec<EntityInfo>> = self
            .client
            .get(format!("{}/v1/entities", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn create_entity(&self, name: &str) -> anyhow::Result<String> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .client
            .post(format!("{}/v1/entities", self.base_url))
            .json(&serde_json::json!({ "name": name }))
            .send()
            .await?
            .json()
            .await?;
        let data = unwrap_envelope(resp)?;
        data["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No id in create_entity response"))
    }

    pub async fn get_entity(&self, entity_id: &str) -> anyhow::Result<EntityInfo> {
        let resp: ApiEnvelope<EntityInfo> = self
            .client
            .get(format!("{}/v1/entities/{}", self.base_url, entity_id))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn get_provider_config(&self, entity_id: &str) -> anyhow::Result<ProviderConfig> {
        let resp: ApiEnvelope<ProviderConfig> = self
            .client
            .get(format!(
                "{}/v1/entities/{}/provider-config",
                self.base_url, entity_id
            ))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn update_entity_config(
        &self,
        entity_id: &str,
        patch: &UpdateEntityConfigRequest,
    ) -> anyhow::Result<UpdateEntityConfigResponse> {
        let resp: ApiEnvelope<UpdateEntityConfigResponse> = self
            .client
            .patch(format!(
                "{}/v1/entities/{}/config",
                self.base_url, entity_id
            ))
            .json(patch)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn store_secret(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let resp: ApiEnvelope<String> = self
            .client
            .post(format!("{}/v1/secrets", self.base_url))
            .json(&serde_json::json!({ "key": key, "value": value }))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)?;
        Ok(())
    }

    pub async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>> {
        let resp: ApiEnvelope<SecretValueResponse> = self
            .client
            .get(format!("{}/v1/secrets/{}", self.base_url, key))
            .send()
            .await?
            .json()
            .await?;
        if resp.ok {
            Ok(resp.data.map(|d| d.value))
        } else {
            Ok(None)
        }
    }

    pub async fn list_secrets(&self) -> anyhow::Result<Vec<String>> {
        let resp: ApiEnvelope<SecretListResponse> = self
            .client
            .get(format!("{}/v1/secrets/list", self.base_url))
            .send()
            .await?
            .json()
            .await?;
        Ok(unwrap_envelope(resp)?.keys)
    }

    pub async fn discover_provider_models(
        &self,
        provider: &str,
        api_key: &str,
    ) -> anyhow::Result<ProviderModelsResponse> {
        let resp: ApiEnvelope<ProviderModelsResponse> = self
            .client
            .post(format!("{}/v1/providers/models", self.base_url))
            .json(&ProviderModelsRequest {
                provider: provider.to_string(),
                api_key: api_key.to_string(),
            })
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn get_skill_assignments(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<SkillAssignmentsResponse> {
        let resp: ApiEnvelope<SkillAssignmentsResponse> = self
            .client
            .get(format!(
                "{}/v1/entities/{}/assignments",
                self.base_url, entity_id
            ))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn get_forge_approval_jobs(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<ForgeApprovalJobsResponse> {
        let resp: ApiEnvelope<ForgeApprovalJobsResponse> = self
            .client
            .get(format!(
                "{}/v1/entities/{}/forge-approvals",
                self.base_url, entity_id
            ))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }
}

fn unwrap_envelope<T>(resp: ApiEnvelope<T>) -> anyhow::Result<T> {
    if resp.ok {
        resp.data
            .ok_or_else(|| anyhow::anyhow!("Empty data in Hive response"))
    } else {
        Err(anyhow::anyhow!(
            "Hive error: {}",
            resp.error.unwrap_or_default()
        ))
    }
}
