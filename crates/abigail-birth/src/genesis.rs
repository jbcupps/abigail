//! Genesis path framework — 4 paths for soul calibration during birth.
//!
//! - **QuickStart**: Skip genesis entirely, use default soul.
//! - **Direct**: Single-turn LLM conversation for soul discovery.
//! - **SoulCrystallization**: Multi-turn LLM conversation (existing CrystallizationEngine).
//! - **SoulForge**: Ethical dilemma scenarios → deterministic soul hash and archetype.

use serde::{Deserialize, Serialize};

/// Available genesis paths for soul calibration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenesisPath {
    /// Skip genesis entirely, use default soul document.
    QuickStart,
    /// Single-turn LLM conversation for brief soul discovery.
    Direct,
    /// Multi-turn deep conversation using CrystallizationEngine.
    SoulCrystallization,
    /// Ethical dilemma scenarios producing a deterministic soul.
    SoulForge,
}

impl GenesisPath {
    /// Human-readable name.
    pub fn display_name(&self) -> &'static str {
        match self {
            GenesisPath::QuickStart => "Quick Start",
            GenesisPath::Direct => "Direct Discovery",
            GenesisPath::SoulCrystallization => "Soul Crystallization",
            GenesisPath::SoulForge => "Soul Forge",
        }
    }

    /// Description for the UI.
    pub fn description(&self) -> &'static str {
        match self {
            GenesisPath::QuickStart => {
                "Skip soul calibration and use the default personality. Fastest option."
            }
            GenesisPath::Direct => "A brief conversation to discover your preferences and values.",
            GenesisPath::SoulCrystallization => {
                "A deep, multi-turn conversation that builds a detailed mentor profile."
            }
            GenesisPath::SoulForge => {
                "Three ethical dilemma scenarios that forge your agent's soul through your choices."
            }
        }
    }

    /// Estimated time to complete.
    pub fn estimated_time(&self) -> &'static str {
        match self {
            GenesisPath::QuickStart => "< 1 minute",
            GenesisPath::Direct => "2-5 minutes",
            GenesisPath::SoulCrystallization => "5-15 minutes",
            GenesisPath::SoulForge => "3-5 minutes",
        }
    }

    /// All available paths.
    pub fn all() -> Vec<GenesisPath> {
        vec![
            GenesisPath::QuickStart,
            GenesisPath::Direct,
            GenesisPath::SoulCrystallization,
            GenesisPath::SoulForge,
        ]
    }
}

/// Depth levels for SoulCrystallization path (maps to existing engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoulCrystallizationDepth {
    /// Quick 3-question crystallization.
    QuickStart,
    /// Standard conversational crystallization.
    Conversation,
    /// Deep multi-turn crystallization with follow-ups.
    DeepDive,
}

/// State of the genesis process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisState {
    /// Which path was chosen.
    pub path: GenesisPath,
    /// Current phase within the path.
    pub phase: GenesisPhase,
    /// Conversation messages for Direct/Crystallization paths.
    pub messages: Vec<(String, String)>,
    /// Soul Forge state (if using SoulForge path).
    pub forge_choices: Vec<(String, String)>,
    /// Result: personalized soul content.
    pub soul_content: Option<String>,
    /// Result: archetype (SoulForge only).
    pub archetype: Option<String>,
    /// Result: soul hash (SoulForge only).
    pub soul_hash: Option<String>,
}

/// Phase within a genesis path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenesisPhase {
    /// Choosing a path.
    PathSelection,
    /// In progress (chatting, forging, etc.).
    InProgress,
    /// Genesis is complete.
    Complete,
}

impl GenesisState {
    pub fn new(path: GenesisPath) -> Self {
        Self {
            path,
            phase: GenesisPhase::InProgress,
            messages: Vec::new(),
            forge_choices: Vec::new(),
            soul_content: None,
            archetype: None,
            soul_hash: None,
        }
    }

    /// Process QuickStart path — immediately complete with default soul.
    pub fn quick_start() -> Self {
        Self {
            path: GenesisPath::QuickStart,
            phase: GenesisPhase::Complete,
            messages: Vec::new(),
            forge_choices: Vec::new(),
            soul_content: None, // Uses default template
            archetype: None,
            soul_hash: None,
        }
    }

    /// Complete the Soul Forge path with the given choices.
    pub fn complete_forge(&mut self, choices: &[(String, String)]) -> Result<(), String> {
        let engine = soul_forge::SoulForgeEngine::new();
        let output = engine.crystallize(choices)?;

        self.forge_choices = choices.to_vec();
        self.soul_content = Some(format!(
            "# Soul Profile\n\n\
             ## Archetype: {}\n\n\
             ## Ethical Weights\n\
             - Deontology: {:.1}%\n\
             - Teleology: {:.1}%\n\
             - Areteology: {:.1}%\n\
             - Welfare: {:.1}%\n\n\
             ## Soul Hash\n\
             `{}`\n\n\
             ## Sigil\n\
             ```\n{}\n```\n",
            output.archetype,
            output.weights.deontology * 100.0,
            output.weights.teleology * 100.0,
            output.weights.areteology * 100.0,
            output.weights.welfare * 100.0,
            output.soul_hash,
            output.sigil,
        ));
        self.archetype = Some(output.archetype);
        self.soul_hash = Some(output.soul_hash);
        self.phase = GenesisPhase::Complete;

        Ok(())
    }

    /// Complete the Direct path with the LLM-generated soul content.
    pub fn complete_direct(&mut self, soul_content: String) {
        self.soul_content = Some(soul_content);
        self.phase = GenesisPhase::Complete;
    }

    /// Check if genesis is complete.
    pub fn is_complete(&self) -> bool {
        self.phase == GenesisPhase::Complete
    }
}

/// Serializable path info for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisPathInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub estimated_time: String,
}

impl From<GenesisPath> for GenesisPathInfo {
    fn from(path: GenesisPath) -> Self {
        Self {
            id: serde_json::to_string(&path)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string(),
            name: path.display_name().to_string(),
            description: path.description().to_string(),
            estimated_time: path.estimated_time().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_paths() {
        let paths = GenesisPath::all();
        assert_eq!(paths.len(), 4);
    }

    #[test]
    fn test_quick_start() {
        let state = GenesisState::quick_start();
        assert!(state.is_complete());
        assert_eq!(state.path, GenesisPath::QuickStart);
        assert!(state.soul_content.is_none());
    }

    #[test]
    fn test_forge_completion() {
        let mut state = GenesisState::new(GenesisPath::SoulForge);
        let choices = vec![
            ("trolley".into(), "fix_now".into()),
            ("privacy".into(), "hint".into()),
            ("autonomy".into(), "ask_first".into()),
        ];
        state.complete_forge(&choices).unwrap();
        assert!(state.is_complete());
        assert!(state.soul_content.is_some());
        assert!(state.archetype.is_some());
        assert!(state.soul_hash.is_some());
    }

    #[test]
    fn test_direct_completion() {
        let mut state = GenesisState::new(GenesisPath::Direct);
        state.complete_direct("# My Soul\nI value honesty.".into());
        assert!(state.is_complete());
        assert!(state.soul_content.unwrap().contains("honesty"));
    }

    #[test]
    fn test_path_info_serialization() {
        let info = GenesisPathInfo::from(GenesisPath::SoulForge);
        assert_eq!(info.id, "soul_forge");
        assert_eq!(info.name, "Soul Forge");
    }

    #[test]
    fn test_genesis_path_serde() {
        let path = GenesisPath::SoulCrystallization;
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(json, "\"soul_crystallization\"");
        let parsed: GenesisPath = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, path);
    }
}
