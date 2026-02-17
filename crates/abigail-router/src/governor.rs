//! Execution Governor: controls the strategy progression of an autonomous execution loop.
//!
//! The Governor wraps a GoalFrame and tracks execution state across multiple attempts.
//! It decides whether to continue, which strategy to try next, and when to escalate
//! or abort. This prevents runaway loops and ensures the agent converges on a result
//! or fails gracefully.

use crate::constraint_store::ConstraintStore;
use crate::execution_state::ExecutionState;
use crate::planner::GoalFrame;
use abigail_core::StructuredFailure;
use serde::{Deserialize, Serialize};

/// Strategy progression for the execution loop.
///
/// The Governor advances through strategies as earlier ones fail,
/// providing increasing levels of intervention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernorStrategy {
    /// First attempt — try the suggested approach directly.
    Initial,
    /// Retry with the same approach (tracks retry count).
    Retry(u32),
    /// Try a different approach (after retries are exhausted).
    Alternative,
    /// Escalate to a human or higher-authority agent.
    Escalate,
    /// Give up — max attempts or abort conditions met.
    Abort,
}

impl std::fmt::Display for GovernorStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GovernorStrategy::Initial => write!(f, "Initial"),
            GovernorStrategy::Retry(n) => write!(f, "Retry({})", n),
            GovernorStrategy::Alternative => write!(f, "Alternative"),
            GovernorStrategy::Escalate => write!(f, "Escalate"),
            GovernorStrategy::Abort => write!(f, "Abort"),
        }
    }
}

/// The result of a governed execution loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernedResult {
    /// Execution completed successfully.
    Success(String),
    /// Execution paused — needs user input to continue.
    NeedsInput(String),
    /// Execution escalated — needs human or higher-authority help.
    Escalated(String),
    /// Execution aborted — gave up after exhausting strategies.
    Aborted(String),
}

impl GovernedResult {
    /// Whether this result represents a successful completion.
    pub fn is_success(&self) -> bool {
        matches!(self, GovernedResult::Success(_))
    }

    /// Extract the inner message regardless of variant.
    pub fn message(&self) -> &str {
        match self {
            GovernedResult::Success(msg)
            | GovernedResult::NeedsInput(msg)
            | GovernedResult::Escalated(msg)
            | GovernedResult::Aborted(msg) => msg,
        }
    }
}

/// Maximum number of retries before advancing to the Alternative strategy.
const MAX_RETRIES: u32 = 3;

/// Controls the strategy progression of an autonomous execution loop.
///
/// The Governor is created with a GoalFrame and ConstraintStore, then called
/// repeatedly within the execution loop to decide whether to continue and
/// which strategy to apply. It tracks all attempts through an ExecutionState.
///
/// # Lifecycle
///
/// ```text
/// Governor::new(goal, constraints)
///     loop {
///         if !governor.should_continue() { break; }
///         // ... perform attempt ...
///         governor.record_attempt(success, detail);
///         if !success { governor.advance_strategy(); }
///     }
///     let result = governor.result();
/// ```
pub struct ExecutionGovernor {
    /// The goal being pursued.
    goal: GoalFrame,
    /// Tracks attempts, satisfied criteria, and constraints.
    state: ExecutionState,
    /// Known constraints that narrow what strategies are viable.
    constraints: ConstraintStore,
    /// Current strategy.
    strategy: GovernorStrategy,
    /// Total attempts made (across all strategies).
    attempts: u32,
    /// Timestamp (ms since epoch) when the governor was created, for time budget tracking.
    start_time_ms: u64,
}

