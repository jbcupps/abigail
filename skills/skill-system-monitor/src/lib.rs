//! System Monitor skill: read-only system information and resource usage.
//!
//! Provides three tools for monitoring the local system:
//! - `system_info`: OS name, version, kernel, hostname, CPU architecture
//! - `system_resources`: CPU usage, memory totals, and availability
//! - `process_list`: Running processes with optional name filter

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig,
    SkillError, SkillHealth, SkillManifest, SkillResult, ToolDescriptor, ToolOutput, ToolParams,
    TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use sysinfo::System;

/// Maximum number of processes returned by `process_list`.
const MAX_PROCESS_RESULTS: usize = 100;

/// System Monitor skill with read-only system queries.
pub struct SystemMonitorSkill {
    manifest: SkillManifest,
}

impl SystemMonitorSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse system-monitor skill.toml")
    }

    /// Create a new system monitor skill.
    pub fn new(manifest: SkillManifest) -> Self {
        Self { manifest }
    }

    /// Gather static system information (OS, kernel, hostname, arch).
    fn system_info() -> ToolOutput {
        ToolOutput::success(serde_json::json!({
            "os_name": System::name().unwrap_or_else(|| "unknown".to_string()),
            "os_version": System::os_version().unwrap_or_else(|| "unknown".to_string()),
            "kernel_version": System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
            "hostname": System::host_name().unwrap_or_else(|| "unknown".to_string()),
            "cpu_arch": std::env::consts::ARCH,
        }))
    }

    /// Gather live resource usage (CPU, memory).
    fn system_resources() -> ToolOutput {
        let mut sys = System::new();
        sys.refresh_cpu();
        sys.refresh_memory();

        let total_memory_mb = sys.total_memory() / (1024 * 1024);
        let used_memory_mb = sys.used_memory() / (1024 * 1024);
        let available_memory_mb = total_memory_mb.saturating_sub(used_memory_mb);
        let cpu_count = sys.cpus().len();
        let cpu_usage_percent: f32 = if cpu_count > 0 {
            sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / cpu_count as f32
        } else {
            0.0
        };

        ToolOutput::success(serde_json::json!({
            "total_memory_mb": total_memory_mb,
            "used_memory_mb": used_memory_mb,
            "available_memory_mb": available_memory_mb,
            "cpu_count": cpu_count,
            "cpu_usage_percent": cpu_usage_percent,
        }))
    }

    /// List running processes, optionally filtered by name.
    fn process_list(filter: Option<&str>) -> ToolOutput {
        let mut sys = System::new();
        sys.refresh_processes();

        let mut processes: Vec<serde_json::Value> = sys
            .processes()
            .values()
            .filter(|p| {
                if let Some(f) = filter {
                    p.name().to_lowercase().contains(&f.to_lowercase())
                } else {
                    true
                }
            })
            .map(|p| {
                serde_json::json!({
                    "pid": p.pid().as_u32(),
                    "name": p.name(),
                    "cpu_usage": p.cpu_usage(),
                    "memory_kb": p.memory() / 1024,
                })
            })
            .collect();

        // Sort by memory descending
        processes.sort_by(|a, b| {
            let mem_b = b["memory_kb"].as_u64().unwrap_or(0);
            let mem_a = a["memory_kb"].as_u64().unwrap_or(0);
            mem_b.cmp(&mem_a)
        });

        // Cap at MAX_PROCESS_RESULTS
        processes.truncate(MAX_PROCESS_RESULTS);

        ToolOutput::success(serde_json::json!({
            "count": processes.len(),
            "processes": processes,
        }))
    }
}

#[async_trait]
impl Skill for SystemMonitorSkill {
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
            ToolDescriptor {
                name: "system_info".to_string(),
                description: "Return static system information: OS name, OS version, kernel version, hostname, and CPU architecture.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "os_name": { "type": "string" },
                        "os_version": { "type": "string" },
                        "kernel_version": { "type": "string" },
                        "hostname": { "type": "string" },
                        "cpu_arch": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "system_resources".to_string(),
                description: "Return live system resource usage: total, used, and available memory (MB), CPU count, and average CPU usage percent.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "total_memory_mb": { "type": "integer" },
                        "used_memory_mb": { "type": "integer" },
                        "available_memory_mb": { "type": "integer" },
                        "cpu_count": { "type": "integer" },
                        "cpu_usage_percent": { "type": "number" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 100,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "process_list".to_string(),
                description: "List running processes with PID, name, CPU usage, and memory. Optionally filter by process name (case-insensitive). Results sorted by memory descending, capped at 100.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filter": {
                            "type": "string",
                            "description": "Optional substring filter for process names (case-insensitive)"
                        }
                    },
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "count": { "type": "integer" },
                        "processes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "pid": { "type": "integer" },
                                    "name": { "type": "string" },
                                    "cpu_usage": { "type": "number" },
                                    "memory_kb": { "type": "integer" }
                                }
                            }
                        }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 200,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![],
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
            "system_info" => Ok(Self::system_info()),
            "system_resources" => Ok(Self::system_resources()),
            "process_list" => {
                let filter: Option<String> = params.get("filter");
                Ok(Self::process_list(filter.as_deref()))
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

    fn test_skill() -> SystemMonitorSkill {
        SystemMonitorSkill::new(SystemMonitorSkill::default_manifest())
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = SystemMonitorSkill::default_manifest();
        assert_eq!(manifest.name, "System Monitor");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "system_info");
        assert_eq!(tools[1].name, "system_resources");
        assert_eq!(tools[2].name, "process_list");
    }

    #[test]
    fn test_all_tools_autonomous() {
        let skill = test_skill();
        for tool in skill.tools() {
            assert!(tool.autonomous, "Tool '{}' should be autonomous", tool.name);
            assert!(
                !tool.requires_confirmation,
                "Tool '{}' should not require confirmation",
                tool.name
            );
        }
    }
}
