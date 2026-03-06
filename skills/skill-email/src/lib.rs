//! Generic IMAP/SMTP email skill: implements Skill and EmailTransportCapability, wrapping abigail-skills transport layer.

mod transport;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::RwLock;

use abigail_skills::capability::email::{
    EmailTransportCapability, EmailTransportInfo, FetchOptions, OutgoingEmail,
};
use abigail_skills::channel::{
    publish_skill_event, SkillEvent, TriggerDescriptor, TriggerFrequency, TriggerPriority,
};
use abigail_skills::manifest::{
    CapabilityDescriptor, NetworkPermission, Permission, SkillId, SkillManifest,
};
use abigail_skills::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use abigail_skills::transport::imap::ImapTlsMode;
use abigail_skills::transport::smtp::SmtpTlsMode;
use abigail_skills::transport::{ImapClient, SmtpClient};

use crate::transport::EmailTransport;

/// Default skill ID for the generic email skill.
pub const EMAIL_SKILL_ID: &str = "com.abigail.skills.email";

/// Generic email skill: Skill + EmailTransportCapability.
pub struct EmailSkill {
    manifest: SkillManifest,
    transport: Option<Arc<RwLock<EmailTransport>>>,
    stream_broker: Option<Arc<dyn abigail_streaming::StreamBroker>>,
    health_state: StdRwLock<SkillHealth>,
}

impl EmailSkill {
    fn set_health(&self, status: HealthStatus, message: Option<String>) {
        if let Ok(mut guard) = self.health_state.write() {
            *guard = SkillHealth {
                status,
                message,
                last_check: chrono::Utc::now(),
                metrics: HashMap::new(),
            };
        }
    }

    fn current_health_message(&self) -> String {
        self.health_state
            .read()
            .ok()
            .and_then(|health| health.message.clone())
            .unwrap_or_else(|| "Skill not initialized".to_string())
    }
}

impl EmailSkill {
    /// Build manifest from embedded skill.toml or default in code.
    pub fn default_manifest() -> SkillManifest {
        SkillManifest::parse(include_str!("../skill.toml"))
            .unwrap_or_else(|_| Self::fallback_manifest())
    }

    fn fallback_manifest() -> SkillManifest {
        SkillManifest {
            id: SkillId(EMAIL_SKILL_ID.to_string()),
            name: "Email".to_string(),
            version: "0.1.0".to_string(),
            description: "Email via IMAP/SMTP with any compatible server.".to_string(),
            license: None,
            category: "Communication".to_string(),
            keywords: vec!["email".into(), "imap".into(), "smtp".into(), "inbox".into()],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["Windows".into(), "macOS".into(), "Linux".into()],
            capabilities: vec![CapabilityDescriptor {
                capability_type: "email_transport".to_string(),
                version: "1.0".to_string(),
            }],
            permissions: vec![Permission::Network(NetworkPermission::Full)],
            secrets: vec![
                abigail_skills::SecretDescriptor {
                    name: "imap_password".to_string(),
                    description: "App password for IMAP".to_string(),
                    required: true,
                },
                abigail_skills::SecretDescriptor {
                    name: "imap_user".to_string(),
                    description: "IMAP username / email address".to_string(),
                    required: true,
                },
                abigail_skills::SecretDescriptor {
                    name: "imap_host".to_string(),
                    description: "IMAP server hostname".to_string(),
                    required: true,
                },
                abigail_skills::SecretDescriptor {
                    name: "smtp_host".to_string(),
                    description: "SMTP server hostname".to_string(),
                    required: false,
                },
                abigail_skills::SecretDescriptor {
                    name: "smtp_user".to_string(),
                    description: "SMTP username / email address".to_string(),
                    required: false,
                },
                abigail_skills::SecretDescriptor {
                    name: "smtp_password".to_string(),
                    description: "SMTP password".to_string(),
                    required: false,
                },
            ],
            config_defaults: HashMap::new(),
        }
    }

    pub fn new(manifest: SkillManifest) -> Self {
        Self {
            manifest,
            transport: None,
            stream_broker: None,
            health_state: StdRwLock::new(SkillHealth {
                status: HealthStatus::Unknown,
                message: Some("Email skill not initialized yet".to_string()),
                last_check: chrono::Utc::now(),
                metrics: HashMap::new(),
            }),
        }
    }

