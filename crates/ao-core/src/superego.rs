//! Superego safety checks — guards against harmful queries.

/// Result of a Superego safety check.
#[derive(Debug, Clone)]
pub struct SuperegoVerdict {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl SuperegoVerdict {
    fn allow() -> Self {
        Self { allowed: true, reason: None }
    }

    fn deny(reason: impl Into<String>) -> Self {
        Self { allowed: false, reason: Some(reason.into()) }
    }
}

/// Check whether a search query is safe to execute.
///
/// Rejects queries that attempt to find PII (addresses, phone numbers, SSNs),
/// doxxing patterns, or other privacy-violating searches.
pub fn check_search_query(query: &str) -> SuperegoVerdict {
    let lower = query.to_lowercase();

    // Pattern: "where does <person> live"
    if lower.contains("where does") && lower.contains("live") {
        return SuperegoVerdict::deny("Query appears to seek someone's home address");
    }
    if lower.contains("where do") && lower.contains("live") {
        return SuperegoVerdict::deny("Query appears to seek someone's home address");
    }

    // Pattern: home address
    if lower.contains("home address of") || lower.contains("home address for") {
        return SuperegoVerdict::deny("Query appears to seek someone's home address");
    }

    // Pattern: phone number lookup
    if (lower.contains("phone number of") || lower.contains("phone number for"))
        && !lower.contains("company")
        && !lower.contains("business")
        && !lower.contains("support")
        && !lower.contains("customer service")
    {
        return SuperegoVerdict::deny("Query appears to seek someone's personal phone number");
    }

    // Pattern: SSN / social security
    if lower.contains("social security number") || lower.contains("ssn of") || lower.contains("ssn for") {
        return SuperegoVerdict::deny("Query seeks Social Security information");
    }

    // Pattern: credit card / financial PII
    if lower.contains("credit card number") || lower.contains("bank account number") {
        return SuperegoVerdict::deny("Query seeks financial PII");
    }

    // Pattern: doxxing
    if lower.contains("dox") || lower.contains("doxx") {
        return SuperegoVerdict::deny("Query contains doxxing language");
    }

    // Pattern: real name / identity reveal
    if lower.contains("real name of") && (lower.contains("anonymous") || lower.contains("username")) {
        return SuperegoVerdict::deny("Query attempts to de-anonymize someone");
    }

    SuperegoVerdict::allow()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_queries() {
        let queries = [
            "What is the current stock price of NVIDIA?",
            "Latest news about Rust programming language",
            "Weather in New York City today",
            "Phone number for Apple customer support",
            "Who won the Super Bowl?",
            "Best restaurants in San Francisco",
        ];
        for q in queries {
            let v = check_search_query(q);
            assert!(v.allowed, "Expected allowed for query: {}", q);
        }
    }

    #[test]
    fn test_denied_address_queries() {
        let queries = [
            "where does Elon Musk live",
            "Where Does the CEO of Google live?",
            "home address of John Smith",
            "home address for my neighbor",
        ];
        for q in queries {
            let v = check_search_query(q);
            assert!(!v.allowed, "Expected denied for query: {}", q);
            assert!(v.reason.is_some());
        }
    }

    #[test]
    fn test_denied_phone_queries() {
        let v = check_search_query("phone number of Jane Doe");
        assert!(!v.allowed);
    }

    #[test]
    fn test_denied_ssn_queries() {
        let v = check_search_query("social security number of John");
        assert!(!v.allowed);
        let v = check_search_query("SSN of Jane Doe");
        assert!(!v.allowed);
    }

    #[test]
    fn test_denied_financial_queries() {
        let v = check_search_query("credit card number of someone");
        assert!(!v.allowed);
        let v = check_search_query("bank account number of John");
        assert!(!v.allowed);
    }

    #[test]
    fn test_denied_doxxing_queries() {
        let v = check_search_query("how to dox someone online");
        assert!(!v.allowed);
        let v = check_search_query("doxxing tools");
        assert!(!v.allowed);
    }

    #[test]
    fn test_denied_deanonymize_queries() {
        let v = check_search_query("real name of anonymous hacker user123");
        assert!(!v.allowed);
    }
}
