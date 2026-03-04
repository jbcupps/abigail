//! Dynamic API Skills — runtime-configurable HTTP-based skills.
//!
//! A `DynamicApiSkill` is driven entirely by a JSON config file.
//! Each tool makes templated HTTP requests, extracts fields from
//! JSON responses, and formats results for the LLM.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use abigail_core::SecretsVault;

use crate::manifest::{
    CapabilityDescriptor, NetworkPermission, Permission, SecretDescriptor, SkillId, SkillManifest,
};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};

// ── Config Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSkillConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub category: String,
    pub created_at: String,
    pub tools: Vec<DynamicToolConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolConfig {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub method: String,
    pub url_template: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body_template: Option<String>,
    #[serde(default)]
    pub response_extract: HashMap<String, String>,
    pub response_format: Option<String>,
}

// ── SSRF Protection ─────────────────────────────────────────────────

const BLOCKED_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.google.com",
    "169.254.169.254",
];

fn is_private_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            octets[0] == 127
                || octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
                || octets[0] == 0
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn validate_url_ssrf(url_str: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;

    if url.scheme() != "https" {
        return Err(format!(
            "Only HTTPS allowed for dynamic skills, got: {}",
            url.scheme()
        ));
    }

    let host = url.host_str().ok_or("URL must have a host")?;
    let host_lower = host.to_lowercase();
    for blocked in BLOCKED_HOSTS {
        if host_lower == *blocked {
            return Err(format!("Host '{}' is blocked (SSRF protection)", host));
        }
    }

    match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            if is_private_ip(&IpAddr::V4(ip)) {
                return Err(format!(
                    "Private/internal IP '{}' is blocked (SSRF protection)",
                    ip
                ));
            }
        }
        Some(url::Host::Ipv6(ip)) => {
            if is_private_ip(&IpAddr::V6(ip)) {
                return Err(format!(
                    "Private/internal IP '{}' is blocked (SSRF protection)",
                    ip
                ));
            }
        }
        Some(url::Host::Domain(d)) => {
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

    Ok(url)
}

// ── Template Rendering ──────────────────────────────────────────────

fn render_template(
    template: &str,
    params: &HashMap<String, serde_json::Value>,
    secrets: Option<&SecretsVault>,
) -> Result<String, String> {
    let mut result = template.to_string();

    // Replace {{secret:key_name}} patterns first
    while let Some(start) = result.find("{{secret:") {
        let end = result[start..]
            .find("}}")
            .ok_or_else(|| "Unclosed {{secret:...}} template".to_string())?;
        let key = &result[start + 9..start + end];
        let value = secrets
            .and_then(|v| v.get_secret(key).map(|s| s.to_string()))
            .ok_or_else(|| {
                tracing::warn!("Secret reference not found in vault");
                "A required secret is not configured. Check skill configuration and store the needed secret with store_secret.".to_string()
            })?;
        result = format!(
            "{}{}{}",
            &result[..start],
            value,
            &result[start + end + 2..]
        );
    }

    // Replace {{param_name}} patterns
    for (key, value) in params {
        let placeholder = format!("{{{{{}}}}}", key);
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }

    Ok(result)
}

