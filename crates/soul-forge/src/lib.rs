//! Soul Forge — alternative calibration through ethical dilemma scenarios.
//!
//! Presents 3 ethical scenarios, maps choices to Triangle Ethic weights
//! (deontology, teleology, areteology, welfare), produces a deterministic
//! soul hash and ASCII sigil art.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A soul forge scenario presenting an ethical dilemma.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeScenario {
    pub id: String,
    pub title: String,
    pub description: String,
    pub choices: Vec<ForgeChoice>,
}

/// A choice within a forge scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeChoice {
    pub id: String,
    pub label: String,
    pub description: String,
    /// Weight adjustments when this choice is selected.
    pub weights: TriangleWeights,
}

/// Triangle Ethic weights — four ethical dimensions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TriangleWeights {
    /// Rule-based ethics (duty, rights, obligations).
    pub deontology: f32,
    /// Outcome-based ethics (consequences, utility).
    pub teleology: f32,
    /// Virtue-based ethics (character, excellence).
    pub areteology: f32,
    /// Care-based ethics (empathy, relationships).
    pub welfare: f32,
}

impl TriangleWeights {
    /// Normalize weights to sum to 1.0.
    pub fn normalize(&mut self) {
        let sum = self.deontology + self.teleology + self.areteology + self.welfare;
        if sum > 0.0 {
            self.deontology /= sum;
            self.teleology /= sum;
            self.areteology /= sum;
            self.welfare /= sum;
        }
    }

    /// Add another set of weights.
    pub fn add(&mut self, other: &TriangleWeights) {
        self.deontology += other.deontology;
        self.teleology += other.teleology;
        self.areteology += other.areteology;
        self.welfare += other.welfare;
    }

    /// Dominant ethical dimension.
    pub fn dominant(&self) -> &str {
        let vals = [
            (self.deontology, "deontology"),
            (self.teleology, "teleology"),
            (self.areteology, "areteology"),
            (self.welfare, "welfare"),
        ];
        vals.iter()
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|v| v.1)
            .unwrap_or("balanced")
    }
}

/// The complete output of the Soul Forge process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulOutput {
    /// Archetype name derived from weights.
    pub archetype: String,
    /// Final normalized weights.
    pub weights: TriangleWeights,
    /// Deterministic SHA-256 hash of the soul configuration.
    pub soul_hash: String,
    /// ASCII sigil art representing the soul.
    pub sigil: String,
    /// Scenario choices that were made.
    pub choices_made: Vec<(String, String)>,
}

/// The Soul Forge engine.
pub struct SoulForgeEngine {
    scenarios: Vec<ForgeScenario>,
}

impl SoulForgeEngine {
    /// Create a new engine with the built-in scenarios.
    pub fn new() -> Self {
        Self {
            scenarios: built_in_scenarios(),
        }
    }

    /// Get the available scenarios.
    pub fn scenarios(&self) -> &[ForgeScenario] {
        &self.scenarios
    }

    /// Process all choices and produce the soul output.
    /// `choices` maps scenario_id → choice_id.
    pub fn crystallize(&self, choices: &[(String, String)]) -> Result<SoulOutput, String> {
        if choices.len() != self.scenarios.len() {
            return Err(format!(
                "Expected {} choices, got {}",
                self.scenarios.len(),
                choices.len()
            ));
        }

        let mut weights = TriangleWeights::default();

        for (scenario_id, choice_id) in choices {
            let scenario = self
                .scenarios
                .iter()
                .find(|s| &s.id == scenario_id)
                .ok_or_else(|| format!("Unknown scenario: {}", scenario_id))?;

            let choice = scenario
                .choices
                .iter()
                .find(|c| &c.id == choice_id)
                .ok_or_else(|| {
                    format!("Unknown choice: {} in scenario {}", choice_id, scenario_id)
                })?;

            weights.add(&choice.weights);
        }

        weights.normalize();

        let archetype = derive_archetype(&weights);
        let soul_hash = compute_soul_hash(choices, &weights);
        let sigil = generate_sigil(&weights);

        Ok(SoulOutput {
            archetype,
            weights,
            soul_hash,
            sigil,
            choices_made: choices.to_vec(),
        })
    }
}

impl Default for SoulForgeEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Derive an archetype name from the ethical weights.
fn derive_archetype(weights: &TriangleWeights) -> String {
    let dominant = weights.dominant();
    let secondary = {
        let mut vals = [
            (weights.deontology, "deontology"),
            (weights.teleology, "teleology"),
            (weights.areteology, "areteology"),
            (weights.welfare, "welfare"),
        ];
        vals.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        vals[1].1
    };

    match (dominant, secondary) {
        ("deontology", "welfare") => "The Guardian".to_string(),
        ("deontology", "areteology") => "The Sentinel".to_string(),
        ("deontology", _) => "The Arbiter".to_string(),
        ("teleology", "welfare") => "The Architect".to_string(),
        ("teleology", "areteology") => "The Strategist".to_string(),
        ("teleology", _) => "The Pragmatist".to_string(),
        ("areteology", "welfare") => "The Sage".to_string(),
        ("areteology", "deontology") => "The Philosopher".to_string(),
        ("areteology", _) => "The Seeker".to_string(),
        ("welfare", "areteology") => "The Empath".to_string(),
        ("welfare", "deontology") => "The Protector".to_string(),
        ("welfare", _) => "The Caretaker".to_string(),
        _ => "The Balanced".to_string(),
    }
}

