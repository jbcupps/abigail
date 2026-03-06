//! SMTP client for sending email via lettre.
//!
//! Supports STARTTLS (port 587) and implicit TLS (port 465).
//! Accepts self-signed certificates only for loopback bridge testing.

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::net::IpAddr;

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
        let mut builder =
            lettre::transport::smtp::client::TlsParameters::builder(self.host.clone());
        if allows_insecure_local_tls(&self.host) {
            builder = builder
                .dangerous_accept_invalid_certs(true)
                .dangerous_accept_invalid_hostnames(true);
        }
        builder.build().expect("TLS parameters build failed")
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

    /// Test the SMTP connection using a NOOP after authentication.
    pub async fn test_connection(&self) -> anyhow::Result<()> {
        let transport = self.build_transport();
        let ok = transport
            .test_connection()
            .await
            .map_err(|e| anyhow::anyhow!("SMTP connection test failed: {}", e))?;
        if ok {
            Ok(())
        } else {
            anyhow::bail!("SMTP connection test returned false")
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

    #[test]
    fn local_hosts_allow_insecure_tls() {
        assert!(allows_insecure_local_tls("localhost"));
        assert!(allows_insecure_local_tls("127.0.0.1"));
        assert!(allows_insecure_local_tls("[::1]"));
        assert!(!allows_insecure_local_tls("smtp.example.com"));
        assert!(!allows_insecure_local_tls("10.0.0.5"));
    }

    #[tokio::test]
    async fn test_smtp_connection() {
        if std::env::var("ABIGAIL_SMTP_TEST").is_err() {
            return;
        }

        let host = std::env::var("ABIGAIL_SMTP_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port: u16 = std::env::var("ABIGAIL_SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let user = std::env::var("ABIGAIL_SMTP_USER").unwrap_or_default();
        let pass = std::env::var("ABIGAIL_SMTP_PASS").unwrap_or_default();
        if user.is_empty() || pass.is_empty() {
            return;
        }

        let tls_mode = match std::env::var("ABIGAIL_SMTP_TLS_MODE")
            .unwrap_or_default()
            .to_uppercase()
            .as_str()
        {
            "IMPLICIT" => SmtpTlsMode::Implicit,
            _ => SmtpTlsMode::StartTls,
        };

        let client = SmtpClient::new(&host, port, &user, &pass).with_tls_mode(tls_mode);
        assert!(
            tokio::time::timeout(Duration::from_secs(30), client.test_connection())
                .await
                .expect("timed out testing SMTP connection")
                .is_ok()
        );
    }
}
