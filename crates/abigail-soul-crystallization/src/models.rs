//! Data models for the Soul Crystallization Protocol.
//!
//! Contains the MentorProfile, psychological instrument scores,
//! ethical framework weights, and conversation state types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Depth level chosen by the mentor at the Spark phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepthLevel {
    /// ~30 seconds: uses existing template system, no conversation.
    QuickStart,
    /// 3-5 minutes: adaptive Socratic dialogue (6-10 turns).
    Conversation,
    /// 10-15 minutes: full dialogue + ethical dilemmas + naming ceremony.
    DeepDive,
}

impl DepthLevel {
    pub fn label(&self) -> &'static str {
        match self {
            DepthLevel::QuickStart => "Quick Start",
            DepthLevel::Conversation => "Conversation",
            DepthLevel::DeepDive => "Deep Dive",
        }
    }

    pub fn estimated_time(&self) -> &'static str {
        match self {
            DepthLevel::QuickStart => "~30 seconds",
            DepthLevel::Conversation => "3-5 minutes",
            DepthLevel::DeepDive => "10-15 minutes",
        }
    }
}

/// Internal phases of the crystallization process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrystallizationPhase {
    /// Phase 0: Intro + depth selection.
    Spark,
    /// Phase 1: Adaptive Socratic dialogue.
    Conversation,
    /// Phase 2: Personality reflection ("Mirror Moment").
    Mirror,
    /// Phase 3: Ethical dilemmas + communication prefs + naming (Deep Dive only).
    Forge,
    /// Phase 4: Soul document generation and review.
    SoulGeneration,
    /// Terminal: crystallization complete.
    Complete,
}

/// Big Five (OCEAN) personality scores.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OceanScores {
    /// Openness to experience (0.0 - 1.0).
    pub openness: f64,
    /// Conscientiousness (0.0 - 1.0).
    pub conscientiousness: f64,
    /// Extraversion (0.0 - 1.0).
    pub extraversion: f64,
    /// Agreeableness (0.0 - 1.0).
    pub agreeableness: f64,
    /// Neuroticism (0.0 - 1.0).
    pub neuroticism: f64,
}

impl OceanScores {
    /// Returns scores initialized to neutral midpoints.
    pub fn neutral() -> Self {
        Self {
            openness: 0.5,
            conscientiousness: 0.5,
            extraversion: 0.5,
            agreeableness: 0.5,
            neuroticism: 0.5,
        }
    }
}

/// Moral Foundations Theory scores (Haidt).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MoralFoundations {
    /// Care/Harm (0.0 - 1.0).
    pub care: f64,
    /// Fairness/Cheating (0.0 - 1.0).
    pub fairness: f64,
    /// Loyalty/Betrayal (0.0 - 1.0).
    pub loyalty: f64,
    /// Authority/Subversion (0.0 - 1.0).
    pub authority: f64,
    /// Sanctity/Degradation (0.0 - 1.0).
    pub sanctity: f64,
    /// Liberty/Oppression (0.0 - 1.0).
    pub liberty: f64,
}

impl MoralFoundations {
    pub fn neutral() -> Self {
        Self {
            care: 0.5,
            fairness: 0.5,
            loyalty: 0.5,
            authority: 0.5,
            sanctity: 0.5,
            liberty: 0.5,
        }
    }
}

/// Attachment style of the mentor (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AttachmentStyle {
    /// Comfortable with closeness and independence.
    #[default]
    Secure,
    /// Desires closeness but fears rejection.
    Anxious,
    /// Values independence, uncomfortable with closeness.
    Avoidant,
    /// Mixed/inconsistent pattern.
    Disorganized,
}

/// Dominant thinking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThinkingMode {
    /// Prefers systematic, analytical approaches.
    Analytical,
    /// Prefers intuitive, holistic approaches.
    Intuitive,
    /// Balances both modes.
    #[default]
    Integrated,
}

/// Cognitive style preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CognitiveStyle {
    pub thinking_mode: ThinkingMode,
    /// How much the mentor values precision vs speed (0=speed, 1=precision).
    pub precision_vs_speed: f64,
    /// How much the mentor values breadth vs depth (0=depth, 1=breadth).
    pub breadth_vs_depth: f64,
}

impl CognitiveStyle {
    pub fn neutral() -> Self {
        Self {
            thinking_mode: ThinkingMode::Integrated,
            precision_vs_speed: 0.5,
            breadth_vs_depth: 0.5,
        }
    }
}

