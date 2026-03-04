//! Git skill: repository operations via the `git` CLI.
//!
//! Provides five tools for common git operations:
//! - `git_status` — porcelain status output
//! - `git_log` — recent commit history
//! - `git_diff` — working tree or staged diff
//! - `git_branch_list` — local and remote branches
//! - `git_commit` — create a commit (requires confirmation)

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, Permission, Skill,
    SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult, ToolDescriptor, ToolOutput,
    ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;

/// Default timeout for git commands in seconds.
const GIT_TIMEOUT_SECS: u64 = 30;

/// Maximum output size per stream (stdout/stderr): 64KB.
const MAX_OUTPUT_BYTES: usize = 65_536;

/// Git skill that shells out to the `git` CLI for all operations.
pub struct GitSkill {
    manifest: SkillManifest,
}

impl GitSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse git skill.toml")
    }

    /// Create a new git skill from the given manifest.
    pub fn new(manifest: SkillManifest) -> Self {
        Self { manifest }
    }

    /// Run a git command with the given arguments.
    ///
    /// If `repo_path` is provided, `-C <repo_path>` is prepended to the
    /// argument list so the command runs in that repository. Output is
    /// captured and truncated to [`MAX_OUTPUT_BYTES`] per stream.
    async fn run_git(&self, args: &[&str], repo_path: Option<&str>) -> SkillResult<ToolOutput> {
        let mut cmd = Command::new("git");

        // If a repo path is specified, use -C to run in that directory.
        if let Some(path) = repo_path {
            cmd.arg("-C").arg(path);
        }

        for arg in args {
            cmd.arg(arg);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let full_args: Vec<&str> = if let Some(path) = repo_path {
            let mut v = vec!["-C", path];
            v.extend_from_slice(args);
            v
        } else {
            args.to_vec()
        };
        tracing::info!(
            "Executing: git {}",
            abigail_core::redact_secrets(&full_args.join(" "))
        );

        let timeout = Duration::from_secs(GIT_TIMEOUT_SECS);
        let result = tokio::time::timeout(timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

                let stdout_truncated = stdout.len() > MAX_OUTPUT_BYTES;
                let stderr_truncated = stderr.len() > MAX_OUTPUT_BYTES;
                if stdout_truncated {
                    stdout.truncate(MAX_OUTPUT_BYTES);
                    stdout.push_str("\n... [output truncated]");
                }
                if stderr_truncated {
                    stderr.truncate(MAX_OUTPUT_BYTES);
                    stderr.push_str("\n... [output truncated]");
                }

                let exit_code = output.status.code().unwrap_or(-1);
                let success = output.status.success();

                let formatted = if success {
                    if stdout.is_empty() {
                        format!(
                            "git command completed successfully (exit code {})",
                            exit_code
                        )
                    } else {
                        stdout.clone()
                    }
                } else {
                    let mut msg = format!("git command failed (exit code {})", exit_code);
                    if !stderr.is_empty() {
                        msg.push_str(&format!("\nStderr: {}", stderr));
                    }
                    if !stdout.is_empty() {
                        msg.push_str(&format!("\nStdout: {}", stdout));
                    }
                    msg
                };

                let data = serde_json::json!({
                    "formatted": formatted,
                    "exit_code": exit_code,
                    "success": success,
                    "stdout": stdout,
                    "stderr": stderr,
                    "stdout_truncated": stdout_truncated,
                    "stderr_truncated": stderr_truncated,
                });
                if success {
                    Ok(ToolOutput::success(data))
                } else {
                    Ok(ToolOutput {
                        success: false,
                        data: Some(data),
                        error: Some(formatted),
                        metadata: Default::default(),
                    })
                }
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!("Failed to execute git: {}", e))),
            Err(_) => Ok(ToolOutput::error(format!(
                "git command timed out after {} seconds",
                GIT_TIMEOUT_SECS
            ))),
        }
    }
}