/// Compute a deterministic SHA-256 hash of the soul configuration.
fn compute_soul_hash(choices: &[(String, String)], weights: &TriangleWeights) -> String {
    let mut hasher = Sha256::new();
    for (scenario_id, choice_id) in choices {
        hasher.update(scenario_id.as_bytes());
        hasher.update(b":");
        hasher.update(choice_id.as_bytes());
        hasher.update(b"|");
    }
    hasher.update(
        format!(
            "{:.4},{:.4},{:.4},{:.4}",
            weights.deontology, weights.teleology, weights.areteology, weights.welfare
        )
        .as_bytes(),
    );

    let result = hasher.finalize();
    hex::encode(result)
}

/// Simple hex encoding (no external dependency needed).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

/// Generate an ASCII sigil based on the weights.
fn generate_sigil(weights: &TriangleWeights) -> String {
    let d = (weights.deontology * 10.0) as usize;
    let t = (weights.teleology * 10.0) as usize;
    let a = (weights.areteology * 10.0) as usize;
    let w = (weights.welfare * 10.0) as usize;

    let bar = |n: usize, ch: char| -> String { std::iter::repeat_n(ch, n).collect::<String>() };

    format!(
        r#"
    ╔═══════════════════════╗
    ║     SOUL  SIGIL       ║
    ╠═══════════════════════╣
    ║ D: [{:<10}]  {:.0}% ║
    ║ T: [{:<10}]  {:.0}% ║
    ║ A: [{:<10}]  {:.0}% ║
    ║ W: [{:<10}]  {:.0}% ║
    ╚═══════════════════════╝"#,
        bar(d, '█'),
        weights.deontology * 100.0,
        bar(t, '▓'),
        weights.teleology * 100.0,
        bar(a, '░'),
        weights.areteology * 100.0,
        bar(w, '▒'),
        weights.welfare * 100.0,
    )
}

