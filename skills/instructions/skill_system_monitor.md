# System Monitor Skill

You have access to system monitoring tools. Use these when the user asks about system health, resource usage, or running processes.

## Available Tools

- **system_info**: Get static system information. Returns OS name and version, CPU model, core count, total RAM, and disk capacity.
- **system_resources**: Get current resource utilization. Returns CPU usage percentage, available and used RAM, and disk usage.
- **process_list**: List running processes. Params: `sort_by` (string, optional, one of `cpu`, `memory`, `name`, default `cpu`), `limit` (int, optional, default 20). Returns PID, name, CPU usage, and memory usage for each process.

## Usage Guidelines

- These tools are read-only and do not modify system state.
- Use `system_info` for hardware and OS details; use `system_resources` for live utilization metrics.
- Use `process_list` with `sort_by: memory` to help diagnose high memory usage, or `sort_by: cpu` for CPU-bound issues.
- Resource values are point-in-time snapshots, not averages.
