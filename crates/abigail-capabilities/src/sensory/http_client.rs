//! Enhanced HTTP client capability — sessions, cookies, all methods, downloads.
//!
//! A high-trust capability that provides full HTTP client features beyond the
//! basic GET/POST offered by the skill-http skill. Includes persistent sessions
//! with cookie jars, all HTTP methods, and file downloads.

use crate::cognitive::ToolDefinition;
use crate::sensory::url_security::{validate_url, UrlSecurityPolicy};
use crate::Capability;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum response body size: 2MB.
const MAX_RESPONSE_BYTES: usize = 2 * 1_048_576;

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum number of concurrent sessions.
const MAX_SESSIONS: usize = 20;

/// A named HTTP session with its own cookie jar.
struct HttpSession {
    client: reqwest::Client,
    base_url: Option<String>,
}

/// Enhanced HTTP client capability.
pub struct HttpClientCapability {
    sessions: Arc<RwLock<HashMap<String, HttpSession>>>,
    default_client: reqwest::Client,
    security_policy: UrlSecurityPolicy,
    download_dir: PathBuf,
}

impl HttpClientCapability {
    /// Create a new HTTP client capability.
    ///
    /// `download_dir` is the base directory for file downloads (typically `data_dir/downloads/`).
    pub fn new(download_dir: PathBuf) -> Self {
        let default_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("Failed to create default HTTP client");

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_client,
            security_policy: UrlSecurityPolicy::default(),
            download_dir,
        }
    }

    /// Return the tool definitions for this capability.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "http_request".into(),
                description: "Make an HTTP request with any method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS). \
                              Optionally use a named session for persistent cookies. Returns status, headers, and body."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "method": {
                            "type": "string",
                            "description": "HTTP method",
                            "enum": ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"]
                        },
                        "url": {
                            "type": "string",
                            "description": "The URL to request"
                        },
                        "session": {
                            "type": "string",
                            "description": "Optional session name for persistent cookies"
                        },
                        "headers": {
                            "type": "object",
                            "description": "Optional HTTP headers as key-value pairs",
                            "additionalProperties": { "type": "string" }
                        },
                        "body": {
                            "type": "string",
                            "description": "Request body (for POST/PUT/PATCH)"
                        },
                        "content_type": {
                            "type": "string",
                            "description": "Content-Type header value"
                        }
                    },
                    "required": ["method", "url"]
                }),
            },
            ToolDefinition {
                name: "http_session_create".into(),
                description: "Create a named HTTP session with persistent cookies. \
                              Optionally set a base URL so subsequent requests can use relative paths."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Unique session name"
                        },
                        "base_url": {
                            "type": "string",
                            "description": "Optional base URL for the session"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "http_session_close".into(),
                description: "Close and destroy a named HTTP session, clearing its cookies.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Session name to close"
                        }
                    },
                    "required": ["name"]
                }),
            },
            ToolDefinition {
                name: "http_download".into(),
                description: "Download a file from a URL to the local downloads directory. \
                              Returns the local file path."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to download from"
                        },
                        "filename": {
                            "type": "string",
                            "description": "Optional filename (auto-detected from URL if omitted)"
                        }
                    },
                    "required": ["url"]
                }),
            },
        ]
    }

    /// Dispatch a tool call by name. Returns a JSON string result.
    pub async fn execute_tool(&self, tool_name: &str, args: &serde_json::Value) -> String {
        match tool_name {
            "http_request" => self.handle_http_request(args).await,
            "http_session_create" => self.handle_session_create(args).await,
            "http_session_close" => self.handle_session_close(args).await,
            "http_download" => self.handle_download(args).await,
            _ => format!("Unknown HTTP tool: {}", tool_name),
        }
    }

    async fn handle_http_request(&self, args: &serde_json::Value) -> String {
        let method = match args.get("method").and_then(|v| v.as_str()) {
            Some(m) => m.to_uppercase(),
            None => return "Missing required parameter: method".into(),
        };
        let url_str = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return "Missing required parameter: url".into(),
        };

        let session_name = args.get("session").and_then(|v| v.as_str());

        // Resolve URL against session base_url if needed
        let full_url = if let Some(name) = session_name {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(name) {
                if let Some(ref base) = session.base_url {
                    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
                        format!("{}{}", base.trim_end_matches('/'), url_str)
                    } else {
                        url_str.to_string()
                    }
                } else {
                    url_str.to_string()
                }
            } else {
                return format!(
                    "Session '{}' not found. Create it first with http_session_create.",
                    name
                );
            }
        } else {
            url_str.to_string()
        };

        // Validate URL for SSRF
        let url = match validate_url(&full_url, &self.security_policy) {
            Ok(u) => u,
            Err(e) => return format!("URL rejected: {}", e),
        };

        // Build request
        let client = if let Some(name) = session_name {
            let sessions = self.sessions.read().await;
            match sessions.get(name) {
                Some(session) => session.client.clone(),
                None => return format!("Session '{}' not found", name),
            }
        } else {
            self.default_client.clone()
        };

        let mut request = match method.as_str() {
            "GET" => client.get(url.as_str()),
            "POST" => client.post(url.as_str()),
            "PUT" => client.put(url.as_str()),
            "DELETE" => client.delete(url.as_str()),
            "PATCH" => client.patch(url.as_str()),
            "HEAD" => client.head(url.as_str()),
            "OPTIONS" => client.request(reqwest::Method::OPTIONS, url.as_str()),
            other => return format!("Unsupported HTTP method: {}", other),
        };

        // Add headers
        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(val) = value.as_str() {
                    request = request.header(key.as_str(), val);
                }
            }
        }

        // Add content type
        if let Some(ct) = args.get("content_type").and_then(|v| v.as_str()) {
            request = request.header("Content-Type", ct);
        }

        // Add body
        if let Some(body) = args.get("body").and_then(|v| v.as_str()) {
            request = request.body(body.to_string());
        }

        // Execute
        execute_request(request).await
    }

    async fn handle_session_create(&self, args: &serde_json::Value) -> String {
        let name = match args.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => return "Missing required parameter: name".into(),
        };
        let base_url = args
            .get("base_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Validate base_url if provided
        if let Some(ref url) = base_url {
            if let Err(e) = validate_url(url, &self.security_policy) {
                return format!("Base URL rejected: {}", e);
            }
        }

        let mut sessions = self.sessions.write().await;
        if sessions.len() >= MAX_SESSIONS && !sessions.contains_key(&name) {
            return format!(
                "Maximum sessions ({}) reached. Close a session first.",
                MAX_SESSIONS
            );
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(10))
            .cookie_store(true)
            .build();

        let client = match client {
            Ok(c) => c,
            Err(e) => return format!("Failed to create session client: {}", e),
        };

        sessions.insert(
            name.clone(),
            HttpSession {
                client,
                base_url: base_url.clone(),
            },
        );

        let base_msg = base_url
            .map(|u| format!(" with base URL '{}'", u))
            .unwrap_or_default();
        format!("Session '{}' created{}", name, base_msg)
    }

    async fn handle_session_close(&self, args: &serde_json::Value) -> String {
        let name = match args.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return "Missing required parameter: name".into(),
        };

        let mut sessions = self.sessions.write().await;
        if sessions.remove(name).is_some() {
            format!("Session '{}' closed", name)
        } else {
            format!("Session '{}' not found", name)
        }
    }

    async fn handle_download(&self, args: &serde_json::Value) -> String {
        let url_str = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return "Missing required parameter: url".into(),
        };

        let url = match validate_url(url_str, &self.security_policy) {
            Ok(u) => u,
            Err(e) => return format!("URL rejected: {}", e),
        };

        // Determine filename
        let filename = if let Some(name) = args.get("filename").and_then(|v| v.as_str()) {
            // Sanitize user-provided filename
            let sanitized: String = name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            if sanitized.is_empty() {
                "download".to_string()
            } else {
                sanitized
            }
        } else {
            // Extract from URL path
            url.path_segments()
                .and_then(|mut seg| seg.next_back())
                .filter(|s| !s.is_empty())
                .unwrap_or("download")
                .to_string()
        };

        // Ensure download directory exists
        if let Err(e) = tokio::fs::create_dir_all(&self.download_dir).await {
            return format!("Failed to create download directory: {}", e);
        }

        let dest = self.download_dir.join(&filename);

        // Download
        let response = match self.default_client.get(url.as_str()).send().await {
            Ok(r) => r,
            Err(e) => return format!("Download request failed: {}", e),
        };

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return format!("Download failed with HTTP {}", status);
        }

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => return format!("Failed to read download body: {}", e),
        };

        if let Err(e) = tokio::fs::write(&dest, &bytes).await {
            return format!("Failed to write file: {}", e);
        }

        serde_json::json!({
            "status": "downloaded",
            "path": dest.to_string_lossy(),
            "size_bytes": bytes.len(),
            "filename": filename,
        })
        .to_string()
    }
}

