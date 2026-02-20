//! Document analysis skill: word count, heading extraction, markdown-to-text
//! conversion, and sentence-based summarization within sandboxed directories.
//!
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

/// Document analysis skill with sandboxed directory access.
pub struct DocumentSkill {
    manifest: SkillManifest,
    /// Root directories where file operations are allowed.
    allowed_roots: Vec<PathBuf>,
}

impl DocumentSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse document skill.toml")
    }

    /// Create a new document skill with the given allowed root directories.
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

    /// Read a file's contents with size validation.
    fn read_file_contents(&self, path_str: &str) -> SkillResult<(PathBuf, String)> {
        let path = self.validate_path(path_str)?;

        if !path.is_file() {
            return Err(SkillError::ToolFailed(format!(
                "'{}' is not a file",
                path_str
            )));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read metadata: {}", e)))?;

        // Limit: don't read files larger than 1MB
        if metadata.len() > 1_048_576 {
            return Err(SkillError::ToolFailed(format!(
                "File too large ({} bytes). Maximum is 1MB.",
                metadata.len()
            )));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot read file: {}", e)))?;

        Ok((path, content))
    }

    /// Count words, lines, and characters in a document.
    fn word_count(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let (path, content) = self.read_file_contents(path_str)?;

        let words = content.split_whitespace().count();
        let lines = content.lines().count();
        let characters = content.chars().count();

        let formatted = format!(
            "Word count for {}:\n  Words: {}\n  Lines: {}\n  Characters: {}",
            path.display(),
            words,
            lines,
            characters
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "words": words,
            "lines": lines,
            "characters": characters,
        })))
    }

    /// Extract markdown headings from a document.
    fn extract_headings(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let (path, content) = self.read_file_contents(path_str)?;

        let heading_re = Regex::new(r"^(#{1,6})\s+(.+)$").expect("Failed to compile heading regex");

        let mut headings: Vec<serde_json::Value> = Vec::new();
        for line in content.lines() {
            if let Some(caps) = heading_re.captures(line) {
                let level = caps[1].len();
                let text = caps[2].trim().to_string();
                headings.push(serde_json::json!({
                    "level": level,
                    "text": text,
                }));
            }
        }

        let formatted = if headings.is_empty() {
            "No headings found.".to_string()
        } else {
            headings
                .iter()
                .map(|h| {
                    let level = h["level"].as_u64().unwrap_or(1);
                    let indent = "  ".repeat((level as usize).saturating_sub(1));
                    format!("{}H{}: {}", indent, level, h["text"].as_str().unwrap_or(""))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "count": headings.len(),
            "headings": headings,
        })))
    }

    /// Convert a markdown document to plain text by stripping markdown syntax.
    fn convert_md_to_text(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let (path, content) = self.read_file_contents(path_str)?;
        let original_length = content.len();

        let mut text = content;

        // Remove code blocks (``` ... ```)
        let code_block_re =
            Regex::new(r"(?ms)```[^\n]*\n.*?```").expect("Failed to compile code block regex");
        text = code_block_re.replace_all(&text, "").to_string();

        // Remove headings markers (# ## ### etc.) but keep the text
        let heading_re =
            Regex::new(r"(?m)^#{1,6}\s+").expect("Failed to compile heading strip regex");
        text = heading_re.replace_all(&text, "").to_string();

        // Remove bold (**text** or __text__)
        let bold_re = Regex::new(r"\*\*(.+?)\*\*|__(.+?)__").expect("Failed to compile bold regex");
        text = bold_re
            .replace_all(&text, |caps: &regex::Captures| {
                caps.get(1)
                    .or_else(|| caps.get(2))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            })
            .to_string();

        // Remove italic (*text* or _text_) — single markers only
        let italic_re =
            Regex::new(r"\*([^*]+)\*|_([^_]+)_").expect("Failed to compile italic regex");
        text = italic_re
            .replace_all(&text, |caps: &regex::Captures| {
                caps.get(1)
                    .or_else(|| caps.get(2))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default()
            })
            .to_string();

        // Convert links [text](url) to just text
        let link_re = Regex::new(r"\[([^\]]+)\]\([^)]+\)").expect("Failed to compile link regex");
        text = link_re.replace_all(&text, "$1").to_string();

        // Remove inline code (`code`)
        let inline_code_re = Regex::new(r"`([^`]+)`").expect("Failed to compile inline code regex");
        text = inline_code_re.replace_all(&text, "$1").to_string();

        // Clean up excessive blank lines
        let blank_lines_re = Regex::new(r"\n{3,}").expect("Failed to compile blank lines regex");
        text = blank_lines_re.replace_all(&text, "\n\n").to_string();

        let text = text.trim().to_string();

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": text,
            "path": path.display().to_string(),
            "original_length": original_length,
            "plain_length": text.len(),
        })))
    }

    /// Summarize a document by extracting the first N sentences.
    fn summarize(&self, path_str: &str, max_sentences: usize) -> SkillResult<ToolOutput> {
        let (path, content) = self.read_file_contents(path_str)?;

        // Split on sentence boundaries (". " followed by content)
        let sentences: Vec<&str> = content
            .split(". ")
            .filter(|s| !s.trim().is_empty())
            .collect();

        let take_count = max_sentences.min(sentences.len());
        let summary_parts: Vec<&str> = sentences[..take_count].to_vec();

        let summary = if summary_parts.is_empty() {
            content.trim().to_string()
        } else {
            let mut joined = summary_parts.join(". ");
            // Add trailing period if the original text had sentence endings
            if !joined.ends_with('.') && sentences.len() > take_count {
                joined.push('.');
            }
            joined
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": summary,
            "path": path.display().to_string(),
            "total_sentences": sentences.len(),
            "summary_sentences": take_count,
        })))
    }
}

