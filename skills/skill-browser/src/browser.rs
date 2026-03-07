use abigail_capabilities::sensory::url_security::{validate_url, UrlSecurityPolicy};
use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, NetworkPermission,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolMetadata, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use playwright_rs::protocol::{
    BrowserContextOptions, BrowserContext as PlaywrightBrowserContext, Page, Playwright,
    ScreenshotOptions, StorageState,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_WAIT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_LOGIN_TIMEOUT_MS: u64 = 90_000;
const MAX_JS_RESULT_BYTES: usize = 10_240;
const SESSION_METADATA_FILE: &str = "browser_session.json";
const STORAGE_STATE_FILE: &str = "storage_state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSessionRecord {
    pub entity_id: Option<String>,
    pub profile_dir: String,
    pub active_in_process: bool,
    pub last_used_at_utc: String,
    pub last_action: Option<String>,
    pub current_url: Option<String>,
    pub page_title: Option<String>,
    pub cookie_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriangleEthicPreview {
    pub action: String,
    pub target: Option<String>,
    pub consent: String,
    pub scope: String,
    pub verification: String,
    pub triangle_ethic_token: String,
}

#[derive(Debug, Clone)]
pub struct WebmailSendRequest {
    pub profile_dir: PathBuf,
    pub entity_id: Option<String>,
    pub provider_hint: Option<String>,
    pub account_hint: Option<String>,
    pub sender_address: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebmailSendResult {
    pub provider: String,
    pub compose_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WebmailProfile {
    provider: &'static str,
    compose_url: &'static str,
    compose_selector: Option<&'static str>,
    to_selector: &'static str,
    subject_selector: &'static str,
    body_selector: &'static str,
    send_selector: &'static str,
}

struct BrowserRuntime {
    #[allow(dead_code)]
    playwright: Playwright,
    context: PlaywrightBrowserContext,
}

pub struct BrowserSkill {
    manifest: SkillManifest,
    security_policy: UrlSecurityPolicy,
    runtime: Arc<Mutex<Option<BrowserRuntime>>>,
    profile_dir: PathBuf,
    entity_id: Option<String>,
    browser_path: Option<PathBuf>,
    headless: bool,
}

impl BrowserSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse browser skill.toml")
    }

    pub fn new(manifest: SkillManifest) -> Self {
        let default_data_dir = abigail_core::AppConfig::default_paths().data_dir;
        Self::new_with_profile(manifest, false, default_data_dir.join("browser_profile"), None)
    }

    pub fn new_with_local_network(manifest: SkillManifest, allow_local_network: bool) -> Self {
        let default_data_dir = abigail_core::AppConfig::default_paths().data_dir;
        Self::new_with_profile(
            manifest,
            allow_local_network,
            default_data_dir.join("browser_profile"),
            None,
        )
    }

    pub fn new_for_entity(
        manifest: SkillManifest,
        allow_local_network: bool,
        data_dir: PathBuf,
        entity_id: Option<String>,
    ) -> Self {
        Self::new_with_profile(
            manifest,
            allow_local_network,
            data_dir.join("browser_profile"),
            entity_id,
        )
    }

    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    pub fn entity_id(&self) -> Option<&str> {
        self.entity_id.as_deref()
    }

    pub fn session_record_path(profile_dir: &Path) -> PathBuf {
        profile_dir.join(SESSION_METADATA_FILE)
    }

    fn new_with_profile(
        manifest: SkillManifest,
        allow_local_network: bool,
        profile_dir: PathBuf,
        entity_id: Option<String>,
    ) -> Self {
        let security_policy = if allow_local_network {
            UrlSecurityPolicy {
                block_private_ips: false,
                ..UrlSecurityPolicy::default()
            }
        } else {
            UrlSecurityPolicy::default()
        };

        Self {
            manifest,
            security_policy,
            runtime: Arc::new(Mutex::new(None)),
            profile_dir,
            entity_id,
            browser_path: detect_browser_executable(),
            headless: browser_headless_default(),
        }
    }

    pub async fn current_session_record(&self) -> Option<BrowserSessionRecord> {
        let is_active = self.runtime.lock().await.is_some();
        let mut record = load_session_record(&self.profile_dir)?;
        record.active_in_process = is_active;
        Some(record)
    }

    pub async fn clear_session(&self) -> Result<(), String> {
        let mut runtime = self.runtime.lock().await;
        if let Some(active) = runtime.take() {
            active
                .context
                .close()
                .await
                .map_err(|err| format!("failed closing persistent browser context: {err}"))?;
        }
        clear_browser_profile_dir(&self.profile_dir)
    }

    fn descriptor(
        &self,
        name: &str,
        description: &str,
        parameters: serde_json::Value,
        requires_confirmation: bool,
    ) -> ToolDescriptor {
        ToolDescriptor {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
            returns: json!({
                "type": ["object", "string"]
            }),
            cost_estimate: CostEstimate {
                latency_ms: 3_000,
                network_bound: true,
                token_cost: None,
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Full)],
            autonomous: !requires_confirmation,
            requires_confirmation,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDescriptor> {
        let preview_token_doc =
            "First call returns a TriangleEthic preview with a token. Replay the exact call with triangle_ethic_token to execute.";
        vec![
            self.descriptor(
                "navigate",
                &format!("Navigate to a URL in the persistent browser context. {preview_token_doc}"),
                json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["url"]
                }),
                false,
            ),
            self.descriptor(
                "click",
                &format!("Click an element using a CSS selector. {preview_token_doc}"),
                json!({
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["selector"]
                }),
                false,
            ),
            self.descriptor(
                "type",
                &format!("Fill text into an element using a CSS selector. {preview_token_doc}"),
                json!({
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" },
                        "text": { "type": "string" },
                        "press_enter": { "type": "boolean", "default": false },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["selector", "text"]
                }),
                false,
            ),
            self.descriptor(
                "screenshot",
                "Capture a PNG screenshot from the current page.",
                json!({
                    "type": "object",
                    "properties": {
                        "full_page": { "type": "boolean", "default": false }
                    }
                }),
                false,
            ),
            self.descriptor(
                "execute_js",
                &format!("Execute JavaScript in the active page. {preview_token_doc}"),
                json!({
                    "type": "object",
                    "properties": {
                        "script": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["script"]
                }),
                false,
            ),
            self.descriptor(
                "login_with_oauth",
                &format!("Open an OAuth or SSO login flow inside the persistent context and wait for a success URL or selector. {preview_token_doc}"),
                json!({
                    "type": "object",
                    "properties": {
                        "start_url": { "type": "string" },
                        "success_url_contains": { "type": "string" },
                        "success_selector": { "type": "string" },
                        "timeout_ms": { "type": "integer", "default": 90000 },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["start_url"]
                }),
                false,
            ),
            self.descriptor(
                "browser_navigate",
                "Compatibility alias for navigate.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["url"]
                }),
                false,
            ),
            self.descriptor(
                "browser_get_content",
                "Get the current page content as text or HTML.",
                json!({
                    "type": "object",
                    "properties": {
                        "format": { "type": "string", "enum": ["text", "html"], "default": "text" }
                    }
                }),
                false,
            ),
            self.descriptor(
                "browser_screenshot",
                "Compatibility alias for screenshot.",
                json!({
                    "type": "object",
                    "properties": {
                        "full_page": { "type": "boolean", "default": false }
                    }
                }),
                false,
            ),
            self.descriptor(
                "browser_click",
                "Compatibility alias for click.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["selector"]
                }),
                false,
            ),
            self.descriptor(
                "browser_type_text",
                "Compatibility alias for type.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" },
                        "text": { "type": "string" },
                        "press_enter": { "type": "boolean", "default": false },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["selector", "text"]
                }),
                false,
            ),
            self.descriptor(
                "browser_fill_form",
                "Fill multiple form fields at once in the current page.",
                json!({
                    "type": "object",
                    "properties": {
                        "fields": {
                            "type": "object",
                            "additionalProperties": { "type": "string" }
                        },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["fields"]
                }),
                false,
            ),
            self.descriptor(
                "browser_wait_for",
                "Wait until a selector is visible in the current page.",
                json!({
                    "type": "object",
                    "properties": {
                        "selector": { "type": "string" },
                        "timeout_ms": { "type": "integer", "default": 10000 }
                    },
                    "required": ["selector"]
                }),
                false,
            ),
            self.descriptor(
                "browser_evaluate_js",
                "Compatibility alias for execute_js.",
                json!({
                    "type": "object",
                    "properties": {
                        "script": { "type": "string" },
                        "triangle_ethic_token": { "type": "string" }
                    },
                    "required": ["script"]
                }),
                false,
            ),
            self.descriptor(
                "browser_get_url",
                "Get the current page URL.",
                json!({
                    "type": "object",
                    "properties": {}
                }),
                false,
            ),
            self.descriptor(
                "browser_get_title",
                "Get the current page title.",
                json!({
                    "type": "object",
                    "properties": {}
                }),
                false,
            ),
            self.descriptor(
                "browser_back",
                "Navigate back in browser history.",
                json!({
                    "type": "object",
                    "properties": {
                        "triangle_ethic_token": { "type": "string" }
                    }
                }),
                false,
            ),
            self.descriptor(
                "browser_forward",
                "Navigate forward in browser history.",
                json!({
                    "type": "object",
                    "properties": {
                        "triangle_ethic_token": { "type": "string" }
                    }
                }),
                false,
            ),
            self.descriptor(
                "browser_close",
                "Close the active browser context but preserve session data on disk.",
                json!({
                    "type": "object",
                    "properties": {}
                }),
                false,
            ),
        ]
    }

    async fn ensure_runtime(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<BrowserRuntime>>, String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.is_some() {
            return Ok(runtime);
        }

        std::fs::create_dir_all(&self.profile_dir)
            .map_err(|err| format!("failed creating browser profile directory: {err}"))?;
        let downloads_dir = self.profile_dir.join("downloads");
        std::fs::create_dir_all(&downloads_dir)
            .map_err(|err| format!("failed creating browser downloads directory: {err}"))?;

        let playwright = Playwright::launch()
            .await
            .map_err(|err| format!("failed launching Playwright driver: {err}"))?;
        let chromium = playwright.chromium();

        let mut options = BrowserContextOptions::builder()
            .headless(self.headless)
            .downloads_path(downloads_dir.to_string_lossy().to_string())
            .args(vec![
                "--disable-dev-shm-usage".to_string(),
                "--disable-features=OptimizationGuideModelDownloading".to_string(),
                "--disable-popup-blocking".to_string(),
            ]);

        if let Some(browser_path) = &self.browser_path {
            options = options.executable_path(browser_path.to_string_lossy().to_string());
        } else if let Some(channel) = detect_browser_channel() {
            options = options.channel(channel.to_string());
        }
        let restored_storage_state = load_storage_state(&self.profile_dir)?;
        if storage_state_path(&self.profile_dir).is_file() {
            options = options.storage_state_path(
                storage_state_path(&self.profile_dir)
                    .to_string_lossy()
                    .to_string(),
            );
        }

        let context = chromium
            .launch_persistent_context_with_options(
                self.profile_dir.to_string_lossy().to_string(),
                options.build(),
            )
            .await
            .map_err(|err| format_browser_launch_error(err.to_string()))?;
        if let Some(storage_state) = restored_storage_state.as_ref() {
            hydrate_context_from_snapshot(&context, storage_state).await?;
        }

        *runtime = Some(BrowserRuntime { playwright, context });
        drop(runtime);
        self.refresh_session_record(Some("launch")).await?;
        Ok(self.runtime.lock().await)
    }

    async fn current_page(&self) -> Result<Page, String> {
        let runtime = self.ensure_runtime().await?;
        let context = &runtime
            .as_ref()
            .ok_or_else(|| "browser runtime missing after initialization".to_string())?
            .context;

        if let Some(page) = context.pages().last().cloned() {
            return Ok(page);
        }

        context
            .new_page()
            .await
            .map_err(|err| format!("failed creating browser page: {err}"))
    }

    async fn refresh_session_record(&self, action: Option<&str>) -> Result<(), String> {
        let runtime = self.runtime.lock().await;
        let Some(runtime) = runtime.as_ref() else {
            return Ok(());
        };
        let page = runtime.context.pages().last().cloned();
        let current_url = page.as_ref().map(Page::url);
        let page_title = match page {
            Some(ref page) => page.title().await.ok(),
            None => None,
        };
        let cookie_count = runtime
            .context
            .storage_state()
            .await
            .map_err(|err| format!("failed capturing browser storage state: {err}"))?;
        persist_storage_state(&self.profile_dir, &cookie_count)?;

        let record = BrowserSessionRecord {
            entity_id: self.entity_id.clone(),
            profile_dir: self.profile_dir.to_string_lossy().to_string(),
            active_in_process: true,
            last_used_at_utc: Utc::now().to_rfc3339(),
            last_action: action.map(str::to_string),
            current_url,
            page_title,
            cookie_count: Some(cookie_count.cookies.len()),
        };
        persist_session_record(&self.profile_dir, &record)
    }

    async fn build_result(
        &self,
        action: &str,
        data: serde_json::Value,
    ) -> Result<ToolOutput, SkillError> {
        self.refresh_session_record(Some(action))
            .await
            .map_err(SkillError::ToolFailed)?;
        let mut metadata = ToolMetadata::default();
        metadata.extra.insert(
            "profile_dir".to_string(),
            json!(self.profile_dir.to_string_lossy().to_string()),
        );
        if let Some(record) = self.current_session_record().await {
            metadata
                .extra
                .insert("browser_session".to_string(), json!(record));
        }
        Ok(ToolOutput {
            success: true,
            data: Some(data),
            error: None,
            metadata,
        })
    }

    fn triangle_ethic_preview(
        &self,
        tool_name: &str,
        params: &ToolParams,
    ) -> TriangleEthicPreview {
        let target = params
            .get::<String>("url")
            .or_else(|| params.get::<String>("start_url"))
            .or_else(|| params.get::<String>("selector"))
            .or_else(|| params.get::<String>("script"));
        let action = normalize_tool_name(tool_name).to_string();
        let token = triangle_ethic_token(tool_name, params);

        TriangleEthicPreview {
            action,
            target,
            consent: "Confirm the browser action is expected for this identity and does not jump to an unrelated account surface.".to_string(),
            scope: "Use the smallest possible selector, URL, and data payload so the browser session only touches the intended surface.".to_string(),
            verification: "Replay the exact action with the returned triangle_ethic_token so runtime enforcement can verify the preview and execution payload match.".to_string(),
            triangle_ethic_token: token,
        }
    }

    fn preview_required(tool_name: &str) -> bool {
        !matches!(
            normalize_tool_name(tool_name),
            "screenshot"
                | "browser_get_content"
                | "browser_get_url"
                | "browser_get_title"
                | "browser_close"
                | "browser_wait_for"
        )
    }

    fn validated_preview_token(&self, tool_name: &str, params: &ToolParams) -> Option<String> {
        params
            .get::<String>("triangle_ethic_token")
            .filter(|provided| provided == &triangle_ethic_token(tool_name, params))
    }

    async fn maybe_return_triangle_ethic_preview(
        &self,
        tool_name: &str,
        params: &ToolParams,
    ) -> Option<SkillResult<ToolOutput>> {
        if !Self::preview_required(tool_name) {
            return None;
        }

        if self.validated_preview_token(tool_name, params).is_some() {
            return None;
        }

        let preview = self.triangle_ethic_preview(tool_name, params);
        // paper Sections 7-11 runtime enforcement + 22-27 verification:
        // the browser action is previewed first and replayed only when the
        // exact payload hash matches the preview token.
        Some(
            self.build_result(
                normalize_tool_name(tool_name),
                json!({
                    "executed": false,
                    "status": "triangle_ethic_preview_required",
                    "triangle_ethic_preview": preview,
                }),
            )
            .await,
        )
    }

    async fn execute_browser_action(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        if let Some(preview) = self
            .maybe_return_triangle_ethic_preview(tool_name, &params)
            .await
        {
            return preview;
        }

        let normalized = normalize_tool_name(tool_name);
        match normalized {
            "navigate" => self.handle_navigate(params).await,
            "click" => self.handle_click(params).await,
            "type" => self.handle_type(params).await,
            "screenshot" => self.handle_screenshot(params).await,
            "execute_js" => self.handle_execute_js(params).await,
            "login_with_oauth" => self.handle_login_with_oauth(params).await,
            "browser_get_content" => self.handle_get_content(params).await,
            "browser_fill_form" => self.handle_fill_form(params).await,
            "browser_wait_for" => self.handle_wait_for(params).await,
            "browser_get_url" => self.handle_get_url().await,
            "browser_get_title" => self.handle_get_title().await,
            "browser_back" => self.handle_history("back").await,
            "browser_forward" => self.handle_history("forward").await,
            "browser_close" => self.handle_close().await,
            _ => Err(SkillError::ToolFailed(format!("Unknown tool: {tool_name}"))),
        }
    }

    async fn handle_navigate(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let url = params
            .get::<String>("url")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: url".to_string()))?;
        validate_url(&url, &self.security_policy)
            .map_err(|err| SkillError::ToolFailed(format!("URL rejected: {err}")))?;
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        page.goto(&url, None)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Navigation failed: {err}")))?;
        let title = page
            .title()
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Failed to read title: {err}")))?;
        self.build_result(
            "navigate",
            json!({
                "status": "navigated",
                "url": page.url(),
                "title": title,
            }),
        )
        .await
    }

    async fn handle_click(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let selector = params.get::<String>("selector").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: selector".to_string())
        })?;
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let locator = page.locator(&selector).await;
        locator
            .click(None)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Click failed: {err}")))?;
        self.build_result(
            "click",
            json!({
                "status": "clicked",
                "selector": selector,
                "url": page.url(),
            }),
        )
        .await
    }

    async fn handle_type(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let selector = params.get::<String>("selector").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: selector".to_string())
        })?;
        let text = params
            .get::<String>("text")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: text".to_string()))?;
        let press_enter = params.get::<bool>("press_enter").unwrap_or(false);
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let locator = page.locator(&selector).await;
        locator
            .fill(&text, None)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Type failed: {err}")))?;
        if press_enter {
            locator
                .press("Enter", None)
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Enter press failed: {err}")))?;
        }
        self.build_result(
            "type",
            json!({
                "status": "typed",
                "selector": selector,
                "chars": text.chars().count(),
                "submitted": press_enter,
            }),
        )
        .await
    }

    async fn handle_screenshot(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let full_page = params.get::<bool>("full_page").unwrap_or(false);
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let png = page
            .screenshot(Some(
                ScreenshotOptions::builder().full_page(full_page).build(),
            ))
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Screenshot failed: {err}")))?;
        self.build_result(
            "screenshot",
            json!({
                "encoding": "base64",
                "full_page": full_page,
                "png_base64": BASE64.encode(png),
            }),
        )
        .await
    }

    async fn handle_execute_js(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let script = params.get::<String>("script").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: script".to_string())
        })?;
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let result = page
            .evaluate_value(&script)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("JavaScript execution failed: {err}")))?;
        self.build_result(
            "execute_js",
            json!({
                "result": truncate_string(result, MAX_JS_RESULT_BYTES),
            }),
        )
        .await
    }

    async fn handle_login_with_oauth(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let start_url = params.get::<String>("start_url").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: start_url".to_string())
        })?;
        validate_url(&start_url, &self.security_policy)
            .map_err(|err| SkillError::ToolFailed(format!("URL rejected: {err}")))?;
        let success_url_contains = params.get::<String>("success_url_contains");
        let success_selector = params.get::<String>("success_selector");
        let timeout_ms = params
            .get::<u64>("timeout_ms")
            .unwrap_or(DEFAULT_LOGIN_TIMEOUT_MS);
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        page.goto(&start_url, None)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("OAuth navigation failed: {err}")))?;

        wait_for_condition(timeout_ms, || {
            let page = page.clone();
            let success_url_contains = success_url_contains.clone();
            let success_selector = success_selector.clone();
            async move {
                if let Some(expected) = success_url_contains {
                    if page.url().contains(&expected) {
                        return Ok(true);
                    }
                }
                if let Some(selector) = success_selector {
                    return page.locator(&selector).await.is_visible().await;
                }
                Ok(!page.url().is_empty())
            }
        })
        .await
        .map_err(|err| SkillError::ToolFailed(format!("OAuth login wait failed: {err}")))?;

        let title = page
            .title()
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Failed to read title: {err}")))?;
        self.build_result(
            "login_with_oauth",
            json!({
                "status": "oauth_session_ready",
                "url": page.url(),
                "title": title,
                "profile_dir": self.profile_dir.to_string_lossy().to_string(),
            }),
        )
        .await
    }

    async fn handle_get_content(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let format = params
            .get::<String>("format")
            .unwrap_or_else(|| "text".to_string());
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let content = if format.eq_ignore_ascii_case("html") {
            page.content()
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Failed to get HTML: {err}")))?
        } else {
            page.locator("body")
                .await
                .inner_text()
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Failed to get page text: {err}")))?
        };
        self.build_result(
            "browser_get_content",
            json!({
                "format": format,
                "content": truncate_string(content, MAX_JS_RESULT_BYTES),
            }),
        )
        .await
    }

    async fn handle_fill_form(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let fields = params
            .get::<HashMap<String, String>>("fields")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: fields".to_string()))?;
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        for (selector, value) in &fields {
            page.locator(selector)
                .await
                .fill(value, None)
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Form fill failed for {selector}: {err}")))?;
        }
        self.build_result(
            "browser_fill_form",
            json!({
                "filled": fields.len(),
            }),
        )
        .await
    }

    async fn handle_wait_for(&self, params: ToolParams) -> SkillResult<ToolOutput> {
        let selector = params.get::<String>("selector").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: selector".to_string())
        })?;
        let timeout_ms = params
            .get::<u64>("timeout_ms")
            .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        wait_for_condition(timeout_ms, || {
            let page = page.clone();
            let selector = selector.clone();
            async move { page.locator(&selector).await.is_visible().await }
        })
        .await
        .map_err(|err| SkillError::ToolFailed(format!("Wait failed: {err}")))?;

        self.build_result(
            "browser_wait_for",
            json!({
                "status": "visible",
                "selector": selector,
            }),
        )
        .await
    }

    async fn handle_get_url(&self) -> SkillResult<ToolOutput> {
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        self.build_result(
            "browser_get_url",
            json!({
                "url": page.url(),
            }),
        )
        .await
    }

    async fn handle_get_title(&self) -> SkillResult<ToolOutput> {
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let title = page
            .title()
            .await
            .map_err(|err| SkillError::ToolFailed(format!("Failed to get title: {err}")))?;
        self.build_result(
            "browser_get_title",
            json!({
                "title": title,
            }),
        )
        .await
    }

    async fn handle_history(&self, direction: &str) -> SkillResult<ToolOutput> {
        let page = self.current_page().await.map_err(SkillError::ToolFailed)?;
        let script = if direction == "back" {
            "window.history.back();"
        } else {
            "window.history.forward();"
        };
        page.evaluate_expression(script)
            .await
            .map_err(|err| SkillError::ToolFailed(format!("History navigation failed: {err}")))?;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        self.build_result(
            if direction == "back" {
                "browser_back"
            } else {
                "browser_forward"
            },
            json!({
                "status": "navigated",
                "direction": direction,
                "url": page.url(),
            }),
        )
        .await
    }

    async fn handle_close(&self) -> SkillResult<ToolOutput> {
        let mut runtime = self.runtime.lock().await;
        if let Some(active) = runtime.take() {
            snapshot_context_state(&self.profile_dir, &active.context)
                .await
                .map_err(SkillError::ToolFailed)?;
            active
                .context
                .close()
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Browser close failed: {err}")))?;
        }
        if let Some(mut record) = load_session_record(&self.profile_dir) {
            record.active_in_process = false;
            persist_session_record(&self.profile_dir, &record)
                .map_err(SkillError::ToolFailed)?;
        }
        Ok(ToolOutput::success(json!({
            "status": "closed",
            "profile_dir": self.profile_dir.to_string_lossy().to_string(),
        })))
    }
}

