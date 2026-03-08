//! Memory layer definitions for the Local Orion hierarchy.
//!
//! Local Orion Memory Hierarchy - SAO_Orion Section 7.3 + Decentralized Trust
//! Framework runtime ethical enforcement.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Tiered memory layers for an Abigail Entity.
///
/// Local Orion Memory Hierarchy - SAO_Orion Section 7.3 + Decentralized Trust
/// Framework runtime ethical enforcement.
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(feature = "postgres", sqlx(type_name = "memory_layer"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLayer {
    /// Short-term buffer for the last 30-60 minutes of activity.
    Working,
    /// Personal events and conversations that form narrative recall.
    Episodic,
    /// Facts, relationships, and durable knowledge.
    Semantic,
    /// Immutable soul and core personality updates tied to birth documents.
    Crystallized,
}

impl MemoryLayer {
    /// Returns the retention window for the layer.
    pub fn retention_policy(&self) -> Duration {
        match self {
            MemoryLayer::Working => Duration::from_secs(60 * 60 * 2),
            MemoryLayer::Episodic => Duration::from_secs(60 * 60 * 24 * 30),
            MemoryLayer::Semantic => Duration::from_secs(60 * 60 * 24 * 365 * 2),
            MemoryLayer::Crystallized => Duration::MAX,
        }
    }

    /// Returns the retrieval priority weight for the layer.
    pub fn priority_weight(&self) -> f32 {
        match self {
            MemoryLayer::Working => 0.3,
            MemoryLayer::Episodic => 0.6,
            MemoryLayer::Semantic => 0.9,
            MemoryLayer::Crystallized => 1.0,
        }
    }

    /// Returns whether the layer is immutable after persistence.
    pub fn is_immutable(&self) -> bool {
        matches!(self, MemoryLayer::Crystallized)
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryLayer;
    use std::time::Duration;

    #[test]
    fn crystallized_layer_is_immutable() {
        assert!(MemoryLayer::Crystallized.is_immutable());
        assert!(!MemoryLayer::Working.is_immutable());
    }

    #[test]
    fn retention_policies_match_expected_windows() {
        assert_eq!(
            MemoryLayer::Working.retention_policy(),
            Duration::from_secs(60 * 60 * 2)
        );
        assert_eq!(
            MemoryLayer::Episodic.retention_policy(),
            Duration::from_secs(60 * 60 * 24 * 30)
        );
        assert_eq!(
            MemoryLayer::Semantic.retention_policy(),
            Duration::from_secs(60 * 60 * 24 * 365 * 2)
        );
        assert_eq!(MemoryLayer::Crystallized.retention_policy(), Duration::MAX);
    }
}
