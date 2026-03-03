//! HTTP client for hive-daemon.

use hive_core::{ApiEnvelope, EntityInfo, ProviderConfig, SecretListResponse, SecretValueResponse};

/// HTTP client wrapping all hive-daemon REST endpoints.
#[derive(Clone)]
pub struct HiveDaemonClient {
    base_url: String,
    client: reqwest::Client,
}

impl HiveDaemonClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
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

    pub async fn get_provider_config(
        &self,
        entity_id: &str,
    ) -> anyhow::Result<ProviderConfig> {
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
