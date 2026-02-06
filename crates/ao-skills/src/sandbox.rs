//! Security sandbox and resource limits.
//!
//! **Permission checks:** Network access is enforced by the executor before each tool run (see
//! `SkillExecutor::audit_action_for_tool` and `check_permission`). File and memory permission
//! logic exists here (`AuditActionKind::FileRead`, `FileWrite`, `MemoryAccess`) but must be
//! invoked by the code path that actually performs file or memory I/O on behalf of a skill (e.g. a
//! capability layer). Skills that do raw file or network I/O should go through such a layer so the
//! sandbox is checked.
//!
//! **Resource limits:** `ResourceLimits` (max_memory_bytes, max_cpu_ms, etc.) are not currently
//! enforced at runtime (no timeout or memory cap on tool execution). They are defined for future
//! use and for documentation. Consider enforcing timeouts per tool call in the executor.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::manifest::{Permission, SkillId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_bytes: u64,
    pub max_cpu_ms: u64,
    pub max_concurrency: u32,
    pub network_bandwidth: Option<u64>,
    pub storage_quota: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_cpu_ms: 30_000,                   // 30s
            max_concurrency: 10,
            network_bandwidth: None,
            storage_quota: 100 * 1024 * 1024, // 100MB
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditAction {
    pub kind: AuditActionKind,
}

#[derive(Debug, Clone)]
pub enum AuditActionKind {
    NetworkRequest { domain: String },
    FileRead { path: String },
    FileWrite { path: String },
    MemoryAccess { namespace: Option<String> },
    Other(String),
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub action: AuditAction,
    pub allowed: bool,
}

pub struct SkillSandbox {
    pub skill_id: SkillId,
    pub granted_permissions: HashSet<Permission>,
    pub resource_limits: ResourceLimits,
    pub audit_log: Vec<AuditEntry>,
}

impl SkillSandbox {
    pub fn new(skill_id: SkillId, granted_permissions: Vec<Permission>, resource_limits: ResourceLimits) -> Self {
        let granted_permissions = granted_permissions.into_iter().collect();
        Self {
            skill_id,
            granted_permissions,
            resource_limits,
            audit_log: Vec::new(),
        }
    }

    /// Check if action is permitted. Denies when not granted; logs to audit_log.
    pub fn check_permission(&mut self, action: &AuditAction) -> bool {
        let allowed = self.check_permission_inner(action);
        self.audit_log.push(AuditEntry {
            timestamp: chrono::Utc::now(),
            action: action.clone(),
            allowed,
        });
        allowed
    }

    fn check_permission_inner(&self, action: &AuditAction) -> bool {
        match &action.kind {
            AuditActionKind::NetworkRequest { domain } => {
                for p in &self.granted_permissions {
                    match p {
                        Permission::Network(np) => match np {
                            crate::manifest::NetworkPermission::Full => return true,
                            crate::manifest::NetworkPermission::LocalOnly => {
                                if domain == "localhost" || domain.starts_with("127.") {
                                    return true;
                                }
                            }
                            crate::manifest::NetworkPermission::Domains(domains) => {
                                if domains.iter().any(|d| domain == d || domain.ends_with(d)) {
                                    return true;
                                }
                            }
                        },
                        _ => {}
                    }
                }
                false
            }
            AuditActionKind::FileRead { path } => {
                for p in &self.granted_permissions {
                    match p {
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Full) => return true,
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Read(allowed)) => {
                            if allowed.iter().any(|a| path == a || path.starts_with(&format!("{}/", a))) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                false
            }
            AuditActionKind::FileWrite { path } => {
                for p in &self.granted_permissions {
                    match p {
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Full) => return true,
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Write(allowed)) => {
                            if allowed.iter().any(|a| path == a || path.starts_with(&format!("{}/", a))) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                false
            }
            AuditActionKind::MemoryAccess { namespace } => {
                for p in &self.granted_permissions {
                    match p {
                        Permission::Memory(crate::manifest::MemoryPermission::ReadOnly)
                        | Permission::Memory(crate::manifest::MemoryPermission::ReadWrite) => return true,
                        Permission::Memory(crate::manifest::MemoryPermission::Namespace(ns)) => {
                            if namespace.as_ref().map(|n| n == ns).unwrap_or(false) {
                                return true;
                            }
                        }
                        _ => {}
                    }
                }
                false
            }
            AuditActionKind::Other(_) => true,
        }
    }
}
