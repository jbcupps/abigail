use abigail_skills::channel::SkillEvent;
use abigail_skills::skill::{ExecutionContext, HealthStatus, Skill, SkillConfig, ToolParams};
use abigail_streaming::{MemoryBroker, StreamBroker, TopicConfig};
use skill_email::{EmailSkill, EMAIL_SKILL_ID};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn env_or_empty(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

struct LiveBridgeEnv {
    imap_host: String,
    imap_port: u16,
    imap_user: String,
    imap_password: String,
    imap_tls_mode: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_user: String,
    smtp_password: String,
    smtp_tls_mode: String,
}

impl LiveBridgeEnv {
    fn load() -> Option<Self> {
        if std::env::var("ABIGAIL_EMAIL_BRIDGE_TEST").map_or(true, |v| v != "1") {
            eprintln!("ABIGAIL_EMAIL_BRIDGE_TEST not set to 1; skipping live bridge test");
            return None;
        }

        let imap_host = env_or_empty("ABIGAIL_IMAP_HOST");
        let imap_user = env_or_empty("ABIGAIL_IMAP_USER");
        let imap_password = env_or_empty("ABIGAIL_IMAP_PASS");
        let smtp_host = env_or_empty("ABIGAIL_SMTP_HOST");

        if imap_host.is_empty()
            || imap_user.is_empty()
            || imap_password.is_empty()
            || smtp_host.is_empty()
        {
            eprintln!("Missing ABIGAIL_IMAP_* or ABIGAIL_SMTP_HOST env vars; skipping");
            return None;
        }

        Some(Self {
            imap_host,
            imap_port: env_or_empty("ABIGAIL_IMAP_PORT").parse().unwrap_or(993),
            imap_user: imap_user.clone(),
            imap_password: imap_password.clone(),
            imap_tls_mode: {
                let mode = env_or_empty("ABIGAIL_IMAP_TLS_MODE");
                if mode.is_empty() {
                    "IMPLICIT".to_string()
                } else {
                    mode
                }
            },
            smtp_host,
            smtp_port: env_or_empty("ABIGAIL_SMTP_PORT").parse().unwrap_or(587),
            smtp_user: {
                let user = env_or_empty("ABIGAIL_SMTP_USER");
                if user.is_empty() {
                    imap_user
                } else {
                    user
                }
            },
            smtp_password: {
                let pass = env_or_empty("ABIGAIL_SMTP_PASS");
                if pass.is_empty() {
                    imap_password
                } else {
                    pass
                }
            },
            smtp_tls_mode: {
                let mode = env_or_empty("ABIGAIL_SMTP_TLS_MODE");
                if mode.is_empty() {
                    "STARTTLS".to_string()
                } else {
                    mode
                }
            },
        })
    }

    fn skill_config(&self, stream_broker: Arc<dyn StreamBroker>) -> SkillConfig {
        let mut values = HashMap::new();
        values.insert(
            "imap_host".to_string(),
            serde_json::Value::String(self.imap_host.clone()),
        );
        values.insert(
            "imap_port".to_string(),
            serde_json::json!(u64::from(self.imap_port)),
        );
        values.insert(
            "imap_user".to_string(),
            serde_json::Value::String(self.imap_user.clone()),
        );
        values.insert(
            "imap_tls_mode".to_string(),
            serde_json::Value::String(self.imap_tls_mode.clone()),
        );
        values.insert(
            "smtp_host".to_string(),
            serde_json::Value::String(self.smtp_host.clone()),
        );
        values.insert(
            "smtp_port".to_string(),
            serde_json::json!(u64::from(self.smtp_port)),
        );
        values.insert(
            "smtp_user".to_string(),
            serde_json::Value::String(self.smtp_user.clone()),
        );
        values.insert(
            "smtp_tls_mode".to_string(),
            serde_json::Value::String(self.smtp_tls_mode.clone()),
        );

        let mut secrets = HashMap::new();
        secrets.insert("imap_password".to_string(), self.imap_password.clone());
        secrets.insert("smtp_password".to_string(), self.smtp_password.clone());

        SkillConfig {
            values,
            secrets,
            limits: Default::default(),
            permissions: vec![],
            stream_broker: Some(stream_broker),
        }
    }
}

#[tokio::test]
async fn live_bridge_init_and_fetch_emits_event() {
    let Some(env) = LiveBridgeEnv::load() else {
        return;
    };

    let broker: Arc<dyn StreamBroker> = Arc::new(MemoryBroker::new(32));
    broker
        .ensure_topic("abigail", "skill-events", TopicConfig::default())
        .await
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let subscription = broker
        .subscribe(
            "abigail",
            "skill-events",
            "skill-email-live-bridge",
            Box::new(move |msg| {
                let tx = tx.clone();
                Box::pin(async move {
                    let _ = tx.send(msg).await;
                })
            }),
        )
        .await
        .unwrap();

    let mut skill = EmailSkill::new(EmailSkill::default_manifest());
    tokio::time::timeout(
        Duration::from_secs(30),
        skill.initialize(env.skill_config(broker.clone())),
    )
    .await
    .expect("timed out initializing email skill against the live IMAP/SMTP bridge")
    .expect("email skill should initialize against the live IMAP/SMTP bridge");

    let health = skill.health();
    assert_eq!(
        health.status,
        HealthStatus::Healthy,
        "expected healthy IMAP+SMTP bridge, got {:?}: {:?}",
        health.status,
        health.message
    );

    let output = tokio::time::timeout(
        Duration::from_secs(30),
        skill.execute_tool(
            "fetch_emails",
            ToolParams::new()
                .with("limit", 5u64)
                .with("unread_only", true),
            &ExecutionContext {
                request_id: "live-bridge-test".to_string(),
                user_id: None,
            },
        ),
    )
    .await
    .expect("timed out fetching emails from the live IMAP bridge")
    .expect("fetch_emails should run against the live IMAP bridge");

    assert!(
        output.success,
        "fetch_emails returned success=false: {:?}",
        output
    );

    let message = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for email_received event")
        .expect("skill-events subscription closed before event");

    let event: SkillEvent = serde_json::from_slice(&message.payload).expect("valid skill event");
    assert_eq!(event.skill_id.0, EMAIL_SKILL_ID);
    assert_eq!(event.trigger, "email_received");

    subscription.cancel();
}
