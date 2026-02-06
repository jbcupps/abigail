//! Filesystem skill: read, write, list, and search files within sandboxed directories.
//!
//! All file operations are restricted to allowed root directories to prevent
//! path traversal and unauthorized file access.

use ao_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, Permission,
    FileSystemPermission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
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

    /// Validate that a path is within one of the allowed roots.
    /// Returns the canonicalized path if valid, or an error.
    fn validate_path(&self, path_str: &str) -> SkillResult<PathBuf> {
        let path = PathBuf::from(path_str);

        // Reject obviously malicious patterns
        let normalized = path_str.replace('\\', "/");
        if normalized.contains("../") || normalized.contains("/..") {
            return Err(SkillError::PermissionDenied(
                "Path traversal (../) is not allowed".to_string(),
            ));
        }

        // For existing paths, canonicalize and check containment
        if path.exists() {
            let canonical = path.canonicalize().map_err(|e| {
                SkillError::ToolFailed(format!("Cannot resolve path: {}", e))
            })?;
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

    /// Check if a canonicalized path is within any allowed root.
    fn is_within_allowed_roots(&self, canonical_path: &Path) -> bool {
        for root in &self.allowed_roots {
            let canonical_root = match root.canonicalize() {
                Ok(r) => r,
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
        let path = self.validate_path(path_str)?;

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

    /// List contents of a directory.
    fn list_directory(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_dir() {
            return Ok(ToolOutput::error(format!("'{}' is not a directory", path_str)));
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
                a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
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
            return Ok(ToolOutput::error(format!("'{}' is not a directory", root_str)));
        }

        // Build full glob pattern rooted at the validated directory
        let full_pattern = format!("{}/{}", root.display(), pattern_str);

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
                let content: String = params.get("content").ok_or_else(|| {
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
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"list_directory"));
        assert!(names.contains(&"search_files"));
    }

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("ao_fs_test_traversal");
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
        let tmp = std::env::temp_dir().join("ao_fs_test_outside");
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
        let tmp = std::env::temp_dir().join("ao_fs_test_read");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("test.txt"), "Hello, AO!").unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill.read_file(&tmp.join("test.txt").display().to_string()).unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["formatted"], "Hello, AO!");
        assert_eq!(data["size_bytes"], 10);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_write_file() {
        let tmp = std::env::temp_dir().join("ao_fs_test_write");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let out_path = tmp.join("output.txt");
        let result = skill
            .write_file(&out_path.display().to_string(), "Written by AO")
            .unwrap();

        assert!(result.success);
        let content = fs::read_to_string(&out_path).unwrap();
        assert_eq!(content, "Written by AO");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_list_directory() {
        let tmp = std::env::temp_dir().join("ao_fs_test_list");
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
        let tmp = std::env::temp_dir().join("ao_fs_test_search");
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
        let tmp = std::env::temp_dir().join("ao_fs_test_nofile");
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
        let tmp = std::env::temp_dir().join("ao_fs_test_health");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let health = skill.health();
        assert_eq!(health.status, HealthStatus::Healthy);

        let _ = fs::remove_dir_all(&tmp);
    }
}