/// Communication preferences (Phase 3 / Deep Dive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationPreference {
    pub dimension: String,
    pub value: String,
}

/// Triangle Ethic weights (must sum to 1.0).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriangleEthicWeights {
    /// Deontological (duty-based).
    pub deontological: f64,
    /// Areteological (virtue-based).
    pub areteological: f64,
    /// Teleological (outcome-based).
    pub teleological: f64,
}

impl Default for TriangleEthicWeights {
    fn default() -> Self {
        Self {
            deontological: 1.0 / 3.0,
            areteological: 1.0 / 3.0,
            teleological: 1.0 / 3.0,
        }
    }
}

impl TriangleEthicWeights {
    /// Normalize weights so they sum to 1.0.
    pub fn normalize(&mut self) {
        let sum = self.deontological + self.areteological + self.teleological;
        if sum > 0.0 {
            self.deontological /= sum;
            self.areteological /= sum;
            self.teleological /= sum;
        } else {
            *self = Self::default();
        }
    }
}

/// A signal extracted from conversation by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// Which instrument this signal applies to (e.g., "big_five", "moral_foundations").
    pub instrument: String,
    /// Which dimension (e.g., "openness", "care").
    pub dimension: String,
    /// Observed value (0.0 - 1.0).
    pub value: f64,
    /// LLM's confidence in this signal (0.0 - 1.0).
    pub confidence: f64,
    /// Brief reasoning for this signal.
    pub reasoning: String,
}

/// A turn in the crystallization conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub role: String,
    pub content: String,
    /// Signals extracted from this turn (if any).
    pub signals: Vec<Signal>,
}

/// The accumulated MentorProfile built during crystallization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentorProfile {
    /// OCEAN personality scores.
    pub ocean: OceanScores,
    /// Confidence per OCEAN dimension (0.0 - 1.0).
    pub ocean_confidence: HashMap<String, f64>,
    /// Moral Foundations scores.
    pub moral_foundations: MoralFoundations,
    /// Confidence per moral foundation (0.0 - 1.0).
    pub moral_foundations_confidence: HashMap<String, f64>,
    /// Attachment style.
    pub attachment_style: AttachmentStyle,
    /// Confidence in attachment style assessment.
    pub attachment_confidence: f64,
    /// Cognitive style.
    pub cognitive_style: CognitiveStyle,
    /// Communication preferences (Deep Dive only).
    pub communication_preferences: Vec<CommunicationPreference>,
    /// Triangle Ethic weights calibrated from profile.
    pub ethics_weights: TriangleEthicWeights,
    /// The "Mirror" text: personality reflection.
    pub mirror_text: Option<String>,
    /// Name choice from the naming ceremony (Deep Dive only).
    pub name_choice: Option<String>,
    /// All raw signals collected.
    pub raw_signals: Vec<Signal>,
    /// Depth level used.
    pub depth: DepthLevel,
    /// Number of conversation turns.
    pub turn_count: usize,
    /// Timestamp of crystallization.
    pub timestamp: Option<String>,
}

impl MentorProfile {
    /// Create a new profile with neutral/default values.
    pub fn new(depth: DepthLevel) -> Self {
        Self {
            ocean: OceanScores::neutral(),
            ocean_confidence: HashMap::new(),
            moral_foundations: MoralFoundations::neutral(),
            moral_foundations_confidence: HashMap::new(),
            attachment_style: AttachmentStyle::default(),
            attachment_confidence: 0.0,
            cognitive_style: CognitiveStyle::neutral(),
            communication_preferences: Vec::new(),
            ethics_weights: TriangleEthicWeights::default(),
            mirror_text: None,
            name_choice: None,
            raw_signals: Vec::new(),
            depth,
            turn_count: 0,
            timestamp: None,
        }
    }

    /// Apply a signal to the profile, updating the relevant dimension.
    pub fn apply_signal(&mut self, signal: &Signal) {
        self.raw_signals.push(signal.clone());

        match signal.instrument.as_str() {
            "big_five" => self.apply_ocean_signal(signal),
            "moral_foundations" => self.apply_moral_signal(signal),
            "attachment" => self.apply_attachment_signal(signal),
            "cognitive" => self.apply_cognitive_signal(signal),
            _ => {
                tracing::debug!("Unknown instrument: {}", signal.instrument);
            }
        }
    }