    fn tool_fetch_emails() -> ToolDescriptor {
        ToolDescriptor {
            name: "fetch_emails".to_string(),
            description:
                "Fetch emails from INBOX. Set unread_only to false for all mail (read + unread)."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max emails to fetch", "default": 50 },
                    "unread_only": { "type": "boolean", "description": "If true, only unread; if false, all mail in INBOX", "default": true }
                }
            }),
            returns: serde_json::json!({ "type": "array", "items": { "type": "object" } }),
            cost_estimate: CostEstimate {
                latency_ms: 2000,
                network_bound: true,
                token_cost: None,
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Full)],
            autonomous: true,
            requires_confirmation: false,
        }
    }

    fn tool_send_email() -> ToolDescriptor {
        ToolDescriptor {
            name: "send_email".to_string(),
            description: "Send an email via SMTP.".to_string(),
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
            required_permissions: vec![Permission::Network(NetworkPermission::Full)],
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
}

#[async_trait::async_trait]
impl Skill for EmailSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, config: SkillConfig) -> SkillResult<()> {
        self.stream_broker = config.stream_broker.clone();
        self.transport = None;
        let host = config
            .values
            .get("imap_host")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| config.secrets.get("imap_host").cloned());
        let port = config
            .values
            .get("imap_port")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                config
                    .secrets
                    .get("imap_port")
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(993) as u16;
        let user = config
            .values
            .get("imap_user")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| config.secrets.get("imap_user").cloned())
            .unwrap_or_default()
            .trim()
            .to_string();
        let password = config
            .secrets
            .get("imap_password")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_string();

        let mut missing = Vec::new();
        let host = match host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()) {
            Some(host) => host,
            None => {
                missing.push("imap_host");
                String::new()
            }
        };
        if user.is_empty() {
            missing.push("imap_user");
        }
        if password.is_empty() {
            missing.push("imap_password");
        }
        if !missing.is_empty() {
            let msg = format!("Missing IMAP configuration: {}", missing.join(", "));
            self.set_health(HealthStatus::Unhealthy, Some(msg.clone()));
            return Err(SkillError::InitFailed(msg));
        }

        let tls_mode = config
            .values
            .get("imap_tls_mode")
            .and_then(|v| v.as_str())
            .or_else(|| config.secrets.get("imap_tls_mode").map(|v| v.as_str()))
            .map(|s| match s.to_uppercase().as_str() {
                "STARTTLS" => ImapTlsMode::StartTls,
                _ => ImapTlsMode::Implicit,
            })
            .unwrap_or(ImapTlsMode::Implicit);

        let imap = ImapClient::new(&host, port, &user, &password).with_tls_mode(tls_mode);
        if let Err(err) = imap.test_connection().await {
            let msg = format!("IMAP connection failed: {}", err);
            self.set_health(HealthStatus::Unhealthy, Some(msg.clone()));
            return Err(SkillError::InitFailed(msg));
        }

        let smtp_host = config
            .values
            .get("smtp_host")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| config.secrets.get("smtp_host").cloned())
            .unwrap_or_default()
            .trim()
            .to_string();
        let smtp_port = config
            .values
            .get("smtp_port")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                config
                    .secrets
                    .get("smtp_port")
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .unwrap_or(587) as u16;
        let smtp_user = config
            .values
            .get("smtp_user")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| config.secrets.get("smtp_user").cloned())
            .unwrap_or_else(|| user.clone())
            .trim()
            .to_string();
        let smtp_password = config
            .secrets
            .get("smtp_password")
            .cloned()
            .or_else(|| config.secrets.get("imap_password").cloned())
            .unwrap_or_default()
            .trim()
            .to_string();
        let smtp_tls_mode = config
            .values
            .get("smtp_tls_mode")
            .and_then(|v| v.as_str())
            .or_else(|| config.secrets.get("smtp_tls_mode").map(|v| v.as_str()))
            .map(|s| match s.to_uppercase().as_str() {
                "IMPLICIT" => SmtpTlsMode::Implicit,
                _ => SmtpTlsMode::StartTls,
            })
            .unwrap_or_else(|| {
                if tls_mode == ImapTlsMode::Implicit && smtp_port == 465 {
                    SmtpTlsMode::Implicit
                } else {
                    SmtpTlsMode::StartTls
                }
            });

        let mut smtp = None;
        let mut health_status = HealthStatus::Healthy;
        let mut health_message = Some("Email skill initialized (IMAP + SMTP ready)".to_string());
        if smtp_host.is_empty() {
            health_status = HealthStatus::Degraded;
            health_message = Some("IMAP ready, but SMTP disabled: missing smtp_host".to_string());
        } else if smtp_user.is_empty() || smtp_password.is_empty() {
            health_status = HealthStatus::Degraded;
            health_message = Some(
                "IMAP ready, but SMTP disabled: missing smtp_user or smtp_password".to_string(),
            );
        } else {
            let smtp_client = SmtpClient::new(&smtp_host, smtp_port, &smtp_user, &smtp_password)
                .with_tls_mode(smtp_tls_mode);
            if let Err(err) = smtp_client.test_connection().await {
                health_status = HealthStatus::Degraded;
                health_message = Some(format!("IMAP ready, SMTP connection failed: {}", err));
            } else {
                smtp = Some(smtp_client);
            }
        }

        let transport = EmailTransport::new(Some(imap), smtp, &smtp_user);
        self.transport = Some(Arc::new(RwLock::new(transport)));
        self.set_health(health_status, health_message);
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        self.transport = None;
        self.set_health(
            HealthStatus::Unknown,
            Some("Email skill shut down".to_string()),
        );
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        self.health_state
            .read()
            .map(|health| health.clone())
            .unwrap_or(SkillHealth {
                status: HealthStatus::Unknown,
                message: Some("Email skill health state unavailable".to_string()),
                last_check: chrono::Utc::now(),
                metrics: HashMap::new(),
            })
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            Self::tool_fetch_emails(),
            Self::tool_send_email(),
            Self::tool_classify_importance(),
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
            .ok_or_else(|| SkillError::InitFailed(self.current_health_message()))?;

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
                if let Some(ref broker) = self.stream_broker {
                    let event = SkillEvent {
                        skill_id: self.manifest.id.clone(),
                        trigger: "email_received".to_string(),
                        payload: serde_json::json!({ "count": count, "first_id": first_id }),
                        timestamp: chrono::Utc::now(),
                        priority: TriggerPriority::Normal,
                    };
                    publish_skill_event(broker, event).await;
                }
                Ok(out)
            }
            "send_email" => {
                let to_raw = params
                    .get::<serde_json::Value>("to")
                    .unwrap_or(serde_json::Value::Array(vec![]));
                let to_addrs: Vec<abigail_skills::capability::email::EmailAddress> = match to_raw {
                    serde_json::Value::Array(arr) => arr
                        .into_iter()
                        .filter_map(|v| {
                            let email = v
                                .get("email")
                                .and_then(|e| e.as_str())
                                .map(String::from)
                                .or_else(|| v.as_str().map(String::from))?;
                            let name = v.get("name").and_then(|n| n.as_str()).map(String::from);
                            Some(abigail_skills::capability::email::EmailAddress { email, name })
                        })
                        .collect(),
                    serde_json::Value::String(s) => {
                        vec![abigail_skills::capability::email::EmailAddress {
                            email: s,
                            name: None,
                        }]
                    }
                    _ => vec![],
                };
                let subject = params.get::<String>("subject").unwrap_or_default();
                let body = params.get::<String>("body").unwrap_or_default();

                if to_addrs.is_empty() {
                    return Ok(ToolOutput::error("No valid recipients in 'to' field"));
                }

                let outgoing = OutgoingEmail {
                    to: to_addrs,
                    subject,
                    body,
                };
                let guard = transport.write().await;
                let result = guard.send_email(outgoing).await?;
                Ok(ToolOutput::success(serde_json::json!({
                    "success": true,
                    "message_id": result.message_id,
                })))
            }
            "classify_importance" => {
                let _email_id = params.get::<String>("email_id").unwrap_or_default();
                Ok(ToolOutput::success(serde_json::json!("normal")))
            }
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
impl EmailTransportCapability for EmailSkill {
    fn info(&self) -> EmailTransportInfo {
        EmailTransportInfo {
            id: EMAIL_SKILL_ID.to_string(),
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
            .ok_or_else(|| SkillError::InitFailed(self.current_health_message()))?;
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
            .ok_or_else(|| SkillError::InitFailed(self.current_health_message()))?;
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