#[async_trait]
impl Skill for BrowserSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, config: SkillConfig) -> SkillResult<()> {
        if let Some(data_dir) = config
            .values
            .get("data_dir")
            .and_then(|value| value.as_str())
            .map(PathBuf::from)
        {
            self.profile_dir = data_dir.join("browser_profile");
        }
        if let Some(entity_id) = config
            .values
            .get("entity_id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
        {
            self.entity_id = Some(entity_id);
        }
        if let Some(headless) = config
            .values
            .get("browser_headless")
            .and_then(|value| value.as_bool())
        {
            self.headless = headless;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        let mut runtime = self.runtime.lock().await;
        if let Some(active) = runtime.take() {
            active
                .context
                .close()
                .await
                .map_err(|err| SkillError::ToolFailed(format!("Browser shutdown failed: {err}")))?;
        }
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let message = if self.profile_dir.exists() {
            Some(format!(
                "Persistent browser profile at {}",
                self.profile_dir.display()
            ))
        } else {
            Some("Persistent browser profile will be created on first use".to_string())
        };
        SkillHealth {
            status: HealthStatus::Healthy,
            message,
            last_check: Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        self.tool_definitions()
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        self.execute_browser_action(tool_name, params, context).await
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![CapabilityDescriptor {
            capability_type: "browser_automation".to_string(),
            version: "2.0".to_string(),
        }]
    }

    fn get_capability(&self, cap_type: &str) -> Option<&dyn Any> {
        if cap_type == "browser_session_control" {
            Some(self)
        } else {
            None
        }
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

pub fn discover_browser_sessions(data_root: &Path) -> Result<Vec<BrowserSessionRecord>, String> {
    let identities_dir = data_root.join("identities");
    let mut sessions = Vec::new();

    if identities_dir.is_dir() {
        let entries = std::fs::read_dir(&identities_dir)
            .map_err(|err| format!("failed reading identities directory: {err}"))?;
        for entry in entries.flatten() {
            let profile_dir = entry.path().join("browser_profile");
            if let Some(record) = load_session_record(&profile_dir) {
                sessions.push(record);
            }
        }
    }

    let root_profile = data_root.join("browser_profile");
    if let Some(record) = load_session_record(&root_profile) {
        sessions.push(record);
    }

    sessions.sort_by(|left, right| right.last_used_at_utc.cmp(&left.last_used_at_utc));
    Ok(sessions)
}

pub fn clear_browser_profile_dir(profile_dir: &Path) -> Result<(), String> {
    if !profile_dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(profile_dir)
        .map_err(|err| format!("failed clearing browser profile {}: {err}", profile_dir.display()))
}

pub async fn webmail_send(request: WebmailSendRequest) -> Result<WebmailSendResult, String> {
    let profile = infer_webmail_profile(&request)
        .ok_or_else(|| "no supported webmail fallback was inferred from the configured account".to_string())?;
    let browser = BrowserSkill::new_for_entity(
        BrowserSkill::default_manifest(),
        false,
        profile_data_dir(&request.profile_dir),
        request.entity_id.clone(),
    );
    let page = browser.current_page().await?;
    page.goto(profile.compose_url, None)
        .await
        .map_err(|err| format!("webmail navigation failed: {err}"))?;

    if let Some(compose_selector) = profile.compose_selector {
        let compose = page.locator(compose_selector).await;
        if compose.is_visible().await.unwrap_or(false) {
            compose
                .click(None)
                .await
                .map_err(|err| format!("webmail compose click failed: {err}"))?;
        }
    }

    wait_for_condition(DEFAULT_WAIT_TIMEOUT_MS, || {
        let page = page.clone();
        let selector = profile.to_selector.to_string();
        async move { page.locator(&selector).await.is_visible().await }
    })
    .await?;

    page.locator(profile.to_selector)
        .await
        .fill(&request.to.join(", "), None)
        .await
        .map_err(|err| format!("webmail recipient fill failed: {err}"))?;
    page.locator(profile.subject_selector)
        .await
        .fill(&request.subject, None)
        .await
        .map_err(|err| format!("webmail subject fill failed: {err}"))?;
    page.locator(profile.body_selector)
        .await
        .fill(&request.body, None)
        .await
        .map_err(|err| format!("webmail body fill failed: {err}"))?;
    page.locator(profile.send_selector)
        .await
        .click(None)
        .await
        .map_err(|err| format!("webmail send click failed: {err}"))?;

    browser.refresh_session_record(Some("webmail_send")).await?;
    Ok(WebmailSendResult {
        provider: profile.provider.to_string(),
        compose_url: profile.compose_url.to_string(),
    })
}

fn profile_data_dir(profile_dir: &Path) -> PathBuf {
    profile_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| profile_dir.to_path_buf())
}

fn storage_state_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(STORAGE_STATE_FILE)
}

fn infer_webmail_profile(request: &WebmailSendRequest) -> Option<WebmailProfile> {
    let hint = request
        .provider_hint
        .as_deref()
        .or(request.account_hint.as_deref())
        .or(request.sender_address.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if hint.contains("gmail") || hint.ends_with("@gmail.com") {
        return Some(WebmailProfile {
            provider: "gmail",
            compose_url: "https://mail.google.com/mail/u/0/#inbox?compose=new",
            compose_selector: Some("div[gh='cm']"),
            to_selector: "input[aria-label='To recipients']",
            subject_selector: "input[name='subjectbox']",
            body_selector: "div[aria-label='Message Body']",
            send_selector: "div[role='button'][data-tooltip*='Send']",
        });
    }

    if hint.contains("outlook")
        || hint.contains("office365")
        || hint.contains("hotmail")
        || hint.contains("live.com")
    {
        return Some(WebmailProfile {
            provider: "outlook",
            compose_url: "https://outlook.office.com/mail/",
            compose_selector: Some("button[aria-label^='New mail']"),
            to_selector: "div[aria-label='To'] input",
            subject_selector: "input[aria-label='Add a subject']",
            body_selector: "div[aria-label='Message body']",
            send_selector: "button[aria-label^='Send']",
        });
    }

    None
}

fn browser_headless_default() -> bool {
    std::env::var("ABIGAIL_BROWSER_HEADLESS")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn detect_browser_channel() -> Option<&'static str> {
    #[cfg(target_os = "windows")]
    {
        if find_existing_path(&candidate_msedge_paths()).is_some() {
            return Some("msedge");
        }
        if find_existing_path(&candidate_chrome_paths()).is_some() {
            return Some("chrome");
        }
    }
    None
}

fn detect_browser_executable() -> Option<PathBuf> {
    std::env::var_os("ABIGAIL_BROWSER_EXECUTABLE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_existing_path(&candidate_msedge_paths()))
        .or_else(|| find_existing_path(&candidate_chrome_paths()))
}

fn candidate_msedge_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(&program_files).join("Microsoft/Edge/Application/msedge.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(&program_files_x86).join("Microsoft/Edge/Application/msedge.exe"),
        );
    }
    candidates
}

fn candidate_chrome_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(&program_files).join("Google/Chrome/Application/chrome.exe"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(&program_files_x86).join("Google/Chrome/Application/chrome.exe"),
        );
    }
    candidates
}

