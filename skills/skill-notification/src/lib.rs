//! Notification skill: send desktop notifications and schedule timed alerts.
//!
//! Provides three tools:
//! - `send_notification`: Send an immediate desktop notification
//! - `schedule_notification`: Schedule a notification after a delay
//! - `cancel_scheduled`: Cancel a previously scheduled notification

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, Permission, Skill,
    SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult, ToolDescriptor, ToolOutput,
    ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Desktop notification skill with scheduling support.
pub struct NotificationSkill {
    manifest: SkillManifest,
    /// Map of schedule_id -> JoinHandle for scheduled notifications.
    scheduled: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl NotificationSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse notification skill.toml")
    }

    /// Create a new notification skill.
    pub fn new(manifest: SkillManifest) -> Self {
        Self {
            manifest,
            scheduled: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send an immediate desktop notification.
    fn send_notification(title: &str, body: &str, timeout_secs: u64) -> SkillResult<ToolOutput> {
        let ms = timeout_secs.saturating_mul(1000);

        let result = notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .timeout(notify_rust::Timeout::Milliseconds(ms as u32))
            .show();

        match result {
            Ok(_) => {
                tracing::info!("Notification sent: {} - {}", title, body);
                Ok(ToolOutput::success(serde_json::json!({
                    "formatted": format!("Notification sent: {}", title),
                    "title": title,
                    "body": body,
                    "timeout_secs": timeout_secs,
                })))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to send notification: {}",
                e
            ))),
        }
    }

    /// Schedule a notification to be sent after a delay.
    fn schedule_notification(
        &self,
        title: String,
        body: String,
        delay_secs: u64,
    ) -> SkillResult<ToolOutput> {
        let schedule_id = uuid::Uuid::new_v4().to_string();
        let scheduled = Arc::clone(&self.scheduled);
        let sid = schedule_id.clone();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

            let result = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .timeout(notify_rust::Timeout::Milliseconds(5000))
                .show();

            match result {
                Ok(_) => tracing::info!("Scheduled notification fired: {} - {}", title, body),
                Err(e) => tracing::error!("Scheduled notification failed: {}", e),
            }

            // Clean up the handle from the map after firing.
            if let Ok(mut map) = scheduled.lock() {
                map.remove(&sid);
            }
        });

        if let Ok(mut map) = self.scheduled.lock() {
            map.insert(schedule_id.clone(), handle);
        }

        tracing::info!(
            "Notification scheduled (id={}) in {} seconds",
            schedule_id,
            delay_secs
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Notification scheduled (id={}) to fire in {} seconds", schedule_id, delay_secs),
            "schedule_id": schedule_id,
            "delay_secs": delay_secs,
        })))
    }

    /// Cancel a previously scheduled notification.
    fn cancel_scheduled(&self, schedule_id: &str) -> SkillResult<ToolOutput> {
        let mut map = self
            .scheduled
            .lock()
            .map_err(|e| SkillError::ToolFailed(format!("Lock poisoned: {}", e)))?;

        if let Some(handle) = map.remove(schedule_id) {
            handle.abort();
            tracing::info!("Cancelled scheduled notification: {}", schedule_id);
            Ok(ToolOutput::success(serde_json::json!({
                "formatted": format!("Cancelled scheduled notification: {}", schedule_id),
                "schedule_id": schedule_id,
                "cancelled": true,
            })))
        } else {
            Ok(ToolOutput::error(format!(
                "No scheduled notification found with id: {}",
                schedule_id
            )))
        }
    }
}

#[async_trait]
impl Skill for NotificationSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        // Cancel all pending scheduled notifications on shutdown.
        if let Ok(mut map) = self.scheduled.lock() {
            for (id, handle) in map.drain() {
                handle.abort();
                tracing::info!("Shutdown: cancelled scheduled notification {}", id);
            }
        }
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
                name: "send_notification".to_string(),
                description: "Send an immediate desktop notification to the user.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The notification title"
                        },
                        "body": {
                            "type": "string",
                            "description": "The notification body text"
                        },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "How long to display the notification in seconds (default 5)"
                        }
                    },
                    "required": ["title", "body"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "timeout_secs": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 100,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Notifications],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "schedule_notification".to_string(),
                description: "Schedule a desktop notification to be sent after a delay."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The notification title"
                        },
                        "body": {
                            "type": "string",
                            "description": "The notification body text"
                        },
                        "delay_secs": {
                            "type": "integer",
                            "description": "Seconds to wait before sending the notification"
                        }
                    },
                    "required": ["title", "body", "delay_secs"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "schedule_id": { "type": "string" },
                        "delay_secs": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Notifications],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "cancel_scheduled".to_string(),
                description: "Cancel a previously scheduled notification by its schedule ID."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "schedule_id": {
                            "type": "string",
                            "description": "The ID returned by schedule_notification"
                        }
                    },
                    "required": ["schedule_id"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "schedule_id": { "type": "string" },
                        "cancelled": { "type": "boolean" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Notifications],
                autonomous: true,
                requires_confirmation: false,
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
            "send_notification" => {
                let title: String = params.get("title").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: title".to_string())
                })?;
                let body: String = params.get("body").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: body".to_string())
                })?;
                let timeout_secs: u64 = params.get("timeout_secs").unwrap_or(5);

                Self::send_notification(&title, &body, timeout_secs)
            }
            "schedule_notification" => {
                let title: String = params.get("title").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: title".to_string())
                })?;
                let body: String = params.get("body").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: body".to_string())
                })?;
                let delay_secs: u64 = params.get("delay_secs").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: delay_secs".to_string())
                })?;

                self.schedule_notification(title, body, delay_secs)
            }
            "cancel_scheduled" => {
                let schedule_id: String = params.get("schedule_id").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: schedule_id".to_string())
                })?;

                self.cancel_scheduled(&schedule_id)
            }
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
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

    fn test_skill() -> NotificationSkill {
        NotificationSkill::new(NotificationSkill::default_manifest())
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = NotificationSkill::default_manifest();
        assert_eq!(manifest.name, "Notification");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "send_notification");
        assert_eq!(tools[1].name, "schedule_notification");
        assert_eq!(tools[2].name, "cancel_scheduled");

        // All tools are autonomous and do not require confirmation.
        for tool in &tools {
            assert!(tool.autonomous);
            assert!(!tool.requires_confirmation);
            assert_eq!(tool.required_permissions, vec![Permission::Notifications]);
        }
    }
}
