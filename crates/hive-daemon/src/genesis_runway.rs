//! Runway Gen-4 Turbo image-to-video adapter, verified against Runway's official
//! API/SDK on 2026-10-17. This module never creates keys or initiates sign-in.
//! Prompt images are private data URIs; output URLs are validated, DNS-pinned,
//! downloaded without API credentials and never exposed in the product.

use super::{
    bounded_json, validate_bytes, GeneratedBytes, GenerationJob, MediaAdapter, MediaKind,
    ProviderResult, MAX_VIDEO_BYTES,
};
use abigail_hive::Hive;
use anyhow::{anyhow, bail, ensure, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{net::IpAddr, sync::Arc, time::Duration};

const API: &str = "https://api.dev.runwayml.com";
const API_VERSION: &str = "2024-11-06";

pub(super) struct RunwayVideoAdapter {
    hive: Arc<Hive>,
    client: reqwest::Client,
}

impl RunwayVideoAdapter {
    pub(super) fn new(hive: Arc<Hive>) -> Result<Self> {
        Ok(Self {
            hive,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .build()?,
        })
    }

    fn key(&self) -> Result<String> {
        self.hive
            .resolve_provider_profile("runway")
            .and_then(|profile| profile.api_key())
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| anyhow!("Save a Runway API connection in Hive to generate a film"))
    }
}

pub(super) fn request_body(job: &GenerationJob, reference: &[u8]) -> Result<Value> {
    ensure!(
        job.model == "gen4_turbo" && [5, 10].contains(&job.seconds),
        "Unsupported Runway model or duration"
    );
    validate_bytes(MediaKind::Portrait, "image/png", reference)?;
    let image = format!("data:image/png;base64,{}", STANDARD.encode(reference));
    ensure!(
        image.len() <= 5_000_000,
        "This portrait exceeds the film provider's image size limit"
    );
    // The image is the canonical identity authority. The motion brief controls
    // its emergence without silently truncating canonical JSON or lore.
    let prompt = format!("Animate this exact fictional character emerging into existence. Preserve its face, clothing, silhouette and palette from the reference. Settle into the same portrait composition. Visual style: {}. Motion: {}", job.preset, job.direction);
    ensure!(
        prompt.encode_utf16().count() <= 1000,
        "Runway film direction must fit within 1000 characters"
    );
    Ok(
        json!({"model":"gen4_turbo","promptImage":image,"promptText":prompt,"ratio":"960:960","duration":job.seconds}),
    )
}

enum TaskResult {
    Pending(u8),
    Complete(String),
    Failed,
}

fn parse_task(value: &Value) -> Result<TaskResult> {
    match value.get("status").and_then(Value::as_str) {
        Some("PENDING" | "THROTTLED") => Ok(TaskResult::Pending(10)),
        Some("RUNNING") => {
            let progress = value
                .get("progress")
                .and_then(Value::as_f64)
                .unwrap_or(0.25);
            Ok(TaskResult::Pending(if progress.is_finite() {
                (progress.clamp(0.0, 1.0) * 100.0) as u8
            } else {
                25
            }))
        }
        Some("SUCCEEDED") => {
            let outputs = value
                .get("output")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("Runway returned no film"))?;
            ensure!(
                outputs.len() == 1,
                "Runway returned an unexpected number of films"
            );
            let url = outputs[0]
                .as_str()
                .ok_or_else(|| anyhow!("Runway returned an invalid film reference"))?;
            validate_output_url(url)?;
            Ok(TaskResult::Complete(url.into()))
        }
        Some("FAILED" | "CANCELED" | "CANCELLED") => Ok(TaskResult::Failed),
        _ => bail!("Runway returned an unsupported task status"),
    }
}

fn task_id(job: &GenerationJob) -> Result<&str> {
    let id = job
        .provider_job_id
        .as_deref()
        .ok_or_else(|| anyhow!("Runway task is missing"))?;
    uuid::Uuid::parse_str(id).map_err(|_| anyhow!("Invalid Runway task identifier"))?;
    Ok(id)
}

#[async_trait]
impl MediaAdapter for RunwayVideoAdapter {
    fn available(&self) -> bool {
        self.key().is_ok()
    }

    async fn submit(
        &self,
        job: &GenerationJob,
        reference: Option<Vec<u8>>,
    ) -> Result<ProviderResult> {
        let reference =
            reference.ok_or_else(|| anyhow!("An approved portrait is required for this film"))?;
        let body = request_body(job, &reference)?;
        let response = self.client.post(format!("{API}/v1/image_to_video"))
            .header("X-Runway-Version", API_VERSION)
            .bearer_auth(self.key()?).json(&body).send().await
            .map_err(|_| anyhow!("Film submission was interrupted. Check Runway usage before confirming a retry."))?;
        let value = bounded_json(response).await?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Runway did not return a task identifier"))?;
        uuid::Uuid::parse_str(id)
            .map_err(|_| anyhow!("Runway returned an invalid task identifier"))?;
        Ok(ProviderResult::Pending {
            provider_job_id: id.into(),
            progress: 5,
        })
    }