fn find_existing_path(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.is_file()).cloned()
}

fn normalize_tool_name(tool_name: &str) -> &str {
    match tool_name {
        "browser_navigate" => "navigate",
        "browser_click" => "click",
        "browser_type_text" => "type",
        "browser_screenshot" => "screenshot",
        "browser_evaluate_js" => "execute_js",
        other => other,
    }
}

fn triangle_ethic_token(tool_name: &str, params: &ToolParams) -> String {
    let mut canonical = serde_json::Map::new();
    for (key, value) in &params.values {
        if key != "triangle_ethic_token" {
            canonical.insert(key.clone(), value.clone());
        }
    }
    let payload = json!({
        "tool_name": normalize_tool_name(tool_name),
        "params": canonical,
    });
    let mut digest = Sha256::new();
    digest.update(payload.to_string().as_bytes());
    format!("{:x}", digest.finalize())
}

fn persist_session_record(profile_dir: &Path, record: &BrowserSessionRecord) -> Result<(), String> {
    std::fs::create_dir_all(profile_dir)
        .map_err(|err| format!("failed creating browser profile directory: {err}"))?;
    let payload = serde_json::to_string_pretty(record)
        .map_err(|err| format!("failed serializing browser session metadata: {err}"))?;
    std::fs::write(BrowserSkill::session_record_path(profile_dir), payload)
        .map_err(|err| format!("failed writing browser session metadata: {err}"))
}

