//! Code Analysis skill: analyze source code structure, count lines, and search patterns.
//!
//! Uses native Rust (regex + recursive file walking) to analyze source code files.
//! All file operations are restricted to allowed root directories to prevent
//! path traversal and unauthorized file access.

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use regex::Regex;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Code Analysis skill with sandboxed directory access.
pub struct CodeAnalysisSkill {
    manifest: SkillManifest,
    /// Root directories where file operations are allowed.
    allowed_roots: Vec<PathBuf>,
}

impl CodeAnalysisSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse code-analysis skill.toml")
    }

    /// Create a new code analysis skill with the given allowed root directories.
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

        // For non-existent paths, check parent exists and is allowed
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

    /// Analyze a single file: count lines, blanks, comments, code lines, and detect extension.
    fn analyze_file(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", path_str)));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read metadata: {}", e)))?;

        // Limit: don't analyze files larger than 10MB
        if metadata.len() > 10_485_760 {
            return Ok(ToolOutput::error(format!(
                "File too large ({} bytes). Maximum is 10MB.",
                metadata.len()
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read file: {}", e)))?;

        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut line_count: usize = 0;
        let mut blank_lines: usize = 0;
        let mut comment_lines: usize = 0;
        let mut code_lines: usize = 0;

        for line in content.lines() {
            line_count += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                blank_lines += 1;
            } else if trimmed.starts_with("//") || trimmed.starts_with('#') {
                comment_lines += 1;
            } else {
                code_lines += 1;
            }
        }

        let formatted = format!(
            "File: {}\n  Extension: {}\n  Total lines: {}\n  Code lines: {}\n  Comment lines: {}\n  Blank lines: {}",
            path.display(),
            if extension.is_empty() { "(none)" } else { &extension },
            line_count,
            code_lines,
            comment_lines,
            blank_lines,
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "file_extension": extension,
            "line_count": line_count,
            "blank_lines": blank_lines,
            "comment_lines": comment_lines,
            "code_lines": code_lines,
        })))
    }

    /// Recursively walk a directory, collecting file paths.
    fn walk_directory(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    Self::walk_directory(&path, files)?;
                } else if path.is_file() {
                    files.push(path);
                }
            }
        }
        Ok(())
    }

    /// Analyze a directory: count files by extension and total lines.
    fn analyze_directory(
        &self,
        path_str: &str,
        extensions: Option<Vec<String>>,
    ) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_dir() {
            return Ok(ToolOutput::error(format!(
                "'{}' is not a directory",
                path_str
            )));
        }

        let mut all_files = Vec::new();
        Self::walk_directory(&path, &mut all_files)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot walk directory: {}", e)))?;

        // Filter to only files within allowed roots
        all_files.retain(|f| {
            if let Ok(canonical) = f.canonicalize() {
                self.is_within_allowed_roots(&canonical)
            } else {
                false
            }
        });

        // Filter by extensions if provided
        if let Some(ref exts) = extensions {
            all_files.retain(|f| {
                if let Some(ext) = f.extension() {
                    exts.iter()
                        .any(|e| e.eq_ignore_ascii_case(&ext.to_string_lossy()))
                } else {
                    false
                }
            });
        }

        let mut files_by_extension: HashMap<String, usize> = HashMap::new();
        let mut total_lines: usize = 0;
        let total_files = all_files.len();

        for file_path in &all_files {
            let ext = file_path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "(no extension)".to_string());

            *files_by_extension.entry(ext).or_insert(0) += 1;

            // Count lines (skip binary/unreadable files)
            if let Ok(content) = std::fs::read_to_string(file_path) {
                total_lines += content.lines().count();
            }
        }

        // Sort extensions by count descending for display
        let mut ext_vec: Vec<(&String, &usize)> = files_by_extension.iter().collect();
        ext_vec.sort_by(|a, b| b.1.cmp(a.1));

        let ext_summary: Vec<String> = ext_vec
            .iter()
            .map(|(ext, count)| format!("  .{}: {} files", ext, count))
            .collect();

        let formatted = format!(
            "Directory: {}\n  Total files: {}\n  Total lines: {}\nFiles by extension:\n{}",
            path.display(),
            total_files,
            total_lines,
            if ext_summary.is_empty() {
                "  (none)".to_string()
            } else {
                ext_summary.join("\n")
            },
        );

        // Convert extension map to JSON-friendly format
        let ext_json: serde_json::Value = files_by_extension
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::json!(v)))
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "total_files": total_files,
            "total_lines": total_lines,
            "files_by_extension": ext_json,
        })))
    }

    /// Search for regex pattern matches in a file or directory. Capped at 100 matches.
    fn search_patterns(&self, path_str: &str, pattern_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        let re = Regex::new(pattern_str).map_err(|e| {
            SkillError::ToolFailed(format!("Invalid regex pattern '{}': {}", pattern_str, e))
        })?;

        let mut matches: Vec<serde_json::Value> = Vec::new();
        let max_matches: usize = 100;

        let files_to_search = if path.is_file() {
            vec![path.clone()]
        } else if path.is_dir() {
            let mut all_files = Vec::new();
            Self::walk_directory(&path, &mut all_files)
                .map_err(|e| SkillError::ToolFailed(format!("Cannot walk directory: {}", e)))?;
            // Filter to allowed roots
            all_files.retain(|f| {
                if let Ok(canonical) = f.canonicalize() {
                    self.is_within_allowed_roots(&canonical)
                } else {
                    false
                }
            });
            all_files
        } else {
            return Ok(ToolOutput::error(format!(
                "'{}' is not a file or directory",
                path_str
            )));
        };

        'outer: for file_path in &files_to_search {
            // Skip files we can't read as text
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for (line_number, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    matches.push(serde_json::json!({
                        "file": file_path.display().to_string(),
                        "line_number": line_number + 1,
                        "line": line,
                    }));
                    if matches.len() >= max_matches {
                        break 'outer;
                    }
                }
            }
        }

        let truncated = matches.len() >= max_matches;
        let formatted = if matches.is_empty() {
            format!("No matches found for pattern '{}'.", pattern_str)
        } else {
            let mut lines: Vec<String> = matches
                .iter()
                .map(|m| {
                    format!(
                        "{}:{}: {}",
                        m["file"].as_str().unwrap_or(""),
                        m["line_number"],
                        m["line"].as_str().unwrap_or(""),
                    )
                })
                .collect();
            if truncated {
                lines.push(format!("... (results capped at {} matches)", max_matches));
            }
            lines.join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "pattern": pattern_str,
            "path": path.display().to_string(),
            "match_count": matches.len(),
            "truncated": truncated,
            "matches": matches,
        })))
    }
}