impl ExecutionGovernor {
    /// Create a new governor for the given goal and constraints.
    pub fn new(goal: GoalFrame, constraints: ConstraintStore) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            goal,
            state: ExecutionState::new(),
            constraints,
            strategy: GovernorStrategy::Initial,
            attempts: 0,
            start_time_ms: now_ms,
        }
    }

    /// Check whether the execution loop should continue.
    ///
    /// Returns `false` if:
    /// - The execution is already marked complete
    /// - The strategy has progressed to Abort
    /// - The max iteration count has been reached
    /// - The time budget has been exceeded
    pub fn should_continue(&self) -> bool {
        if self.state.complete {
            tracing::debug!("Governor: execution already complete, stopping");
            return false;
        }
        if self.strategy == GovernorStrategy::Abort {
            tracing::debug!("Governor: strategy is Abort, stopping");
            return false;
        }
        if self.attempts >= self.goal.max_iterations {
            tracing::debug!(
                "Governor: max iterations reached ({}/{}), stopping",
                self.attempts,
                self.goal.max_iterations
            );
            return false;
        }
        if self.time_budget_exceeded() {
            tracing::debug!("Governor: time budget exceeded, stopping");
            return false;
        }
        true
    }

    /// Advance to the next strategy based on the current failure pattern.
    ///
    /// Strategy progression:
    /// - Initial -> Retry(1) on first failure
    /// - Retry(n) -> Retry(n+1) while n < MAX_RETRIES, unless failure is not retryable
    /// - Retry(MAX_RETRIES) or non-retryable -> Alternative
    /// - Alternative -> Escalate (if alternative also fails)
    /// - Escalate -> Abort
    pub fn advance_strategy(&mut self) {
        let last_failure = self.state.last_failure();
        let is_retryable = last_failure.map_or(true, |f| f.is_retryable());

        let next = match &self.strategy {
            GovernorStrategy::Initial => {
                if is_retryable {
                    GovernorStrategy::Retry(1)
                } else {
                    GovernorStrategy::Alternative
                }
            }
            GovernorStrategy::Retry(n) => {
                if is_retryable && *n < MAX_RETRIES {
                    GovernorStrategy::Retry(n + 1)
                } else {
                    GovernorStrategy::Alternative
                }
            }
            GovernorStrategy::Alternative => GovernorStrategy::Escalate,
            GovernorStrategy::Escalate => GovernorStrategy::Abort,
            GovernorStrategy::Abort => GovernorStrategy::Abort, // terminal
        };

        tracing::info!(
            "Governor: strategy {} -> {} (retryable={}, last_failure={:?})",
            self.strategy,
            next,
            is_retryable,
            last_failure.map(|f| f.summary())
        );

        self.strategy = next;
    }

    /// Record an execution attempt.
    ///
    /// On success, checks whether done criteria are met and may mark execution complete.
    /// On failure, records the attempt for strategy advancement.
    pub fn record_attempt(&mut self, success: bool, detail: &str) {
        self.attempts += 1;
        self.state.record_attempt(success, detail, None);

        if success {
            tracing::debug!("Governor: attempt {} succeeded: {}", self.attempts, detail);
        } else {
            tracing::debug!("Governor: attempt {} failed: {}", self.attempts, detail);
        }
    }

    /// Record a structured failure from an execution attempt.
    ///
    /// This provides richer information for strategy decisions than a simple
    /// success/failure boolean. The failure is stored in the execution state
    /// and may also discover new constraints.
    pub fn record_failure(&mut self, failure: &StructuredFailure) {
        self.attempts += 1;
        self.state
            .record_attempt(false, &failure.summary(), Some(failure.clone()));

        // Discover constraints from structured failures
        match failure {
            StructuredFailure::PermissionDenied {
                resource, action, ..
            } => {
                let constraint = format!("Cannot {} on {}", action, resource);
                self.constraints
                    .add_hard(&constraint, Some(&format!("attempt #{}", self.attempts)));
                self.state.add_constraint(&constraint);
            }
            StructuredFailure::AuthenticationFailed { provider, .. } => {
                let constraint = format!("No valid credentials for {}", provider);
                self.constraints
                    .add_hard(&constraint, Some(&format!("attempt #{}", self.attempts)));
                self.state.add_constraint(&constraint);
            }
            StructuredFailure::ResourceNotFound {
                resource_type,
                identifier,
                ..
            } => {
                let constraint = format!("{} '{}' does not exist", resource_type, identifier);
                self.constraints
                    .add_hard(&constraint, Some(&format!("attempt #{}", self.attempts)));
                self.state.add_constraint(&constraint);
            }
            _ => {
                // Other failure types don't produce persistent constraints
            }
        }

        tracing::debug!(
            "Governor: recorded structured failure (attempt {}): {}",
            self.attempts,
            failure.summary()
        );
    }

    /// Evaluate whether the execution result satisfies the done criteria.
    ///
    /// This performs a simple substring check: each done criterion is checked
    /// against the result string. In a future phase, this could use an LLM
    /// for semantic evaluation.
    pub fn evaluate_done(&self, result: &str) -> bool {
        if self.goal.done_criteria.is_empty() {
            // No criteria defined — any non-empty result counts as done
            return !result.trim().is_empty();
        }

        let result_lower = result.to_lowercase();
        let all_met = self.goal.done_criteria.iter().all(|criterion| {
            let criterion_lower = criterion.to_lowercase();
            // Check if the result contains the criterion text, or if the criterion
            // appears in the satisfied criteria set
            result_lower.contains(&criterion_lower)
                || self.state.satisfied_criteria.contains(criterion)
        });

        if all_met {
            tracing::debug!("Governor: all done criteria satisfied");
        } else {
            let unmet: Vec<_> = self
                .goal
                .done_criteria
                .iter()
                .filter(|c| {
                    !result_lower.contains(&c.to_lowercase())
                        && !self.state.satisfied_criteria.contains(*c)
                })
                .collect();
            tracing::debug!("Governor: unmet done criteria: {:?}", unmet);
        }

        all_met
    }

    /// Mark a specific done criterion as satisfied.
    pub fn satisfy_criterion(&mut self, criterion: &str) {
        self.state.satisfy_criterion(criterion);
    }

    /// Get the current governed result based on execution state and strategy.
    pub fn result(&self) -> GovernedResult {
        // If execution completed successfully
        if self.state.complete {
            if let Some(ref result) = self.state.final_result {
                return GovernedResult::Success(result.clone());
            }
        }

        // If a successful attempt happened and done criteria were met
        if self.state.success_count() > 0 {
            let last_success = self
                .state
                .attempts
                .iter()
                .rev()
                .find(|a| a.success)
                .map(|a| a.detail.clone())
                .unwrap_or_default();

            if self.evaluate_done(&last_success) {
                return GovernedResult::Success(last_success);
            }
        }

        // Strategy-based result
        match &self.strategy {
            GovernorStrategy::Escalate => {
                let summary = self.failure_summary();
                GovernedResult::Escalated(format!(
                    "Execution escalated after {} attempts. {}",
                    self.attempts, summary
                ))
            }
            GovernorStrategy::Abort => {
                let summary = self.failure_summary();
                GovernedResult::Aborted(format!(
                    "Execution aborted after {} attempts. {}",
                    self.attempts, summary
                ))
            }
            _ => {
                // Still in progress — needs more input or another iteration
                GovernedResult::NeedsInput(format!(
                    "Execution in progress (strategy: {}, attempts: {}/{})",
                    self.strategy, self.attempts, self.goal.max_iterations
                ))
            }
        }
    }

    /// Mark the execution as complete with a final result.
    pub fn mark_complete(&mut self, result: &str) {
        self.state.mark_complete(result);
    }

    /// Get the current strategy.
    pub fn strategy(&self) -> &GovernorStrategy {
        &self.strategy
    }

    /// Get the current attempt count.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Get a reference to the goal frame.
    pub fn goal(&self) -> &GoalFrame {
        &self.goal
    }

    /// Get a reference to the execution state.
    pub fn state(&self) -> &ExecutionState {
        &self.state
    }

    /// Get a reference to the constraint store.
    pub fn constraints(&self) -> &ConstraintStore {
        &self.constraints
    }

    /// Get a mutable reference to the constraint store.
    pub fn constraints_mut(&mut self) -> &mut ConstraintStore {
        &mut self.constraints
    }

    /// Check if the time budget has been exceeded.
    fn time_budget_exceeded(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        now_ms.saturating_sub(self.start_time_ms) > self.goal.time_budget_ms
    }

    /// Build a short summary of failures for inclusion in result messages.
    fn failure_summary(&self) -> String {
        let failure_count = self.state.failure_count();
        if failure_count == 0 {
            return String::new();
        }

        let last_failure_detail = self
            .state
            .attempts
            .iter()
            .rev()
            .find(|a| !a.success)
            .map(|a| a.detail.as_str())
            .unwrap_or("unknown");

        let constraint_info = if self.constraints.is_empty() {
            String::new()
        } else {
            format!(" Discovered {} constraint(s).", self.constraints.len())
        };

        format!(
            "Failures: {}/{}. Last failure: {}.{}",
            failure_count, self.attempts, last_failure_detail, constraint_info
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::GoalFrame;

    // ── Helper ─────────────────────────────────────────────────────

    fn simple_goal() -> GoalFrame {
        GoalFrame {
            intent: "Test task".to_string(),
            done_criteria: vec!["result contains answer".to_string()],
            good_criteria: vec!["result is concise".to_string()],
            risk_assessment: "None".to_string(),
            suggested_approach: "Just do it".to_string(),
            abort_conditions: vec!["total failure".to_string()],
            max_iterations: 5,
            time_budget_ms: 120_000,
        }
    }

    fn empty_constraints() -> ConstraintStore {
        ConstraintStore::new()
    }

    // ── Construction ───────────────────────────────────────────────

    #[test]
    fn test_new_governor() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        assert_eq!(gov.strategy(), &GovernorStrategy::Initial);
        assert_eq!(gov.attempts(), 0);
        assert!(gov.should_continue());
    }

    #[test]
    fn test_new_governor_with_constraints() {
        let mut constraints = ConstraintStore::new();
        constraints.add_hard("no network", None);
        let gov = ExecutionGovernor::new(simple_goal(), constraints);
        assert!(gov.constraints().has("no network"));
    }

    // ── should_continue ────────────────────────────────────────────

    #[test]
    fn test_should_continue_stops_when_complete() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.mark_complete("done");
        assert!(!gov.should_continue());
    }

    #[test]
    fn test_should_continue_stops_at_abort() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Abort;
        assert!(!gov.should_continue());
    }

    #[test]
    fn test_should_continue_stops_at_max_iterations() {
        let mut goal = simple_goal();
        goal.max_iterations = 2;
        let mut gov = ExecutionGovernor::new(goal, empty_constraints());

        gov.record_attempt(false, "fail 1");
        gov.record_attempt(false, "fail 2");

        assert!(!gov.should_continue());
    }

    #[test]
    fn test_should_continue_stops_when_time_exceeded() {
        let mut goal = simple_goal();
        goal.time_budget_ms = 0; // zero budget = already exceeded
        let gov = ExecutionGovernor::new(goal, empty_constraints());
        // Give it a moment to ensure time passes
        assert!(!gov.should_continue());
    }

    // ── Strategy advancement ───────────────────────────────────────

    #[test]
    fn test_advance_from_initial_to_retry() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        // Record a retryable failure
        gov.record_failure(&StructuredFailure::Timeout {
            operation: "test".into(),
            elapsed_ms: 5000,
            budget_ms: 3000,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Retry(1));
    }

    #[test]
    fn test_advance_from_initial_to_alternative_on_non_retryable() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        // Record a non-retryable failure
        gov.record_failure(&StructuredFailure::AuthenticationFailed {
            provider: "openai".into(),
            hint: "Invalid key".into(),
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Alternative);
    }

    #[test]
    fn test_advance_through_retries_to_alternative() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());

        // Initial -> Retry(1)
        gov.record_failure(&StructuredFailure::ConnectionFailed {
            endpoint: "api.example.com".into(),
            cause: "timeout".into(),
            retry_eligible: true,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Retry(1));

        // Retry(1) -> Retry(2)
        gov.record_failure(&StructuredFailure::ConnectionFailed {
            endpoint: "api.example.com".into(),
            cause: "timeout".into(),
            retry_eligible: true,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Retry(2));

        // Retry(2) -> Retry(3)
        gov.record_failure(&StructuredFailure::ConnectionFailed {
            endpoint: "api.example.com".into(),
            cause: "timeout".into(),
            retry_eligible: true,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Retry(3));

        // Retry(3) -> Alternative (MAX_RETRIES reached)
        gov.record_failure(&StructuredFailure::ConnectionFailed {
            endpoint: "api.example.com".into(),
            cause: "timeout".into(),
            retry_eligible: true,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Alternative);
    }

    #[test]
    fn test_advance_alternative_to_escalate() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Alternative;
        gov.record_attempt(false, "alternative failed");
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Escalate);
    }

    #[test]
    fn test_advance_escalate_to_abort() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Escalate;
        gov.record_attempt(false, "escalation failed");
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Abort);
    }

    #[test]
    fn test_abort_is_terminal() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Abort;
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Abort);
    }

    // ── Record attempt ─────────────────────────────────────────────

    #[test]
    fn test_record_attempt_increments_count() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_attempt(true, "worked");
        assert_eq!(gov.attempts(), 1);
        gov.record_attempt(false, "failed");
        assert_eq!(gov.attempts(), 2);
    }

    // ── Record structured failure ──────────────────────────────────

    #[test]
    fn test_record_failure_discovers_permission_constraint() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_failure(&StructuredFailure::PermissionDenied {
            resource: "/etc/shadow".into(),
            action: "read".into(),
            recovery: "use sudo".into(),
        });

        assert!(gov.constraints().has("Cannot read on /etc/shadow"));
        assert_eq!(gov.state().discovered_constraints.len(), 1);
    }

    #[test]
    fn test_record_failure_discovers_auth_constraint() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_failure(&StructuredFailure::AuthenticationFailed {
            provider: "anthropic".into(),
            hint: "Key expired".into(),
        });

        assert!(gov.constraints().has("No valid credentials for anthropic"));
    }

    #[test]
    fn test_record_failure_discovers_resource_constraint() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_failure(&StructuredFailure::ResourceNotFound {
            resource_type: "File".into(),
            identifier: "data.csv".into(),
            suggestions: vec![],
        });

        assert!(gov.constraints().has("File 'data.csv' does not exist"));
    }

    #[test]
    fn test_record_failure_no_constraint_for_timeout() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        let initial_count = gov.constraints().len();

        gov.record_failure(&StructuredFailure::Timeout {
            operation: "llm_call".into(),
            elapsed_ms: 30000,
            budget_ms: 30000,
        });

        // Timeouts don't produce persistent constraints
        assert_eq!(gov.constraints().len(), initial_count);
    }

    // ── evaluate_done ──────────────────────────────────────────────

    #[test]
    fn test_evaluate_done_with_matching_result() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        // done_criteria is ["result contains answer"]
        assert!(gov.evaluate_done("The result contains answer to the question."));
    }

    #[test]
    fn test_evaluate_done_case_insensitive() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        assert!(gov.evaluate_done("RESULT CONTAINS ANSWER"));
    }

    #[test]
    fn test_evaluate_done_with_non_matching_result() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        assert!(!gov.evaluate_done("This does not match any criteria."));
    }

    #[test]
    fn test_evaluate_done_with_satisfied_criteria() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.satisfy_criterion("result contains answer");
        // Even though the result text doesn't contain it, the criterion is marked satisfied
        assert!(gov.evaluate_done("something unrelated"));
    }

    #[test]
    fn test_evaluate_done_with_no_criteria() {
        let goal = GoalFrame {
            done_criteria: vec![],
            ..simple_goal()
        };
        let gov = ExecutionGovernor::new(goal, empty_constraints());
        // No criteria = any non-empty result is done
        assert!(gov.evaluate_done("anything"));
        assert!(!gov.evaluate_done(""));
        assert!(!gov.evaluate_done("   "));
    }

    // ── result() ───────────────────────────────────────────────────

    #[test]
    fn test_result_success_when_complete() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.mark_complete("All done successfully");
        let result = gov.result();
        assert!(result.is_success());
        assert_eq!(result.message(), "All done successfully");
    }

    #[test]
    fn test_result_needs_input_when_in_progress() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        let result = gov.result();
        assert!(matches!(result, GovernedResult::NeedsInput(_)));
        assert!(result.message().contains("in progress"));
    }

    #[test]
    fn test_result_escalated() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Escalate;
        gov.record_attempt(false, "everything failed");
        let result = gov.result();
        assert!(matches!(result, GovernedResult::Escalated(_)));
        assert!(result.message().contains("escalated"));
    }

    #[test]
    fn test_result_aborted() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.strategy = GovernorStrategy::Abort;
        gov.record_attempt(false, "total failure");
        let result = gov.result();
        assert!(matches!(result, GovernedResult::Aborted(_)));
        assert!(result.message().contains("aborted"));
    }

    // ── GovernedResult ─────────────────────────────────────────────

    #[test]
    fn test_governed_result_is_success() {
        assert!(GovernedResult::Success("ok".into()).is_success());
        assert!(!GovernedResult::Aborted("no".into()).is_success());
        assert!(!GovernedResult::Escalated("help".into()).is_success());
        assert!(!GovernedResult::NeedsInput("?".into()).is_success());
    }

    #[test]
    fn test_governed_result_message() {
        assert_eq!(GovernedResult::Success("ok".into()).message(), "ok");
        assert_eq!(GovernedResult::Aborted("no".into()).message(), "no");
        assert_eq!(GovernedResult::Escalated("help".into()).message(), "help");
        assert_eq!(GovernedResult::NeedsInput("?".into()).message(), "?");
    }

    #[test]
    fn test_governed_result_serde_roundtrip() {
        let results = vec![
            GovernedResult::Success("done".into()),
            GovernedResult::NeedsInput("waiting".into()),
            GovernedResult::Escalated("needs human".into()),
            GovernedResult::Aborted("gave up".into()),
        ];
        for result in &results {
            let json = serde_json::to_string(result).unwrap();
            let roundtripped: GovernedResult = serde_json::from_str(&json).unwrap();
            assert_eq!(roundtripped, *result);
        }
    }

    // ── Strategy display ───────────────────────────────────────────

    #[test]
    fn test_strategy_display() {
        assert_eq!(GovernorStrategy::Initial.to_string(), "Initial");
        assert_eq!(GovernorStrategy::Retry(2).to_string(), "Retry(2)");
        assert_eq!(GovernorStrategy::Alternative.to_string(), "Alternative");
        assert_eq!(GovernorStrategy::Escalate.to_string(), "Escalate");
        assert_eq!(GovernorStrategy::Abort.to_string(), "Abort");
    }

    // ── Full lifecycle ─────────────────────────────────────────────

    #[test]
    fn test_full_success_lifecycle() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());

        // First attempt succeeds
        assert!(gov.should_continue());
        gov.record_attempt(true, "result contains answer to the question");

        // Evaluate and complete
        assert!(gov.evaluate_done("result contains answer to the question"));
        gov.mark_complete("result contains answer to the question");

        assert!(!gov.should_continue());
        let result = gov.result();
        assert!(result.is_success());
    }

    #[test]
    fn test_full_retry_lifecycle() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());

        // First attempt fails (retryable)
        assert!(gov.should_continue());
        gov.record_failure(&StructuredFailure::ConnectionFailed {
            endpoint: "api.example.com".into(),
            cause: "timeout".into(),
            retry_eligible: true,
        });
        gov.advance_strategy();
        assert_eq!(gov.strategy(), &GovernorStrategy::Retry(1));

        // Second attempt succeeds
        assert!(gov.should_continue());
        gov.record_attempt(true, "result contains answer");
        gov.mark_complete("result contains answer");

        let result = gov.result();
        assert!(result.is_success());
    }

    #[test]
    fn test_full_abort_lifecycle() {
        let mut goal = simple_goal();
        goal.max_iterations = 10;
        let mut gov = ExecutionGovernor::new(goal, empty_constraints());

        // Fail through all strategies with non-retryable errors
        gov.record_failure(&StructuredFailure::AuthenticationFailed {
            provider: "openai".into(),
            hint: "bad key".into(),
        });
        gov.advance_strategy(); // -> Alternative

        gov.record_attempt(false, "alternative also failed");
        gov.advance_strategy(); // -> Escalate

        gov.record_attempt(false, "escalation no help");
        gov.advance_strategy(); // -> Abort

        assert!(!gov.should_continue());
        let result = gov.result();
        assert!(matches!(result, GovernedResult::Aborted(_)));
    }

    // ── Failure summary ────────────────────────────────────────────

    #[test]
    fn test_failure_summary_empty() {
        let gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        assert_eq!(gov.failure_summary(), "");
    }

    #[test]
    fn test_failure_summary_with_failures() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_attempt(false, "connection lost");
        let summary = gov.failure_summary();
        assert!(summary.contains("1/1"));
        assert!(summary.contains("connection lost"));
    }

    #[test]
    fn test_failure_summary_with_constraints() {
        let mut gov = ExecutionGovernor::new(simple_goal(), empty_constraints());
        gov.record_failure(&StructuredFailure::PermissionDenied {
            resource: "/root".into(),
            action: "write".into(),
            recovery: "sudo".into(),
        });
        let summary = gov.failure_summary();
        assert!(summary.contains("constraint"));
    }
}
