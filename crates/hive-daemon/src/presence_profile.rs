//! Presentation metadata only. These settings never grant permissions or alter
//! the entity's name, system prompt, provider, or birth configuration.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

pub const MAX_PROFILE_BYTES: usize = 32 * 1024;
const PROFILE_FILE: &str = "presence.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Temperament {
    Gentle,
    Direct,
    Curious,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualIdentity {
    Aether,
    Ember,
    Lumen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceProfile {
    /// Display label only; changing this does not rename the registered entity.
    pub name: String,
    pub purpose: String,
    pub temperament: Temperament,
    /// Reviewable design notes, not an enforcement policy or permission grant.
    pub boundaries: String,
    pub identity: VisualIdentity,
}

impl PresenceProfile {
    pub fn for_entity(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            purpose: String::new(),
            temperament: Temperament::Gentle,
            boundaries: String::new(),
            identity: VisualIdentity::Aether,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err("Display name must contain 1 to 80 characters".into());
        }
        if self.name.chars().any(char::is_control) {
            return Err("Display name cannot contain control characters".into());
        }
        for (label, value) in [("Purpose", &self.purpose), ("Boundaries", &self.boundaries)] {
            if value.chars().count() > 2000 {
                return Err(format!("{label} must contain at most 2000 characters"));
            }
            if value
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(format!("{label} contains unsupported control characters"));
            }
        }
        Ok(())
    }
}

/// Only a missing sidecar receives defaults. Corrupt or unreadable saved data
/// must surface an error so callers never mistake a failed load for a reset.
pub fn load_profile(directory: &Path, entity_name: &str) -> Result<PresenceProfile, String> {
    let path = directory.join(PROFILE_FILE);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PresenceProfile::for_entity(entity_name));
        }
        Err(error) => return Err(format!("Could not read saved presence: {error}")),
    };
    let mut bytes = Vec::new();
    file.take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Could not read saved presence: {error}"))?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("Saved presence exceeds the 32 KiB size limit".into());
    }
    let profile: PresenceProfile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Saved presence is invalid: {error}"))?;
    profile
        .validate()
        .map_err(|error| format!("Saved presence is invalid: {error}"))?;
    Ok(profile)
}

/// The directory must come from IdentityManager::agent_dir, never request data.
pub fn save_profile(directory: &Path, profile: &PresenceProfile) -> Result<(), String> {
    profile.validate()?;
    let bytes = serde_json::to_vec_pretty(profile)
        .map_err(|error| format!("Could not encode presence: {error}"))?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("Presence exceeds the 32 KiB size limit".into());
    }
    abigail_core::secure_fs::write_bytes_atomic(&directory.join(PROFILE_FILE), &bytes)
        .map_err(|error| format!("Could not save presence: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("abigail-presence-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_profile_defaults_to_the_registered_name() {
        let directory = TestDirectory::new();
        let profile = load_profile(&directory.0, "Juniper").unwrap();
        assert_eq!(profile.name, "Juniper");
        assert_eq!(profile.identity, VisualIdentity::Aether);
        assert!(!directory.0.join(PROFILE_FILE).exists());
    }

    #[test]
    fn save_reopen_and_replace_preserve_only_the_profile() {
        let directory = TestDirectory::new();
        let config_path = directory.0.join("config.json");
        std::fs::write(&config_path, "unchanged runtime configuration").unwrap();
        let mut profile = PresenceProfile::for_entity("Juniper");
        profile.purpose = "Help me reflect".into();
        profile.boundaries = "Ask before acting\nKeep sources visible".into();
        profile.temperament = Temperament::Curious;
        save_profile(&directory.0, &profile).unwrap();
        assert_eq!(
            load_profile(&directory.0, "Original name").unwrap(),
            profile
        );
        profile.name = "New display label".into();
        profile.identity = VisualIdentity::Ember;
        save_profile(&directory.0, &profile).unwrap();
        assert_eq!(
            load_profile(&directory.0, "Original name").unwrap(),
            profile
        );
        assert_eq!(
            std::fs::read_to_string(config_path).unwrap(),
            "unchanged runtime configuration"
        );
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
    }

    #[test]
    fn invalid_updates_leave_previous_profile_intact() {
        let directory = TestDirectory::new();
        let original = PresenceProfile::for_entity("Juniper");
        save_profile(&directory.0, &original).unwrap();
        let mut invalid = original.clone();
        invalid.name = " ".into();
        assert!(save_profile(&directory.0, &invalid).is_err());
        assert_eq!(load_profile(&directory.0, "Juniper").unwrap(), original);
        invalid = original.clone();
        invalid.purpose = "x".repeat(2001);
        assert!(invalid.validate().is_err());
        invalid = original.clone();
        invalid.boundaries = "x".repeat(2001);
        assert!(invalid.validate().is_err());
        invalid = original.clone();
        invalid.name = "x".repeat(81);
        assert!(invalid.validate().is_err());
        invalid.name = "hello\nworld".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn unicode_limits_count_characters_and_accept_document_whitespace() {
        let mut profile = PresenceProfile::for_entity(&"é".repeat(80));
        profile.purpose = "好".repeat(2000);
        profile.boundaries = "First line\n\tSecond line\r\n".into();
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn unknown_enums_and_authority_fields_are_rejected() {
        let original = serde_json::to_value(PresenceProfile::for_entity("Juniper")).unwrap();
        for (key, value) in [
            ("identity", "unknown"),
            ("temperament", "obedient"),
            ("permissions", "all"),
        ] {
            let mut body = original.clone();
            body[key] = value.into();
            assert!(serde_json::from_value::<PresenceProfile>(body).is_err());
        }
    }

    #[test]
    fn corrupt_or_oversized_saved_profile_is_not_silently_reset() {
        let directory = TestDirectory::new();
        let path = directory.0.join(PROFILE_FILE);
        std::fs::write(&path, "{broken").unwrap();
        assert!(load_profile(&directory.0, "Juniper")
            .unwrap_err()
            .contains("invalid"));
        std::fs::write(&path, " ".repeat(MAX_PROFILE_BYTES + 1)).unwrap();
        assert!(load_profile(&directory.0, "Juniper")
            .unwrap_err()
            .contains("size limit"));
        let invalid = PresenceProfile::for_entity("");
        std::fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(load_profile(&directory.0, "Juniper").is_err());
    }
}
