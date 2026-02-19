//! Skill manifest types and skill.toml parsing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SkillId(pub String);

impl std::fmt::Display for SkillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub capability_type: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "value")]
pub enum Permission {
    Network(NetworkPermission),
    FileSystem(FileSystemPermission),
    Memory(MemoryPermission),
    Notifications,
    Clipboard,
    Microphone,
    Camera,
    ScreenCapture,
    ShellExecute,
    SkillInteraction(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NetworkPermission {
    Domains(Vec<String>),
    Full,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FileSystemPermission {
    Read(Vec<String>),
    Write(Vec<String>),
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MemoryPermission {
    ReadOnly,
    ReadWrite,
    Namespace(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: SkillId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: Option<String>,
    pub category: String,
    pub keywords: Vec<String>,
    pub runtime: String,
    pub min_abigail_version: String,
    pub platforms: Vec<String>,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub permissions: Vec<Permission>,
    pub secrets: Vec<SecretDescriptor>,
    pub config_defaults: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDescriptor {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// Raw skill.toml structure for parsing.
#[derive(Debug, Deserialize)]
pub struct SkillManifestFile {
    pub skill: SkillManifestSection,
    #[serde(default)]
    pub author: Option<AuthorSection>,
    #[serde(default)]
    pub runtime: Option<RuntimeSection>,
    #[serde(default)]
    pub capabilities: Vec<CapabilitySection>,
    #[serde(default)]
    pub permissions: Vec<PermissionSection>,
    #[serde(default)]
    pub secrets: Vec<SecretSection>,
    #[serde(default)]
    pub config: Option<ConfigSection>,
}

#[derive(Debug, Deserialize)]
pub struct SkillManifestSection {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorSection {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeSection {
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default)]
    pub min_abigail_version: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

fn default_runtime() -> String {
    "Native".to_string()
}

#[derive(Debug, Deserialize)]
pub struct CapabilitySection {
    pub capability: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PermissionSection {
    pub permission: toml::Value,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub optional: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SecretSection {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigSection {
    #[serde(default)]
    pub defaults: HashMap<String, toml::Value>,
}

impl SkillManifest {
    /// Parse manifest from skill.toml file path.
    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::parse(&s)
    }

    /// Parse manifest from skill.toml content.
    pub fn parse(s: &str) -> Result<Self, String> {
        let raw: SkillManifestFile = toml::from_str(s).map_err(|e| e.to_string())?;
        let runtime = raw.runtime.as_ref();
        let caps: Vec<CapabilityDescriptor> = raw
            .capabilities
            .iter()
            .map(|c| CapabilityDescriptor {
                capability_type: c.capability.clone(),
                version: c.version.clone().unwrap_or_else(|| "1.0".to_string()),
            })
            .collect();
        let perms = parse_permissions(&raw.permissions);
        let secrets: Vec<SecretDescriptor> = raw
            .secrets
            .iter()
            .map(|s| SecretDescriptor {
                name: s.name.clone(),
                description: s.description.clone().unwrap_or_default(),
                required: s.required.unwrap_or(true),
            })
            .collect();
        let config_defaults: HashMap<String, serde_json::Value> = raw
            .config
            .as_ref()
            .map(|c| {
                c.defaults
                    .iter()
                    .filter_map(|(k, v)| {
                        serde_json::to_value(v.clone()).ok().map(|v| (k.clone(), v))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            id: SkillId(raw.skill.id.clone()),
            name: raw.skill.name.clone(),
            version: raw.skill.version.clone(),
            description: raw.skill.description.clone(),
            license: raw.skill.license.clone(),
            category: raw
                .skill
                .category
                .clone()
                .unwrap_or_else(|| "General".to_string()),
            keywords: raw.skill.keywords.clone(),
            runtime: runtime
                .as_ref()
                .map(|r| r.runtime.clone())
                .unwrap_or_else(|| "Native".to_string()),
            min_abigail_version: runtime
                .and_then(|r| r.min_abigail_version.clone())
                .unwrap_or_else(|| "0.1.0".to_string()),
            platforms: runtime
                .as_ref()
                .map(|r| r.platforms.clone())
                .unwrap_or_else(|| vec!["All".to_string()]),
            capabilities: caps,
            permissions: perms,
            secrets,
            config_defaults,
        })
    }
}

fn parse_permissions(sections: &[PermissionSection]) -> Vec<Permission> {
    let mut out = Vec::new();
    for s in sections {
        // Handle string permissions (e.g. "ShellExecute")
        if let Some(perm_str) = s.permission.as_str() {
            if perm_str == "ShellExecute" {
                out.push(Permission::ShellExecute);
            }
            continue;
        }

        if let Some(t) = s.permission.as_table() {
            // ── Network ──
            if let Some(domains) = t
                .get("Network")
                .and_then(|n| n.get("Domains"))
                .and_then(|d| d.as_array())
            {
                let domains: Vec<String> = domains
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                out.push(Permission::Network(NetworkPermission::Domains(domains)));
            } else if t.contains_key("Network") {
                out.push(Permission::Network(NetworkPermission::Full));
            }

            // ── FileSystem ──
            if let Some(fs) = t.get("FileSystem") {
                if let Some(fs_str) = fs.as_str() {
                    if fs_str == "Full" {
                        out.push(Permission::FileSystem(FileSystemPermission::Full));
                    }
                } else if let Some(fs_table) = fs.as_table() {
                    if let Some(read_paths) = fs_table.get("Read").and_then(|r| r.as_array()) {
                        let paths: Vec<String> = read_paths
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        out.push(Permission::FileSystem(FileSystemPermission::Read(paths)));
                    }
                    if let Some(write_paths) = fs_table.get("Write").and_then(|w| w.as_array()) {
                        let paths: Vec<String> = write_paths
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                        out.push(Permission::FileSystem(FileSystemPermission::Write(paths)));
                    }
                }
            }

            // ── Memory ──
            if let Some(mem) = t.get("Memory") {
                if let Some(s) = mem.as_str() {
                    out.push(Permission::Memory(MemoryPermission::Namespace(
                        s.to_string(),
                    )));
                } else {
                    out.push(Permission::Memory(MemoryPermission::ReadWrite));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filesystem_read_permission() {
        let toml = r#"
[skill]
id = "test.fs"
name = "FS Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = { FileSystem = { Read = ["~"] } }
reason = "Read files"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert!(manifest.permissions.contains(&Permission::FileSystem(
            FileSystemPermission::Read(vec!["~".to_string()])
        )));
    }

    #[test]
    fn parse_filesystem_write_permission() {
        let toml = r#"
[skill]
id = "test.fs"
name = "FS Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = { FileSystem = { Write = ["/tmp"] } }
reason = "Write files"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert!(manifest.permissions.contains(&Permission::FileSystem(
            FileSystemPermission::Write(vec!["/tmp".to_string()])
        )));
    }

    #[test]
    fn parse_filesystem_full_permission() {
        let toml = r#"
[skill]
id = "test.fs"
name = "FS Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = { FileSystem = "Full" }
reason = "Full filesystem access"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert!(manifest
            .permissions
            .contains(&Permission::FileSystem(FileSystemPermission::Full)));
    }

    #[test]
    fn parse_shell_execute_permission() {
        let toml = r#"
[skill]
id = "test.shell"
name = "Shell Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = "ShellExecute"
reason = "Execute shell commands"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert!(manifest.permissions.contains(&Permission::ShellExecute));
    }

    #[test]
    fn parse_filesystem_read_and_write_combined() {
        let toml = r#"
[skill]
id = "test.fs"
name = "FS Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = { FileSystem = { Read = ["~"], Write = ["~"] } }
reason = "Read and write files"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert_eq!(manifest.permissions.len(), 2);
        assert!(manifest.permissions.contains(&Permission::FileSystem(
            FileSystemPermission::Read(vec!["~".to_string()])
        )));
        assert!(manifest.permissions.contains(&Permission::FileSystem(
            FileSystemPermission::Write(vec!["~".to_string()])
        )));
    }

    #[test]
    fn parse_network_domains_still_works() {
        let toml = r#"
[skill]
id = "test.net"
name = "Net Test"
version = "0.1.0"
description = "Test"

[[permissions]]
permission = { Network = { Domains = ["api.example.com"] } }
reason = "API access"
"#;
        let manifest = SkillManifest::parse(toml).unwrap();
        assert!(manifest
            .permissions
            .contains(&Permission::Network(NetworkPermission::Domains(vec![
                "api.example.com".to_string()
            ]))));
    }
}
