//! IMAP client for Abigail's email account.
//!
//! Supports both implicit TLS (port 993) and STARTTLS (plain connect then upgrade).

use async_imap::Session;
use async_native_tls::TlsConnector;
use futures_util::StreamExt;
use std::net::IpAddr;
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

    fn tls_connector(&self) -> TlsConnector {
        let mut connector = TlsConnector::new();
        if allows_insecure_local_tls(&self.host) {
            connector = connector
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
        connector
    }

    async fn connect_implicit(&self) -> anyhow::Result<Session<TlsStream>> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect(&addr).await?;
        let stream = stream.compat();
        let tls = self.tls_connector().connect(&self.host, stream).await?;
        let mut client = async_imap::Client::new(tls);
        let _ = client.read_response().await;
        let session = client
            .login(&self.user, &self.password)
            .await
            .map_err(|(e, _)| e)?;
        Ok(session)
    }

    async fn connect_starttls(&self) -> anyhow::Result<Session<TlsStream>> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect(&addr).await?.compat();
        let mut client = async_imap::Client::new(stream);
        let _ = client.read_response().await;
        client.run_command_and_check_ok("STARTTLS", None).await?;

        let stream = client.into_inner();
        let tls = self.tls_connector().connect(&self.host, stream).await?;
        let client = async_imap::Client::new(tls);
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

fn allows_insecure_local_tls(host: &str) -> bool {
    let normalized = host.trim().trim_matches(['[', ']']);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .is_ok_and(|addr| addr.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn placeholder_client(host: &str) -> ImapClient {
        ImapClient {
            host: host.to_string(),
            port: 143,
            user: String::new(),
            password: String::new(),
            tls_mode: ImapTlsMode::default(),
        }
    }

    #[test]
    fn default_tls_mode_is_implicit() {
        assert_eq!(
            placeholder_client("localhost").tls_mode,
            ImapTlsMode::Implicit
        );
    }

    #[test]
    fn with_tls_mode_overrides_default() {
        assert_eq!(
            placeholder_client("mail.example.com")
                .with_tls_mode(ImapTlsMode::StartTls)
                .tls_mode,
            ImapTlsMode::StartTls
        );
    }

    #[test]
    fn local_hosts_allow_insecure_tls() {
        assert!(allows_insecure_local_tls("localhost"));
        assert!(allows_insecure_local_tls("127.0.0.1"));
        assert!(allows_insecure_local_tls("[::1]"));
        assert!(!allows_insecure_local_tls("mail.example.com"));
        assert!(!allows_insecure_local_tls("192.168.1.10"));
    }

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
        assert!(
            tokio::time::timeout(Duration::from_secs(30), client.test_connection())
                .await
                .expect("timed out testing IMAP connection")
                .is_ok()
        );
    }
}
