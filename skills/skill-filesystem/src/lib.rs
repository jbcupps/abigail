//! Filesystem skill: read, write, list, and search files within sandboxed directories.
//!
//! All file operations are restricted to allowed root directories to prevent
//! path traversal and unauthorized file access.

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Filesystem skill with sandboxed directory access.
pub struct FilesystemSkill {
    manifest: SkillManifest,
    /// Root directories where file operations are allowed.
    allowed_roots: Vec<PathBuf>,
}

impl FilesystemSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse filesystem skill.toml")
    }

    /// Create a new filesystem skill with the given allowed root directories.
    pub fn new(manifest: SkillManifest, allowed_roots: Vec<PathBuf>) -> Self {
        Self {
            manifest,
            allowed_roots,
        }
    }

    fn reject_malicious_path(path_str: &str) -> SkillResult<()> {
        let path = PathBuf::from(path_str);

        // Reject obviously malicious patterns
        let normalized = path_str.replace('\\', "/");
        if normalized.contains("../") || normalized.contains("/..") {
            return Err(SkillError::PermissionDenied(
                "Path traversal (../) is not allowed".to_string(),
            ));
        }
        if path.components().count() == 0 {
            return Err(SkillError::InvalidArguments(
                "Path cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Validate that a path is within one of the allowed roots.
    /// Returns the canonicalized path if valid, or an error.
    fn validate_path(&self, path_str: &str) -> SkillResult<PathBuf> {
        let path = PathBuf::from(path_str);
        Self::reject_malicious_path(path_str)?;

        // For existing paths, canonicalize and check containment
        if path.exists() {
            let canonical = path
                .canonicalize()
                .map_err(|e| SkillError::ToolFailed(format!("Cannot resolve path: {}", e)))?;
            if self.is_within_allowed_roots(&canonical) {
                return Ok(canonical);
            }
            return Err(SkillError::PermissionDenied(format!(
                "Path '{}' is outside allowed directories",
                path_str
            )));
        }

        // For new paths (write_file), check parent exists and is allowed
        if let Some(parent) = path.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    SkillError::ToolFailed(format!("Cannot resolve parent path: {}", e))
                })?;
                if self.is_within_allowed_roots(&canonical_parent) {
                    return Ok(canonical_parent.join(path.file_name().unwrap_or_default()));
                }
            }
        }

        Err(SkillError::PermissionDenied(format!(
            "Path '{}' is outside allowed directories",
            path_str
        )))
    }

    /// Validate a path for write/create operations, allowing nested paths whose
    /// nearest existing ancestor is inside an allowed root.
    fn validate_write_path(&self, path_str: &str) -> SkillResult<PathBuf> {
        let path = PathBuf::from(path_str);
        Self::reject_malicious_path(path_str)?;

        if path.exists() {
            return self.validate_path(path_str);
        }

        let mut cursor = path.as_path();
        while let Some(parent) = cursor.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    SkillError::ToolFailed(format!("Cannot resolve parent path: {}", e))
                })?;
                if self.is_within_allowed_roots(&canonical_parent) {
                    let suffix = path.strip_prefix(parent).map_err(|_| {
                        SkillError::ToolFailed("Failed to resolve requested path".to_string())
                    })?;
                    return Ok(canonical_parent.join(suffix));
                }
                return Err(SkillError::PermissionDenied(format!(
                    "Path '{}' is outside allowed directories",
                    path_str
                )));
            }
            cursor = parent;
        }

        Err(SkillError::PermissionDenied(format!(
            "Path '{}' is outside allowed directories",
            path_str
        )))
    }

    /// Strip the Windows extended-length path prefix (`\\?\`) if present.
    #[cfg(target_os = "windows")]
    fn strip_unc_prefix(p: &Path) -> PathBuf {
        let s = p.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            p.to_path_buf()
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn strip_unc_prefix(p: &Path) -> PathBuf {
        p.to_path_buf()
    }

    /// Check if a canonicalized path is within any allowed root.
    fn is_within_allowed_roots(&self, canonical_path: &Path) -> bool {
        let canonical_path = Self::strip_unc_prefix(canonical_path);
        for root in &self.allowed_roots {
            let canonical_root = match root.canonicalize() {
                Ok(r) => Self::strip_unc_prefix(&r),
                Err(_) => root.clone(),
            };
            if canonical_path.starts_with(&canonical_root) {
                return true;
            }
        }
        false
    }

    /// Read a file's contents.
    fn read_file(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", path_str)));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read metadata: {}", e)))?;

        // Limit: don't read files larger than 1MB
        if metadata.len() > 1_048_576 {
            return Ok(ToolOutput::error(format!(
                "File too large ({} bytes). Maximum is 1MB.",
                metadata.len()
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read file: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": content,
            "path": path.display().to_string(),
            "size_bytes": metadata.len(),
        })))
    }

    /// Write content to a file.
    fn write_file(&self, path_str: &str, content: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_write_path(path_str)?;

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SkillError::ToolFailed(format!("Cannot create directories: {}", e)))?;
        }

        std::fs::write(&path, content)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot write file: {}", e)))?;

        let size = content.len();
        tracing::info!("Wrote {} bytes to {}", size, path.display());

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Written {} bytes to {}", size, path.display()),
            "path": path.display().to_string(),
            "size_bytes": size,
        })))
    }

    fn describe_filesystem(&self) -> SkillResult<ToolOutput> {
        let cwd = std::env::current_dir()
            .map(|dir| Self::strip_unc_prefix(&dir))
            .map_err(|e| {
                SkillError::ToolFailed(format!("Cannot resolve current directory: {}", e))
            })?;
        let temp_dir = Self::strip_unc_prefix(&std::env::temp_dir());
        let workspace_root = self
            .allowed_roots
            .first()
            .cloned()
            .unwrap_or_else(|| cwd.clone());
        let workspace_root = Self::strip_unc_prefix(&workspace_root);
        let recommended = ["handoff", "inbox", "outbox", "scratch", "logs"]
            .into_iter()
            .map(|name| {
                let path = workspace_root.join(name);
                serde_json::json!({
                    "name": name,
                    "path": path.display().to_string(),
                    "exists": path.exists(),
                })
            })
            .collect::<Vec<_>>();
        let allowed_roots = self
            .allowed_roots
            .iter()
            .map(|root| {
                let clean = Self::strip_unc_prefix(root);
                serde_json::json!({
                    "path": clean.display().to_string(),
                    "exists": clean.exists(),
                    "readable": clean.exists(),
                    "writable": clean.exists(),
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!(
                "cwd: {}\nworkspace: {}\ntemp: {}\nallowed roots:\n{}",
                cwd.display(),
                workspace_root.display(),
                temp_dir.display(),
                allowed_roots
                    .iter()
                    .map(|root| format!("- {}", root["path"].as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            "current_working_directory": cwd.display().to_string(),
            "workspace_root": workspace_root.display().to_string(),
            "temp_directory": temp_dir.display().to_string(),
            "read_roots": allowed_roots,
            "write_roots": self.allowed_roots.iter().map(|root| Self::strip_unc_prefix(root).display().to_string()).collect::<Vec<_>>(),
            "recommended_subdirectories": recommended,
        })))
    }

    fn probe_path_access(&self, path_str: &str, mode: &str) -> ToolOutput {
        let mode = mode.to_lowercase();
        let result = match mode.as_str() {
            "read" => self.validate_path(path_str),
            "write" => self.validate_write_path(path_str),
            other => {
                return ToolOutput::error(format!(
                    "Unsupported mode '{}'. Use 'read' or 'write'.",
                    other
                ));
            }
        };

        match result {
            Ok(path) => ToolOutput::success(serde_json::json!({
                "formatted": format!("{} access allowed: {}", mode, path.display()),
                "allowed": true,
                "mode": mode,
                "canonical_path": path.display().to_string(),
                "exists": path.exists(),
            })),
            Err(err) => ToolOutput::success(serde_json::json!({
                "formatted": format!("{} access denied for {}: {}", mode, path_str, err),
                "allowed": false,
                "mode": mode,
                "canonical_path": serde_json::Value::Null,
                "exists": PathBuf::from(path_str).exists(),
                "reason": err.to_string(),
            })),
        }
    }

    fn mkdir(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_write_path(path_str)?;
        if path.is_file() {
            return Ok(ToolOutput::error(format!(
                "Cannot create directory '{}': a file already exists there",
                path.display()
            )));
        }
        std::fs::create_dir_all(&path).map_err(|e| {
            SkillError::ToolFailed(format!(
                "Cannot create directory '{}': {}",
                path.display(),
                e
            ))
        })?;
        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Created directory {}", path.display()),
            "path": path.display().to_string(),
            "created": true,
        })))
    }

    /// List contents of a directory.
    fn list_directory(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_dir() {
            return Ok(ToolOutput::error(format!(
                "'{}' is not a directory",
                path_str
            )));
        }

        let entries = std::fs::read_dir(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read directory: {}", e)))?;

        let mut items: Vec<serde_json::Value> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type().ok();
            let is_dir = file_type.as_ref().map(|t| t.is_dir()).unwrap_or(false);
            let size = entry.metadata().ok().map(|m| m.len()).unwrap_or(0);

            items.push(serde_json::json!({
                "name": name,
                "is_directory": is_dir,
                "size_bytes": if is_dir { 0 } else { size },
            }));
        }

        // Sort: directories first, then alphabetical
        items.sort_by(|a, b| {
            let a_dir = a["is_directory"].as_bool().unwrap_or(false);
            let b_dir = b["is_directory"].as_bool().unwrap_or(false);
            b_dir.cmp(&a_dir).then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
            })
        });

        let formatted = items
            .iter()
            .map(|item| {
                let prefix = if item["is_directory"].as_bool().unwrap_or(false) {
                    "📁"
                } else {
                    "📄"
                };
                format!("{} {}", prefix, item["name"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "count": items.len(),
            "entries": items,
        })))
    }

    /// Search for files matching a glob pattern.
    fn search_files(&self, pattern_str: &str, root_str: &str) -> SkillResult<ToolOutput> {
        let root = self.validate_path(root_str)?;

        if !root.is_dir() {
            return Ok(ToolOutput::error(format!(
                "'{}' is not a directory",
                root_str
            )));
        }

        // Build full glob pattern rooted at the validated directory.
        // On Windows, canonicalize() returns UNC paths like \\?\C:\... where
        // the '?' is a glob metacharacter. Strip the prefix, then normalise
        // to forward slashes so the glob crate works cross-platform.
        let mut root_str_normalized = root.display().to_string();
        if root_str_normalized.starts_with(r"\\?\") {
            root_str_normalized = root_str_normalized[4..].to_string();
        }
        let root_str_normalized = root_str_normalized.replace('\\', "/");
        let full_pattern = format!("{}/{}", root_str_normalized, pattern_str);

        let matches: Vec<String> = glob::glob(&full_pattern)
            .map_err(|e| SkillError::ToolFailed(format!("Invalid glob pattern: {}", e)))?
            .filter_map(|entry| entry.ok())
            .filter(|path| self.is_within_allowed_roots(path))
            .take(100) // Cap results
            .map(|path| path.display().to_string())
            .collect();

        let formatted = if matches.is_empty() {
            "No files found matching the pattern.".to_string()
        } else {
            matches.join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "pattern": pattern_str,
            "root": root.display().to_string(),
            "count": matches.len(),
            "files": matches,
        })))
    }
}