#[async_trait]
impl Skill for DocumentSkill {
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
                name: "doc_word_count".to_string(),
                description: "Count words, lines, and characters in a document file.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the document file"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "words": { "type": "integer" },
                        "lines": { "type": "integer" },
                        "characters": { "type": "integer" }
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
                name: "doc_extract_headings".to_string(),
                description: "Extract markdown headings from a document. Returns a list of headings with their level and text.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the markdown document"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "count": { "type": "integer" },
                        "headings": { "type": "array" }
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
                name: "doc_convert_md_to_text".to_string(),
                description: "Convert a markdown document to plain text by stripping markdown syntax (headings, bold, italic, links, code blocks).".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the markdown file"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "original_length": { "type": "integer" },
                        "plain_length": { "type": "integer" }
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
                name: "doc_summarize".to_string(),
                description: "Summarize a document by extracting the first N sentences. Defaults to 5 sentences.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the document file"
                        },
                        "max_sentences": {
                            "type": "integer",
                            "description": "Maximum number of sentences to extract (default: 5)"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "total_sentences": { "type": "integer" },
                        "summary_sentences": { "type": "integer" }
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
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "doc_word_count" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.word_count(&path)
            }
            "doc_extract_headings" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.extract_headings(&path)
            }
            "doc_convert_md_to_text" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.convert_md_to_text(&path)
            }
            "doc_summarize" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let max_sentences: usize = params.get("max_sentences").unwrap_or(5);
                self.summarize(&path, max_sentences)
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

    fn test_skill(roots: Vec<PathBuf>) -> DocumentSkill {
        DocumentSkill::new(DocumentSkill::default_manifest(), roots)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = DocumentSkill::default_manifest();
        assert_eq!(manifest.name, "Document");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"doc_word_count"));
        assert!(names.contains(&"doc_extract_headings"));
        assert!(names.contains(&"doc_convert_md_to_text"));
        assert!(names.contains(&"doc_summarize"));
    }

    #[test]
    fn test_all_tools_autonomous() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        for tool in &tools {
            assert!(
                !tool.requires_confirmation,
                "Tool '{}' should not require confirmation",
                tool.name
            );
            assert!(tool.autonomous, "Tool '{}' should be autonomous", tool.name);
        }
    }
}
