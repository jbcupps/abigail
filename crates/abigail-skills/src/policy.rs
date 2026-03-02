//! Runtime skill execution policy (approval + signed allowlist verification).
//!
//! Enforces:
//! - Optional legacy approval list (`approved_skill_ids`)
//! - Signed allowlist entry verification (Ed25519)
//! - Trusted signer allowlist (`trusted_skill_signers`)
//!
//! Fail-closed behavior:
//! - Active signed entry with invalid signature => deny.
//! - Active signed entry from untrusted signer => deny.
//! - Strict signed mode for external skills when trusted signers are configured.

use std::collections::{HashMap, HashSet};

use abigail_core::{config::SignedSkillAllowlistEntry, AppConfig};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};

#[derive(Debug, Clone, Default)]
pub struct SkillExecutionPolicy {
    approved_skill_ids: HashSet<String>,
    active_signed_entries: HashMap<String, SignedSkillAllowlistEntry>,
    trusted_signers: HashMap<String, VerifyingKey>,
    strict_signed_external: bool,
    configuration_error: Option<String>,
}

impl SkillExecutionPolicy {
    pub fn from_app_config(config: &AppConfig) -> Self {
        let mut trusted_signers = HashMap::new();
        let mut configuration_error = None;

        for signer in &config.trusted_skill_signers {
            let trimmed = signer.trim();
            if trimmed.is_empty() {
                continue;
            }

            match decode_signer_key(trimmed) {
                Ok(key) => {
                    trusted_signers.insert(trimmed.to_string(), key);
                }
                Err(e) => {
                    configuration_error =
                        Some(format!("Invalid trusted signer key '{}': {}", trimmed, e));
                    break;
                }
            }
        }

        let approved_skill_ids = config
            .approved_skill_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(|id| id.to_string())
            .collect::<HashSet<_>>();

        let active_signed_entries = config
            .signed_skill_allowlist
            .iter()
            .filter(|entry| entry.active)
            .map(|entry| (entry.skill_id.clone(), entry.clone()))
            .collect::<HashMap<_, _>>();

        Self {
            approved_skill_ids,
            active_signed_entries,
            trusted_signers,
            // When trusted signers are configured, external skills require
            // a valid signed allowlist entry.
            strict_signed_external: !config.trusted_skill_signers.is_empty(),
            configuration_error,
        }
    }

    pub fn evaluate_activation(&self, skill_id: &str) -> Result<(), String> {
        if let Some(err) = &self.configuration_error {
            return Err(format!("Skill policy configuration error: {}", err));
        }

        let has_valid_signed_entry = self.verify_active_signed_entry(skill_id)?;

        if self.strict_signed_external && is_external_skill(skill_id) && !has_valid_signed_entry {
            return Err(format!(
                "Skill '{}' is blocked: signed allowlist verification is required for external skills and no valid active entry was found.",
                skill_id
            ));
        }

        Ok(())
    }

    pub fn evaluate_execution(&self, skill_id: &str) -> Result<(), String> {
        self.evaluate_activation(skill_id)?;

        let has_valid_signed_entry = self.active_signed_entries.contains_key(skill_id)
            && self.verify_active_signed_entry(skill_id)?;

        if !self.approved_skill_ids.is_empty()
            && !self.approved_skill_ids.contains(skill_id)
            && !has_valid_signed_entry
        {
            return Err(format!(
                "Skill '{}' is blocked: not in approved_skill_ids and no valid signed allowlist entry is active.",
                skill_id
            ));
        }

        Ok(())
    }

    fn verify_active_signed_entry(&self, skill_id: &str) -> Result<bool, String> {
        let Some(entry) = self.active_signed_entries.get(skill_id) else {
            return Ok(false);
        };

        let Some(verifying_key) = self.trusted_signers.get(entry.signer.trim()) else {
            return Err(format!(
                "Skill '{}' is blocked: signed allowlist entry signer '{}' is not in trusted_skill_signers.",
                skill_id, entry.signer
            ));
        };

        let signature_bytes = BASE64
            .decode(entry.signature.trim())
            .map_err(|e| format!("Invalid base64 signature for '{}': {}", skill_id, e))?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|e| format!("Invalid Ed25519 signature for '{}': {}", skill_id, e))?;

