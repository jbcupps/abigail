//! URL security validation — shared SSRF protection for capabilities.
//!
//! Provides configurable URL validation to prevent Server-Side Request Forgery
//! (SSRF) attacks. Used by both the HTTP client and browser capabilities.

use std::net::IpAddr;
use url::Url;

/// Blocked hostnames (cloud metadata, internal services).
const DEFAULT_BLOCKED_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.google.com",
    "169.254.169.254", // AWS/GCP/Azure metadata
];

/// Policy for URL validation.
#[derive(Debug, Clone)]
pub struct UrlSecurityPolicy {
    /// Allowed URL schemes (default: http, https).
    pub allowed_schemes: Vec<String>,
    /// Explicitly blocked hostnames.
    pub blocked_hosts: Vec<String>,
    /// If non-empty, only these domains (and subdomains) are allowed.
    pub allowed_domains: Vec<String>,
    /// Block private/internal IP ranges (default: true).
    pub block_private_ips: bool,
}

impl Default for UrlSecurityPolicy {
    fn default() -> Self {
        Self {
            allowed_schemes: vec!["http".into(), "https".into()],
            blocked_hosts: DEFAULT_BLOCKED_HOSTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            block_private_ips: true,
            allowed_domains: Vec::new(),
        }
    }
}

/// Validate a URL against the given security policy.
///
/// Returns the parsed URL if valid, or an error string describing why it was rejected.
pub fn validate_url(url_str: &str, policy: &UrlSecurityPolicy) -> Result<Url, String> {
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;

    // Check scheme
    let scheme = url.scheme();
    if !policy.allowed_schemes.iter().any(|s| s == scheme) {
        return Err(format!(
            "Scheme '{}' not allowed, permitted: {:?}",
            scheme, policy.allowed_schemes
        ));
    }

    let host = url.host_str().ok_or("URL must have a host")?;
    let host_lower = host.to_lowercase();

    // Check blocked hostnames
    for blocked in &policy.blocked_hosts {
        if host_lower == blocked.to_lowercase() {
            return Err(format!("Host '{}' is blocked (SSRF protection)", host));
        }
    }

    // Check allowed domains (if configured)
    if !policy.allowed_domains.is_empty() {
        let is_allowed = policy.allowed_domains.iter().any(|d| {
            let d_lower = d.to_lowercase();
            host_lower == d_lower || host_lower.ends_with(&format!(".{}", d_lower))
        });
        if !is_allowed {
            return Err(format!("Host '{}' is not in allowed domains list", host));
        }
    }

    // Check for private/internal IPs and domains
    if policy.block_private_ips {
        match url.host() {
            Some(url::Host::Ipv4(ip)) => {
                let addr = IpAddr::V4(ip);
                if is_private_ip(&addr) {
                    return Err(format!(
                        "Private/internal IP '{}' is blocked (SSRF protection)",
                        ip
                    ));
                }
            }
            Some(url::Host::Ipv6(ip)) => {
                let addr = IpAddr::V6(ip);
                if is_private_ip(&addr) {
                    return Err(format!(
                        "Private/internal IP '{}' is blocked (SSRF protection)",
                        ip
                    ));
                }
            }
            Some(url::Host::Domain(d)) => {
                let d_lower = d.to_lowercase();
                if d_lower == "localhost"
                    || d_lower == "0.0.0.0"
                    || d_lower.ends_with(".local")
                    || d_lower.ends_with(".internal")
                {
                    return Err(format!(
                        "Local/internal domain '{}' is blocked (SSRF protection)",
                        d
                    ));
                }
            }
            None => return Err("URL must have a host".to_string()),
        }
    }

    Ok(url)
}

