//! CrystallizationEngine: state machine driving the crystallization process.
//!
//! For Sprint 1, only Quick Start is fully implemented. The engine
//! immediately transitions to SoulGeneration for Quick Start depth.

use crate::ethics_calibrator::calibrate_triangle_ethic;
use crate::models::{ConversationTurn, CrystallizationPhase, DepthLevel, MentorProfile, Signal};

use abigail_capabilities::cognitive::provider::CompletionResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Result of processing an LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessResult {
    /// Text to display to the user.
    pub display_text: String,
    /// Whether the engine wants to advance to the next phase.
    pub phase_complete: bool,
    /// Current phase after processing.
    pub current_phase: CrystallizationPhase,
    /// Signals extracted in this turn.
    pub signals_extracted: Vec<Signal>,
}

/// Status summary of the crystallization engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrystallizationStatus {
    pub phase: CrystallizationPhase,
    pub depth: DepthLevel,
    pub turn_count: usize,
    pub profile_completeness: f64,
}

/// The crystallization engine state machine.
pub struct CrystallizationEngine {
    profile: MentorProfile,
    depth: DepthLevel,
    current_phase: CrystallizationPhase,
    conversation_history: Vec<ConversationTurn>,
    turn_count: usize,
    used_question_ids: HashSet<String>,
}

impl CrystallizationEngine {
    /// Create a new engine for the given depth level.
    pub fn new(depth: DepthLevel) -> Self {
        let initial_phase = match depth {
            DepthLevel::QuickStart => CrystallizationPhase::SoulGeneration,
            DepthLevel::Conversation | DepthLevel::DeepDive => CrystallizationPhase::Conversation,
        };

        Self {
            profile: MentorProfile::new(depth),
            depth,
            current_phase: initial_phase,
            conversation_history: Vec::new(),
            turn_count: 0,
            used_question_ids: HashSet::new(),
        }
    }

    /// Get the current crystallization phase.
    pub fn current_phase(&self) -> CrystallizationPhase {
        self.current_phase
    }

    /// Get the depth level.
    pub fn depth(&self) -> DepthLevel {
        self.depth
    }

    /// Get the current turn count.
    pub fn turn_count(&self) -> usize {
        self.turn_count
    }

    /// Get a reference to the mentor profile.
    pub fn profile(&self) -> &MentorProfile {
        &self.profile
    }

    /// Get a mutable reference to the mentor profile.
    pub fn profile_mut(&mut self) -> &mut MentorProfile {
        &mut self.profile
    }

    /// Get the conversation history.
    pub fn conversation_history(&self) -> &[ConversationTurn] {
        &self.conversation_history
    }

    /// Get the status summary.
    pub fn status(&self) -> CrystallizationStatus {
        CrystallizationStatus {
            phase: self.current_phase,
            depth: self.depth,
            turn_count: self.turn_count,
            profile_completeness: self.profile.completeness(),
        }
    }

    /// Add a user message to the conversation history.
    pub fn add_user_message(&mut self, content: &str) {
        self.conversation_history.push(ConversationTurn {
            role: "user".to_string(),
            content: content.to_string(),
            signals: Vec::new(),
        });
        self.turn_count += 1;
        self.profile.turn_count = self.turn_count;
    }

    /// Process an LLM response, extracting signals from tool calls.
    pub fn process_response(&mut self, response: &CompletionResponse) -> ProcessResult {
        let mut signals_extracted = Vec::new();

        // Extract signals from tool calls
        if let Some(tool_calls) = &response.tool_calls {
            for tc in tool_calls {
                if tc.name == "record_signal" {
                    if let Ok(signals) = parse_record_signal_args(&tc.arguments) {
                        for signal in &signals {
                            self.profile.apply_signal(signal);
                        }
                        signals_extracted.extend(signals);
                    }
                }
            }
        }

        // Add assistant turn to history
        self.conversation_history.push(ConversationTurn {
            role: "assistant".to_string(),
            content: response.content.clone(),
            signals: signals_extracted.clone(),
        });

        // Check if we should advance phase
        let phase_complete = self.should_advance_phase();

        ProcessResult {
            display_text: response.content.clone(),
            phase_complete,
            current_phase: self.current_phase,
            signals_extracted,
        }
    }

