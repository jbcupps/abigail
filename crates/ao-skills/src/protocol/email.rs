//! Email transport capability trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::SkillResult;

#[derive(Debug, Clone)]
pub struct EmailTransportInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAddress {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub id: String,
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: Option<String>,
    pub date: chrono::DateTime<chrono::Utc>,
    pub is_read: bool,
}

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub folder: Option<String>,
    pub limit: Option<u32>,
    pub unread_only: bool,
}

#[derive(Debug, Clone)]
pub struct OutgoingEmail {
    pub to: Vec<EmailAddress>,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct SendResult {
    pub message_id: Option<String>,
}

#[async_trait]
pub trait EmailTransportCapability: Send + Sync {
    fn info(&self) -> EmailTransportInfo;
    async fn connect(&mut self) -> SkillResult<()>;
    async fn disconnect(&mut self) -> SkillResult<()>;
    async fn fetch_emails(&self, options: FetchOptions) -> SkillResult<Vec<Email>>;
    async fn send_email(&self, email: OutgoingEmail) -> SkillResult<SendResult>;
    async fn move_email(&self, email_id: &str, folder: &str) -> SkillResult<()>;
    async fn delete_email(&self, email_id: &str) -> SkillResult<()>;
}