    fn apply_ocean_signal(&mut self, signal: &Signal) {
        let current_confidence = self
            .ocean_confidence
            .get(&signal.dimension)
            .copied()
            .unwrap_or(0.0);

        // Weighted running average: new value weighted by its confidence
        let total_weight = current_confidence + signal.confidence;
        if total_weight > 0.0 {
            let blend = |old: f64, new: f64| -> f64 {
                (old * current_confidence + new * signal.confidence) / total_weight
            };

            match signal.dimension.as_str() {
                "openness" => self.ocean.openness = blend(self.ocean.openness, signal.value),
                "conscientiousness" => {
                    self.ocean.conscientiousness = blend(self.ocean.conscientiousness, signal.value)
                }
                "extraversion" => {
                    self.ocean.extraversion = blend(self.ocean.extraversion, signal.value)
                }
                "agreeableness" => {
                    self.ocean.agreeableness = blend(self.ocean.agreeableness, signal.value)
                }
                "neuroticism" => {
                    self.ocean.neuroticism = blend(self.ocean.neuroticism, signal.value)
                }
                _ => {}
            }

            self.ocean_confidence
                .insert(signal.dimension.clone(), total_weight.min(1.0));
        }
    }

    fn apply_moral_signal(&mut self, signal: &Signal) {
        let current_confidence = self
            .moral_foundations_confidence
            .get(&signal.dimension)
            .copied()
            .unwrap_or(0.0);

        let total_weight = current_confidence + signal.confidence;
        if total_weight > 0.0 {
            let blend = |old: f64, new: f64| -> f64 {
                (old * current_confidence + new * signal.confidence) / total_weight
            };

            match signal.dimension.as_str() {
                "care" => {
                    self.moral_foundations.care = blend(self.moral_foundations.care, signal.value)
                }
                "fairness" => {
                    self.moral_foundations.fairness =
                        blend(self.moral_foundations.fairness, signal.value)
                }
                "loyalty" => {
                    self.moral_foundations.loyalty =
                        blend(self.moral_foundations.loyalty, signal.value)
                }
                "authority" => {
                    self.moral_foundations.authority =
                        blend(self.moral_foundations.authority, signal.value)
                }
                "sanctity" => {
                    self.moral_foundations.sanctity =
                        blend(self.moral_foundations.sanctity, signal.value)
                }
                "liberty" => {
                    self.moral_foundations.liberty =
                        blend(self.moral_foundations.liberty, signal.value)
                }
                _ => {}
            }

            self.moral_foundations_confidence
                .insert(signal.dimension.clone(), total_weight.min(1.0));
        }
    }

    fn apply_attachment_signal(&mut self, signal: &Signal) {
        // Use the highest-confidence attachment signal
        if signal.confidence > self.attachment_confidence {
            self.attachment_style = match signal.dimension.as_str() {
                "secure" => AttachmentStyle::Secure,
                "anxious" => AttachmentStyle::Anxious,
                "avoidant" => AttachmentStyle::Avoidant,
                "disorganized" => AttachmentStyle::Disorganized,
                _ => return,
            };
            self.attachment_confidence = signal.confidence;
        }
    }

    fn apply_cognitive_signal(&mut self, signal: &Signal) {
        match signal.dimension.as_str() {
            "thinking_mode" => {
                self.cognitive_style.thinking_mode = if signal.value < 0.33 {
                    ThinkingMode::Analytical
                } else if signal.value > 0.66 {
                    ThinkingMode::Intuitive
                } else {
                    ThinkingMode::Integrated
                };
            }
            "precision_vs_speed" => {
                self.cognitive_style.precision_vs_speed = signal.value;
            }
            "breadth_vs_depth" => {
                self.cognitive_style.breadth_vs_depth = signal.value;
            }
            _ => {}
        }
    }

    /// Average confidence across OCEAN dimensions.
    pub fn avg_ocean_confidence(&self) -> f64 {
        if self.ocean_confidence.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.ocean_confidence.values().sum();
        sum / self.ocean_confidence.len() as f64
    }

    /// Average confidence across moral foundation dimensions.
    pub fn avg_moral_confidence(&self) -> f64 {
        if self.moral_foundations_confidence.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.moral_foundations_confidence.values().sum();
        sum / self.moral_foundations_confidence.len() as f64
    }

