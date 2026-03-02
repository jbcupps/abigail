//! Thin wrapper over abigail-skills ImapClient/SmtpClient for email operations.

use abigail_skills::capability::email::{
    Email, EmailAddress, FetchOptions, OutgoingEmail, SendResult,
};
use abigail_skills::transport::{ImapClient, SmtpClient};
use abigail_skills::{SkillError, SkillResult};

/// Transport state: IMAP and optional SMTP. Sessions are not persisted; each fetch connects fresh.
pub struct EmailTransport {
    pub imap: Option<ImapClient>,
    pub smtp: Option<SmtpClient>,
    pub from_address: String,
}

impl EmailTransport {
    pub fn new(imap: Option<ImapClient>, smtp: Option<SmtpClient>, from_address: &str) -> Self {
        Self {
            imap,
            smtp,
            from_address: from_address.to_string(),
        }
    }

    pub async fn test_connection(&self) -> SkillResult<()> {
        let imap = self
            .imap
            .as_ref()
            .ok_or_else(|| SkillError::InitFailed("IMAP not configured".to_string()))?;
        imap.test_connection()
            .await
            .map_err(|e| SkillError::InitFailed(e.to_string()))
    }

    pub async fn fetch_emails(&self, options: FetchOptions) -> SkillResult<Vec<Email>> {
        let imap = self
            .imap
            .as_ref()
            .ok_or_else(|| SkillError::InitFailed("IMAP not configured".to_string()))?;
        let limit = options.limit.unwrap_or(50);
        let summaries = if options.unread_only {
            imap.fetch_unread()
                .await
                .map_err(|e| SkillError::ToolFailed(e.to_string()))?
        } else {
            imap.fetch_all(limit)
                .await
                .map_err(|e| SkillError::ToolFailed(e.to_string()))?
        };
        let emails: Vec<Email> = summaries
            .into_iter()
            .take(limit as usize)
            .map(|s| {
                let date = s
                    .date
                    .as_ref()
                    .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);
                Email {
                    id: s.id.clone(),
                    from: EmailAddress {
                        email: s.from.clone(),
                        name: None,
                    },
                    to: vec![],
                    subject: s.subject,
                    body_text: None,
                    date,
                    is_read: false,
                }
            })
            .collect();
        Ok(emails)
    }

    pub async fn send_email(&self, email: OutgoingEmail) -> SkillResult<SendResult> {
        let smtp = self
            .smtp
            .as_ref()
            .ok_or_else(|| SkillError::ToolFailed("SMTP not configured".to_string()))?;

        let to_addrs: Vec<&str> = email.to.iter().map(|a| a.email.as_str()).collect();

        let response = smtp
            .send(&self.from_address, &to_addrs, &email.subject, &email.body)
            .await
            .map_err(|e| SkillError::ToolFailed(format!("SMTP send failed: {}", e)))?;

        Ok(SendResult {
            message_id: Some(response),
        })
    }
}
