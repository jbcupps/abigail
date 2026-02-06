//! Central skill registry.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use ao_core::SecretsVault;

use crate::manifest::{CapabilityDescriptor, SkillId, SkillManifest};
use crate::skill::{Skill, SkillError, SkillResult};

pub struct RegisteredSkill {
    pub skill: Arc<dyn Skill>,
    pub manifest: SkillManifest,
}

/// Describes a secret that a skill requires but is not yet stored in the vault.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MissingSkillSecret {
    pub skill_id: String,
    pub skill_name: String,
    pub secret_name: String,
    pub secret_description: String,
    pub required: bool,
}

pub struct SkillRegistry {
    skills: RwLock<HashMap<SkillId, RegisteredSkill>>,
    pub skill_paths: Vec<PathBuf>,
    secrets: Option<Arc<Mutex<SecretsVault>>>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            skill_paths: Vec::new(),
            secrets: None,
        }
    }

    /// Create a registry backed by a SecretsVault for secret validation.
    pub fn with_secrets(secrets: Arc<Mutex<SecretsVault>>) -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            skill_paths: Vec::new(),
            secrets: Some(secrets),
        }
    }

    /// Register a skill. Caller must initialize the skill before registering.
    pub fn register(&self, skill_id: SkillId, skill: Arc<dyn Skill>) -> SkillResult<()> {
        let manifest = skill.manifest().clone();
        let mut skills = self
            .skills
            .write()
            .map_err(|e| SkillError::InitFailed(e.to_string()))?;
        skills.insert(skill_id, RegisteredSkill { skill, manifest });
        Ok(())
    }

    pub fn unregister(&self, skill_id: &SkillId) -> SkillResult<()> {
        let mut skills = self
            .skills
            .write()
            .map_err(|e| SkillError::InitFailed(e.to_string()))?;
        skills.remove(skill_id);
        Ok(())
    }

    pub fn list(&self) -> SkillResult<Vec<SkillManifest>> {
        let skills = self
            .skills
            .read()
            .map_err(|e| SkillError::InitFailed(e.to_string()))?;
        Ok(skills.values().map(|r| r.manifest.clone()).collect())
    }

    pub fn find_by_capability(&self, capability: &str) -> Vec<SkillId> {
        let skills = match self.skills.read() {
            Ok(g) => g,
            Err(_) => return vec![],
        };
        skills
            .iter()
            .filter(|(_, r)| {
                r.manifest
                    .capabilities
                    .iter()
                    .any(|c: &CapabilityDescriptor| c.capability_type == capability)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get a clone of the skill Arc and its manifest for execution. Caller can then call execute_tool without holding the lock.
    pub fn get_skill(&self, skill_id: &SkillId) -> SkillResult<(Arc<dyn Skill>, SkillManifest)> {
        let skills = self
            .skills
            .read()
            .map_err(|e| SkillError::InitFailed(e.to_string()))?;
        let reg = skills
            .get(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.clone()))?;
        Ok((reg.skill.clone(), reg.manifest.clone()))
    }

    /// Check which secrets a manifest requires that are missing from the vault.
    pub fn check_missing_secrets(&self, manifest: &SkillManifest) -> Vec<MissingSkillSecret> {
        let vault = match &self.secrets {
            Some(s) => s,
            None => return vec![],
        };
        let vault = match vault.lock() {
            Ok(v) => v,
            Err(_) => return vec![],
        };
        manifest
            .secrets
            .iter()
            .filter(|s| !vault.exists(&s.name))
            .map(|s| MissingSkillSecret {
                skill_id: manifest.id.0.clone(),
                skill_name: manifest.name.clone(),
                secret_name: s.name.clone(),
                secret_description: s.description.clone(),
                required: s.required,
            })
            .collect()
    }

    /// List all missing secrets across all discovered manifests.
    pub fn list_all_missing_secrets(&self, paths: &[PathBuf]) -> Vec<MissingSkillSecret> {
        let manifests = Self::discover(paths);
        manifests
            .iter()
            .flat_map(|m| self.check_missing_secrets(m))
            .collect()
    }

    /// Discover skill manifests from disk (metadata only; no loading of native code).
    /// For each path: if directory, looks for path/skill.toml and path/*/skill.toml; if file, treats as skill.toml path.
    pub fn discover(paths: &[PathBuf]) -> Vec<SkillManifest> {
        let mut manifests = Vec::new();
        for path in paths {
            if path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            let toml_path = p.join("skill.toml");
                            if toml_path.is_file() {
                                if let Ok(m) = SkillManifest::load_from_path(&toml_path) {
                                    manifests.push(m);
                                }
                            }
                        }
                    }
                }
                let root_toml = path.join("skill.toml");
                if root_toml.is_file() {
                    if let Ok(m) = SkillManifest::load_from_path(&root_toml) {
                        if !manifests.iter().any(|x| x.id == m.id) {
                            manifests.push(m);
                        }
                    }
                }
            } else if path.is_file()
                && path.file_name().and_then(|n| n.to_str()) == Some("skill.toml")
            {
                if let Ok(m) = SkillManifest::load_from_path(path) {
                    manifests.push(m);
                }
            }
        }
        manifests
    }
}
