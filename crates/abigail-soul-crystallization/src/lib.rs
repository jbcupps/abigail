//! Soul Crystallization Protocol for Project Abigail.
//!
//! Replaces the static template-based soul generation with an adaptive,
//! LLM-driven conversational experience that builds a psychologically-grounded
//! MentorProfile and generates a deeply personalized soul document.
//!
//! Three depth levels:
//! - **Quick Start**: ~30s, equivalent to current static template system
//! - **Conversation**: 3-5 min, adaptive Socratic dialogue (6-10 turns)
//! - **Deep Dive**: 10-15 min, full dialogue + ethical dilemmas + naming
//!
//! # Architecture
//!
//! The crate exposes a `CrystallizationEngine` state machine that accepts
//! `&dyn LlmProvider` injected by the caller. It does NOT depend on
//! `abigail-router` or `abigail-birth`.

pub mod engine;
pub mod ethics_calibrator;
pub mod models;

pub use engine::{CrystallizationEngine, CrystallizationStatus, ProcessResult};
pub use ethics_calibrator::calibrate_triangle_ethic;
pub use models::{
    AttachmentStyle, CognitiveStyle, CommunicationPreference, ConversationTurn,
    CrystallizationPhase, DepthLevel, MentorProfile, MoralFoundations, OceanScores, Signal,
    ThinkingMode, TriangleEthicWeights,
};
