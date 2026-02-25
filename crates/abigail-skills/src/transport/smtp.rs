//! SMTP client for sending email via lettre.
//!
//! Supports STARTTLS (port 587) and implicit TLS (port 465).
//! Accepts self-signed certificates for local bridge testing.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// TLS mode for the SMTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmtpTlsMode {
    /// Plain TCP connect, then STARTTLS upgrade (typical for port 587).
    #[default]
    StartTls,
    /// Immediate TLS wrap on connect (typical for port 465).
    Implicit,
}

pub struct SmtpClient {
    host: String,
    port: u16,
    user: String,
    password: String,
    tls_mode: SmtpTlsMode,
}

impl SmtpClient {
    pub fn new(host: &str, port: u16, user: &str, password: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            user: user.to_string(),
            password: password.to_string(),
            tls_mode: SmtpTlsMode::default(),
        }
    }

    pub fn with_tls_mode(mut self, mode: SmtpTlsMode) -> Self {
        self.tls_mode = mode;
        self
    }

    fn build_tls_parameters(&self) -> lettre::transport::smtp::client::TlsParameters {
        lettre::transport::smtp::client::TlsParameters::new_native(self.host.clone())
            .expect("TLS parameters build failed")
    }

    fn build_transport(&self) -> AsyncSmtpTransport<Tokio1Executor> {
        let creds = Credentials::new(self.user.clone(), self.password.clone());
        let tls_params = self.build_tls_parameters();

        match self.tls_mode {
            SmtpTlsMode::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host)
                    .port(self.port)
                    .tls(lettre::transport::smtp::client::Tls::Required(tls_params))
                    .credentials(creds)
                    .build()
            }
            SmtpTlsMode::Implicit => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host)
                    .port(self.port)
                    .tls(lettre::transport::smtp::client::Tls::Wrapper(tls_params))
                    .credentials(creds)
                    .build()
            }
        }
    }

    /// Send an email. Returns the SMTP response string on success.
    pub async fn send(
        &self,
        from: &str,
        to: &[&str],
        subject: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let from_mailbox: Mailbox = from
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid 'from' address '{}': {}", from, e))?;

        let mut builder = Message::builder().from(from_mailbox).subject(subject);

        for addr in to {
            let to_mailbox: Mailbox = addr
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid 'to' address '{}': {}", addr, e))?;
            builder = builder.to(to_mailbox);
        }

        let email = builder
            .body(body.to_string())
            .map_err(|e| anyhow::anyhow!("Failed to build email message: {}", e))?;

        let transport = self.build_transport();

        let response = transport
            .send(email)
            .await
            .map_err(|e| anyhow::anyhow!("SMTP send failed: {}", e))?;

        Ok(format!(
            "{}: {}",
            response.code(),
            response.message().collect::<Vec<_>>().join(" ")
        ))
    }
}