/// Extract secret key names referenced by `{{secret:key_name}}` in config.
pub fn extract_secret_keys(config: &DynamicSkillConfig) -> Vec<String> {
    let mut keys = Vec::new();
    for tool in &config.tools {
        extract_secret_refs(&tool.url_template, &mut keys);
        for v in tool.headers.values() {
            extract_secret_refs(v, &mut keys);
        }
        if let Some(ref body) = tool.body_template {
            extract_secret_refs(body, &mut keys);
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn extract_secret_refs(template: &str, out: &mut Vec<String>) {
    let mut search = template;
    while let Some(start) = search.find("{{secret:") {
        let rest = &search[start + 9..];
        if let Some(end) = rest.find("}}") {
            out.push(rest[..end].to_string());
            search = &rest[end + 2..];
        } else {
            break;
        }
    }
}

// ── JSON Path Extraction ────────────────────────────────────────────

fn extract_json_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current.clone())
}

// ── Validation ──────────────────────────────────────────────────────

fn validate_config(config: &DynamicSkillConfig) -> Result<(), String> {
    // ID must start with a recognized namespace prefix
    if !config.id.starts_with("dynamic.") && !config.id.starts_with("custom.") {
        return Err("Skill ID must start with 'dynamic.' or 'custom.'".to_string());
    }
    // ID chars: alphanumeric, dots, underscores
    if !config
        .id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_')
    {
        return Err(
            "Skill ID may only contain alphanumeric characters, dots, and underscores".to_string(),
        );
    }

    if config.name.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }

    if config.tools.is_empty() {
        return Err("Skill must have at least one tool".to_string());
    }
    if config.tools.len() > 10 {
        return Err("Skill may have at most 10 tools".to_string());
    }

    let mut tool_names = std::collections::HashSet::new();
    for tool in &config.tools {
        validate_tool_config(tool)?;
        if !tool_names.insert(&tool.name) {
            return Err(format!("Duplicate tool name: {}", tool.name));
        }
    }

    Ok(())
}

fn validate_tool_config(tool: &DynamicToolConfig) -> Result<(), String> {
    // Name: alphanumeric + underscores, 3-64 chars
    if tool.name.len() < 3 || tool.name.len() > 64 {
        return Err(format!("Tool name '{}' must be 3-64 characters", tool.name));
    }
    if !tool.name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!(
            "Tool name '{}' may only contain alphanumeric characters and underscores",
            tool.name
        ));
    }

    // Method
    if !matches!(
        tool.method.to_uppercase().as_str(),
        "GET" | "POST" | "PUT" | "DELETE"
    ) {
        return Err(format!(
            "Method '{}' must be GET, POST, PUT, or DELETE",
            tool.method
        ));
    }

    // URL template must start with https://
    if !tool.url_template.starts_with("https://") {
        return Err(format!(
            "URL template must start with https://, got: {}",
            tool.url_template
        ));
    }

    Ok(())
}

// ── DynamicApiSkill ─────────────────────────────────────────────────

pub struct DynamicApiSkill {
    manifest: SkillManifest,
    config: DynamicSkillConfig,
    secrets: Option<Arc<Mutex<SecretsVault>>>,
}

impl DynamicApiSkill {
    pub fn from_config(
        config: DynamicSkillConfig,
        secrets: Option<Arc<Mutex<SecretsVault>>>,
    ) -> Result<Self, String> {
        validate_config(&config)?;

        let secret_keys = extract_secret_keys(&config);
        let secret_descriptors: Vec<SecretDescriptor> = secret_keys
            .iter()
            .map(|k| SecretDescriptor {
                name: k.clone(),
                description: format!("API key for dynamic skill '{}'", config.name),
                required: true,
            })
            .collect();

        let manifest = SkillManifest {
            id: SkillId(config.id.clone()),
            name: config.name.clone(),
            version: config.version.clone(),
            description: config.description.clone(),
            license: None,
            category: config.category.clone(),
            keywords: vec!["dynamic".to_string(), "api".to_string()],
            runtime: "Dynamic".to_string(),
            min_abigail_version: "0.0.1".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![Permission::Network(NetworkPermission::Full)],
            secrets: secret_descriptors,
            config_defaults: HashMap::new(),
        };

        Ok(Self {
            manifest,
            config,
            secrets,
        })
    }

