//! IMAP client for AO's email account. Proton-style defaults: mail.proton.me:993.

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

pub struct ImapClient {
    host: String,
    port: u16,
    user: String,
    password: String,
}

type TlsStream = async_native_tls::TlsStream<tokio_util::compat::Compat<TcpStream>>;

impl ImapClient {
    pub fn new(host: &str, port: u16, user: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            user: user.to_string(),
            password: password.to_string(),
        }
    }

    async fn connect(&self) -> anyhow::Result<Session<TlsStream>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_imap_connection() {
        if std::env::var("AO_IMAP_TEST").is_err() {
            return;
        }
        let host = std::env::var("AO_IMAP_HOST").unwrap_or_else(|_| "mail.proton.me".into());
        let port: u16 = std::env::var("AO_IMAP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(993);
        let user = std::env::var("AO_IMAP_USER").unwrap_or_default();
        let pass = std::env::var("AO_IMAP_PASS").unwrap_or_default();
        if user.is_empty() || pass.is_empty() {
            return;
        }
        let client = ImapClient::new(&host, port, &user, &pass);
        assert!(client.test_connection().await.is_ok());
    }
}
