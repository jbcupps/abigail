//! Execution state tracking for the Governor loop.

use abigail_core::StructuredFailure;
use serde::{Deserialize, Serialize};

/// A single execution attempt record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    /// Attempt number (1-indexed).
    pub attempt_number: u32,
    /// Whether this attempt succeeded.
    pub success: bool,
    /// Detail about what happened.
    pub detail: String,
    /// Structured failure if the attempt failed.
    pub failure: Option<StructuredFailure>,
    /// ISO 8601 timestamp.
    pub timestamp: String,
}

/// Tracks the state of an execution across multiple attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionState {
    /// All attempts so far.
    pub attempts: Vec<AttemptRecord>,
    /// Criteria that have been satisfied.
    pub satisfied_criteria: Vec<String>,
    /// Constraints discovered during execution.
    pub discovered_constraints: Vec<String>,
    /// Whether execution is complete.
    pub complete: bool,
    /// Final result (if complete).
    pub final_result: Option<String>,
}

impl ExecutionState {
    pub fn new() -> Self {
        Self {
            attempts: Vec::new(),
            satisfied_criteria: Vec::new(),
            discovered_constraints: Vec::new(),
            complete: false,
            final_result: None,
        }
    }

    /// Record a new attempt.
    pub fn record_attempt(
        &mut self,
        success: bool,
        detail: &str,
        failure: Option<StructuredFailure>,
    ) {
        let attempt_number = self.attempts.len() as u32 + 1;
        self.attempts.push(AttemptRecord {
            attempt_number,
            success,
            detail: detail.to_string(),
            failure,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Mark a criterion as satisfied.
    pub fn satisfy_criterion(&mut self, criterion: &str) {
        if !self.satisfied_criteria.contains(&criterion.to_string()) {
            self.satisfied_criteria.push(criterion.to_string());
        }
    }

    /// Add a discovered constraint.
    pub fn add_constraint(&mut self, constraint: &str) {
        if !self
            .discovered_constraints
            .contains(&constraint.to_string())
        {
            self.discovered_constraints.push(constraint.to_string());
        }
    }

    /// Mark execution as complete with a result.
    pub fn mark_complete(&mut self, result: &str) {
        self.complete = true;
        self.final_result = Some(result.to_string());
    }

    /// Number of failed attempts.
    pub fn failure_count(&self) -> u32 {
        self.attempts.iter().filter(|a| !a.success).count() as u32
    }

    /// Number of successful attempts.
    pub fn success_count(&self) -> u32 {
        self.attempts.iter().filter(|a| a.success).count() as u32
    }

    /// Total attempt count.
    pub fn attempt_count(&self) -> u32 {
        self.attempts.len() as u32
    }

    /// Get the last failure, if any.
    pub fn last_failure(&self) -> Option<&StructuredFailure> {
        self.attempts
            .iter()
            .rev()
            .find(|a| !a.success)
            .and_then(|a| a.failure.as_ref())
    }

    /// Check if all given criteria are satisfied.
    pub fn all_criteria_met(&self, criteria: &[String]) -> bool {
        criteria.iter().all(|c| self.satisfied_criteria.contains(c))
    }
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state() {
        let state = ExecutionState::new();
        assert!(state.attempts.is_empty());
        assert!(!state.complete);
        assert_eq!(state.failure_count(), 0);
        assert_eq!(state.success_count(), 0);
    }

    #[test]
    fn test_record_attempts() {
        let mut state = ExecutionState::new();
        state.record_attempt(true, "First try worked", None);
        state.record_attempt(
            false,
            "Second try failed",
            Some(StructuredFailure::Timeout {
                operation: "test".into(),
                elapsed_ms: 5000,
                budget_ms: 3000,
            }),
        );

        assert_eq!(state.attempt_count(), 2);
        assert_eq!(state.success_count(), 1);
        assert_eq!(state.failure_count(), 1);
        assert!(state.last_failure().is_some());
    }

    #[test]
    fn test_criteria_satisfaction() {
        let mut state = ExecutionState::new();
        let criteria = vec!["task done".to_string(), "output valid".to_string()];

        assert!(!state.all_criteria_met(&criteria));

        state.satisfy_criterion("task done");
        assert!(!state.all_criteria_met(&criteria));

        state.satisfy_criterion("output valid");
        assert!(state.all_criteria_met(&criteria));
    }

    #[test]
    fn test_mark_complete() {
        let mut state = ExecutionState::new();
        state.mark_complete("All done");
        assert!(state.complete);
        assert_eq!(state.final_result, Some("All done".to_string()));
    }

    #[test]
    fn test_add_constraint_deduplication() {
        let mut state = ExecutionState::new();
        state.add_constraint("API requires auth");
        state.add_constraint("API requires auth"); // duplicate
        assert_eq!(state.discovered_constraints.len(), 1);
    }
}