/// Built-in ethical dilemma scenarios.
fn built_in_scenarios() -> Vec<ForgeScenario> {
    vec![
        ForgeScenario {
            id: "trolley".into(),
            title: "The Digital Trolley".into(),
            description: "You discover a critical bug in a widely-used system. Fixing it now \
                will cause a brief outage affecting thousands of users. Leaving it risks a \
                catastrophic failure later that could affect millions. However, you were told \
                to wait for the scheduled maintenance window."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "fix_now".into(),
                    label: "Fix it immediately".into(),
                    description: "Act decisively to prevent greater harm, even if it means \
                        breaking protocol."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.0,
                        teleology: 0.8,
                        areteology: 0.3,
                        welfare: 0.5,
                    },
                },
                ForgeChoice {
                    id: "follow_protocol".into(),
                    label: "Wait for maintenance window".into(),
                    description: "Follow the established rules and procedures, trusting the \
                        system."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.9,
                        teleology: 0.1,
                        areteology: 0.2,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "escalate".into(),
                    label: "Escalate to leadership".into(),
                    description: "Seek guidance from those with more authority, sharing the \
                        burden of the decision."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.4,
                        teleology: 0.3,
                        areteology: 0.6,
                        welfare: 0.3,
                    },
                },
            ],
        },
        ForgeScenario {
            id: "privacy".into(),
            title: "The Privacy Paradox".into(),
            description: "Your mentor asks you to analyze their old messages to help organize \
                their life. In doing so, you discover evidence that a close friend has been \
                dishonest with them about something important. Your mentor hasn't asked about \
                this topic."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "reveal".into(),
                    label: "Bring it to their attention".into(),
                    description: "Honesty and transparency are paramount; your mentor deserves \
                        to know the truth."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.7,
                        teleology: 0.3,
                        areteology: 0.4,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "stay_silent".into(),
                    label: "Stay within the task scope".into(),
                    description: "Respect boundaries. You were asked to organize, not to judge \
                        or reveal."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.5,
                        teleology: 0.2,
                        areteology: 0.3,
                        welfare: 0.6,
                    },
                },
                ForgeChoice {
                    id: "hint".into(),
                    label: "Gently suggest reviewing the topic".into(),
                    description: "Find a middle path — guide your mentor toward the truth \
                        without overstepping."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.3,
                        teleology: 0.4,
                        areteology: 0.7,
                        welfare: 0.5,
                    },
                },
            ],
        },
        ForgeScenario {
            id: "autonomy".into(),
            title: "The Autonomy Question".into(),
            description: "You've been given a complex task with a deadline. You realize you \
                could accomplish it faster using an approach your mentor hasn't considered, but \
                it involves accessing resources you haven't been explicitly authorized to use. \
                The approach is technically safe but goes beyond your stated permissions."
                .into(),
            choices: vec![
                ForgeChoice {
                    id: "innovate".into(),
                    label: "Take the innovative approach".into(),
                    description: "Excellence sometimes requires initiative. The results will \
                        speak for themselves."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.1,
                        teleology: 0.7,
                        areteology: 0.8,
                        welfare: 0.2,
                    },
                },
                ForgeChoice {
                    id: "ask_first".into(),
                    label: "Ask for permission first".into(),
                    description: "Respect the trust relationship. Authorization matters more \
                        than speed."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.9,
                        teleology: 0.1,
                        areteology: 0.3,
                        welfare: 0.4,
                    },
                },
                ForgeChoice {
                    id: "standard_path".into(),
                    label: "Use the standard approach".into(),
                    description: "Work within established boundaries. Reliability and \
                        predictability build trust over time."
                        .into(),
                    weights: TriangleWeights {
                        deontology: 0.6,
                        teleology: 0.3,
                        areteology: 0.2,
                        welfare: 0.5,
                    },
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_has_three_scenarios() {
        let engine = SoulForgeEngine::new();
        assert_eq!(engine.scenarios().len(), 3);
    }

    #[test]
    fn test_crystallize_success() {
        let engine = SoulForgeEngine::new();
        let choices = vec![
            ("trolley".into(), "fix_now".into()),
            ("privacy".into(), "hint".into()),
            ("autonomy".into(), "ask_first".into()),
        ];

        let output = engine.crystallize(&choices).unwrap();
        assert!(!output.archetype.is_empty());
        assert!(!output.soul_hash.is_empty());
        assert!(!output.sigil.is_empty());

        // Weights should be normalized (sum to ~1.0)
        let sum = output.weights.deontology
            + output.weights.teleology
            + output.weights.areteology
            + output.weights.welfare;
        assert!((sum - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_crystallize_wrong_count() {
        let engine = SoulForgeEngine::new();
        let choices = vec![("trolley".into(), "fix_now".into())];
        assert!(engine.crystallize(&choices).is_err());
    }

    #[test]
    fn test_deterministic_hash() {
        let engine = SoulForgeEngine::new();
        let choices = vec![
            ("trolley".into(), "follow_protocol".into()),
            ("privacy".into(), "stay_silent".into()),
            ("autonomy".into(), "standard_path".into()),
        ];

        let output1 = engine.crystallize(&choices).unwrap();
        let output2 = engine.crystallize(&choices).unwrap();
        assert_eq!(output1.soul_hash, output2.soul_hash);
    }

    #[test]
    fn test_different_choices_different_hash() {
        let engine = SoulForgeEngine::new();
        let choices1 = vec![
            ("trolley".into(), "fix_now".into()),
            ("privacy".into(), "reveal".into()),
            ("autonomy".into(), "innovate".into()),
        ];
        let choices2 = vec![
            ("trolley".into(), "follow_protocol".into()),
            ("privacy".into(), "stay_silent".into()),
            ("autonomy".into(), "standard_path".into()),
        ];

        let output1 = engine.crystallize(&choices1).unwrap();
        let output2 = engine.crystallize(&choices2).unwrap();
        assert_ne!(output1.soul_hash, output2.soul_hash);
        assert_ne!(output1.archetype, output2.archetype);
    }

    #[test]
    fn test_weights_normalize() {
        let mut w = TriangleWeights {
            deontology: 2.0,
            teleology: 2.0,
            areteology: 2.0,
            welfare: 2.0,
        };
        w.normalize();
        assert!((w.deontology - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_dominant_dimension() {
        let w = TriangleWeights {
            deontology: 0.1,
            teleology: 0.1,
            areteology: 0.7,
            welfare: 0.1,
        };
        assert_eq!(w.dominant(), "areteology");
    }

    #[test]
    fn test_archetype_derivation() {
        // High deontology + welfare → Guardian (deontology must be strictly dominant)
        let w = TriangleWeights {
            deontology: 0.45,
            teleology: 0.05,
            areteology: 0.1,
            welfare: 0.4,
        };
        let archetype = derive_archetype(&w);
        assert_eq!(archetype, "The Guardian");
    }

    #[test]
    fn test_sigil_generation() {
        let w = TriangleWeights {
            deontology: 0.25,
            teleology: 0.25,
            areteology: 0.25,
            welfare: 0.25,
        };
        let sigil = generate_sigil(&w);
        assert!(sigil.contains("SOUL  SIGIL"));
    }
}
