//! MCP (Model Context Protocol) client and bridge.
//!
//! Implements JSON-RPC 2.0 lifecycle, tools/list, and tools/call for HTTP transport.
//! See https://modelcontextprotocol.io/specification/2025-11-25/server/tools

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

use crate::manifest::{CapabilityDescriptor, SkillId, SkillManifest};
use crate::manifest::{NetworkPermission, Permission};
use crate::skill::{
    CostEstimate, ExecutionContext, Skill, SkillConfig, SkillError, SkillHealth, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams,
};

/// MCP tool definition (from tools/list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub input_schema: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response (result).
#[derive(Debug, Deserialize)]
struct JsonRpcResult {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<u64>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

/// Tools/list result.
#[derive(Debug, Deserialize)]
struct ToolsListResult {
    tools: Vec<McpTool>,
}

/// Tools/call result content item.
#[derive(Debug, Deserialize)]
struct ToolResultContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
}

/// Tools/call result.
#[derive(Debug, Deserialize)]
struct ToolCallResult {
    content: Vec<ToolResultContent>,
    #[serde(default)]
    is_error: bool,
}

/// HTTP-based MCP client. Connects to an MCP server over HTTP (Streamable HTTP or simple POST).
pub struct HttpMcpClient {
    base_url: String,
    client: reqwest::Client,
    next_id: std::sync::atomic::AtomicU64,
}