    /// Overall profile completeness as a percentage (0.0 - 1.0).
    pub fn completeness(&self) -> f64 {
        let ocean_score = self.avg_ocean_confidence();
        let moral_score = self.avg_moral_confidence();
        let attachment_score = self.attachment_confidence;

        // Weighted average: OCEAN 40%, Moral 30%, Attachment 30%
        ocean_score * 0.4 + moral_score * 0.3 + attachment_score * 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_depth_level_labels() {
        assert_eq!(DepthLevel::QuickStart.label(), "Quick Start");
        assert_eq!(DepthLevel::Conversation.label(), "Conversation");
        assert_eq!(DepthLevel::DeepDive.label(), "Deep Dive");
    }

    #[test]
    fn test_triangle_ethic_normalize() {
        let mut weights = TriangleEthicWeights {
            deontological: 2.0,
            areteological: 3.0,
            teleological: 5.0,
        };
        weights.normalize();
        assert!((weights.deontological - 0.2).abs() < 1e-10);
        assert!((weights.areteological - 0.3).abs() < 1e-10);
        assert!((weights.teleological - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_triangle_ethic_normalize_zero() {
        let mut weights = TriangleEthicWeights {
            deontological: 0.0,
            areteological: 0.0,
            teleological: 0.0,
        };
        weights.normalize();
        // Should fall back to default equal weights
        let expected = 1.0 / 3.0;
        assert!((weights.deontological - expected).abs() < 1e-10);
    }

    #[test]
    fn test_mentor_profile_new() {
        let profile = MentorProfile::new(DepthLevel::Conversation);
        assert_eq!(profile.depth, DepthLevel::Conversation);
        assert_eq!(profile.turn_count, 0);
        assert!(profile.ocean_confidence.is_empty());
        assert!(profile.mirror_text.is_none());
    }

    #[test]
    fn test_apply_ocean_signal() {
        let mut profile = MentorProfile::new(DepthLevel::Conversation);
        let signal = Signal {
            instrument: "big_five".to_string(),
            dimension: "openness".to_string(),
            value: 0.8,
            confidence: 0.7,
            reasoning: "Shows curiosity".to_string(),
        };
        profile.apply_signal(&signal);
        assert!(profile.ocean.openness > 0.5); // Moved from neutral toward 0.8
        assert!(profile.ocean_confidence.contains_key("openness"));
    }

    #[test]
    fn test_apply_multiple_signals_blends() {
        let mut profile = MentorProfile::new(DepthLevel::Conversation);

        // First signal: high openness
        profile.apply_signal(&Signal {
            instrument: "big_five".to_string(),
            dimension: "openness".to_string(),
            value: 0.9,
            confidence: 0.5,
            reasoning: "Very open".to_string(),
        });

        // Second signal: low openness but lower confidence
        profile.apply_signal(&Signal {
            instrument: "big_five".to_string(),
            dimension: "openness".to_string(),
            value: 0.3,
            confidence: 0.2,
            reasoning: "Less open here".to_string(),
        });

        // Result should be closer to 0.9 due to higher confidence
        assert!(profile.ocean.openness > 0.6);
    }

    #[test]
    fn test_completeness_zero_for_new_profile() {
        let profile = MentorProfile::new(DepthLevel::QuickStart);
        assert_eq!(profile.completeness(), 0.0);
    }

    #[test]
    fn test_ocean_scores_neutral() {
        let scores = OceanScores::neutral();
        assert_eq!(scores.openness, 0.5);
        assert_eq!(scores.conscientiousness, 0.5);
    }

    #[test]
    fn test_moral_foundations_neutral() {
        let mf = MoralFoundations::neutral();
        assert_eq!(mf.care, 0.5);
        assert_eq!(mf.liberty, 0.5);
    }

    #[test]
    fn test_attachment_signal_highest_confidence_wins() {
        let mut profile = MentorProfile::new(DepthLevel::Conversation);

        profile.apply_signal(&Signal {
            instrument: "attachment".to_string(),
            dimension: "anxious".to_string(),
            value: 0.8,
            confidence: 0.3,
            reasoning: "Some anxiety".to_string(),
        });
        assert_eq!(profile.attachment_style, AttachmentStyle::Anxious);

        profile.apply_signal(&Signal {
            instrument: "attachment".to_string(),
            dimension: "secure".to_string(),
            value: 0.9,
            confidence: 0.6,
            reasoning: "More secure overall".to_string(),
        });
        assert_eq!(profile.attachment_style, AttachmentStyle::Secure);
    }
}
