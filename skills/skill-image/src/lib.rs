//! Image processing skill: get info, resize, and convert images within sandboxed directories.
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

/// Image processing skill with sandboxed directory access.
pub struct ImageSkill {
    manifest: SkillManifest,
    /// Root directories where file operations are allowed.
    allowed_roots: Vec<PathBuf>,
}

impl ImageSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse image skill.toml")
    }

    /// Create a new image skill with the given allowed root directories.
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

        // For new paths (output files), check parent exists and is allowed
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

    /// Get image metadata: width, height, format, and color type.
    fn image_info(&self, path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", path_str)));
        }

        let img = image::open(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot open image: {}", e)))?;

        let width = img.width();
        let height = img.height();
        let color_type = format!("{:?}", img.color());

        let format = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let formatted = format!(
            "Image: {} ({}x{}, {}, {})",
            path.display(),
            width,
            height,
            format,
            color_type
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "path": path.display().to_string(),
            "width": width,
            "height": height,
            "format": format,
            "color_type": color_type,
        })))
    }

    /// Resize an image to the specified dimensions and save to the output path.
    fn image_resize(
        &self,
        path_str: &str,
        output_path_str: &str,
        width: u32,
        height: u32,
    ) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;
        let output_path = self.validate_path(output_path_str)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", path_str)));
        }

        let img = image::open(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot open image: {}", e)))?;

        let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);

        resized
            .save(&output_path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot save resized image: {}", e)))?;

        let formatted = format!(
            "Resized {} to {}x{} -> {}",
            path.display(),
            width,
            height,
            output_path.display()
        );

        tracing::info!("{}", formatted);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "input_path": path.display().to_string(),
            "output_path": output_path.display().to_string(),
            "width": width,
            "height": height,
        })))
    }

    /// Convert an image to a different format by saving to the output path.
    /// The output format is inferred from the output path's file extension.
    fn image_convert(&self, path_str: &str, output_path_str: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(path_str)?;
        let output_path = self.validate_path(output_path_str)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", path_str)));
        }

        let img = image::open(&path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot open image: {}", e)))?;

        // Detect the output format from the extension to give a better error message
        let _output_format = image::ImageFormat::from_path(&output_path).map_err(|e| {
            SkillError::ToolFailed(format!("Cannot determine output format: {}", e))
        })?;

        img.save(&output_path)
            .map_err(|e| SkillError::ToolFailed(format!("Cannot save converted image: {}", e)))?;

        let input_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let output_ext = output_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let formatted = format!(
            "Converted {} ({}) -> {} ({})",
            path.display(),
            input_ext,
            output_path.display(),
            output_ext
        );

        tracing::info!("{}", formatted);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "input_path": path.display().to_string(),
            "output_path": output_path.display().to_string(),
            "input_format": input_ext,
            "output_format": output_ext,
        })))
    }
}

#[async_trait]
impl Skill for ImageSkill {
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
                name: "image_info".to_string(),
                description: "Get image metadata: width, height, format, and color type."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the image file"
                        }
                    },
                    "required": ["path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "path": { "type": "string" },
                        "width": { "type": "integer" },
                        "height": { "type": "integer" },
                        "format": { "type": "string" },
                        "color_type": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 50,
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
                name: "image_resize".to_string(),
                description:
                    "Resize an image to specified dimensions and save to an output path."
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source image file"
                        },
                        "output_path": {
                            "type": "string",
                            "description": "Absolute path for the resized output image"
                        },
                        "width": {
                            "type": "integer",
                            "description": "Target width in pixels"
                        },
                        "height": {
                            "type": "integer",
                            "description": "Target height in pixels"
                        }
                    },
                    "required": ["path", "output_path", "width", "height"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "input_path": { "type": "string" },
                        "output_path": { "type": "string" },
                        "width": { "type": "integer" },
                        "height": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 500,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                    Permission::FileSystem(FileSystemPermission::Write(vec!["~".to_string()])),
                ],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "image_convert".to_string(),
                description:
                    "Convert an image to a different format. The output format is inferred from the file extension."
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute path to the source image file"
                        },
                        "output_path": {
                            "type": "string",
                            "description": "Absolute path for the converted output image (format inferred from extension)"
                        }
                    },
                    "required": ["path", "output_path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "input_path": { "type": "string" },
                        "output_path": { "type": "string" },
                        "input_format": { "type": "string" },
                        "output_format": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 500,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                    Permission::FileSystem(FileSystemPermission::Write(vec!["~".to_string()])),
                ],
                autonomous: false,
                requires_confirmation: true,
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
            "image_info" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                self.image_info(&path)
            }
            "image_resize" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let output_path: String = params.get("output_path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: output_path".to_string())
                })?;
                let width: u32 = params.get("width").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: width".to_string())
                })?;
                let height: u32 = params.get("height").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: height".to_string())
                })?;
                self.image_resize(&path, &output_path, width, height)
            }
            "image_convert" => {
                let path: String = params.get("path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: path".to_string())
                })?;
                let output_path: String = params.get("output_path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: output_path".to_string())
                })?;
                self.image_convert(&path, &output_path)
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

    fn test_skill(roots: Vec<PathBuf>) -> ImageSkill {
        ImageSkill::new(ImageSkill::default_manifest(), roots)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = ImageSkill::default_manifest();
        assert_eq!(manifest.name, "Image");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"image_info"));
        assert!(names.contains(&"image_resize"));
        assert!(names.contains(&"image_convert"));
    }

    #[test]
    fn test_resize_requires_confirmation() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        let resize = tools.iter().find(|t| t.name == "image_resize").unwrap();
        assert!(resize.requires_confirmation);
        assert!(!resize.autonomous);
    }

    #[test]
    fn test_convert_requires_confirmation() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        let convert = tools.iter().find(|t| t.name == "image_convert").unwrap();
        assert!(convert.requires_confirmation);
        assert!(!convert.autonomous);
    }
}