    pub fn load_from_path(
        path: &Path,
        secrets: Option<Arc<Mutex<SecretsVault>>>,
    ) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let config: DynamicSkillConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Invalid dynamic skill JSON: {}", e))?;
        Self::from_config(config, secrets)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Discover dynamic skills by scanning `dir` for `*.json` files.
    /// Also recurses one level into subdirectories (`dir/*/\*.json`)
    /// so that scaffold and factory output layouts are found.
    pub fn discover(dir: &Path, secrets: Option<Arc<Mutex<SecretsVault>>>) -> Vec<Self> {
        let mut skills = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return skills,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                Self::try_load(&path, secrets.clone(), &mut skills);
            } else if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sp = sub.path();
                        if sp.is_file() && sp.extension().and_then(|e| e.to_str()) == Some("json") {
                            Self::try_load(&sp, secrets.clone(), &mut skills);
                        }
                    }
                }
            }
        }
        skills
    }

    fn try_load(path: &Path, secrets: Option<Arc<Mutex<SecretsVault>>>, out: &mut Vec<Self>) {
        match Self::load_from_path(path, secrets) {
            Ok(skill) => {
                tracing::debug!(
                    "Discovered dynamic skill: {} at {:?}",
                    skill.config.id,
                    path
                );
                out.push(skill);
            }
            Err(e) => {
                tracing::warn!("Failed to load dynamic skill from {:?}: {}", path, e);
            }
        }
    }

    pub fn config(&self) -> &DynamicSkillConfig {
        &self.config
    }

    fn build_tool_descriptors(&self) -> Vec<ToolDescriptor> {
        self.config
            .tools
            .iter()
            .map(|t| {
                let method = t.method.to_uppercase();
                let (autonomous, requires_confirmation) = match method.as_str() {
                    "GET" | "HEAD" => (true, false),
                    _ => (false, true),
                };
                ToolDescriptor {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                    returns: serde_json::json!({"type": "string"}),
                    cost_estimate: CostEstimate {
                        latency_ms: 2000,
                        network_bound: true,
                        token_cost: None,
                    },
                    required_permissions: vec![Permission::Network(NetworkPermission::Full)],
                    autonomous,
                    requires_confirmation,
                }
            })
            .collect()
    }

    async fn execute_dynamic_tool(
        &self,
        tool_config: &DynamicToolConfig,
        params: &HashMap<String, serde_json::Value>,
    ) -> SkillResult<ToolOutput> {
        // Render all templates under the lock, then drop before any async work.
        let (rendered_url, headers, rendered_body) = {
            let secrets_ref = match &self.secrets {
                Some(s) => {
                    let guard = s
                        .lock()
                        .map_err(|e| SkillError::ToolFailed(e.to_string()))?;
                    Some(guard)
                }
                None => None,
            };
            let vault = secrets_ref.as_deref();

            let rendered_url = render_template(&tool_config.url_template, params, vault)
                .map_err(SkillError::ToolFailed)?;

            validate_url_ssrf(&rendered_url).map_err(SkillError::ToolFailed)?;

            let mut headers = reqwest::header::HeaderMap::new();
            for (k, v) in &tool_config.headers {
                let rendered_value =
                    render_template(v, params, vault).map_err(SkillError::ToolFailed)?;
                let header_name =
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()).map_err(|e| {
                        SkillError::ToolFailed(format!("Invalid header name '{}': {}", k, e))
                    })?;
                let header_value = reqwest::header::HeaderValue::from_str(&rendered_value)
                    .map_err(|e| {
                        SkillError::ToolFailed(format!("Invalid header value for '{}': {}", k, e))
                    })?;
                headers.insert(header_name, header_value);
            }

            let rendered_body = match tool_config.body_template {
                Some(ref body_tmpl) => Some(
                    render_template(body_tmpl, params, vault).map_err(SkillError::ToolFailed)?,
                ),
                None => None,
            };

            (rendered_url, headers, rendered_body)
            // secrets_ref (MutexGuard) dropped here
        };

        // Build request — no lock held
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| SkillError::ToolFailed(format!("Failed to create HTTP client: {}", e)))?;
        let method = tool_config.method.to_uppercase();
        let mut request = match method.as_str() {
            "GET" => client.get(&rendered_url),
            "POST" => client.post(&rendered_url),
            "PUT" => client.put(&rendered_url),
            "DELETE" => client.delete(&rendered_url),
            _ => {
                return Err(SkillError::ToolFailed(format!(
                    "Unsupported method: {}",
                    method
                )))
            }
        };
        request = request.headers(headers);

        if let Some(body) = rendered_body {
            request = request
                .header("Content-Type", "application/json")
                .body(body);
        }

        // Execute request
        let response = request
            .send()
            .await
            .map_err(|e| SkillError::ToolFailed(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(|e| SkillError::ToolFailed(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Ok(ToolOutput::error(format!(
                "HTTP {} error: {}",
                status, body_text
            )));
        }

        // Parse response as JSON if possible
        let json_response: Result<serde_json::Value, _> = serde_json::from_str(&body_text);

        // Extract fields if we have a JSON response and extraction rules
        if let Ok(ref json_val) = json_response {
            if !tool_config.response_extract.is_empty() {
                let mut extracted = HashMap::new();
                for (field_name, path) in &tool_config.response_extract {
                    if let Some(val) = extract_json_path(json_val, path) {
                        extracted.insert(field_name.clone(), val);
                    }
                }

                // Format using response_format template if provided
                if let Some(ref fmt) = tool_config.response_format {
                    let mut result = fmt.clone();
                    for (key, val) in &extracted {
                        let placeholder = format!("{{{{{}}}}}", key);
                        let replacement = match val {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        result = result.replace(&placeholder, &replacement);
                    }
                    return Ok(ToolOutput::success(serde_json::json!({
                        "formatted": result,
                        "extracted": extracted,
                    })));
                }

                return Ok(ToolOutput::success(
                    serde_json::json!({ "extracted": extracted }),
                ));
            }

            // No extraction rules — return raw JSON
            return Ok(ToolOutput::success(json_val.clone()));
        }

        // Non-JSON response
        Ok(ToolOutput::success(serde_json::json!({ "raw": body_text })))
    }
}

#[async_trait]
impl Skill for DynamicApiSkill {
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
        self.build_tool_descriptors()
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        let tool_config = self
            .config
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| {
                SkillError::ToolFailed(format!("Unknown tool '{}' in dynamic skill", tool_name))
            })?;

        self.execute_dynamic_tool(tool_config, &params.values).await
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {
        None
    }

    fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
        vec![]
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> DynamicSkillConfig {
        DynamicSkillConfig {
            id: "dynamic.test_api".to_string(),
            name: "Test API".to_string(),
            description: "A test dynamic skill".to_string(),
            version: "1.0.0".to_string(),
            category: "Testing".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            tools: vec![DynamicToolConfig {
                name: "get_data".to_string(),
                description: "Fetch data from test API".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" }
                    },
                    "required": ["query"]
                }),
                method: "GET".to_string(),
                url_template: "https://api.example.com/data?q={{query}}".to_string(),
                headers: HashMap::new(),
                body_template: None,
                response_extract: HashMap::new(),
                response_format: None,
            }],
        }
    }

    #[test]
    fn test_validate_config_valid() {
        let config = sample_config();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_dynamic_prefix() {
        let mut config = sample_config();
        config.id = "test_api".to_string();
        assert!(validate_config(&config).unwrap_err().contains("dynamic."));
    }

    #[test]
    fn test_validate_config_bad_id_chars() {
        let mut config = sample_config();
        config.id = "dynamic.test api!".to_string();
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_validate_config_empty_tools() {
        let mut config = sample_config();
        config.tools.clear();
        assert!(validate_config(&config)
            .unwrap_err()
            .contains("at least one"));
    }

    #[test]
    fn test_validate_config_too_many_tools() {
        let mut config = sample_config();
        for i in 0..11 {
            config.tools.push(DynamicToolConfig {
                name: format!("tool_{:03}", i),
                description: "Extra".to_string(),
                parameters: serde_json::json!({}),
                method: "GET".to_string(),
                url_template: "https://example.com".to_string(),
                headers: HashMap::new(),
                body_template: None,
                response_extract: HashMap::new(),
                response_format: None,
            });
        }
        assert!(validate_config(&config).unwrap_err().contains("at most 10"));
    }

    #[test]
    fn test_validate_config_duplicate_tool_names() {
        let mut config = sample_config();
        config.tools.push(config.tools[0].clone());
        assert!(validate_config(&config).unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_validate_tool_name_too_short() {
        let mut config = sample_config();
        config.tools[0].name = "ab".to_string();
        assert!(validate_config(&config).unwrap_err().contains("3-64"));
    }

    #[test]
    fn test_validate_tool_bad_method() {
        let mut config = sample_config();
        config.tools[0].method = "PATCH".to_string();
        assert!(validate_config(&config).unwrap_err().contains("Method"));
    }

    #[test]
    fn test_validate_tool_non_https_url() {
        let mut config = sample_config();
        config.tools[0].url_template = "http://api.example.com/data".to_string();
        assert!(validate_config(&config).unwrap_err().contains("https://"));
    }

    #[test]
    fn test_render_template_basic() {
        let mut params = HashMap::new();
        params.insert(
            "city".to_string(),
            serde_json::Value::String("Austin".to_string()),
        );
        let result = render_template(
            "https://api.example.com/weather?city={{city}}",
            &params,
            None,
        );
        assert_eq!(
            result.unwrap(),
            "https://api.example.com/weather?city=Austin"
        );
    }

    #[test]
    fn test_render_template_numeric_param() {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), serde_json::json!(10));
        let result = render_template(
            "https://api.example.com/data?limit={{limit}}",
            &params,
            None,
        );
        assert_eq!(result.unwrap(), "https://api.example.com/data?limit=10");
    }

    #[test]
    fn test_render_template_secret_missing() {
        let params = HashMap::new();
        let result = render_template(
            "https://api.example.com?key={{secret:mykey}}",
            &params,
            None,
        );
        let err = result.unwrap_err();
        assert!(err.contains("secret is not configured"));
        // Secret name must NOT appear in the error message (information leak)
        assert!(!err.contains("mykey"));
    }

    #[test]
    fn test_render_template_with_secret() {
        let params = HashMap::new();
        let mut vault = SecretsVault::new(std::env::temp_dir());
        vault.set_secret("mykey", "abc123");
        let result = render_template(
            "https://api.example.com?key={{secret:mykey}}",
            &params,
            Some(&vault),
        );
        assert_eq!(result.unwrap(), "https://api.example.com?key=abc123");
    }

    #[test]
    fn test_extract_json_path_simple() {
        let val = serde_json::json!({"main": {"temp": 72.5}});
        let result = extract_json_path(&val, "main.temp");
        assert_eq!(result, Some(serde_json::json!(72.5)));
    }

    #[test]
    fn test_extract_json_path_array() {
        let val = serde_json::json!({"weather": [{"description": "sunny"}]});
        let result = extract_json_path(&val, "weather.0.description");
        assert_eq!(result, Some(serde_json::json!("sunny")));
    }

    #[test]
    fn test_extract_json_path_missing() {
        let val = serde_json::json!({"a": 1});
        assert_eq!(extract_json_path(&val, "b.c"), None);
    }

    #[test]
    fn test_extract_secret_keys_from_config() {
        let config = DynamicSkillConfig {
            id: "dynamic.weather".to_string(),
            name: "Weather".to_string(),
            description: "".to_string(),
            version: "1.0.0".to_string(),
            category: "API".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            tools: vec![DynamicToolConfig {
                name: "get_weather".to_string(),
                description: "Get weather".to_string(),
                parameters: serde_json::json!({}),
                method: "GET".to_string(),
                url_template: "https://api.weather.com?key={{secret:weather_api_key}}".to_string(),
                headers: {
                    let mut h = HashMap::new();
                    h.insert(
                        "Authorization".to_string(),
                        "Bearer {{secret:weather_token}}".to_string(),
                    );
                    h
                },
                body_template: None,
                response_extract: HashMap::new(),
                response_format: None,
            }],
        };
        let keys = extract_secret_keys(&config);
        assert!(keys.contains(&"weather_api_key".to_string()));
        assert!(keys.contains(&"weather_token".to_string()));
    }

    #[test]
    fn test_ssrf_blocks_private_ips() {
        assert!(validate_url_ssrf("https://127.0.0.1/api").is_err());
        assert!(validate_url_ssrf("https://10.0.0.1/api").is_err());
        assert!(validate_url_ssrf("https://192.168.1.1/api").is_err());
        assert!(validate_url_ssrf("https://172.16.0.1/api").is_err());
        assert!(validate_url_ssrf("https://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn test_ssrf_blocks_non_https() {
        assert!(validate_url_ssrf("http://example.com/api").is_err());
        assert!(validate_url_ssrf("ftp://example.com/file").is_err());
    }

    #[test]
    fn test_ssrf_blocks_localhost() {
        assert!(validate_url_ssrf("https://localhost/api").is_err());
        assert!(validate_url_ssrf("https://foo.local/api").is_err());
        assert!(validate_url_ssrf("https://bar.internal/api").is_err());
    }

    #[test]
    fn test_ssrf_allows_public_https() {
        assert!(validate_url_ssrf("https://api.openweathermap.org/data").is_ok());
        assert!(validate_url_ssrf("https://example.com/api").is_ok());
    }

    #[test]
    fn test_from_config_creates_manifest() {
        let config = sample_config();
        let skill = DynamicApiSkill::from_config(config.clone(), None).unwrap();
        assert_eq!(skill.manifest().id.0, "dynamic.test_api");
        assert_eq!(skill.manifest().name, "Test API");
        assert_eq!(skill.tools().len(), 1);
        assert_eq!(skill.tools()[0].name, "get_data");
    }

    #[test]
    fn test_save_and_load() {
        let config = sample_config();
        let skill = DynamicApiSkill::from_config(config, None).unwrap();

        let tmp = std::env::temp_dir().join("abigail_dynamic_test");
        let _ = std::fs::create_dir_all(&tmp);
        let path = tmp.join("test.json");

        skill.save_to_path(&path).unwrap();
        let loaded = DynamicApiSkill::load_from_path(&path, None).unwrap();
        assert_eq!(loaded.manifest().id.0, "dynamic.test_api");
        assert_eq!(loaded.tools().len(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_loads_json_files() {
        let tmp = std::env::temp_dir().join("abigail_dynamic_discover_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let config = sample_config();
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(tmp.join("dynamic.test_api.json"), &json).unwrap();

        let skills = DynamicApiSkill::discover(&tmp, None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest().id.0, "dynamic.test_api");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_recurses_one_level() {
        let tmp = std::env::temp_dir().join("abigail_dynamic_discover_nested");
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("skill-weather");
        std::fs::create_dir_all(&sub).unwrap();

        let mut config = sample_config();
        config.id = "custom.weather".to_string();
        config.name = "Weather".to_string();
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(sub.join("custom_weather.json"), &json).unwrap();

        let skills = DynamicApiSkill::discover(&tmp, None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest().id.0, "custom.weather");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_finds_both_flat_and_nested() {
        let tmp = std::env::temp_dir().join("abigail_dynamic_discover_both");
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let config_flat = sample_config();
        std::fs::write(
            tmp.join("dynamic.test_api.json"),
            serde_json::to_string_pretty(&config_flat).unwrap(),
        )
        .unwrap();

        let mut config_nested = sample_config();
        config_nested.id = "custom.nested".to_string();
        config_nested.name = "Nested".to_string();
        std::fs::write(
            sub.join("custom_nested.json"),
            serde_json::to_string_pretty(&config_nested).unwrap(),
        )
        .unwrap();

        let skills = DynamicApiSkill::discover(&tmp, None);
        assert_eq!(skills.len(), 2);
        let ids: Vec<&str> = skills.iter().map(|s| s.manifest().id.0.as_str()).collect();
        assert!(ids.contains(&"dynamic.test_api"));
        assert!(ids.contains(&"custom.nested"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_discover_ignores_non_json_files() {
        let tmp = std::env::temp_dir().join("abigail_dynamic_discover_ignore");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(tmp.join("readme.txt"), "not a skill").unwrap();
        std::fs::write(tmp.join("config.toml"), "[skill]\nid=\"x\"").unwrap();

        let skills = DynamicApiSkill::discover(&tmp, None);
        assert_eq!(skills.len(), 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
