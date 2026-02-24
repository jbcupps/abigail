//! IMAP client for Abigail's email account.
//!
//! Supports both implicit TLS (port 993) and STARTTLS (plain connect then upgrade).

use async_imap::Session;
use async_native_tls::TlsConnector;
use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncReadCompatExt;

#[derive(Debug, Clone)]
pub struct EmailSummary {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: Option<String>,
}

/// TLS mode for the IMAP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImapTlsMode {
    /// Immediate TLS wrap on connect (typical for port 993).
    #[default]
    Implicit,
    /// Plain TCP connect, then STARTTLS upgrade (typical for port 143 or custom bridges).
    StartTls,
}

pub struct ImapClient {
    host: String,
    port: u16,
    user: String,
    password: String,
    tls_mode: ImapTlsMode,
}

type TlsStream = async_native_tls::TlsStream<tokio_util::compat::Compat<TcpStream>>;

impl ImapClient {
    pub fn new(host: &str, port: u16, user: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            user: user.to_string(),
            password: password.to_string(),
            tls_mode: ImapTlsMode::default(),
        }
    }

    pub fn with_tls_mode(mut self, mode: ImapTlsMode) -> Self {
        self.tls_mode = mode;
        self
    }

    async fn connect_implicit(&self) -> anyhow::Result<Session<TlsStream>> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect(&addr).await?;
        let stream = stream.compat();
        let tls = TlsConnector::new().connect(&self.host, stream).await?;
        let mut client = async_imap::Client::new(tls);
        let _ = client.read_response().await;
        let session = client
            .login(&self.user, &self.password)
            .await
            .map_err(|(e, _)| e)?;
        Ok(session)
    }

    async fn connect_starttls(&self) -> anyhow::Result<Session<TlsStream>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let addr = format!("{}:{}", self.host, self.port);
        let mut stream = TcpStream::connect(&addr).await?;

        // Read server greeting on the plain connection.
        let mut greeting = vec![0u8; 1024];
        let n = stream.read(&mut greeting).await?;
        tracing::debug!("IMAP greeting: {}", String::from_utf8_lossy(&greeting[..n]));

        // Send STARTTLS command.
        stream.write_all(b"A01 STARTTLS\r\n").await?;
        let mut resp = vec![0u8; 1024];
        let n = stream.read(&mut resp).await?;
        let resp_str = String::from_utf8_lossy(&resp[..n]);
        tracing::debug!("STARTTLS response: {}", resp_str);
        if !resp_str.contains("A01 OK") {
            anyhow::bail!("STARTTLS rejected by server: {}", resp_str.trim());
        }

        // Upgrade to TLS (accept self-signed certs for local bridges).
        let mut builder = native_tls::TlsConnector::builder();
        builder.danger_accept_invalid_certs(true);
        builder.danger_accept_invalid_hostnames(true);
        let connector = TlsConnector::from(builder);
        let tls = connector.connect(&self.host, stream.compat()).await?;

        let mut client = async_imap::Client::new(tls);
        let _ = client.read_response().await;
        let session = client
            .login(&self.user, &self.password)
            .await
            .map_err(|(e, _)| e)?;
        Ok(session)
    }

    async fn connect(&self) -> anyhow::Result<Session<TlsStream>> {
        match self.tls_mode {
            ImapTlsMode::Implicit => self.connect_implicit().await,
            ImapTlsMode::StartTls => self.connect_starttls().await,
        }
    }

    /// Test connection (for birth sequence validation).
    pub async fn test_connection(&self) -> anyhow::Result<()> {
        let mut session = self.connect().await?;
        session.logout().await?;
        Ok(())
    }

    /// Fetch unread email summaries from INBOX.
    pub async fn fetch_unread(&self) -> anyhow::Result<Vec<EmailSummary>> {
        let mut session = self.connect().await?;
        session.select("INBOX").await?;
        let unseen = session.search("UNSEEN").await?;
        let mut summaries = Vec::new();
        for seq in &unseen {
            let seq_str = format!("{}", seq);
            let mut stream = session.fetch(&seq_str, "RFC822.HEADER").await?;
            while let Some(msg) = stream.next().await {
                let msg = msg?;
                let header = msg.header().unwrap_or_default();
                let (from, subject, date) =
                    if let Some(parsed) = mail_parser::MessageParser::default().parse(header) {
                        let from = parsed
                            .return_address()
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        let subject = parsed.subject().unwrap_or("").to_string();
                        let date = parsed.date().map(|d| d.to_string());
                        (from, subject, date)
                    } else {
                        (String::new(), String::new(), None)
                    };
                summaries.push(EmailSummary {
                    id: seq_str.clone(),
                    from,
                    subject,
                    date,
                });
            }
        }
        session.logout().await?;
        Ok(summaries)
    }

    /// Fetch all email summaries from INBOX (read and unread).
    pub async fn fetch_all(&self, limit: u32) -> anyhow::Result<Vec<EmailSummary>> {
        let mut session = self.connect().await?;
        let mailbox = session.select("INBOX").await?;
        let exists = mailbox.exists;
        if exists == 0 {
            session.logout().await?;
            return Ok(Vec::new());
        }
        let start = if exists > limit {
            exists - limit + 1
        } else {
            1
        };
        let range = format!("{}:{}", start, exists);
        let mut summaries = Vec::new();
        let mut stream = session.fetch(&range, "RFC822.HEADER").await?;
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let seq_str = format!("{}", msg.message);
            let header = msg.header().unwrap_or_default();
            let (from, subject, date) =
                if let Some(parsed) = mail_parser::MessageParser::default().parse(header) {
                    let from = parsed
                        .return_address()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    let subject = parsed.subject().unwrap_or("").to_string();
                    let date = parsed.date().map(|d| d.to_string());
                    (from, subject, date)
                } else {
                    (String::new(), String::new(), None)
                };
            summaries.push(EmailSummary {
                id: seq_str,
                from,
                subject,
                date,
            });
        }
        drop(stream);
        session.logout().await?;
        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_imap_connection() {
        if std::env::var("ABIGAIL_IMAP_TEST").is_err() {
            return;
        }
        let host = std::env::var("ABIGAIL_IMAP_HOST").unwrap_or_else(|_| "mail.proton.me".into());
        let port: u16 = std::env::var("ABIGAIL_IMAP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(993);
        let user = std::env::var("ABIGAIL_IMAP_USER").unwrap_or_default();
        let pass = std::env::var("ABIGAIL_IMAP_PASS").unwrap_or_default();
        if user.is_empty() || pass.is_empty() {
            return;
        }
        let tls_mode = match std::env::var("ABIGAIL_IMAP_TLS_MODE")
            .unwrap_or_default()
            .to_uppercase()
            .as_str()
        {
            "STARTTLS" => ImapTlsMode::StartTls,
            _ => ImapTlsMode::Implicit,
        };
        let client = ImapClient::new(&host, port, &user, &pass).with_tls_mode(tls_mode);
        assert!(client.test_connection().await.is_ok());
    }
}
