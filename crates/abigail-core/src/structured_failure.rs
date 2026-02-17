//! Structured failure types with actionable recovery data.
//!
//! These carry enough context for the Execution Governor to make
//! intelligent retry/escalation decisions instead of opaque error strings.

use serde::{Deserialize, Serialize};

/// A structured failure with actionable recovery information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StructuredFailure {
    /// Attempted an operation that requires elevated permissions.
    PermissionDenied {
        resource: String,
        action: String,
        recovery: String,
    },
    /// A required resource (file, URL, service) was not found.
    ResourceNotFound {
        resource_type: String,
        identifier: String,
        suggestions: Vec<String>,
    },
    /// Network connection failed.
    ConnectionFailed {
        endpoint: String,
        cause: String,
        retry_eligible: bool,
    },
    /// API key invalid or expired.
    AuthenticationFailed { provider: String, hint: String },
    /// Bad input from the user or a prior step.
    InvalidInput { field: String, reason: String },
    /// Operation exceeded its time budget.
    Timeout {
        operation: String,
        elapsed_ms: u64,
        budget_ms: u64,
    },
    /// Provider rate-limited the request.
    RateLimited {
        provider: String,
        retry_after_ms: Option<u64>,
    },
    /// An unstructured error that couldn't be classified.
    Unknown { message: String },
}

impl StructuredFailure {
    /// Whether this failure is eligible for automatic retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            StructuredFailure::ConnectionFailed {
                retry_eligible: true,
                ..
            } | StructuredFailure::Timeout { .. }
                | StructuredFailure::RateLimited { .. }
        )
    }

    /// A human-readable summary of the failure.
    pub fn summary(&self) -> String {
        match self {
            StructuredFailure::PermissionDenied {
                resource, action, ..
            } => {
                format!("Permission denied: {} on {}", action, resource)
            }
            StructuredFailure::ResourceNotFound {
                resource_type,
                identifier,
                ..
            } => {
                format!("{} not found: {}", resource_type, identifier)
            }
            StructuredFailure::ConnectionFailed {
                endpoint, cause, ..
            } => {
                format!("Connection to {} failed: {}", endpoint, cause)
            }
            StructuredFailure::AuthenticationFailed { provider, hint } => {
                format!("Auth failed for {}: {}", provider, hint)
            }
            StructuredFailure::InvalidInput { field, reason } => {
                format!("Invalid input for '{}': {}", field, reason)
            }
            StructuredFailure::Timeout {
                operation,
                elapsed_ms,
                budget_ms,
            } => {
                format!(
                    "Timeout: {} took {}ms (budget: {}ms)",
                    operation, elapsed_ms, budget_ms
                )
            }
            StructuredFailure::RateLimited { provider, .. } => {
                format!("Rate limited by {}", provider)
            }
            StructuredFailure::Unknown { message } => message.clone(),
        }
    }

    /// Try to classify a generic error string into a structured failure.
    pub fn from_error_string(error: &str) -> Self {
        let lower = error.to_lowercase();

        if lower.contains("permission denied") || lower.contains("access denied") {
            return StructuredFailure::PermissionDenied {
                resource: "unknown".into(),
                action: "unknown".into(),
                recovery: "Check permissions or run with elevated privileges".into(),
            };
        }

        if lower.contains("not found") || lower.contains("404") {
            return StructuredFailure::ResourceNotFound {
                resource_type: "resource".into(),
                identifier: "unknown".into(),
                suggestions: vec![],
            };
        }

        if lower.contains("timeout") || lower.contains("timed out") {
            return StructuredFailure::Timeout {
                operation: "unknown".into(),
                elapsed_ms: 0,
                budget_ms: 0,
            };
        }

        if lower.contains("unauthorized")
            || lower.contains("401")
            || lower.contains("invalid api key")
        {
            return StructuredFailure::AuthenticationFailed {
                provider: "unknown".into(),
                hint: "Check your API key".into(),
            };
        }

        if lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("too many requests")
        {
            return StructuredFailure::RateLimited {
                provider: "unknown".into(),
                retry_after_ms: None,
            };
        }

        if lower.contains("connection refused")
            || lower.contains("connection reset")
            || lower.contains("dns")
        {
            return StructuredFailure::ConnectionFailed {
                endpoint: "unknown".into(),
                cause: error.to_string(),
                retry_eligible: true,
            };
        }

        StructuredFailure::Unknown {
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for StructuredFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.summary())
    }
}

impl std::error::Error for StructuredFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable() {
        let timeout = StructuredFailure::Timeout {
            operation: "llm_call".into(),
            elapsed_ms: 30000,
            budget_ms: 30000,
        };
        assert!(timeout.is_retryable());

        let perm_denied = StructuredFailure::PermissionDenied {
            resource: "/etc/shadow".into(),
            action: "read".into(),
            recovery: "Use sudo".into(),
        };
        assert!(!perm_denied.is_retryable());
    }

    #[test]
    fn test_from_error_string() {
        let f = StructuredFailure::from_error_string("connection refused to localhost:8080");
        assert!(matches!(f, StructuredFailure::ConnectionFailed { .. }));

        let f = StructuredFailure::from_error_string("401 Unauthorized");
        assert!(matches!(f, StructuredFailure::AuthenticationFailed { .. }));

        let f = StructuredFailure::from_error_string("Something weird happened");
        assert!(matches!(f, StructuredFailure::Unknown { .. }));
    }

    #[test]
    fn test_summary() {
        let f = StructuredFailure::RateLimited {
            provider: "openai".into(),
            retry_after_ms: Some(5000),
        };
        assert_eq!(f.summary(), "Rate limited by openai");
    }

    #[test]
    fn test_serde_roundtrip() {
        let f = StructuredFailure::AuthenticationFailed {
            provider: "anthropic".into(),
            hint: "Key expired".into(),
        };
        let json = serde_json::to_string(&f).unwrap();
        let deserialized: StructuredFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(f.summary(), deserialized.summary());
    }
}