        let payload = signed_allowlist_payload(entry);
        verifying_key
            .verify(payload.as_bytes(), &signature)
            .map_err(|_| {
                format!(
                    "Skill '{}' is blocked: signed allowlist signature verification failed for signer '{}'.",
                    skill_id, entry.signer
                )
            })?;

        Ok(true)
    }
}

fn decode_signer_key(raw: &str) -> Result<VerifyingKey, String> {
    let decoded = BASE64
        .decode(raw)
        .map_err(|e| format!("base64 decode failed: {}", e))?;
    if decoded.len() != 32 {
        return Err(format!(
            "expected 32-byte Ed25519 public key, got {} bytes",
            decoded.len()
        ));
    }
    let bytes: [u8; 32] = decoded
        .try_into()
        .map_err(|_| "expected 32-byte Ed25519 public key".to_string())?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| e.to_string())
}

/// Build the canonical payload string for a signed skill allowlist entry.
///
/// This is the exact format that must be Ed25519-signed to create a valid
/// `SignedSkillAllowlistEntry`. Both the runtime verifier and the
/// `entity-cli skill-sign` command share this function.
pub fn build_allowlist_payload(skill_id: &str, signer: &str, source: &str, active: bool) -> String {
    format!(
        "abigail-signed-skill-allowlist-v1\nskill_id={}\nsigner={}\nsource={}\nactive={}",
        skill_id, signer, source, active
    )
}

fn signed_allowlist_payload(entry: &SignedSkillAllowlistEntry) -> String {
    build_allowlist_payload(&entry.skill_id, &entry.signer, &entry.source, entry.active)
}

fn is_external_skill(skill_id: &str) -> bool {
    !(skill_id.starts_with("builtin.")
        || skill_id.starts_with("com.abigail.skills.")
        || skill_id.starts_with("skill."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_core::config::SignedSkillAllowlistEntry;
    use ed25519_dalek::Signer as _;

    fn base_config() -> AppConfig {
        AppConfig::default_paths()
    }

    fn sign_payload(
        signing_key: &ed25519_dalek::SigningKey,
        entry: &SignedSkillAllowlistEntry,
    ) -> String {
        let payload = signed_allowlist_payload(entry);
        let sig = signing_key.sign(payload.as_bytes());
        BASE64.encode(sig.to_bytes())
    }

    #[test]
    fn valid_signed_entry_allows_external_skill() {
        let mut config = base_config();
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let signer = BASE64.encode(signing_key.verifying_key().to_bytes());
        config.trusted_skill_signers = vec![signer.clone()];

        let mut entry = SignedSkillAllowlistEntry {
            skill_id: "dynamic.github_api".to_string(),
            signer: signer.clone(),
            signature: String::new(),
            source: "test".to_string(),
            added_at: "2026-03-01T00:00:00Z".to_string(),
            active: true,
        };
        entry.signature = sign_payload(&signing_key, &entry);
        config.signed_skill_allowlist = vec![entry];

        let policy = SkillExecutionPolicy::from_app_config(&config);
        assert!(policy.evaluate_execution("dynamic.github_api").is_ok());
    }

    #[test]
    fn invalid_signature_fails_closed() {
        let mut config = base_config();
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let signer = BASE64.encode(signing_key.verifying_key().to_bytes());
        config.trusted_skill_signers = vec![signer.clone()];
        config.signed_skill_allowlist = vec![SignedSkillAllowlistEntry {
            skill_id: "dynamic.github_api".to_string(),
            signer,
            signature: BASE64.encode([0u8; 64]),
            source: "test".to_string(),
            added_at: "2026-03-01T00:00:00Z".to_string(),
            active: true,
        }];

        let policy = SkillExecutionPolicy::from_app_config(&config);
        let err = policy.evaluate_execution("dynamic.github_api").unwrap_err();
        assert!(err.contains("verification failed"));
    }

    #[test]
    fn strict_signed_mode_blocks_unsigned_external_skill() {
        let mut config = base_config();
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let signer = BASE64.encode(signing_key.verifying_key().to_bytes());
        config.trusted_skill_signers = vec![signer];

        let policy = SkillExecutionPolicy::from_app_config(&config);
        let err = policy.evaluate_execution("dynamic.untrusted").unwrap_err();
        assert!(err.contains("signed allowlist verification is required"));
    }
}
