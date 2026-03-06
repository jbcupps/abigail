//! Sensory capabilities — hearing (audio), vision (video), web search, browsing, HTTP, file ingestion.

#[cfg(feature = "browser")]
pub mod browser;
#[cfg(not(feature = "browser"))]
pub mod browser {
    use crate::cognitive::ToolDefinition;
    use crate::Capability;
    use serde_json::Value;
    use std::path::PathBuf;

    /// Stub browser config used when browser feature is disabled.
    #[derive(Debug, Clone)]
    pub struct BrowserCapabilityConfig {
        pub headless: bool,
        pub max_pages: usize,
        pub nav_timeout_ms: u64,
        pub browser_path: Option<PathBuf>,
    }

    impl Default for BrowserCapabilityConfig {
        fn default() -> Self {
            Self {
                headless: true,
                max_pages: 0,
                nav_timeout_ms: 0,
                browser_path: None,
            }
        }
    }

    /// Stub browser capability that returns explicit disabled errors.
    pub struct BrowserCapability {
        _config: BrowserCapabilityConfig,
    }

    impl BrowserCapability {
        pub fn new(config: BrowserCapabilityConfig) -> Self {
            Self { _config: config }
        }

        pub fn new_with_security_policy(
            config: BrowserCapabilityConfig,
            _policy: super::url_security::UrlSecurityPolicy,
        ) -> Self {
            Self { _config: config }
        }

        pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
            Vec::new()
        }

        pub async fn execute_tool(&self, _tool: &str, _params: &Value) -> anyhow::Result<Value> {
            anyhow::bail!("Browser capability is disabled at compile time")
        }
    }

    #[async_trait::async_trait]
    impl Capability for BrowserCapability {
        async fn initialize(
            &mut self,
            _secrets: &mut abigail_core::secrets::SecretsVault,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn shutdown(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn name(&self) -> &str {
            "browser"
        }
    }
}
pub mod file_ingestion;
pub mod http_client;
pub mod url_security;
pub mod web_search;

pub use http_client::*;
pub use url_security::*;
pub use web_search::*;
