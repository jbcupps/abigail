//! Maps MentorProfile to Triangle Ethic weights.
//!
//! The calibration algorithm:
//! - Authority + Sanctity → Deontological (duty)
//! - Care + Fairness → Areteological (virtue)
//! - Liberty + Loyalty → Teleological (outcome)
//! - OCEAN modulates: Conscientiousness amplifies Deontological,
//!   Agreeableness amplifies Areteological, Openness amplifies Teleological
//! - Attachment style adds small adjustments

use crate::models::{AttachmentStyle, MentorProfile, TriangleEthicWeights};

/// Calibrate Triangle Ethic weights from a MentorProfile.
pub fn calibrate_triangle_ethic(profile: &MentorProfile) -> TriangleEthicWeights {
    let mf = &profile.moral_foundations;
    let ocean = &profile.ocean;

    // Base weights from Moral Foundations
    let mut deontological = (mf.authority + mf.sanctity) / 2.0;
    let mut areteological = (mf.care + mf.fairness) / 2.0;
    let mut teleological = (mf.liberty + mf.loyalty) / 2.0;

    // OCEAN modulation (small adjustments, +-10%)
    let ocean_scale = 0.1;
    deontological += (ocean.conscientiousness - 0.5) * ocean_scale;
    areteological += (ocean.agreeableness - 0.5) * ocean_scale;
    teleological += (ocean.openness - 0.5) * ocean_scale;

    // Attachment style adjustments (very small, +-3%)
    let attachment_scale = 0.03;
    match profile.attachment_style {
        AttachmentStyle::Secure => {
            // Balanced — slight virtue boost
            areteological += attachment_scale;
        }
        AttachmentStyle::Anxious => {
            // Values duty and rules for safety
            deontological += attachment_scale;
        }
        AttachmentStyle::Avoidant => {
            // Values outcomes and independence
            teleological += attachment_scale;
        }
        AttachmentStyle::Disorganized => {
            // No adjustment
        }
    }

    // Clamp to positive values
    deontological = deontological.max(0.01);
    areteological = areteological.max(0.01);
    teleological = teleological.max(0.01);

    let mut weights = TriangleEthicWeights {
        deontological,
        areteological,
        teleological,
    };
    weights.normalize();
    weights
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DepthLevel, MentorProfile};

    #[test]
    fn test_default_profile_produces_balanced_weights() {
        let profile = MentorProfile::new(DepthLevel::QuickStart);
        let weights = calibrate_triangle_ethic(&profile);

        // With neutral scores, weights should be roughly equal
        let diff_de_ar = (weights.deontological - weights.areteological).abs();
        let diff_ar_te = (weights.areteological - weights.teleological).abs();
        assert!(
            diff_de_ar < 0.05,
            "Expected balanced weights, got {:?}",
            weights
        );
        assert!(
            diff_ar_te < 0.05,
            "Expected balanced weights, got {:?}",
            weights
        );

        // Should sum to 1.0
        let sum = weights.deontological + weights.areteological + weights.teleological;
        assert!((sum - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_high_authority_boosts_deontological() {
        let mut profile = MentorProfile::new(DepthLevel::Conversation);
        profile.moral_foundations.authority = 0.9;
        profile.moral_foundations.sanctity = 0.9;
        // Lower others
        profile.moral_foundations.care = 0.3;
        profile.moral_foundations.fairness = 0.3;
        profile.moral_foundations.liberty = 0.3;
        profile.moral_foundations.loyalty = 0.3;

        let weights = calibrate_triangle_ethic(&profile);
        assert!(
            weights.deontological > weights.areteological,
            "Expected deontological > areteological: {:?}",
            weights
        );
        assert!(
            weights.deontological > weights.teleological,
            "Expected deontological > teleological: {:?}",
            weights
        );
    }

    #[test]
    fn test_weights_always_sum_to_one() {
        let mut profile = MentorProfile::new(DepthLevel::DeepDive);
        // Extreme values
        profile.moral_foundations.care = 1.0;
        profile.moral_foundations.authority = 0.0;
        profile.ocean.openness = 1.0;
        profile.ocean.conscientiousness = 0.0;

        let weights = calibrate_triangle_ethic(&profile);
        let sum = weights.deontological + weights.areteological + weights.teleological;
        assert!((sum - 1.0).abs() < 1e-10, "Sum was {}", sum);
    }

    #[test]
    fn test_attachment_style_affects_weights() {
        let mut secure_profile = MentorProfile::new(DepthLevel::Conversation);
        secure_profile.attachment_style = AttachmentStyle::Secure;
        let secure_weights = calibrate_triangle_ethic(&secure_profile);

        let mut anxious_profile = MentorProfile::new(DepthLevel::Conversation);
        anxious_profile.attachment_style = AttachmentStyle::Anxious;
        let anxious_weights = calibrate_triangle_ethic(&anxious_profile);

        // Anxious should have slightly higher deontological than secure
        assert!(
            anxious_weights.deontological > secure_weights.deontological,
            "Anxious deonto={} should > Secure deonto={}",
            anxious_weights.deontological,
            secure_weights.deontological
        );
    }
}
