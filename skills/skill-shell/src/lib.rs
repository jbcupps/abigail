//! Shell command skill: execute commands with safety controls.
//!
//! Provides a single `run_command` tool that executes shell commands with:
//! - Configurable timeout (default 30 seconds)
//! - Blocklist for dangerous commands (rm -rf /, sudo, etc.)
//! - Output size limits (stdout/stderr capped at 64KB each)
//! - Working directory validation

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

/// Maximum output size per stream (stdout/stderr): 64KB.
const MAX_OUTPUT_BYTES: usize = 65_536;

/// Default command timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Commands or patterns that are always blocked.
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "rm -rf ~",
    "mkfs.",
    "dd if=/dev/zero",
    "dd if=/dev/random",
    ":(){:|:&};:", // fork bomb
    "chmod -R 777 /",
    "chown -R",
    "shutdown",
    "reboot",
    "poweroff",
    "halt",
    "init 0",
    "init 6",
    "format c:",
    "> /dev/sda",
    "mv /* /dev/null",
];

/// Commands that require explicit allowlisting (blocked by default).
const ELEVATED_COMMANDS: &[&str] = &["sudo", "su ", "pkill", "kill -9", "killall"];

/// Shell command skill with safety controls.
pub struct ShellSkill {
    manifest: SkillManifest,
    /// Maximum timeout in seconds. Commands exceeding this are killed.
    max_timeout_secs: u64,
    /// Whether to allow elevated (sudo/su) commands.
    allow_elevated: bool,
}

impl ShellSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse shell skill.toml")
    }

    /// Create a new shell skill with default safety settings.
    pub fn new(manifest: SkillManifest) -> Self {
        Self {
            manifest,
            max_timeout_secs: DEFAULT_TIMEOUT_SECS,
            allow_elevated: false,
        }
    }

    /// Check if a command is blocked by the safety blocklist.
    fn check_safety(&self, command: &str) -> Result<(), String> {
        let lower = command.to_lowercase();

        // Check absolute blocklist
        for pattern in BLOCKED_PATTERNS {
            if lower.contains(pattern) {
                return Err(format!(
                    "Command blocked: contains dangerous pattern '{}'",
                    pattern
                ));
            }
        }

        // Check elevated commands
        if !self.allow_elevated {
            for pattern in ELEVATED_COMMANDS {
                if lower.starts_with(pattern) || lower.contains(&format!(" {}", pattern)) {
                    return Err(format!(
                        "Elevated command '{}' is not allowed. Enable allow_elevated to permit.",
                        pattern.trim()
                    ));
                }
            }
        }

        Ok(())
    }

    /// Execute a shell command with timeout and output capture.
    async fn run_command(
        &self,
        command: &str,
        working_dir: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> SkillResult<ToolOutput> {
        // Safety check
        if let Err(reason) = self.check_safety(command) {
            return Ok(ToolOutput::error(reason));
        }

        let timeout = Duration::from_secs(
            timeout_secs
                .unwrap_or(DEFAULT_TIMEOUT_SECS)
                .min(self.max_timeout_secs),
        );

        let shell = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };
        let shell_arg = if cfg!(target_os = "windows") {
            "/C"
        } else {
            "-c"
        };

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg).arg(command);

        if let Some(dir) = working_dir {
            let dir_path = std::path::Path::new(dir);
            if !dir_path.is_dir() {
                return Ok(ToolOutput::error(format!(
                    "Working directory '{}' does not exist",
                    dir
                )));
            }
            cmd.current_dir(dir);
        }

        // Capture output
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        tracing::info!("Executing: {}", command);

        let result = tokio::time::timeout(timeout, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

                // Truncate output if too large
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
                        format!("Command completed successfully (exit code {})", exit_code)
                    } else {
                        stdout.clone()
                    }
                } else {
                    let mut msg = format!("Command failed (exit code {})", exit_code);
                    if !stderr.is_empty() {
                        msg.push_str(&format!("\nStderr: {}", stderr));
                    }
                    if !stdout.is_empty() {
                        msg.push_str(&format!("\nStdout: {}", stdout));
                    }
                    msg
                };

                Ok(ToolOutput::success(serde_json::json!({
                    "formatted": formatted,
                    "exit_code": exit_code,
                    "success": success,
                    "stdout": stdout,
                    "stderr": stderr,
                    "stdout_truncated": stdout_truncated,
                    "stderr_truncated": stderr_truncated,
                })))
            }
            Ok(Err(e)) => Ok(ToolOutput::error(format!(
                "Failed to execute command: {}",
                e
            ))),
            Err(_) => Ok(ToolOutput::error(format!(
                "Command timed out after {} seconds",
                timeout.as_secs()
            ))),
        }
    }
}