    /// Check whether the current phase should advance.
    pub fn should_advance_phase(&self) -> bool {
        match self.current_phase {
            CrystallizationPhase::Spark => {
                // Spark completes when depth is selected (handled externally)
                false
            }
            CrystallizationPhase::Conversation => {
                // Confidence thresholds OR hard cap
                let ocean_ok = self.profile.avg_ocean_confidence() >= 0.4;
                let moral_ok = self.profile.avg_moral_confidence() >= 0.3;
                let attachment_ok = self.profile.attachment_confidence >= 0.5;
                let hard_cap = self.turn_count >= 10;

                (ocean_ok && moral_ok && attachment_ok) || hard_cap
            }
            CrystallizationPhase::Mirror => {
                // Mirror completes when mirror_text is set (handled externally)
                self.profile.mirror_text.is_some()
            }
            CrystallizationPhase::Forge => {
                // Forge completes after dilemmas are done (Sprint 4)
                false
            }
            CrystallizationPhase::SoulGeneration => {
                // Always ready
                true
            }
            CrystallizationPhase::Complete => false,
        }
    }

    /// Advance to the next phase. Returns the new phase, or None if already complete.
    pub fn advance_phase(&mut self) -> Option<CrystallizationPhase> {
        let next = match (self.current_phase, self.depth) {
            (CrystallizationPhase::Spark, _) => CrystallizationPhase::Conversation,
            (CrystallizationPhase::Conversation, _) => CrystallizationPhase::Mirror,
            (CrystallizationPhase::Mirror, DepthLevel::DeepDive) => CrystallizationPhase::Forge,
            (CrystallizationPhase::Mirror, _) => CrystallizationPhase::SoulGeneration,
            (CrystallizationPhase::Forge, _) => CrystallizationPhase::SoulGeneration,
            (CrystallizationPhase::SoulGeneration, _) => CrystallizationPhase::Complete,
            (CrystallizationPhase::Complete, _) => return None,
        };

        self.current_phase = next;
        Some(next)
    }

    /// Calibrate the Triangle Ethic weights from the current profile.
    pub fn calibrate_ethics(&mut self) {
        self.profile.ethics_weights = calibrate_triangle_ethic(&self.profile);
    }

    /// Mark a question ID as used.
    pub fn mark_question_used(&mut self, id: &str) {
        self.used_question_ids.insert(id.to_string());
    }

    /// Check if a question ID has been used.
    pub fn is_question_used(&self, id: &str) -> bool {
        self.used_question_ids.contains(id)
    }

    /// Set the mirror text on the profile.
    pub fn set_mirror_text(&mut self, text: String) {
        self.profile.mirror_text = Some(text);
    }

    /// Set the crystallization timestamp.
    pub fn set_timestamp(&mut self, timestamp: String) {
        self.profile.timestamp = Some(timestamp);
    }
}

