//! HTTP request skill: make GET and POST requests with SSRF protection.
//!
//! Provides tools for making HTTP requests to external APIs and websites.
//! Includes SSRF protection: blocks requests to private/internal IP ranges
//! and cloud metadata endpoints.

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, NetworkPermission,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

/// Maximum response body size: 1MB.
const MAX_RESPONSE_BYTES: usize = 1_048_576;

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Blocked hostnames (cloud metadata, internal services).
const BLOCKED_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.google.com",
    "169.254.169.254", // AWS/GCP/Azure metadata
];

/// HTTP request skill with SSRF protection.
pub struct HttpSkill {
    manifest: SkillManifest,
    client: reqwest::Client,
    allow_local_network: bool,
}

impl HttpSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse http skill.toml")
    }

    /// Create a new HTTP skill.
    pub fn new(manifest: SkillManifest) -> Self {
        Self::new_with_local_network(manifest, false)
    }

    pub fn new_with_local_network(manifest: SkillManifest, allow_local_network: bool) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            manifest,
            client,
            allow_local_network,
        }
    }

    /// Validate a URL for SSRF safety.
    /// Blocks private IPs, loopback, link-local, and cloud metadata endpoints.
    fn validate_url(&self, url_str: &str) -> Result<url::Url, String> {
        let url = url::Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;

        // Only allow http and https
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(format!("Only http/https allowed, got: {}", scheme));
        }

        let host = url.host_str().ok_or("URL must have a host")?;

        // Check blocked hostnames
        let host_lower = host.to_lowercase();
        for blocked in BLOCKED_HOSTS {
            if host_lower == *blocked {
                return Err(format!("Host '{}' is blocked (SSRF protection)", host));
            }
        }

        // Check for private/internal IPs
        if !self.allow_local_network {
            match url.host() {
                Some(url::Host::Ipv4(ip)) => {
                    let addr = IpAddr::V4(ip);
                    if is_private_ip(&addr) {
                        return Err(format!(
                            "Private/internal IP '{}' is blocked (SSRF protection)",
                            ip
                        ));
                    }
                }
                Some(url::Host::Ipv6(ip)) => {
                    let addr = IpAddr::V6(ip);
                    if is_private_ip(&addr) {
                        return Err(format!(
                            "Private/internal IP '{}' is blocked (SSRF protection)",
                            ip
                        ));
                    }
                }
                Some(url::Host::Domain(d)) => {
                    // Block localhost/loopback domains
                    let d_lower = d.to_lowercase();
                    if d_lower == "localhost"
                        || d_lower == "0.0.0.0"
                        || d_lower.ends_with(".local")
                        || d_lower.ends_with(".internal")
                    {
                        return Err(format!(
                            "Local/internal domain '{}' is blocked (SSRF protection)",
                            d
                        ));
                    }
                }
                None => return Err("URL must have a host".to_string()),
            }
        }

        Ok(url)
    }

    /// Execute an HTTP GET request.
    async fn http_get(
        &self,
        url_str: &str,
        headers: Option<HashMap<String, String>>,
    ) -> SkillResult<ToolOutput> {
        let url = self.validate_url(url_str).map_err(SkillError::ToolFailed)?;

        let mut request = self.client.get(url.as_str());

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(&key, &value);
            }
        }

        execute_request(request).await
    }

    /// Execute an HTTP POST request.
    async fn http_post(
        &self,
        url_str: &str,
        body: Option<&str>,
        content_type: Option<&str>,
        headers: Option<HashMap<String, String>>,
    ) -> SkillResult<ToolOutput> {
        let url = self.validate_url(url_str).map_err(SkillError::ToolFailed)?;

        let mut request = self.client.post(url.as_str());

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(&key, &value);
            }
        }

        if let Some(ct) = content_type {
            request = request.header("Content-Type", ct);
        }

        if let Some(body_str) = body {
            request = request.body(body_str.to_string());
        }

        execute_request(request).await
    }
}

/// Execute a request and return a ToolOutput.
async fn execute_request(request: reqwest::RequestBuilder) -> SkillResult<ToolOutput> {
    let response = request
        .send()
        .await
        .map_err(|e| SkillError::ToolFailed(format!("Request failed: {}", e)))?;

    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();

    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();

    let content_type = response_headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| SkillError::ToolFailed(format!("Failed to read response body: {}", e)))?;

    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    let body_bytes = if truncated {
        &bytes[..MAX_RESPONSE_BYTES]
    } else {
        &bytes[..]
    };

    let body = String::from_utf8_lossy(body_bytes).to_string();

    let formatted = if (200..300).contains(&status) {
        if body.len() > 2000 {
            format!(
                "{} ({})\n{}...\n[truncated at 2000 chars, full body in 'body' field]",
                status,
                status_text,
                &body[..2000]
            )
        } else {
            body.clone()
        }
    } else {
        format!("HTTP {} {}\n{}", status, status_text, body)
    };

    Ok(ToolOutput::success(serde_json::json!({
        "formatted": formatted,
        "status": status,
        "status_text": status_text,
        "body": body,
        "body_truncated": truncated,
        "content_type": content_type,
        "headers": response_headers,
    })))
}

