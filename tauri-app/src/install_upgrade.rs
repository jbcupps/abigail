use crate::identity_manager::IdentityManager;
use abigail_core::SecretsVault;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const INSTALL_UPGRADE_STATE_VERSION: u32 = 1;
const INSTALL_UPGRADE_STATE_FILE: &str = "install_upgrade_state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallUpgradeState {
    #[serde(default = "default_install_upgrade_state_version")]
    schema_version: u32,
    #[serde(default)]
    last_run_app_version: Option<String>,
    #[serde(default)]
    legacy_identity_agent_id: Option<String>,
    #[serde(default)]
    last_upgraded_at: Option<String>,
}

impl Default for InstallUpgradeState {
    fn default() -> Self {
        Self {
            schema_version: INSTALL_UPGRADE_STATE_VERSION,
            last_run_app_version: None,
            legacy_identity_agent_id: None,
            last_upgraded_at: None,
        }
    }
}

fn default_install_upgrade_state_version() -> u32 {
    INSTALL_UPGRADE_STATE_VERSION
}

pub fn run_preflight(data_root: &Path, current_version: &str) -> Result<(), String> {
    std::fs::create_dir_all(data_root).map_err(|e| e.to_string())?;

    let mut state = load_state(data_root)?;
    let previous_version = state.last_run_app_version.clone();
    let mut migrated_paths = Vec::new();

    for filename in ["secrets.bin", "skills.bin", "hive_secrets.bin"] {
        if let Some(path) = migrate_legacy_vault(data_root, filename)? {
            migrated_paths.push(path);
        }
    }

    let identities_dir = data_root.join("identities");
    if identities_dir.exists() {
        for entry in std::fs::read_dir(&identities_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            for filename in ["secrets.bin", "skills.bin"] {
                if let Some(migrated) = migrate_legacy_vault(&path, filename)? {
                    migrated_paths.push(migrated);
                }
            }
        }
    }

    let version_changed = state.last_run_app_version.as_deref() != Some(current_version);
    if version_changed || !migrated_paths.is_empty() {
        state.last_run_app_version = Some(current_version.to_string());
        state.last_upgraded_at = Some(chrono::Utc::now().to_rfc3339());
        save_state(data_root, &state)?;
    }

    if version_changed {
        tracing::info!(
            "Install upgrade preflight: {} -> {}",
            previous_version.as_deref().unwrap_or("fresh-install"),
            current_version
        );
    }

    for path in migrated_paths {
        tracing::info!(
            "Install upgrade preflight migrated legacy vault {}",
            path.display()
        );
    }

    Ok(())
}

pub fn run_identity_upgrade(
    data_root: &Path,
    current_version: &str,
    identity_manager: &IdentityManager,
) -> Result<Option<String>, String> {
    let mut state = load_state(data_root)?;
    let mut state_dirty = false;

    if let Some(agent_id) = state.legacy_identity_agent_id.clone() {
        if let Ok(agent_dir) = identity_manager.agent_dir(&agent_id) {
            if agent_dir.join("config.json").exists() {
                persist_state_version(data_root, current_version, &mut state, state_dirty)?;
                return Ok(Some(agent_id));
            }
        }

        state.legacy_identity_agent_id = None;
        state_dirty = true;
    }

    if identity_manager.has_agents() {
        persist_state_version(data_root, current_version, &mut state, state_dirty)?;
        return Ok(None);
    }

    let migrated = identity_manager.migrate_legacy_identity()?;
    if let Some(agent_id) = migrated.clone() {
        state.legacy_identity_agent_id = Some(agent_id.clone());
        state.last_run_app_version = Some(current_version.to_string());
        state.last_upgraded_at = Some(chrono::Utc::now().to_rfc3339());
        save_state(data_root, &state)?;
        tracing::info!(
            "Install upgrade migrated legacy identity into agent {}",
            agent_id
        );
    } else {
        persist_state_version(data_root, current_version, &mut state, state_dirty)?;
    }

    Ok(migrated)
}

fn migrate_legacy_vault(dir: &Path, filename: &str) -> Result<Option<PathBuf>, String> {
    if SecretsVault::migrate_legacy_custom(dir.to_path_buf(), filename)
        .map_err(|e| e.to_string())?
    {
        Ok(Some(dir.join(vault_filename(filename))))
    } else {
        Ok(None)
    }
}

fn vault_filename(filename: &str) -> String {
    let stem = filename
        .strip_suffix(".bin")
        .or_else(|| filename.strip_suffix(".vault"))
        .unwrap_or(filename);
    format!("{stem}.vault")
}

fn persist_state_version(
    data_root: &Path,
    current_version: &str,
    state: &mut InstallUpgradeState,
    force_save: bool,
) -> Result<(), String> {
    if !force_save && state.last_run_app_version.as_deref() == Some(current_version) {
        return Ok(());
    }

    state.last_run_app_version = Some(current_version.to_string());
    state.last_upgraded_at = Some(chrono::Utc::now().to_rfc3339());
    save_state(data_root, state)
}

fn load_state(data_root: &Path) -> Result<InstallUpgradeState, String> {
    let path = state_path(data_root);
    if !path.exists() {
        return Ok(InstallUpgradeState::default());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut state: InstallUpgradeState =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    state.schema_version = INSTALL_UPGRADE_STATE_VERSION;
    Ok(state)
}

fn save_state(data_root: &Path, state: &InstallUpgradeState) -> Result<(), String> {
    let path = state_path(data_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let content = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

fn state_path(data_root: &Path) -> PathBuf {
    data_root.join(INSTALL_UPGRADE_STATE_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_upgrade_state_roundtrip() {
        let tmp = std::env::temp_dir().join("abigail_install_upgrade_state_roundtrip");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let state = InstallUpgradeState {
            schema_version: INSTALL_UPGRADE_STATE_VERSION,
            last_run_app_version: Some("0.0.7".to_string()),
            legacy_identity_agent_id: Some("agent-123".to_string()),
            last_upgraded_at: Some("2026-03-06T15:00:00Z".to_string()),
        };

        save_state(&tmp, &state).unwrap();
        let loaded = load_state(&tmp).unwrap();

        assert_eq!(loaded.schema_version, INSTALL_UPGRADE_STATE_VERSION);
        assert_eq!(loaded.last_run_app_version.as_deref(), Some("0.0.7"));
        assert_eq!(
            loaded.legacy_identity_agent_id.as_deref(),
            Some("agent-123")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn persist_state_version_saves_state_changes_without_version_bump() {
        let tmp = std::env::temp_dir().join("abigail_install_upgrade_state_persist_same_version");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let mut state = InstallUpgradeState {
            schema_version: INSTALL_UPGRADE_STATE_VERSION,
            last_run_app_version: Some("0.0.7".to_string()),
            legacy_identity_agent_id: None,
            last_upgraded_at: Some("2026-03-06T15:00:00Z".to_string()),
        };
        save_state(&tmp, &state).unwrap();

        state.legacy_identity_agent_id = Some("agent-456".to_string());
        persist_state_version(&tmp, "0.0.7", &mut state, true).unwrap();

        let loaded = load_state(&tmp).unwrap();
        assert_eq!(
            loaded.legacy_identity_agent_id.as_deref(),
            Some("agent-456")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
