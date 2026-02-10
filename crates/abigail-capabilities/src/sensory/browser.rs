//! Browser automation capability via Chrome DevTools Protocol (CDP).
//!
//! Provides headless browser control for navigating websites, extracting content,
//! taking screenshots, and interacting with page elements. Uses chromiumoxide
//! to communicate with Chrome/Edge/Chromium via CDP.

use crate::cognitive::ToolDefinition;
use crate::sensory::url_security::{validate_url, UrlSecurityPolicy};
use crate::Capability;
use async_trait::async_trait;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default navigation timeout in milliseconds.
const DEFAULT_NAV_TIMEOUT_MS: u64 = 30_000;

/// Maximum number of open pages/tabs.
const MAX_PAGES: usize = 5;

/// Maximum size of JS eval result returned to the LLM.
const MAX_JS_RESULT_BYTES: usize = 10_240;

/// Default wait-for-element timeout in milliseconds.
const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;

/// Configuration for the browser capability.
#[derive(Debug, Clone)]
pub struct BrowserCapabilityConfig {
    pub headless: bool,
    pub max_pages: usize,
    pub nav_timeout_ms: u64,
    /// Explicit path to Chrome/Edge/Chromium binary. Auto-detected if None.
    pub browser_path: Option<PathBuf>,
}

impl Default for BrowserCapabilityConfig {
    fn default() -> Self {
        Self {
            headless: true,
            max_pages: MAX_PAGES,
            nav_timeout_ms: DEFAULT_NAV_TIMEOUT_MS,
            browser_path: None,
        }
    }
}

/// Internal browser state (lazily initialized).
struct BrowserState {
    #[allow(dead_code)] // Kept alive to maintain browser process; dropped on close
    browser: Browser,
    pages: Vec<Page>,
    active_page: usize,
}

/// Browser automation capability.
pub struct BrowserCapability {
    config: BrowserCapabilityConfig,
    state: Arc<RwLock<Option<BrowserState>>>,
    security_policy: UrlSecurityPolicy,
}

