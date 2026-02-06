//! Validation of local LLM base URL to prevent SSRF.
//!
//! Only allows http(s) URLs whose host is localhost, 127.0.0.1, or [::1].
//! Private IP ranges (169.254.x.x, 10.x, 192.168.x, etc.) are rejected.

use crate::error::{CoreError, Result};
use url::Url;

/// Allowed hostnames for local LLM (case-insensitive).
const ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];

/// Validates that the URL is safe for use as a local LLM base URL (SSRF mitigation).
/// Returns the normalized URL string (with trailing slash removed) on success.
pub fn validate_local_llm_url(url_str: &str) -> Result<String> {
    let s = url_str.trim();
    if s.is_empty() {
        return Err(CoreError::Config("Local LLM URL cannot be empty".into()));
    }

    let url = Url::parse(s)
        .map_err(|e| CoreError::Config(format!("Invalid local LLM URL: {}", e)))?;

    if url.cannot_be_a_base() {
        return Err(CoreError::Config(
            "Invalid local LLM URL: opaque or invalid base".into(),
        ));
    }

    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(CoreError::Config(format!(
            "Local LLM URL must use http or https, got: {}",
            scheme
        )));
    }

    let host = url
        .host_str()
        .ok_or_else(|| CoreError::Config("Local LLM URL must have a host".into()))?;

    let allowed = match url.host() {
        Some(url::Host::Domain(d)) => ALLOWED_HOSTS
            .iter()
            .any(|h| d.eq_ignore_ascii_case(h)),
        Some(url::Host::Ipv4(ip)) => {
            let octets = ip.octets();
            octets[0] == 127 // 127.0.0.0/8 loopback
        }
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };

    if !allowed {
        return Err(CoreError::Config(format!(
            "Local LLM URL host not allowed (SSRF protection). Use localhost, 127.0.0.1, or [::1]. Got: {}",
            host
        )));
    }

    let normalized = url.as_str().trim_end_matches('/').to_string();
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localhost_http() {
        let u = validate_local_llm_url("http://localhost:1234").unwrap();
        assert_eq!(u, "http://localhost:1234");
    }

    #[test]
    fn test_127_loopback() {
        let u = validate_local_llm_url("http://127.0.0.1:11434").unwrap();
        assert_eq!(u, "http://127.0.0.1:11434");
    }

    #[test]
    fn test_trailing_slash_stripped() {
        let u = validate_local_llm_url("http://localhost:1234/").unwrap();
        assert_eq!(u, "http://localhost:1234");
    }

    #[test]
    fn test_private_ip_rejected() {
        assert!(validate_local_llm_url("http://192.168.1.1:1234").is_err());
        assert!(validate_local_llm_url("http://10.0.0.1:1234").is_err());
        assert!(validate_local_llm_url("http://169.254.169.254/").is_err());
    }

    #[test]
    fn test_scheme_rejected() {
        assert!(validate_local_llm_url("file:///tmp/x").is_err());
        assert!(validate_local_llm_url("ftp://localhost").is_err());
    }

    #[test]
    fn test_empty_rejected() {
        assert!(validate_local_llm_url("").is_err());
        assert!(validate_local_llm_url("   ").is_err());
    }
}