/// Check if an IP address is private/internal (SSRF protection).
fn is_private_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            // Loopback: 127.0.0.0/8
            if octets[0] == 127 {
                return true;
            }
            // Private: 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }
            // Private: 172.16.0.0/12
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }
            // Private: 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            // Link-local: 169.254.0.0/16
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // Current network: 0.0.0.0/8
            if octets[0] == 0 {
                return true;
            }
            false
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local: fc00::/7
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                // Link-local: fe80::/10
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[async_trait]
impl Skill for HttpSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        SkillHealth {
            status: HealthStatus::Healthy,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "http_get".to_string(),
                description:
                    "Make an HTTP GET request to a URL. Returns status, headers, and body."
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to request"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Optional HTTP headers as key-value pairs",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["url"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "status": { "type": "integer" },
                        "body": { "type": "string" },
                        "headers": { "type": "object" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 3000,
                    network_bound: true,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Network(NetworkPermission::Full)],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "http_post".to_string(),
                description: "Make an HTTP POST request. Supports JSON and form data.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to POST to"
                        },
                        "body": {
                            "type": "string",
                            "description": "Request body (JSON string, form data, etc.)"
                        },
                        "content_type": {
                            "type": "string",
                            "description": "Content-Type header (default: application/json)"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Optional HTTP headers",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["url"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "status": { "type": "integer" },
                        "body": { "type": "string" },
                        "headers": { "type": "object" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 3000,
                    network_bound: true,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Network(NetworkPermission::Full)],
                autonomous: false,
                requires_confirmation: true,
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "http_get" => {
                let url: String = params.get("url").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: url".to_string())
                })?;
                let headers: Option<HashMap<String, String>> = params.get("headers");
                self.http_get(&url, headers).await
            }
            "http_post" => {
                let url: String = params.get("url").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: url".to_string())
                })?;
                let body: Option<String> = params.get("body");
                let content_type: Option<String> = params.get("content_type");
                let headers: Option<HashMap<String, String>> = params.get("headers");
                self.http_post(&url, body.as_deref(), content_type.as_deref(), headers)
                    .await
            }
            other => Err(SkillError::ToolFailed(format!("Unknown tool: {}", other))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parses() {
        let manifest = HttpSkill::default_manifest();
        assert_eq!(manifest.name, "HTTP");
    }

    #[test]
    fn test_tools_list() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        let tools = skill.tools();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"http_get"));
        assert!(names.contains(&"http_post"));
    }

    #[test]
    fn test_ssrf_blocks_localhost() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill.validate_url("http://localhost:8080/api").is_err());
        assert!(skill.validate_url("http://127.0.0.1:1234").is_err());
        assert!(skill.validate_url("http://0.0.0.0").is_err());
    }

    #[test]
    fn test_ssrf_blocks_private_ips() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill.validate_url("http://10.0.0.1/admin").is_err());
        assert!(skill.validate_url("http://172.16.0.1/").is_err());
        assert!(skill.validate_url("http://192.168.1.1/").is_err());
    }

    #[test]
    fn test_ssrf_blocks_metadata() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill
            .validate_url("http://169.254.169.254/latest/meta-data/")
            .is_err());
        assert!(skill
            .validate_url("http://metadata.google.internal/")
            .is_err());
    }

    #[test]
    fn test_ssrf_blocks_internal_domains() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill.validate_url("http://service.local/api").is_err());
        assert!(skill.validate_url("http://db.internal/").is_err());
    }

    #[test]
    fn test_ssrf_allows_public_urls() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill.validate_url("https://api.github.com/repos").is_ok());
        assert!(skill.validate_url("https://example.com/page").is_ok());
        assert!(skill.validate_url("http://httpbin.org/get").is_ok());
    }

    #[test]
    fn test_ssrf_blocks_non_http() {
        let skill = HttpSkill::new(HttpSkill::default_manifest());
        assert!(skill.validate_url("file:///etc/passwd").is_err());
        assert!(skill.validate_url("ftp://ftp.example.com/").is_err());
        assert!(skill.validate_url("gopher://evil.com/").is_err());
    }

    #[test]
    fn test_desktop_operator_mode_allows_local_network() {
        let skill = HttpSkill::new_with_local_network(HttpSkill::default_manifest(), true);
        assert!(skill.validate_url("http://localhost:8080/api").is_ok());
        assert!(skill.validate_url("http://127.0.0.1:1234").is_ok());
        assert!(skill.validate_url("http://192.168.1.10/api").is_ok());
    }

    #[test]
    fn test_is_private_ip() {
        use std::net::Ipv4Addr;

        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));

        // Public IPs should not be private
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
    }
}