    async fn poll(&self, job: &GenerationJob) -> Result<ProviderResult> {
        let id = task_id(job)?;
        let response = self
            .client
            .get(format!("{API}/v1/tasks/{id}"))
            .header("X-Runway-Version", API_VERSION)
            .bearer_auth(self.key()?)
            .send()
            .await
            .map_err(|_| anyhow!("Runway is temporarily unavailable"))?;
        match parse_task(&bounded_json(response).await?)? {
            TaskResult::Failed => Ok(ProviderResult::Failed { message:"Runway could not complete this film. Review provider usage before confirming another attempt.".into() }),
            TaskResult::Pending(progress) => Ok(ProviderResult::Pending {
                provider_job_id: id.into(),
                progress,
            }),
            TaskResult::Complete(url) => {
                let bytes = download_video(&url).await?;
                Ok(ProviderResult::Complete(vec![GeneratedBytes {
                    bytes,
                    mime_type: "video/mp4".into(),
                }]))
            }
        }
    }

    async fn cancel(&self, job: &GenerationJob) -> Result<()> {
        let id = task_id(job)?;
        let response = self
            .client
            .delete(format!("{API}/v1/tasks/{id}"))
            .header("X-Runway-Version", API_VERSION)
            .bearer_auth(self.key()?)
            .send()
            .await
            .map_err(|_| anyhow!("Runway could not be reached to stop the film"))?;
        ensure!(
            response.status().is_success() || response.status().as_u16() == 404,
            "Runway has not confirmed cancellation"
        );
        Ok(())
    }
}

fn validate_output_url(raw: &str) -> Result<reqwest::Url> {
    ensure!(raw.len() <= 8192, "Invalid video download URL");
    let url = reqwest::Url::parse(raw).map_err(|_| anyhow!("Invalid video download URL"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Missing video download host"))?;
    ensure!(
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Video download must use secure provider storage"
    );
    // Official output documentation uses signed cloudfront.net URLs. These
    // provider/CDN suffixes cannot resolve to internal addresses below.
    ensure!(
        host.ends_with(".cloudfront.net")
            || host.ends_with(".runwayml.com")
            || host.ends_with(".runway.com"),
        "Video download host is outside provider storage"
    );
    Ok(url)
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Accept ordinary global unicast only; deny mapped IPv4, local,
            // multicast, documentation and transition/tunnel ranges.
            (0x2000..=0x3fff).contains(&segments[0])
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && (segments[1] == 0 || segments[1] == 0xdb8))
        }
    }
}

async fn download_video(raw: &str) -> Result<Vec<u8>> {
    let url = validate_output_url(raw)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Missing video download host"))?;
    let addresses: Vec<_> = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| anyhow!("Provider video storage could not be resolved"))?
        .collect();
    ensure!(
        !addresses.is_empty() && addresses.iter().all(|address| public_address(address.ip())),
        "Provider video storage resolved to a disallowed address"
    );
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|_| anyhow!("Unable to prepare private film download"))?;
    // No API key is ever attached to storage requests. DNS is pinned to the
    // exact checked addresses, eliminating validation/request rebinding races.
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| anyhow!("Film download was interrupted"))?;
    ensure!(
        response.status().is_success(),
        "Provider film download failed"
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_VIDEO_BYTES as u64),
        "Film exceeds the private storage limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("Film download was interrupted"))?
    {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_VIDEO_BYTES,
            "Film exceeds the private storage limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    validate_bytes(MediaKind::Manifestation, "video/mp4", &bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_accept_only_secure_provider_storage() {
        assert!(
            validate_output_url("https://dnznrvs05pmza.cloudfront.net/output.mp4?_jwt=opaque")
                .is_ok()
        );
        for value in [
            "http://dnznrvs05pmza.cloudfront.net/a.mp4",
            "https://127.0.0.1/a.mp4",
            "https://runwayml.com.evil.example/a.mp4",
            "https://cloudfront.net.evil.example/a.mp4",
            "https://user:secret@cdn.runwayml.com/a.mp4",
            "https://cdn.runwayml.com:8443/a.mp4",
            "file:///etc/passwd",
            "https://cdn.runwayml.com/a.mp4#fragment",
        ] {
            assert!(validate_output_url(value).is_err(), "{value}");
        }
    }

    #[test]
    fn resolved_download_addresses_must_be_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "192.168.1.1",
            "172.16.0.1",
            "100.100.100.200",
            "0.0.0.0",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:7f00:1::1",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_address("1.1.1.1".parse().unwrap()));
        assert!(public_address("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn task_statuses_are_normalized_without_exposing_provider_failure_body() {
        assert!(matches!(
            parse_task(&json!({"status":"RUNNING","progress":0.5})).unwrap(),
            TaskResult::Pending(50)
        ));
        assert!(matches!(
            parse_task(&json!({"status":"THROTTLED"})).unwrap(),
            TaskResult::Pending(10)
        ));
        assert!(matches!(
            parse_task(
                &json!({"status":"SUCCEEDED","output":["https://cdn.runwayml.com/film.mp4"]})
            )
            .unwrap(),
            TaskResult::Complete(_)
        ));
        assert!(
            parse_task(&json!({"status":"SUCCEEDED","output":["https://localhost/film.mp4"]}))
                .is_err()
        );
        assert!(matches!(
            parse_task(&json!({"status":"FAILED","failure":"private-secret"})).unwrap(),
            TaskResult::Failed
        ));
    }
}
