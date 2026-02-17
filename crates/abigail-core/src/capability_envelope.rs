//! Capability policy envelope — gates dangerous operations based on SuperegoL2Mode.

use crate::config::SuperegoL2Mode;
use serde::{Deserialize, Serialize};

/// Describes what capabilities an operation is allowed to use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityEnvelope {
    /// Allow web/network access (HTTP requests, browsing).
    pub allow_web: bool,
    /// Allow command execution (shell, subprocess).
    pub allow_exec: bool,
    /// Allow writing files to disk.
    pub allow_file_write: bool,
    /// Require user confirmation before executing.
    pub require_confirmation: bool,
}

impl Default for CapabilityEnvelope {
    fn default() -> Self {
        Self {
            allow_web: true,
            allow_exec: true,
            allow_file_write: true,
            require_confirmation: false,
        }
    }
}

impl CapabilityEnvelope {
    /// Build a default envelope appropriate for the given Superego L2 mode.
    pub fn for_l2_mode(mode: SuperegoL2Mode) -> Self {
        match mode {
            SuperegoL2Mode::Off => Self::default(),
            SuperegoL2Mode::Advisory => Self {
                allow_web: true,
                allow_exec: true,
                allow_file_write: true,
                // Advisory mode: confirm mutations but don't block
                require_confirmation: true,
            },
            SuperegoL2Mode::Enforce => Self {
                allow_web: true,
                // Enforce mode: block exec and file write without confirmation
                allow_exec: false,
                allow_file_write: false,
                require_confirmation: true,
            },
        }
    }
}

/// The capability that an operation wants to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestedCapability {
    WebAccess,
    ShellExec,
    FileWrite,
    FileRead,
    MemoryWrite,
}

/// Result of evaluating a capability gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityGateResult {
    /// Operation is allowed to proceed.
    Allowed,
    /// Operation needs user confirmation before proceeding.
    NeedsConfirmation(String),
    /// Operation is denied with a reason.
    Denied(String),
}

/// Evaluate whether a requested capability is allowed under the given envelope.
pub fn evaluate_gate(
    envelope: &CapabilityEnvelope,
    capability: RequestedCapability,
) -> CapabilityGateResult {
    match capability {
        RequestedCapability::WebAccess => {
            if !envelope.allow_web {
                CapabilityGateResult::Denied("Web access is not allowed".into())
            } else if envelope.require_confirmation {
                CapabilityGateResult::NeedsConfirmation("Web access requires confirmation".into())
            } else {
                CapabilityGateResult::Allowed
            }
        }
        RequestedCapability::ShellExec => {
            if !envelope.allow_exec {
                CapabilityGateResult::Denied(
                    "Shell execution is not allowed in current safety mode".into(),
                )
            } else if envelope.require_confirmation {
                CapabilityGateResult::NeedsConfirmation(
                    "Shell execution requires confirmation".into(),
                )
            } else {
                CapabilityGateResult::Allowed
            }
        }
        RequestedCapability::FileWrite => {
            if !envelope.allow_file_write {
                CapabilityGateResult::Denied(
                    "File write is not allowed in current safety mode".into(),
                )
            } else if envelope.require_confirmation {
                CapabilityGateResult::NeedsConfirmation("File write requires confirmation".into())
            } else {
                CapabilityGateResult::Allowed
            }
        }
        // Read-only operations are always allowed
        RequestedCapability::FileRead | RequestedCapability::MemoryWrite => {
            CapabilityGateResult::Allowed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_envelope_allows_all() {
        let env = CapabilityEnvelope::default();
        assert_eq!(
            evaluate_gate(&env, RequestedCapability::WebAccess),
            CapabilityGateResult::Allowed
        );
        assert_eq!(
            evaluate_gate(&env, RequestedCapability::ShellExec),
            CapabilityGateResult::Allowed
        );
        assert_eq!(
            evaluate_gate(&env, RequestedCapability::FileWrite),
            CapabilityGateResult::Allowed
        );
    }

    #[test]
    fn test_off_mode_allows_all() {
        let env = CapabilityEnvelope::for_l2_mode(SuperegoL2Mode::Off);
        assert_eq!(
            evaluate_gate(&env, RequestedCapability::ShellExec),
            CapabilityGateResult::Allowed
        );
    }

    #[test]
    fn test_advisory_mode_needs_confirmation() {
        let env = CapabilityEnvelope::for_l2_mode(SuperegoL2Mode::Advisory);
        assert!(matches!(
            evaluate_gate(&env, RequestedCapability::ShellExec),
            CapabilityGateResult::NeedsConfirmation(_)
        ));
        assert!(matches!(
            evaluate_gate(&env, RequestedCapability::FileWrite),
            CapabilityGateResult::NeedsConfirmation(_)
        ));
    }

    #[test]
    fn test_enforce_mode_denies_exec() {
        let env = CapabilityEnvelope::for_l2_mode(SuperegoL2Mode::Enforce);
        assert!(matches!(
            evaluate_gate(&env, RequestedCapability::ShellExec),
            CapabilityGateResult::Denied(_)
        ));
        assert!(matches!(
            evaluate_gate(&env, RequestedCapability::FileWrite),
            CapabilityGateResult::Denied(_)
        ));
    }

    #[test]
    fn test_read_always_allowed() {
        let env = CapabilityEnvelope::for_l2_mode(SuperegoL2Mode::Enforce);
        assert_eq!(
            evaluate_gate(&env, RequestedCapability::FileRead),
            CapabilityGateResult::Allowed
        );
    }
}
