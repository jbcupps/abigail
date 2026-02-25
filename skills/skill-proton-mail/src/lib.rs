//! Proton Mail–style skill: implements Skill and EmailTransportCapability, wrapping abigail-senses IMAP/SMTP.

mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use abigail_skills::capability::email::{
    EmailTransportCapability, EmailTransportInfo, FetchOptions, OutgoingEmail,
};
use abigail_skills::channel::{SkillEvent, TriggerDescriptor, TriggerFrequency, TriggerPriority};
use abigail_skills::manifest::{
    CapabilityDescriptor, NetworkPermission, Permission, SkillId, SkillManifest,
};
use abigail_skills::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use abigail_skills::transport::imap::ImapTlsMode;
use abigail_skills::transport::ImapClient;

use crate::transport::ProtonMailTransport;

/// Default skill ID for Proton Mail.
pub const PROTON_MAIL_SKILL_ID: &str = "com.abigail.skills.proton-mail";

/// Proton Mail skill: Skill + EmailTransportCapability.
pub struct ProtonMailSkill {
    manifest: SkillManifest,
    transport: Option<Arc<RwLock<ProtonMailTransport>>>,
    event_sender: Option<std::sync::Arc<tokio::sync::broadcast::Sender<SkillEvent>>>,
}

impl ProtonMailSkill {
    /// Build manifest from embedded skill.toml or default in code.
    pub fn default_manifest() -> SkillManifest {
        SkillManifest::parse(include_str!("../skill.toml"))
            .unwrap_or_else(|_| Self::fallback_manifest())
    }

    fn fallback_manifest() -> SkillManifest {
        SkillManifest {
            id: SkillId(PROTON_MAIL_SKILL_ID.to_string()),
            name: "Proton Mail".to_string(),
            version: "0.1.0".to_string(),
            description: "Proton Mail–style email via IMAP/SMTP.".to_string(),
            license: None,
            category: "Communication".to_string(),
            keywords: vec![
                "email".into(),
                "proton".into(),
                "imap".into(),
                "smtp".into(),
            ],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["Windows".into(), "macOS".into(), "Linux".into()],
            capabilities: vec![CapabilityDescriptor {
                capability_type: "email_transport".to_string(),
                version: "1.0".to_string(),
            }],
            permissions: vec![Permission::Network(NetworkPermission::Domains(vec![
                "mail.proton.me".into(),
                "smtp.proton.me".into(),
            ]))],
            secrets: vec![abigail_skills::SecretDescriptor {
                name: "imap_password".to_string(),
                description: "App password for IMAP".to_string(),
                required: true,
            }],
            config_defaults: HashMap::new(),
        }
    }

    pub fn new(manifest: SkillManifest) -> Self {
        Self {
            manifest,
            transport: None,
            event_sender: None,
        }
    }

    fn tool_fetch_emails() -> ToolDescriptor {
        ToolDescriptor {
            name: "fetch_emails".to_string(),
            description: "Fetch unread emails from INBOX.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max emails to fetch", "default": 50 },
                    "unread_only": { "type": "boolean", "default": true }
                }
            }),
            returns: serde_json::json!({ "type": "array", "items": { "type": "object" } }),
            cost_estimate: CostEstimate {
                latency_ms: 2000,
                network_bound: true,
                token_cost: None,
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Domains(vec![
                "mail.proton.me".into(),
            ]))],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    fn tool_send_email() -> ToolDescriptor {
        ToolDescriptor {
            name: "send_email".to_string(),
            description: "Send an email (stub; not yet implemented).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": { "type": "array", "items": { "type": "object" } },
                    "subject": { "type": "string" },
                    "body": { "type": "string" }
                },
                "required": ["to", "subject", "body"]
            }),
            returns: serde_json::json!({ "type": "object" }),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![Permission::Network(NetworkPermission::Domains(vec![
                "smtp.proton.me".into(),
            ]))],
            autonomous: false,
            requires_confirmation: true,
        }
    }

    fn tool_classify_importance() -> ToolDescriptor {
        ToolDescriptor {
            name: "classify_importance".to_string(),
            description: "Classify email importance (stub; rule-based or future LLM).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "email_id": { "type": "string" } }
            }),
            returns: serde_json::json!({ "type": "string", "enum": ["low", "normal", "high"] }),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    fn tool_create_filter() -> ToolDescriptor {
        ToolDescriptor {
            name: "create_filter".to_string(),
            description: "Create a filter rule (stub; not yet implemented).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "criteria": { "type": "object" }
                }
            }),
            returns: serde_json::json!({ "type": "object" }),
            cost_estimate: CostEstimate::default(),
            required_permissions: vec![],
            autonomous: false,
            requires_confirmation: true,
        }
    }
}