/// Check if an IP address is in a private/internal range.
pub fn is_private_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            // Loopback: 127.0.0.0/8
            if octets[0] == 127 {
                return true;
            }
            // Private: 10.0.0.0/8
            if octets[0] == 10 {
                return true;
            }
            // Private: 172.16.0.0/12
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true;
            }
            // Private: 192.168.0.0/16
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            // Link-local: 169.254.0.0/16
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            // Current network: 0.0.0.0/8
            if octets[0] == 0 {
                return true;
            }
            false
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local: fc00::/7
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                // Link-local: fe80::/10
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::net::Ipv6Addr;

    fn default_policy() -> UrlSecurityPolicy {
        UrlSecurityPolicy::default()
    }

    #[test]
    fn test_allows_public_urls() {
        let policy = default_policy();
        assert!(validate_url("https://api.github.com/repos", &policy).is_ok());
        assert!(validate_url("https://example.com/page", &policy).is_ok());
        assert!(validate_url("http://httpbin.org/get", &policy).is_ok());
    }

    #[test]
    fn test_blocks_non_http_schemes() {
        let policy = default_policy();
        assert!(validate_url("file:///etc/passwd", &policy).is_err());
        assert!(validate_url("ftp://ftp.example.com/", &policy).is_err());
        assert!(validate_url("gopher://evil.com/", &policy).is_err());
        assert!(validate_url("javascript:alert(1)", &policy).is_err());
    }

    #[test]
    fn test_blocks_localhost() {
        let policy = default_policy();
        assert!(validate_url("http://localhost:8080/api", &policy).is_err());
        assert!(validate_url("http://127.0.0.1:1234", &policy).is_err());
        assert!(validate_url("http://0.0.0.0", &policy).is_err());
    }

    #[test]
    fn test_blocks_private_ips() {
        let policy = default_policy();
        assert!(validate_url("http://10.0.0.1/admin", &policy).is_err());
        assert!(validate_url("http://172.16.0.1/", &policy).is_err());
        assert!(validate_url("http://192.168.1.1/", &policy).is_err());
    }

    #[test]
    fn test_blocks_metadata_endpoints() {
        let policy = default_policy();
        assert!(validate_url("http://169.254.169.254/latest/meta-data/", &policy).is_err());
        assert!(validate_url("http://metadata.google.internal/", &policy).is_err());
        assert!(validate_url("http://metadata.google.com/", &policy).is_err());
    }

    #[test]
    fn test_blocks_internal_domains() {
        let policy = default_policy();
        assert!(validate_url("http://service.local/api", &policy).is_err());
        assert!(validate_url("http://db.internal/", &policy).is_err());
    }

    #[test]
    fn test_allowed_domains_whitelist() {
        let policy = UrlSecurityPolicy {
            allowed_domains: vec!["example.com".into(), "api.github.com".into()],
            ..default_policy()
        };
        assert!(validate_url("https://example.com/page", &policy).is_ok());
        assert!(validate_url("https://sub.example.com/page", &policy).is_ok());
        assert!(validate_url("https://api.github.com/repos", &policy).is_ok());
        assert!(validate_url("https://evil.com/", &policy).is_err());
    }

    #[test]
    fn test_private_ip_bypass_when_disabled() {
        let policy = UrlSecurityPolicy {
            block_private_ips: false,
            ..default_policy()
        };
        assert!(validate_url("http://127.0.0.1:8080", &policy).is_ok());
        assert!(validate_url("http://192.168.1.1/", &policy).is_ok());
    }

    #[test]
    fn test_is_private_ip_v4() {
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 31, 255, 255))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 169, 254
        ))));
        assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));

        // Public IPs should not be private
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 15, 0, 1))));
        assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 32, 0, 1))));
    }

    #[test]
    fn test_is_private_ip_v6() {
        // Loopback
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
        // Unspecified
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        // Unique local (fc00::/7)
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfd00, 0, 0, 0, 0, 0, 0, 1
        ))));
        // Link-local (fe80::/10)
        assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));

        // Public IPv6
        assert!(!is_private_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn test_invalid_urls() {
        let policy = default_policy();
        assert!(validate_url("not-a-url", &policy).is_err());
        assert!(validate_url("", &policy).is_err());
        assert!(validate_url("://missing-scheme", &policy).is_err());
    }

    #[test]
    fn test_custom_blocked_hosts() {
        let policy = UrlSecurityPolicy {
            blocked_hosts: vec!["evil.example.com".into()],
            ..default_policy()
        };
        assert!(validate_url("https://evil.example.com/malware", &policy).is_err());
        assert!(validate_url("https://good.example.com/", &policy).is_ok());
    }
}