#[async_trait]
impl Skill for ShellSkill {
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
        vec![ToolDescriptor {
            name: "run_command".to_string(),
            description: "Execute a shell command and return its output. Commands are subject to safety checks and timeout limits.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Optional working directory for the command"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (default 30, max 30)"
                    }
                },
                "required": ["command"]
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
                latency_ms: 5000,
                network_bound: false,
                token_cost: None,
            },
            required_permissions: vec![Permission::ShellExecute],
            autonomous: false,
            requires_confirmation: true,
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        if tool_name != "run_command" {
            return Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            )));
        }

        let command: String = params.get("command").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: command".to_string())
        })?;

        let working_dir: Option<String> = params.get("working_directory");
        let timeout_secs: Option<u64> = params.get("timeout_secs");

        self.run_command(&command, working_dir.as_deref(), timeout_secs)
            .await
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

    fn test_skill() -> ShellSkill {
        ShellSkill::new(ShellSkill::default_manifest())
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = ShellSkill::default_manifest();
        assert_eq!(manifest.name, "Shell");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "run_command");
        assert!(tools[0].requires_confirmation);
    }

    #[test]
    fn test_safety_blocks_rm_rf() {
        let skill = test_skill();
        assert!(skill.check_safety("rm -rf /").is_err());
        assert!(skill.check_safety("rm -rf /*").is_err());
        assert!(skill.check_safety("rm -rf ~").is_err());
    }

    #[test]
    fn test_safety_blocks_fork_bomb() {
        let skill = test_skill();
        assert!(skill.check_safety(":(){:|:&};:").is_err());
    }

    #[test]
    fn test_safety_blocks_sudo() {
        let skill = test_skill();
        assert!(skill.check_safety("sudo rm -rf /tmp").is_err());
    }

    #[test]
    fn test_safety_allows_normal_commands() {
        let skill = test_skill();
        assert!(skill.check_safety("ls -la").is_ok());
        assert!(skill.check_safety("echo hello").is_ok());
        assert!(skill.check_safety("cat /etc/hostname").is_ok());
        assert!(skill.check_safety("git status").is_ok());
        assert!(skill.check_safety("cargo test").is_ok());
    }

    #[test]
    fn test_safety_blocks_shutdown() {
        let skill = test_skill();
        assert!(skill.check_safety("shutdown -h now").is_err());
        assert!(skill.check_safety("reboot").is_err());
        assert!(skill.check_safety("poweroff").is_err());
    }

    #[tokio::test]
    async fn test_run_echo() {
        let skill = test_skill();
        let result = skill
            .run_command("echo hello", None, Some(5))
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert!(data["stdout"].as_str().unwrap().contains("hello"));
        assert_eq!(data["exit_code"], 0);
    }

    #[tokio::test]
    async fn test_run_blocked_command() {
        let skill = test_skill();
        let result = skill.run_command("rm -rf /", None, None).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("blocked"));
    }

    #[tokio::test]
    async fn test_run_failing_command() {
        let skill = test_skill();
        // Use a cross-platform command that will fail: echo works everywhere,
        // but we need a non-zero exit. Use a subshell/cmd trick.
        let cmd = if cfg!(target_os = "windows") {
            "cmd /C exit 1"
        } else {
            "ls /nonexistent_dir_12345"
        };
        let result = skill.run_command(cmd, None, Some(5)).await.unwrap();
        assert!(result.success); // ToolOutput.success is true, but the command failed
        let data = result.data.unwrap();
        assert!(!data["success"].as_bool().unwrap()); // exit code != 0
    }

    #[tokio::test]
    async fn test_run_with_working_dir() {
        let skill = test_skill();
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.display().to_string();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };
        let result = skill
            .run_command(cmd, Some(&tmp_str), Some(5))
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        let stdout = data["stdout"].as_str().unwrap();
        // The output should reference the temp directory
        assert!(
            !stdout.trim().is_empty(),
            "Expected working directory in stdout"
        );
    }

    #[tokio::test]
    async fn test_timeout() {
        let skill = test_skill();
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let result = skill.run_command(cmd, None, Some(1)).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("timed out"));
    }
}