#[async_trait::async_trait]
impl Skill for ProtonMailSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, config: SkillConfig) -> SkillResult<()> {
        self.event_sender = config.event_sender.clone();
        let host = config
            .values
            .get("imap_host")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| "mail.proton.me".to_string());
        let port = config
            .values
            .get("imap_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(993) as u16;
        let user = config
            .values
            .get("imap_user")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| config.secrets.get("imap_user").cloned())
            .unwrap_or_default();
        let password = config
            .secrets
            .get("imap_password")
            .cloned()
            .unwrap_or_default();

        if user.is_empty() || password.is_empty() {
            return Err(SkillError::InitFailed(
                "imap_user and imap_password (secret) required".to_string(),
            ));
        }

        let tls_mode = config
            .values
            .get("imap_tls_mode")
            .and_then(|v| v.as_str())
            .map(|s| match s.to_uppercase().as_str() {
                "STARTTLS" => ImapTlsMode::StartTls,
                _ => ImapTlsMode::Implicit,
            })
            .unwrap_or(ImapTlsMode::Implicit);

        let imap = ImapClient::new(&host, port, &user, &password).with_tls_mode(tls_mode);
        imap.test_connection()
            .await
            .map_err(|e| SkillError::InitFailed(e.to_string()))?;

        let transport = ProtonMailTransport::new(Some(imap), None);
        self.transport = Some(Arc::new(RwLock::new(transport)));
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        self.transport = None;
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let status = if self.transport.is_some() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unknown
        };
        SkillHealth {
            status,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            Self::tool_fetch_emails(),
            Self::tool_send_email(),
            Self::tool_classify_importance(),
            Self::tool_create_filter(),
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| SkillError::InitFailed("Skill not initialized".to_string()))?;

        match tool_name {
            "fetch_emails" => {
                let limit = params.get::<u64>("limit").unwrap_or(50);
                let unread_only = params.get::<bool>("unread_only").unwrap_or(true);
                let options = FetchOptions {
                    folder: None,
                    limit: Some(limit as u32),
                    unread_only,
                };
                let guard = transport.write().await;
                let emails = guard.fetch_emails(options).await?;
                let count = emails.len();
                let first_id = emails.first().map(|e| e.id.clone());
                let out = ToolOutput::success(
                    emails
                        .into_iter()
                        .map(|e| serde_json::json!({ "id": e.id, "from": e.from.email, "subject": e.subject, "date": e.date.to_rfc3339() }))
                        .collect::<Vec<_>>(),
                );
                if let Some(ref sender) = self.event_sender {
                    let _ = sender.send(SkillEvent {
                        skill_id: self.manifest.id.clone(),
                        trigger: "email_received".to_string(),
                        payload: serde_json::json!({ "count": count, "first_id": first_id }),
                        timestamp: chrono::Utc::now(),
                        priority: TriggerPriority::Normal,
                    });
                }
                Ok(out)
            }
            "send_email" => Ok(ToolOutput::error("send_email not yet implemented")),
            "classify_importance" => {
                let _email_id = params.get::<String>("email_id").unwrap_or_default();
                Ok(ToolOutput::success(serde_json::json!("normal")))
            }
            "create_filter" => Ok(ToolOutput::error("create_filter not yet implemented")),
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![CapabilityDescriptor {
            capability_type: "email_transport".to_string(),
            version: "1.0".to_string(),
        }]
    }

    fn get_capability(&self, cap_type: &str) -> Option<&dyn std::any::Any> {
        if cap_type == "email_transport" {
            Some(self)
        } else {
            None
        }
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![
            TriggerDescriptor {
                name: "email_received".to_string(),
                description: "Fired when new email is received.".to_string(),
                payload_schema: serde_json::json!({ "email_id": "string" }),
                frequency: TriggerFrequency::Occasional,
                priority: TriggerPriority::Normal,
            },
            TriggerDescriptor {
                name: "important_email".to_string(),
                description: "Fired when an important email is detected.".to_string(),
                payload_schema: serde_json::json!({ "email_id": "string" }),
                frequency: TriggerFrequency::Rare,
                priority: TriggerPriority::High,
            },
        ]
    }
}

#[async_trait::async_trait]
impl EmailTransportCapability for ProtonMailSkill {
    fn info(&self) -> EmailTransportInfo {
        EmailTransportInfo {
            id: PROTON_MAIL_SKILL_ID.to_string(),
            name: self.manifest.name.clone(),
        }
    }

    async fn connect(&mut self) -> SkillResult<()> {
        if let Some(ref t) = self.transport {
            t.read().await.test_connection().await
        } else {
            Err(SkillError::InitFailed(
                "Transport not initialized".to_string(),
            ))
        }
    }

    async fn disconnect(&mut self) -> SkillResult<()> {
        Ok(())
    }

    async fn fetch_emails(
        &self,
        options: FetchOptions,
    ) -> SkillResult<Vec<abigail_skills::capability::email::Email>> {
        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| SkillError::InitFailed("Skill not initialized".to_string()))?;
        let guard = transport.read().await;
        guard.fetch_emails(options).await
    }

    async fn send_email(
        &self,
        email: OutgoingEmail,
    ) -> SkillResult<abigail_skills::capability::email::SendResult> {
        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| SkillError::InitFailed("Skill not initialized".to_string()))?;
        let guard = transport.read().await;
        guard.send_email(email).await
    }

    async fn move_email(&self, _email_id: &str, _folder: &str) -> SkillResult<()> {
        Err(SkillError::ToolFailed(
            "move_email not yet implemented".to_string(),
        ))
    }

    async fn delete_email(&self, _email_id: &str) -> SkillResult<()> {
        Err(SkillError::ToolFailed(
            "delete_email not yet implemented".to_string(),
        ))
    }
}