#[async_trait]
impl Skill for CodeAnalysisSkill {
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
                name: "analyze_file".to_string(),
                description: "Analyze a source code file: count total lines, blank lines, comment lines (// or #), and code lines.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source file to analyze"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "file_extension": { "type": "string" },
                        "line_count": { "type": "integer" },
                        "blank_lines": { "type": "integer" },
                        "comment_lines": { "type": "integer" },
                        "code_lines": { "type": "integer" }
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
                name: "analyze_directory".to_string(),
                description: "Analyze a directory of source code: count files by extension, total lines, and total files. Optionally filter by file extensions.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the directory to analyze"
                        },
                        "extensions": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional list of file extensions to include (e.g. [\"rs\", \"py\"])"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "total_files": { "type": "integer" },
                        "total_lines": { "type": "integer" },
                        "files_by_extension": { "type": "object" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 500,
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
                name: "search_patterns".to_string(),
                description: "Search for regex pattern matches in a file or directory. Returns matching lines with line numbers, capped at 100 matches.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to a file or directory to search"
                        },
                        "pattern": {
                            "type": "string",
                            "description": "Regular expression pattern to search for"
                        }
                    },
                    "required": ["path", "pattern"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "pattern": { "type": "string" },
                        "path": { "type": "string" },
                        "match_count": { "type": "integer" },
                        "truncated": { "type": "boolean" },
                        "matches": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 200,
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
            "analyze_file" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.analyze_file(&path)
            }
            "analyze_directory" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let extensions: Option<Vec<String>> = params.get("extensions");
                self.analyze_directory(&path, extensions)
            }
            "search_patterns" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let pattern: String = params.get("pattern").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: pattern".to_string())
                })?;
                self.search_patterns(&path, &pattern)
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

    fn test_skill(roots: Vec<PathBuf>) -> CodeAnalysisSkill {
        CodeAnalysisSkill::new(CodeAnalysisSkill::default_manifest(), roots)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = CodeAnalysisSkill::default_manifest();
        assert_eq!(manifest.name, "Code Analysis");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"analyze_file"));
        assert!(names.contains(&"analyze_directory"));
        assert!(names.contains(&"search_patterns"));
    }

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("abigail_ca_test_traversal");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);

        let result = skill.validate_path("../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_analyze_file() {
        let tmp = std::env::temp_dir().join("abigail_ca_test_analyze_file");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let test_content =
            "// A comment\nfn main() {\n    println!(\"hello\");\n}\n\n# another comment\n";
        std::fs::write(tmp.join("test.rs"), test_content).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill
            .analyze_file(&tmp.join("test.rs").display().to_string())
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["line_count"], 6);
        assert_eq!(data["blank_lines"], 1);
        assert_eq!(data["comment_lines"], 2);
        assert_eq!(data["code_lines"], 3);
        assert_eq!(data["file_extension"], "rs");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_analyze_directory() {
        let tmp = std::env::temp_dir().join("abigail_ca_test_analyze_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("sub")).unwrap();

        std::fs::write(tmp.join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(tmp.join("b.rs"), "fn b() {}\n// comment\n").unwrap();
        std::fs::write(tmp.join("sub/c.py"), "# comment\nprint('hi')\n").unwrap();
        std::fs::write(tmp.join("readme.md"), "# Title\n").unwrap();

        let skill = test_skill(vec![tmp.clone()]);

        // Analyze all files
        let result = skill
            .analyze_directory(&tmp.display().to_string(), None)
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["total_files"], 4);

        // Analyze only .rs files
        let result = skill
            .analyze_directory(&tmp.display().to_string(), Some(vec!["rs".to_string()]))
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["total_files"], 2);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_search_patterns() {
        let tmp = std::env::temp_dir().join("abigail_ca_test_search");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("example.rs"),
            "fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}\n",
        )
        .unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let result = skill
            .search_patterns(&tmp.display().to_string(), r"fn\s+\w+")
            .unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["match_count"], 1);
        assert!(!data["truncated"].as_bool().unwrap());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_health_check() {
        let tmp = std::env::temp_dir().join("abigail_ca_test_health");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let health = skill.health();
        assert_eq!(health.status, HealthStatus::Healthy);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