/// Parse the arguments of a `record_signal` tool call into signals.
fn parse_record_signal_args(arguments: &str) -> Result<Vec<Signal>, serde_json::Error> {
    #[derive(Deserialize)]
    struct RecordSignalArgs {
        signals: Vec<Signal>,
    }

    // Try parsing as { "signals": [...] }
    if let Ok(args) = serde_json::from_str::<RecordSignalArgs>(arguments) {
        return Ok(args.signals);
    }

    // Try parsing as a single signal
    if let Ok(signal) = serde_json::from_str::<Signal>(arguments) {
        return Ok(vec![signal]);
    }

    // Try parsing as an array of signals directly
    serde_json::from_str::<Vec<Signal>>(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_capabilities::cognitive::provider::ToolCall;

    #[test]
    fn test_quick_start_skips_to_soul_generation() {
        let engine = CrystallizationEngine::new(DepthLevel::QuickStart);
        assert_eq!(engine.current_phase(), CrystallizationPhase::SoulGeneration);
    }

    #[test]
    fn test_conversation_starts_at_conversation_phase() {
        let engine = CrystallizationEngine::new(DepthLevel::Conversation);
        assert_eq!(engine.current_phase(), CrystallizationPhase::Conversation);
    }

    #[test]
    fn test_deep_dive_starts_at_conversation_phase() {
        let engine = CrystallizationEngine::new(DepthLevel::DeepDive);
        assert_eq!(engine.current_phase(), CrystallizationPhase::Conversation);
    }

    #[test]
    fn test_status_returns_correct_info() {
        let engine = CrystallizationEngine::new(DepthLevel::Conversation);
        let status = engine.status();
        assert_eq!(status.phase, CrystallizationPhase::Conversation);
        assert_eq!(status.depth, DepthLevel::Conversation);
        assert_eq!(status.turn_count, 0);
        assert_eq!(status.profile_completeness, 0.0);
    }

    #[test]
    fn test_add_user_message_increments_turn() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);
        engine.add_user_message("Hello");
        assert_eq!(engine.turn_count(), 1);
        assert_eq!(engine.conversation_history().len(), 1);
    }

    #[test]
    fn test_process_response_extracts_signals() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);

        let response = CompletionResponse {
            content: "Interesting! Tell me more.".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "tc_1".to_string(),
                name: "record_signal".to_string(),
                arguments: serde_json::json!({
                    "signals": [{
                        "instrument": "big_five",
                        "dimension": "openness",
                        "value": 0.8,
                        "confidence": 0.6,
                        "reasoning": "Shows curiosity"
                    }]
                })
                .to_string(),
            }]),
        };

        let result = engine.process_response(&response);
        assert_eq!(result.signals_extracted.len(), 1);
        assert_eq!(result.display_text, "Interesting! Tell me more.");
        assert!(engine.profile().ocean.openness > 0.5);
    }

    #[test]
    fn test_conversation_hard_cap_at_10_turns() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);
        for i in 0..10 {
            engine.add_user_message(&format!("Message {}", i));
        }
        assert!(engine.should_advance_phase());
    }

    #[test]
    fn test_phase_advancement_conversation_depth() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);
        assert_eq!(engine.current_phase(), CrystallizationPhase::Conversation);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::Mirror);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::SoulGeneration);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::Complete);
    }

    #[test]
    fn test_phase_advancement_deep_dive() {
        let mut engine = CrystallizationEngine::new(DepthLevel::DeepDive);
        assert_eq!(engine.current_phase(), CrystallizationPhase::Conversation);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::Mirror);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::Forge);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::SoulGeneration);

        engine.advance_phase();
        assert_eq!(engine.current_phase(), CrystallizationPhase::Complete);
    }

    #[test]
    fn test_parse_record_signal_args_object() {
        let args = serde_json::json!({
            "signals": [{
                "instrument": "big_five",
                "dimension": "openness",
                "value": 0.7,
                "confidence": 0.5,
                "reasoning": "test"
            }]
        })
        .to_string();
        let signals = parse_record_signal_args(&args).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].instrument, "big_five");
    }

    #[test]
    fn test_parse_record_signal_args_single() {
        let args = serde_json::json!({
            "instrument": "attachment",
            "dimension": "secure",
            "value": 0.8,
            "confidence": 0.6,
            "reasoning": "test"
        })
        .to_string();
        let signals = parse_record_signal_args(&args).unwrap();
        assert_eq!(signals.len(), 1);
    }

    #[test]
    fn test_calibrate_ethics_updates_profile() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);
        engine.calibrate_ethics();
        let w = &engine.profile().ethics_weights;
        let sum = w.deontological + w.areteological + w.teleological;
        assert!((sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_question_tracking() {
        let mut engine = CrystallizationEngine::new(DepthLevel::Conversation);
        assert!(!engine.is_question_used("q1"));
        engine.mark_question_used("q1");
        assert!(engine.is_question_used("q1"));
    }
}