/// Execute a request and return a formatted string result.
async fn execute_request(request: reqwest::RequestBuilder) -> String {
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => return format!("Request failed: {}", e),
    };

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

    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => return format!("Failed to read response body: {}", e),
    };

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

    serde_json::json!({
        "formatted": formatted,
        "status": status,
        "status_text": status_text,
        "body": body,
        "body_truncated": truncated,
        "content_type": content_type,
        "headers": response_headers,
    })
    .to_string()
}

#[async_trait]
impl Capability for HttpClientCapability {
    async fn initialize(
        &mut self,
        _secrets: &mut abigail_core::secrets::SecretsVault,
    ) -> anyhow::Result<()> {
        // Ensure download directory exists
        std::fs::create_dir_all(&self.download_dir)?;
        Ok(())
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.clear();
        Ok(())
    }

    fn name(&self) -> &str {
        "http_client"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_capability() -> HttpClientCapability {
        HttpClientCapability::new(PathBuf::from("/tmp/abigail_test_downloads"))
    }

    #[test]
    fn test_tool_definitions_count() {
        let cap = make_capability();
        let tools = cap.tool_definitions();
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"http_request"));
        assert!(names.contains(&"http_session_create"));
        assert!(names.contains(&"http_session_close"));
        assert!(names.contains(&"http_download"));
    }

    #[test]
    fn test_capability_name() {
        let cap = make_capability();
        assert_eq!(cap.name(), "http_client");
    }

    #[tokio::test]
    async fn test_session_create_and_close() {
        let cap = make_capability();
        let args = serde_json::json!({ "name": "test_session" });
        let result = cap.handle_session_create(&args).await;
        assert!(result.contains("created"), "got: {}", result);

        let result = cap
            .handle_session_close(&serde_json::json!({ "name": "test_session" }))
            .await;
        assert!(result.contains("closed"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_session_close_nonexistent() {
        let cap = make_capability();
        let result = cap
            .handle_session_close(&serde_json::json!({ "name": "nope" }))
            .await;
        assert!(result.contains("not found"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_session_limit() {
        let cap = make_capability();
        for i in 0..MAX_SESSIONS {
            let args = serde_json::json!({ "name": format!("s{}", i) });
            let result = cap.handle_session_create(&args).await;
            assert!(result.contains("created"), "got: {}", result);
        }
        let args = serde_json::json!({ "name": "overflow" });
        let result = cap.handle_session_create(&args).await;
        assert!(result.contains("Maximum sessions"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_request_ssrf_blocked() {
        let cap = make_capability();
        let args = serde_json::json!({
            "method": "GET",
            "url": "http://169.254.169.254/latest/meta-data/"
        });
        let result = cap.handle_http_request(&args).await;
        assert!(
            result.contains("rejected") || result.contains("blocked"),
            "got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_request_missing_method() {
        let cap = make_capability();
        let args = serde_json::json!({ "url": "https://example.com" });
        let result = cap.handle_http_request(&args).await;
        assert!(result.contains("Missing"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_request_missing_url() {
        let cap = make_capability();
        let args = serde_json::json!({ "method": "GET" });
        let result = cap.handle_http_request(&args).await;
        assert!(result.contains("Missing"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_download_ssrf_blocked() {
        let cap = make_capability();
        let args = serde_json::json!({ "url": "http://10.0.0.1/secret" });
        let result = cap.handle_download(&args).await;
        assert!(
            result.contains("rejected") || result.contains("blocked"),
            "got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_session_base_url_ssrf() {
        let cap = make_capability();
        let args = serde_json::json!({
            "name": "evil",
            "base_url": "http://192.168.1.1"
        });
        let result = cap.handle_session_create(&args).await;
        assert!(
            result.contains("rejected") || result.contains("blocked"),
            "got: {}",
            result
        );
    }
}