fn persist_storage_state(profile_dir: &Path, state: &StorageState) -> Result<(), String> {
    std::fs::create_dir_all(profile_dir)
        .map_err(|err| format!("failed creating browser profile directory: {err}"))?;
    let payload = serde_json::to_string_pretty(state)
        .map_err(|err| format!("failed serializing browser storage state: {err}"))?;
    std::fs::write(storage_state_path(profile_dir), payload)
        .map_err(|err| format!("failed writing browser storage state: {err}"))
}

fn load_session_record(profile_dir: &Path) -> Option<BrowserSessionRecord> {
    let path = BrowserSkill::session_record_path(profile_dir);
    let payload = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&payload).ok()
}

fn load_storage_state(profile_dir: &Path) -> Result<Option<StorageState>, String> {
    let path = storage_state_path(profile_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let payload = std::fs::read_to_string(&path).map_err(|err| {
        format!(
            "failed reading browser storage state {}: {err}",
            path.display()
        )
    })?;
    let state = serde_json::from_str(&payload).map_err(|err| {
        format!(
            "failed parsing browser storage state {}: {err}",
            path.display()
        )
    })?;
    Ok(Some(state))
}

async fn snapshot_context_state(
    profile_dir: &Path,
    context: &PlaywrightBrowserContext,
) -> Result<(), String> {
    let storage_state = context
        .storage_state()
        .await
        .map_err(|err| format!("failed capturing browser storage state: {err}"))?;
    persist_storage_state(profile_dir, &storage_state)
}

async fn hydrate_context_from_snapshot(
    context: &PlaywrightBrowserContext,
    storage_state: &StorageState,
) -> Result<(), String> {
    if !storage_state.cookies.is_empty() {
        context
            .add_cookies(&storage_state.cookies)
            .await
            .map_err(|err| format!("failed restoring browser cookies: {err}"))?;
    }
    if !storage_state.origins.is_empty() {
        let by_origin = storage_state
            .origins
            .iter()
            .map(|origin| {
                let items = origin
                    .local_storage
                    .iter()
                    .map(|item| json!({ "name": item.name, "value": item.value }))
                    .collect::<Vec<_>>();
                (origin.origin.clone(), json!(items))
            })
            .collect::<serde_json::Map<String, serde_json::Value>>();
        let origin_json = serde_json::to_string(&by_origin)
            .map_err(|err| format!("failed serializing localStorage restore payload: {err}"))?;
        let script = format!(
            "(() => {{ const restored = {origin_json}; const items = restored[window.location.origin] || []; for (const item of items) {{ localStorage.setItem(item.name, item.value); }} }})()"
        );
        context
            .add_init_script(&script)
            .await
            .map_err(|err| format!("failed restoring browser localStorage init script: {err}"))?;
    }
    Ok(())
}

async fn wait_for_condition<F, Fut>(timeout_ms: u64, mut check: F) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, playwright_rs::Error>>,
{
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        if check().await.map_err(|err| err.to_string())? {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("timed out after {timeout_ms} ms"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

fn truncate_string(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let truncated = value
        .chars()
        .take(max_bytes.saturating_sub(32))
        .collect::<String>();
    format!("{truncated}...[truncated]")
}

fn format_browser_launch_error(message: String) -> String {
    if message.to_ascii_lowercase().contains("install") {
        return format!(
            "{message}. Install a compatible browser with `npx playwright@{} install chromium` or point `ABIGAIL_BROWSER_EXECUTABLE` at Chrome/Edge.",
            playwright_rs::PLAYWRIGHT_VERSION
        );
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses() {
        let manifest = BrowserSkill::default_manifest();
        assert_eq!(manifest.id.0, "com.abigail.skills.browser");
    }

    #[test]
    fn tool_inventory_includes_aliases_and_oauth() {
        let skill = BrowserSkill::new(BrowserSkill::default_manifest());
        let names = skill
            .tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"navigate".to_string()));
        assert!(names.contains(&"browser_navigate".to_string()));
        assert!(names.contains(&"execute_js".to_string()));
        assert!(names.contains(&"login_with_oauth".to_string()));
    }

    #[test]
    fn triangle_ethic_token_is_stable_for_same_payload() {
        let params = ToolParams::new()
            .with("selector", "#compose")
            .with("text", "hello");
        let left = triangle_ethic_token("type", &params);
        let right = triangle_ethic_token("browser_type_text", &params);
        assert_eq!(left, right);
    }

    #[test]
    fn discovers_profile_records_from_identity_root() {
        let temp_dir = tempfile::tempdir().unwrap();
        let profile_dir = temp_dir
            .path()
            .join("identities")
            .join("entity-123")
            .join("browser_profile");
        let record = BrowserSessionRecord {
            entity_id: Some("entity-123".to_string()),
            profile_dir: profile_dir.to_string_lossy().to_string(),
            active_in_process: false,
            last_used_at_utc: "2026-03-06T00:00:00Z".to_string(),
            last_action: Some("navigate".to_string()),
            current_url: Some("https://example.com".to_string()),
            page_title: Some("Example".to_string()),
            cookie_count: Some(1),
        };
        persist_session_record(&profile_dir, &record).unwrap();
        let sessions = discover_browser_sessions(temp_dir.path()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].entity_id.as_deref(), Some("entity-123"));
    }

    #[test]
    fn infers_gmail_webmail_profile_from_sender_address() {
        let request = WebmailSendRequest {
            profile_dir: PathBuf::from("C:/tmp/browser_profile"),
            entity_id: None,
            provider_hint: None,
            account_hint: None,
            sender_address: Some("mentor@gmail.com".to_string()),
            to: vec!["family@example.com".to_string()],
            subject: "Hello".to_string(),
            body: "World".to_string(),
        };

        let profile = infer_webmail_profile(&request).expect("gmail profile");
        assert_eq!(profile.provider, "gmail");
    }

    #[test]
    fn infers_outlook_webmail_profile_from_provider_hint() {
        let request = WebmailSendRequest {
            profile_dir: PathBuf::from("C:/tmp/browser_profile"),
            entity_id: None,
            provider_hint: Some("outlook.office.com".to_string()),
            account_hint: None,
            sender_address: None,
            to: vec!["family@example.com".to_string()],
            subject: "Hello".to_string(),
            body: "World".to_string(),
        };

        let profile = infer_webmail_profile(&request).expect("outlook profile");
        assert_eq!(profile.provider, "outlook");
    }
}