#[async_trait]
impl Skill for GitSkill {
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
        SkillHealth {
            status: HealthStatus::Healthy,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            // 1. git_status
            ToolDescriptor {
                name: "git_status".to_string(),
                description: "Show the working tree status in porcelain format.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Optional path to the git repository"
                        }
                    },
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "success": { "type": "boolean" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 2000,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::ShellExecute],
                autonomous: true,
                requires_confirmation: false,
            },
            // 2. git_log
            ToolDescriptor {
                name: "git_log".to_string(),
                description: "Show recent commit history in oneline format.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Optional path to the git repository"
                        },
                        "count": {
                            "type": "integer",
                            "description": "Number of commits to show (default 10)"
                        }
                    },
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "success": { "type": "boolean" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 2000,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::ShellExecute],
                autonomous: true,
                requires_confirmation: false,
            },
            // 3. git_diff
            ToolDescriptor {
                name: "git_diff".to_string(),
                description: "Show changes in the working tree or staged changes.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Optional path to the git repository"
                        },
                        "staged": {
                            "type": "boolean",
                            "description": "If true, show staged changes (--cached). Default false."
                        }
                    },
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "success": { "type": "boolean" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 2000,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::ShellExecute],
                autonomous: true,
                requires_confirmation: false,
            },
            // 4. git_branch_list
            ToolDescriptor {
                name: "git_branch_list".to_string(),
                description: "List all local and remote branches.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Optional path to the git repository"
                        }
                    },
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "success": { "type": "boolean" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 2000,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::ShellExecute],
                autonomous: true,
                requires_confirmation: false,
            },
            // 5. git_commit
            ToolDescriptor {
                name: "git_commit".to_string(),
                description: "Create a git commit with the given message. Only commits already-staged changes.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo_path": {
                            "type": "string",
                            "description": "Optional path to the git repository"
                        },
                        "message": {
                            "type": "string",
                            "description": "The commit message"
                        }
                    },
                    "required": ["message"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "exit_code": { "type": "integer" },
                        "success": { "type": "boolean" },
                        "stdout": { "type": "string" },
                        "stderr": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 3000,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::ShellExecute],
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
        let repo_path: Option<String> = params.get("repo_path");

        match tool_name {
            "git_status" => {
                self.run_git(&["status", "--porcelain=v1"], repo_path.as_deref())
                    .await
            }
            "git_log" => {
                let count: u64 = params.get("count").unwrap_or(10);
                let count_str = format!("-{}", count);
                self.run_git(&["log", "--oneline", &count_str], repo_path.as_deref())
                    .await
            }
            "git_diff" => {
                let staged: bool = params.get("staged").unwrap_or(false);
                if staged {
                    self.run_git(&["diff", "--cached"], repo_path.as_deref())
                        .await
                } else {
                    self.run_git(&["diff"], repo_path.as_deref()).await
                }
            }
            "git_branch_list" => self.run_git(&["branch", "-a"], repo_path.as_deref()).await,
            "git_commit" => {
                let message: String = params.get("message").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: message".to_string())
                })?;
                self.run_git(&["commit", "-m", &message], repo_path.as_deref())
                    .await
            }
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
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

    fn test_skill() -> GitSkill {
        GitSkill::new(GitSkill::default_manifest())
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = GitSkill::default_manifest();
        assert_eq!(manifest.name, "Git");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 5);
        assert_eq!(tools[0].name, "git_status");
        assert_eq!(tools[1].name, "git_log");
        assert_eq!(tools[2].name, "git_diff");
        assert_eq!(tools[3].name, "git_branch_list");
        assert_eq!(tools[4].name, "git_commit");
    }

    #[test]
    fn test_git_commit_requires_confirmation() {
        let skill = test_skill();
        let tools = skill.tools();
        let commit_tool = tools.iter().find(|t| t.name == "git_commit").unwrap();
        assert!(
            commit_tool.requires_confirmation,
            "git_commit must require confirmation"
        );
        assert!(!commit_tool.autonomous, "git_commit must not be autonomous");

        // All read-only tools should be autonomous and not require confirmation
        for tool_name in &["git_status", "git_log", "git_diff", "git_branch_list"] {
            let tool = tools.iter().find(|t| t.name == *tool_name).unwrap();
            assert!(tool.autonomous, "{} should be autonomous", tool_name);
            assert!(
                !tool.requires_confirmation,
                "{} should not require confirmation",
                tool_name
            );
        }
    }
}
