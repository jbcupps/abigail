//! HTTP client for hive-daemon.

use hive_core::{
    local_auth_header_value, ApiEnvelope, EntityInfo, ForgeApprovalJobsResponse, OutboxSyncRequest,
    OutboxSyncResponse, ProviderConfig, ProviderModelsRequest, ProviderModelsResponse,
    RuntimeHeartbeatRequest, RuntimeHeartbeatResponse, RuntimeRegistrationRequest,
    RuntimeSessionLease, RuntimeSessionRequest, RuntimeSessionStatus, SecretListResponse,
    SecretValueResponse, SkillAssignmentsResponse, UpdateEntityConfigRequest,
    UpdateEntityConfigResponse, LOCAL_AUTH_ENV,
};

/// HTTP client wrapping all hive-daemon REST endpoints.
#[derive(Clone)]
pub struct HiveDaemonClient {
    base_url: String,
    client: reqwest::Client,
    auth_token: Option<String>,
}

impl HiveDaemonClient {
    pub fn new(base_url: &str) -> Self {
        Self::with_auth(base_url, std::env::var(LOCAL_AUTH_ENV).ok())
    }

    pub fn with_auth(base_url: &str, auth_token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("control-plane client"),
            auth_token: auth_token.filter(|t| !t.is_empty()),
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        self.auth_token = if token.is_empty() { None } else { Some(token) };
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_token {
            Some(token) => req.header(
                reqwest::header::AUTHORIZATION,
                local_auth_header_value(token),
            ),
            None => req,
        }
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
            .apply_auth(self.client.get(format!("{}/v1/entities", self.base_url)))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn create_entity(&self, name: &str) -> anyhow::Result<String> {
        let resp: ApiEnvelope<serde_json::Value> = self
            .apply_auth(self.client.post(format!("{}/v1/entities", self.base_url)))
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
            .apply_auth(
                self.client
                    .get(format!("{}/v1/entities/{}", self.base_url, entity_id)),
            )
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn get_provider_config(&self, entity_id: &str) -> anyhow::Result<ProviderConfig> {
        let resp: ApiEnvelope<ProviderConfig> = self
            .apply_auth(self.client.get(format!(
                "{}/v1/entities/{}/provider-config",
                self.base_url, entity_id
            )))
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
            .apply_auth(self.client.patch(format!(
                "{}/v1/entities/{}/config",
                self.base_url, entity_id
            )))
            .json(patch)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn store_secret(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let resp: ApiEnvelope<String> = self
            .apply_auth(self.client.post(format!("{}/v1/secrets", self.base_url)))
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
            .apply_auth(
                self.client
                    .get(format!("{}/v1/secrets/{}", self.base_url, key)),
            )
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
            .apply_auth(
                self.client
                    .get(format!("{}/v1/secrets/list", self.base_url)),
            )
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
            .apply_auth(
                self.client
                    .post(format!("{}/v1/providers/models", self.base_url)),
            )
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

    pub async fn issue_runtime_session(
        &self,
        entity_id: &str,
        runtime_id: Option<String>,
    ) -> anyhow::Result<RuntimeSessionLease> {
        let resp: ApiEnvelope<RuntimeSessionLease> = self
            .apply_auth(
                self.client
                    .post(format!("{}/v1/runtime/sessions", self.base_url)),
            )
            .json(&RuntimeSessionRequest {
                entity_id: entity_id.to_string(),
                runtime_id,
            })
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn register_runtime(
        &self,
        request: &RuntimeRegistrationRequest,
    ) -> anyhow::Result<RuntimeSessionStatus> {
        let resp: ApiEnvelope<RuntimeSessionStatus> = self
            .apply_auth(
                self.client
                    .post(format!("{}/v1/runtime/register", self.base_url)),
            )
            .json(request)
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn heartbeat(
        &self,
        request: &RuntimeHeartbeatRequest,
    ) -> anyhow::Result<RuntimeHeartbeatResponse> {
        let resp: ApiEnvelope<RuntimeHeartbeatResponse> = self
            .apply_auth(
                self.client
                    .post(format!("{}/v1/runtime/heartbeat", self.base_url)),
            )
            .json(request)
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
            .apply_auth(self.client.get(format!(
                "{}/v1/entities/{}/assignments",
                self.base_url, entity_id
            )))
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
            .apply_auth(self.client.get(format!(
                "{}/v1/entities/{}/forge-approvals",
                self.base_url, entity_id
            )))
            .send()
            .await?
            .json()
            .await?;
        unwrap_envelope(resp)
    }

    pub async fn sync_outbox(
        &self,
        request: &OutboxSyncRequest,
    ) -> anyhow::Result<OutboxSyncResponse> {
        let resp: ApiEnvelope<OutboxSyncResponse> = self
            .apply_auth(
                self.client
                    .post(format!("{}/v1/runtime/outbox/sync", self.base_url)),
            )
            .json(request)
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