impl BrowserCapability {
    /// Create a new browser capability with the given config.
    /// The browser process is NOT launched until first use (lazy init).
    pub fn new(config: BrowserCapabilityConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(None)),
            security_policy: UrlSecurityPolicy::default(),
        }
    }

    /// Return the tool definitions for this capability.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "browser_navigate".into(),
                description: "Navigate the browser to a URL and wait for the page to load.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to navigate to"
                        }
                    },
                    "required": ["url"]
                }),
            },
            ToolDefinition {
                name: "browser_get_content".into(),
                description: "Get the content of the current page as text or HTML.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "format": {
                            "type": "string",
                            "description": "Output format",
                            "enum": ["text", "html"],
                            "default": "text"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: "browser_screenshot".into(),
                description: "Take a screenshot of the current page. Returns base64-encoded PNG.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "full_page": {
                            "type": "boolean",
                            "description": "Capture the full scrollable page (default: false)",
                            "default": false
                        }
                    }
                }),
            },
            ToolDefinition {
                name: "browser_click".into(),
                description: "Click an element on the page identified by CSS selector.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "selector": {
                            "type": "string",
                            "description": "CSS selector for the element to click"
                        }
                    },
                    "required": ["selector"]
                }),
            },
            ToolDefinition {
                name: "browser_type_text".into(),
                description: "Type text into an input element identified by CSS selector.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "selector": {
                            "type": "string",
                            "description": "CSS selector for the input element"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        }
                    },
                    "required": ["selector", "text"]
                }),
            },
            ToolDefinition {
                name: "browser_fill_form".into(),
                description: "Fill multiple form fields at once. Each field is a CSS selector mapped to a value.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "fields": {
                            "type": "object",
                            "description": "Map of CSS selector -> value to fill",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["fields"]
                }),
            },
            ToolDefinition {
                name: "browser_wait_for".into(),
                description: "Wait for an element matching a CSS selector to appear on the page.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "selector": {
                            "type": "string",
                            "description": "CSS selector to wait for"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Timeout in milliseconds (default: 10000)",
                            "default": 10000
                        }
                    },
                    "required": ["selector"]
                }),
            },
            ToolDefinition {
                name: "browser_evaluate_js".into(),
                description: "Execute JavaScript in the current page context. Returns the result (truncated to 10KB).".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "script": {
                            "type": "string",
                            "description": "JavaScript code to execute"
                        }
                    },
                    "required": ["script"]
                }),
            },
            ToolDefinition {
                name: "browser_get_url".into(),
                description: "Get the current page URL.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "browser_get_title".into(),
                description: "Get the current page title.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "browser_back".into(),
                description: "Navigate back in browser history.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "browser_forward".into(),
                description: "Navigate forward in browser history.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: "browser_close".into(),
                description: "Close the browser and release all resources.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
        ]
    }

    /// Ensure the browser is launched, lazily initializing on first call.
    async fn ensure_browser(&self) -> Result<(), String> {
        let mut state = self.state.write().await;
        if state.is_some() {
            return Ok(());
        }

        tracing::info!("Launching browser (headless={})", self.config.headless);

        let mut builder = BrowserConfig::builder()
            .no_sandbox()
            .arg("--no-first-run")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--disable-translate")
            .arg("--disable-default-apps");

        if self.config.headless {
            builder = builder.arg("--headless=new");
        }

        if let Some(ref path) = self.config.browser_path {
            builder = builder.chrome_executable(path);
        }

        let config = builder
            .build()
            .map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        // Spawn the CDP handler in the background
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                let _ = event;
            }
        });

        // Open initial page
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create initial page: {}", e))?;

        *state = Some(BrowserState {
            browser,
            pages: vec![page],
            active_page: 0,
        });

        Ok(())
    }

    /// Get a reference to the active page. Browser must be initialized.
    async fn with_active_page<F, Fut, T>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(Page) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        self.ensure_browser().await?;
        let state = self.state.read().await;
        let bs = state.as_ref().ok_or("Browser not initialized")?;
        let page = bs
            .pages
            .get(bs.active_page)
            .ok_or("No active page")?
            .clone();
        drop(state); // Release lock before async work
        f(page).await
    }

    /// Dispatch a tool call by name. Returns a string result.
    pub async fn execute_tool(&self, tool_name: &str, args: &serde_json::Value) -> String {
        match tool_name {
            "browser_navigate" => self.handle_navigate(args).await,
            "browser_get_content" => self.handle_get_content(args).await,
            "browser_screenshot" => self.handle_screenshot(args).await,
            "browser_click" => self.handle_click(args).await,
            "browser_type_text" => self.handle_type_text(args).await,
            "browser_fill_form" => self.handle_fill_form(args).await,
            "browser_wait_for" => self.handle_wait_for(args).await,
            "browser_evaluate_js" => self.handle_evaluate_js(args).await,
            "browser_get_url" => self.handle_get_url().await,
            "browser_get_title" => self.handle_get_title().await,
            "browser_back" => self.handle_back().await,
            "browser_forward" => self.handle_forward().await,
            "browser_close" => self.handle_close().await,
            _ => format!("Unknown browser tool: {}", tool_name),
        }
    }

    async fn handle_navigate(&self, args: &serde_json::Value) -> String {
        let url_str = match args.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return "Missing required parameter: url".into(),
        };

        // SSRF check
        if let Err(e) = validate_url(url_str, &self.security_policy) {
            return format!("URL rejected: {}", e);
        }

        match self
            .with_active_page(|page| async move {
                page.goto(url_str)
                    .await
                    .map_err(|e| format!("Navigation failed: {}", e))?;

                // Wait for page to be ready
                page.wait_for_navigation()
                    .await
                    .map_err(|e| format!("Wait for navigation failed: {}", e))?;

                let title = page
                    .get_title()
                    .await
                    .map_err(|e| format!("Failed to get title: {}", e))?;
                let current_url = page
                    .url()
                    .await
                    .map_err(|e| format!("Failed to get URL: {}", e))?;

                Ok(serde_json::json!({
                    "status": "navigated",
                    "url": current_url.unwrap_or_default(),
                    "title": title.unwrap_or_default(),
                })
                .to_string())
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_get_content(&self, args: &serde_json::Value) -> String {
        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        match self
            .with_active_page(|page| async move {
                let content = match format {
                    "html" => page
                        .content()
                        .await
                        .map_err(|e| format!("Failed to get HTML: {}", e))?,
                    _ => {
                        // Get text content via JS
                        let result: String = page
                            .evaluate("document.body.innerText || document.body.textContent || ''")
                            .await
                            .map_err(|e| format!("Failed to get text: {}", e))?
                            .into_value()
                            .map_err(|e| format!("Failed to parse text result: {}", e))?;
                        result
                    }
                };

                // Truncate very large content
                let truncated = content.len() > MAX_JS_RESULT_BYTES;
                let output = if truncated {
                    format!(
                        "{}...\n[Content truncated at {} bytes]",
                        &content[..MAX_JS_RESULT_BYTES],
                        MAX_JS_RESULT_BYTES
                    )
                } else {
                    content
                };

                Ok(output)
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_screenshot(&self, args: &serde_json::Value) -> String {
        let full_page = args
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match self
            .with_active_page(|page| async move {
                let screenshot_data = if full_page {
                    page.screenshot(
                        chromiumoxide::page::ScreenshotParams::builder()
                            .full_page(true)
                            .build(),
                    )
                    .await
                    .map_err(|e| format!("Screenshot failed: {}", e))?
                } else {
                    page.screenshot(
                        chromiumoxide::page::ScreenshotParams::builder().build(),
                    )
                    .await
                    .map_err(|e| format!("Screenshot failed: {}", e))?
                };

                use base64::Engine as _;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&screenshot_data);

                Ok(serde_json::json!({
                    "format": "png",
                    "encoding": "base64",
                    "data": b64,
                    "size_bytes": screenshot_data.len(),
                })
                .to_string())
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_click(&self, args: &serde_json::Value) -> String {
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return "Missing required parameter: selector".into(),
        };

        match self
            .with_active_page(|page| async move {
                let element = page
                    .find_element(&selector)
                    .await
                    .map_err(|e| format!("Element not found '{}': {}", selector, e))?;

                element
                    .click()
                    .await
                    .map_err(|e| format!("Click failed: {}", e))?;

                Ok(format!("Clicked element matching '{}'", selector))
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_type_text(&self, args: &serde_json::Value) -> String {
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return "Missing required parameter: selector".into(),
        };
        let text = match args.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => return "Missing required parameter: text".into(),
        };

        match self
            .with_active_page(|page| async move {
                let element = page
                    .find_element(&selector)
                    .await
                    .map_err(|e| format!("Element not found '{}': {}", selector, e))?;

                element
                    .click()
                    .await
                    .map_err(|e| format!("Failed to focus element: {}", e))?;
                element
                    .type_str(&text)
                    .await
                    .map_err(|e| format!("Type failed: {}", e))?;

                Ok(format!("Typed {} chars into '{}'", text.len(), selector))
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_fill_form(&self, args: &serde_json::Value) -> String {
        let fields = match args.get("fields").and_then(|v| v.as_object()) {
            Some(f) => f.clone(),
            None => return "Missing required parameter: fields".into(),
        };

        match self
            .with_active_page(|page| async move {
                let mut filled = 0;
                for (selector, value) in &fields {
                    let val = value.as_str().unwrap_or("");
                    let element = page
                        .find_element(selector)
                        .await
                        .map_err(|e| format!("Element not found '{}': {}", selector, e))?;
                    element
                        .click()
                        .await
                        .map_err(|e| format!("Failed to focus '{}': {}", selector, e))?;
                    element
                        .type_str(val)
                        .await
                        .map_err(|e| format!("Failed to type into '{}': {}", selector, e))?;
                    filled += 1;
                }
                Ok(format!("Filled {} form fields", filled))
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_wait_for(&self, args: &serde_json::Value) -> String {
        let selector = match args.get("selector").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return "Missing required parameter: selector".into(),
        };
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);

        match self
            .with_active_page(|page| async move {
                tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    page.find_element(&selector),
                )
                .await
                .map_err(|_| format!("Timeout waiting for '{}' after {}ms", selector, timeout_ms))?
                .map_err(|e| format!("Element not found '{}': {}", selector, e))?;

                Ok(format!("Element '{}' found", selector))
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_evaluate_js(&self, args: &serde_json::Value) -> String {
        let script = match args.get("script").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return "Missing required parameter: script".into(),
        };

        match self
            .with_active_page(|page| async move {
                let result: serde_json::Value = page
                    .evaluate(&script)
                    .await
                    .map_err(|e| format!("JS evaluation failed: {}", e))?
                    .into_value()
                    .map_err(|e| format!("Failed to parse JS result: {}", e))?;

                let result_str = serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| result.to_string());

                // Truncate large results
                if result_str.len() > MAX_JS_RESULT_BYTES {
                    Ok(format!(
                        "{}...\n[Result truncated at {} bytes]",
                        &result_str[..MAX_JS_RESULT_BYTES],
                        MAX_JS_RESULT_BYTES
                    ))
                } else {
                    Ok(result_str)
                }
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_get_url(&self) -> String {
        match self
            .with_active_page(|page| async move {
                let url = page
                    .url()
                    .await
                    .map_err(|e| format!("Failed to get URL: {}", e))?;
                Ok(url.unwrap_or_else(|| "about:blank".into()))
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_get_title(&self) -> String {
        match self
            .with_active_page(|page| async move {
                let title = page
                    .get_title()
                    .await
                    .map_err(|e| format!("Failed to get title: {}", e))?;
                Ok(title.unwrap_or_default())
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_back(&self) -> String {
        match self
            .with_active_page(|page| async move {
                page.evaluate("window.history.back()")
                    .await
                    .map_err(|e| format!("Back navigation failed: {}", e))?;
                // Small delay for navigation
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok("Navigated back".to_string())
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_forward(&self) -> String {
        match self
            .with_active_page(|page| async move {
                page.evaluate("window.history.forward()")
                    .await
                    .map_err(|e| format!("Forward navigation failed: {}", e))?;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                Ok("Navigated forward".to_string())
            })
            .await
        {
            Ok(result) => result,
            Err(e) => e,
        }
    }

    async fn handle_close(&self) -> String {
        let mut state = self.state.write().await;
        if state.is_none() {
            return "Browser is not running".to_string();
        }
        *state = None;
        "Browser closed".to_string()
    }
}

/// Auto-detect Chrome/Edge/Chromium executable path.
pub fn detect_browser_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ];
        for path in &candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
        // Try LOCALAPPDATA
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let chrome = PathBuf::from(&local).join(r"Google\Chrome\Application\chrome.exe");
            if chrome.exists() {
                return Some(chrome);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ];
        for path in &candidates {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
        ];
        for name in &candidates {
            if let Ok(output) = std::process::Command::new("which").arg(name).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Some(PathBuf::from(path));
                    }
                }
            }
        }
    }

    None
}

#[async_trait]
impl Capability for BrowserCapability {
    async fn initialize(
        &mut self,
        _secrets: &mut abigail_core::secrets::SecretsVault,
    ) -> anyhow::Result<()> {
        // Lazy init — browser is not launched until first tool call.
        // But we can auto-detect the browser path here if not configured.
        if self.config.browser_path.is_none() {
            self.config.browser_path = detect_browser_path();
            if let Some(ref path) = self.config.browser_path {
                tracing::info!("Auto-detected browser at: {}", path.display());
            } else {
                tracing::warn!("No Chrome/Edge/Chromium found. Browser tools will fail until a browser is installed.");
            }
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        let mut state = self.state.write().await;
        *state = None;
        Ok(())
    }

    fn name(&self) -> &str {
        "browser"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BrowserCapabilityConfig::default();
        assert!(config.headless);
        assert_eq!(config.max_pages, MAX_PAGES);
        assert_eq!(config.nav_timeout_ms, DEFAULT_NAV_TIMEOUT_MS);
        assert!(config.browser_path.is_none());
    }

    #[test]
    fn test_tool_definitions_count() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        let tools = cap.tool_definitions();
        assert_eq!(tools.len(), 13);

        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"browser_navigate"));
        assert!(names.contains(&"browser_get_content"));
        assert!(names.contains(&"browser_screenshot"));
        assert!(names.contains(&"browser_click"));
        assert!(names.contains(&"browser_type_text"));
        assert!(names.contains(&"browser_fill_form"));
        assert!(names.contains(&"browser_wait_for"));
        assert!(names.contains(&"browser_evaluate_js"));
        assert!(names.contains(&"browser_get_url"));
        assert!(names.contains(&"browser_get_title"));
        assert!(names.contains(&"browser_back"));
        assert!(names.contains(&"browser_forward"));
        assert!(names.contains(&"browser_close"));
    }

    #[test]
    fn test_capability_name() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        assert_eq!(cap.name(), "browser");
    }

    #[test]
    fn test_detect_browser_path_runs() {
        // Just ensure the function doesn't panic; result depends on the machine.
        let _path = detect_browser_path();
    }

    #[tokio::test]
    async fn test_navigate_ssrf_blocked() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        let args = serde_json::json!({ "url": "http://169.254.169.254/latest/" });
        let result = cap.handle_navigate(&args).await;
        assert!(
            result.contains("rejected") || result.contains("blocked"),
            "got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_navigate_missing_url() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        let args = serde_json::json!({});
        let result = cap.handle_navigate(&args).await;
        assert!(result.contains("Missing"), "got: {}", result);
    }

    #[tokio::test]
    async fn test_close_when_not_running() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        let result = cap.handle_close().await;
        assert_eq!(result, "Browser is not running");
    }

    #[tokio::test]
    async fn test_click_missing_selector() {
        let cap = BrowserCapability::new(BrowserCapabilityConfig::default());
        let args = serde_json::json!({});
        let result = cap.handle_click(&args).await;
        assert!(result.contains("Missing"), "got: {}", result);
    }
}