#[async_trait]
impl Skill for FilesystemSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let all_accessible = self.allowed_roots.iter().all(|r| r.exists());
        SkillHealth {
            status: if all_accessible {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if !all_accessible {
                Some("Some allowed root directories are not accessible".to_string())
            } else {
                None
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "describe_filesystem".to_string(),
                description: "Show authoritative filesystem boundaries for this runtime: current working directory, writable roots, temp directory, and recommended workspace subdirectories.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "current_working_directory": { "type": "string" },
                        "workspace_root": { "type": "string" },
                        "temp_directory": { "type": "string" },
                        "read_roots": { "type": "array" },
                        "write_roots": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 5,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "read_file".to_string(),
                description: "Read the contents of a file. Returns the text content for files under 1MB.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the file to read"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "size_bytes": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "write_file".to_string(),
                description: "Write content to a file. Creates parent directories if needed.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to write the file"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "size_bytes": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Write(vec!["~".to_string()]),
                )],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "list_directory".to_string(),
                description: "List the contents of a directory, showing files and subdirectories.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the directory to list"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "entries": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "probe_path_access".to_string(),
                description: "Validate whether a path is allowed before attempting to read or write it.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to validate"
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["read", "write"],
                            "description": "Whether to validate the path for reading or writing"
                        }
                    },
                    "required": ["path", "mode"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "allowed": { "type": "boolean" },
                        "canonical_path": { "type": ["string", "null"] },
                        "reason": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 5,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "mkdir".to_string(),
                description: "Create a directory recursively inside an allowed writable root.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute directory path to create"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "created": { "type": "boolean" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Write(vec!["~".to_string()]),
                )],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "search_files".to_string(),
                description: "Search for files matching a glob pattern within a directory. Returns up to 100 matching paths.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern to match (e.g. '**/*.txt', '*.rs')"
                        },
                        "root": {
                            "type": "string",
                            "description": "Root directory to search from"
                        }
                    },
                    "required": ["pattern", "root"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "files": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 100,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "describe_filesystem" => self.describe_filesystem(),
            "read_file" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.read_file(&path)
            }
            "write_file" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                // Use lenient getter — LLMs sometimes send content as non-string JSON
                let content: String = params.get_string("content").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: content".to_string())
                })?;
                self.write_file(&path, &content)
            }
            "list_directory" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.list_directory(&path)
            }
            "probe_path_access" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let mode: String = params.get("mode").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: mode".to_string())
                })?;
                Ok(self.probe_path_access(&path, &mode))
            }
            "mkdir" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.mkdir(&path)
            }
            "search_files" => {
                let pattern: String = params.get("pattern").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: pattern".to_string())
                })?;
                let root: String = params.get("root").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: root".to_string())
                })?;
                self.search_files(&pattern, &root)
            }
            other => Err(SkillError::ToolFailed(format!("Unknown tool: {}", other))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_skill(roots: Vec<PathBuf>) -> FilesystemSkill {
        FilesystemSkill::new(FilesystemSkill::default_manifest(), roots)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = FilesystemSkill::default_manifest();
        assert_eq!(manifest.name, "Filesystem");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        assert_eq!(tools.len(), 7);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"describe_filesystem"));
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"probe_path_access"));
        assert!(names.contains(&"mkdir"));
        assert!(names.contains(&"search_files"));
    }

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_traversal");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);

        // Traversal should be blocked
        let result = skill.validate_path("../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_path_outside_roots_blocked() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_outside");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);

        // Path outside allowed roots
        let result = skill.validate_path("/etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("outside allowed"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_file() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_read");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("test.txt"), "Hello, Abigail!").unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill
            .read_file(&tmp.join("test.txt").display().to_string())
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["formatted"], "Hello, Abigail!");
        assert_eq!(data["size_bytes"], 15);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_file() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_write");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let out_path = tmp.join("output.txt");
        let result = skill
            .write_file(&out_path.display().to_string(), "Written by Abigail")
            .unwrap();

        assert!(result.success);
        let content = fs::read_to_string(&out_path).unwrap();
        assert_eq!(content, "Written by Abigail");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_mkdir_nested_path() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_mkdir");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let nested = tmp.join("handoff").join("2026").join("notes");
        let result = skill.mkdir(&nested.display().to_string()).unwrap();

        assert!(result.success);
        assert!(nested.is_dir());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_probe_path_access_denies_outside_root() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_probe");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill.probe_path_access("C:\\outside.txt", "write");

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["allowed"], false);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_describe_filesystem_reports_workspace() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_describe");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill.describe_filesystem().unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["workspace_root"], tmp.display().to_string());
        assert!(data["recommended_subdirectories"].as_array().unwrap().len() >= 5);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_list_directory() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_list");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("subdir")).unwrap();
        fs::write(tmp.join("a.txt"), "a").unwrap();
        fs::write(tmp.join("b.txt"), "b").unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill.list_directory(&tmp.display().to_string()).unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["count"], 3); // subdir + a.txt + b.txt

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_search_files() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_search");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::write(tmp.join("one.txt"), "1").unwrap();
        fs::write(tmp.join("two.txt"), "2").unwrap();
        fs::write(tmp.join("sub/three.txt"), "3").unwrap();
        fs::write(tmp.join("data.json"), "{}").unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill
            .search_files("**/*.txt", &tmp.display().to_string())
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["count"], 3); // three .txt files

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_nonexistent_file() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_nofile");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill.validate_path(&tmp.join("nonexistent.txt").display().to_string());
        // validate_path for a nonexistent file with existing parent should succeed
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_health_check() {
        let tmp = std::env::temp_dir().join("abigail_fs_test_health");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let health = skill.health();
        assert_eq!(health.status, HealthStatus::Healthy);

        let _ = fs::remove_dir_all(&tmp);
    }
}
