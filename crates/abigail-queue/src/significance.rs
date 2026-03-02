//! Significance scoring for job results.
//!
//! Evaluates how important a job's result is based on keyword matching and
//! built-in indicators, returning a decision on how to handle it.

use serde::{Deserialize, Serialize};

/// What to do with a job's result based on significance scoring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignificanceDecision {
    /// Low significance — just log it silently.
    SilentLog,
    /// Medium significance — spawn an agentic run to handle it.
    SpawnAgentic,
    /// High significance — flag the mentor for attention.
    FlagMentor,
}

/// Score the significance of a result based on keywords and a threshold.
///
/// Returns `(score, decision)` where score is 0.0–1.0 and decision indicates
/// the recommended action.
pub fn score_significance(
    result: &str,
    keywords: &[String],
    threshold: f32,
) -> (f32, SignificanceDecision) {
    let lower = result.to_lowercase();
    let mut score: f32 = 0.0;

    // User-defined keyword matching
    for keyword in keywords {
        if lower.contains(&keyword.to_lowercase()) {
            score += 0.3;
        }
    }

    // Built-in urgency indicators
    let urgent_keywords = ["urgent", "error", "failure", "critical", "alert", "warning"];
    for kw in &urgent_keywords {
        if lower.contains(kw) {
            score += 0.2;
        }
    }

    score = score.min(1.0);

    let decision = if score >= 0.8 {
        SignificanceDecision::FlagMentor
    } else if score >= threshold {
        SignificanceDecision::SpawnAgentic
    } else {
        SignificanceDecision::SilentLog
    };

    (score, decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_significance() {
        let (score, decision) = score_significance(
            "Nothing interesting happened today",
            &["important".into()],
            0.5,
        );
        assert!(score < 0.5);
        assert_eq!(decision, SignificanceDecision::SilentLog);
    }

    #[test]
    fn high_significance() {
        let (score, decision) = score_significance(
            "URGENT ALERT: critical error detected, this is important",
            &["important".into()],
            0.5,
        );
        assert!(score >= 0.8);
        assert_eq!(decision, SignificanceDecision::FlagMentor);
    }

    #[test]
    fn medium_significance() {
        let (_, decision) = score_significance(
            "New deploy available with a warning",
            &["deploy".into()],
            0.3,
        );
        assert_eq!(decision, SignificanceDecision::SpawnAgentic);
    }
}