impl HttpMcpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Send a JSON-RPC request and return the result value (or error).
    async fn send(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> SkillResult<serde_json::Value> {
        let id = self.next_id();
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };
        let url = self.base_url.trim_end_matches('/').to_string() + "/";
        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| crate::SkillError::ToolFailed(format!("MCP request failed: {}", e)))?;
        let status = res.status();
        let body: JsonRpcResult = res.json().await.map_err(|e| {
            crate::SkillError::ToolFailed(format!("MCP response parse failed: {}", e))
        })?;
        if let Some(err) = body.error {
            return Err(crate::SkillError::ToolFailed(format!(
                "MCP error {}: {}",
                err.code, err.message
            )));
        }
        if !status.is_success() {
            return Err(crate::SkillError::ToolFailed(format!(
                "MCP HTTP {}",
                status
            )));
        }
        body.result
            .ok_or_else(|| crate::SkillError::ToolFailed("MCP response missing result".to_string()))
    }

    /// Initialize the session (MCP lifecycle). Optional for some servers.
    pub async fn initialize(&self) -> SkillResult<()> {
        let _ = self
            .send(
                "initialize",
                Some(serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "abigail", "version": "0.0.1" }
                })),
            )
            .await?;
        let _ = self.send("notifications/initialized", None).await;
        Ok(())
    }

    /// List tools from the MCP server.
    pub async fn list_tools_impl(&self) -> SkillResult<Vec<McpTool>> {
        let result = self.send("tools/list", Some(serde_json::json!({}))).await?;
        let list: ToolsListResult = serde_json::from_value(result)
            .map_err(|e| crate::SkillError::ToolFailed(format!("tools/list parse: {}", e)))?;
        Ok(list.tools)
    }

    /// Read a resource by URI (e.g. ui:// for MCP Apps).
    pub async fn read_resource(&self, uri: &str) -> SkillResult<String> {
        let result = self
            .send("resources/read", Some(serde_json::json!({ "uri": uri })))
            .await?;
        #[derive(Deserialize)]
        struct ReadResult {
            contents: Vec<ResourceContent>,
        }
        #[derive(Deserialize)]
        struct ResourceContent {
            #[serde(default)]
            text: Option<String>,
        }
        let read: ReadResult = serde_json::from_value(result)
            .map_err(|e| crate::SkillError::ToolFailed(format!("resources/read parse: {}", e)))?;
        read.contents
            .into_iter()
            .next()
            .and_then(|c| c.text)
            .ok_or_else(|| {
                crate::SkillError::ToolFailed("resources/read: no text content".to_string())
            })
    }

    /// Call a tool by name with the given arguments.
    pub async fn call_tool_impl(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> SkillResult<serde_json::Value> {
        let result = self
            .send(
                "tools/call",
                Some(serde_json::json!({ "name": name, "arguments": arguments })),
            )
            .await?;
        let call_result: ToolCallResult = serde_json::from_value(result)
            .map_err(|e| crate::SkillError::ToolFailed(format!("tools/call parse: {}", e)))?;
        if call_result.is_error {
            let msg = call_result
                .content
                .iter()
                .find(|c| c.content_type == "text")
                .and_then(|c| c.text.as_deref())
                .unwrap_or("Unknown error");
            return Err(crate::SkillError::ToolFailed(msg.to_string()));
        }
        let text: String = call_result
            .content
            .iter()
            .filter_map(|c| {
                if c.content_type == "text" {
                    c.text.as_deref()
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(serde_json::json!({ "content": text }))
    }
}

#[async_trait]
impl McpClientCapability for HttpMcpClient {
    async fn list_tools(&self) -> SkillResult<Vec<McpTool>> {
        self.list_tools_impl().await
    }

    async fn call_tool(
        &self,
        tool: &str,
        args: serde_json::Value,
    ) -> SkillResult<serde_json::Value> {
        self.call_tool_impl(tool, args).await
    }
}

/// Trait for MCP client capability (list tools, call tool).
#[async_trait]
pub trait McpClientCapability: Send + Sync {
    async fn list_tools(&self) -> SkillResult<Vec<McpTool>> {
        Ok(vec![])
    }

    async fn call_tool(
        &self,
        _tool: &str,
        _args: serde_json::Value,
    ) -> SkillResult<serde_json::Value> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}

/// Map MCP tool to Abigail ToolDescriptor (for use by McpSkillRuntime).
pub fn mcp_tool_to_descriptor(t: &McpTool) -> ToolDescriptor {
    ToolDescriptor {
        name: t.name.clone(),
        description: t
            .description
            .clone()
            .or_else(|| t.title.clone())
            .unwrap_or_else(|| t.name.clone()),
        parameters: t
            .input_schema
            .clone()
            .unwrap_or_else(|| serde_json::json!({ "type": "object", "properties": {} })),
        returns: serde_json::json!({ "type": "object" }),
        cost_estimate: CostEstimate::default(),
        required_permissions: vec![Permission::Network(NetworkPermission::Full)],
        autonomous: false,
        requires_confirmation: true,
    }
}

/// Skill implementation that exposes an MCP server's tools as Abigail tools.
pub struct McpSkillRuntime {
    manifest: SkillManifest,
    client: HttpMcpClient,
    tools_cache: RwLock<Vec<ToolDescriptor>>,
}

impl McpSkillRuntime {
    pub fn new(
        server_id: impl Into<String>,
        server_name: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let id = server_id.into();
        let name = server_name.into();
        let manifest = SkillManifest {
            id: SkillId(id.clone()),
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: format!("MCP server: {}", name),
            license: None,
            category: "MCP".to_string(),
            keywords: vec!["mcp".to_string()],
            runtime: "MCP".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![CapabilityDescriptor {
                capability_type: "McpClient".to_string(),
                version: "1.0".to_string(),
            }],
            permissions: vec![Permission::Network(NetworkPermission::Full)],
            secrets: vec![],
            config_defaults: std::collections::HashMap::new(),
        };
        Self {
            manifest,
            client: HttpMcpClient::new(base_url),
            tools_cache: RwLock::new(Vec::new()),
        }
    }

    pub fn client(&self) -> &HttpMcpClient {
        &self.client
    }
}

#[async_trait]
impl Skill for McpSkillRuntime {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        self.client.initialize().await?;
        let tools = self.client.list_tools_impl().await?;
        let descriptors: Vec<ToolDescriptor> = tools.iter().map(mcp_tool_to_descriptor).collect();
        if let Ok(mut cache) = self.tools_cache.write() {
            *cache = descriptors;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        SkillHealth {
            status: crate::skill::HealthStatus::Unknown,
            message: Some("MCP server health not checked".to_string()),
            last_check: chrono::Utc::now(),
            metrics: std::collections::HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        self.tools_cache
            .read()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        let args = serde_json::to_value(&params.values)
            .map_err(|e| SkillError::ToolFailed(format!("MCP tool params serialize: {}", e)))?;
        let result = self.client.call_tool_impl(tool_name, args).await?;
        Ok(ToolOutput::success(result))
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.manifest.capabilities.clone()
    }

    fn get_capability(&self, cap_type: &str) -> Option<&dyn std::any::Any> {
        if cap_type == "McpClient" {
            Some(self.client())
        } else {
            None
        }
    }

    fn triggers(&self) -> Vec<crate::channel::TriggerDescriptor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_to_descriptor_maps_name_and_schema() {
        let t = McpTool {
            name: "test_tool".to_string(),
            description: Some("A test".to_string()),
            title: Some("Test Tool".to_string()),
            input_schema: Some(serde_json::json!({
                "type": "object",
                "properties": { "x": { "type": "string" } }
            })),
        };
        let d = mcp_tool_to_descriptor(&t);
        assert_eq!(d.name, "test_tool");
        assert_eq!(d.description, "A test");
        assert_eq!(d.parameters["type"], "object");
        assert!(d.requires_confirmation);
        assert_eq!(d.required_permissions.len(), 1);
    }
}
