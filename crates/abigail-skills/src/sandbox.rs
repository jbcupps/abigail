//! Security sandbox and resource limits.
//!
//! **Permission checks:** Network access is enforced by the executor before each tool run (see
//! `SkillExecutor::audit_action_for_tool` and `check_permission`). File and memory permission
//! logic exists here (`AuditActionKind::FileRead`, `FileWrite`, `MemoryAccess`) but must be
//! invoked by the code path that actually performs file or memory I/O on behalf of a skill (e.g. a
//! capability layer). Skills that do raw file or network I/O should go through such a layer so the
//! sandbox is checked.
//!
//! **Resource limits:** Timeouts and concurrency are enforced in `SkillExecutor` (see
//! `executor.rs`): each tool call is bounded by `ResourceLimits::max_cpu_ms` and global
//! concurrency by `max_concurrency`. Memory and storage caps (max_memory_bytes, storage_quota)
//! are intended to be enforced by capability layers and/or a future WASM runtime for untrusted skills.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::manifest::{Permission, SkillId};

/// Normalize a path by resolving `.` and `..` components lexically.
/// This does not resolve symlinks (that is done by canonicalization helpers).
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past root/prefix.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonicalize a path for policy checks.
///
/// - Existing paths are fully canonicalized (resolves symlinks).
/// - Non-existing paths resolve the nearest existing ancestor and append
///   the remaining lexical suffix, so write targets can be validated safely.
fn canonicalize_for_policy(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let normalized = normalize_path(&absolute);

    if normalized.exists() {
        return normalized.canonicalize().ok();
    }

    let mut cursor = normalized.as_path();
    let mut suffix = Vec::new();
    while !cursor.exists() {
        let name = cursor.file_name()?.to_os_string();
        suffix.push(name);
        cursor = cursor.parent()?;
    }

    let mut canonical = cursor.canonicalize().ok()?;
    for part in suffix.into_iter().rev() {
        canonical.push(part);
    }
    Some(canonical)
}

/// Check if `path` is under `allowed_prefix` using canonical path comparison.
fn path_is_under(path: &str, allowed_prefix: &str) -> bool {
    let policy_path = match canonicalize_for_policy(Path::new(path)) {
        Some(p) => p,
        None => return false,
    };
    let policy_prefix = match canonicalize_for_policy(Path::new(allowed_prefix)) {
        Some(p) => p,
        None => return false,
    };
    policy_path == policy_prefix || policy_path.starts_with(&policy_prefix)
}

/// Domain allow-list match: exact or true subdomain only.
fn domain_matches_allowed(domain: &str, allowed: &str) -> bool {
    let domain = domain.trim_end_matches('.').to_lowercase();
    let allowed = allowed.trim_end_matches('.').to_lowercase();
    if domain.is_empty() || allowed.is_empty() {
        return false;
    }
    domain == allowed || domain.ends_with(&format!(".{}", allowed))
}

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
            max_cpu_ms: 30_000,                  // 30s
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
    pub fn new(
        skill_id: SkillId,
        granted_permissions: Vec<Permission>,
        resource_limits: ResourceLimits,
    ) -> Self {
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
                    if let Permission::Network(np) = p {
                        match np {
                            crate::manifest::NetworkPermission::Full => return true,
                            crate::manifest::NetworkPermission::LocalOnly => {
                                if domain == "localhost" || domain.starts_with("127.") {
                                    return true;
                                }
                            }
                            crate::manifest::NetworkPermission::Domains(domains) => {
                                if domains.iter().any(|d| domain_matches_allowed(domain, d)) {
                                    return true;
                                }
                            }
                        }
                    }
                }
                false
            }
            AuditActionKind::FileRead { path } => {
                for p in &self.granted_permissions {
                    match p {
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Full) => {
                            return true
                        }
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Read(
                            allowed,
                        )) => {
                            if allowed.iter().any(|a| path_is_under(path, a)) {
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
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Full) => {
                            return true
                        }
                        Permission::FileSystem(crate::manifest::FileSystemPermission::Write(
                            allowed,
                        )) => {
                            if allowed.iter().any(|a| path_is_under(path, a)) {
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
                        | Permission::Memory(crate::manifest::MemoryPermission::ReadWrite) => {
                            return true
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn as_str_path(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    #[test]
    fn domain_match_allows_exact_and_subdomain() {
        assert!(domain_matches_allowed("api.example.com", "api.example.com"));
        assert!(domain_matches_allowed(
            "v1.api.example.com",
            "api.example.com"
        ));
        assert!(domain_matches_allowed(
            "api.example.com.",
            "api.example.com"
        ));
    }

    #[test]
    fn domain_match_rejects_suffix_impersonation() {
        assert!(!domain_matches_allowed(
            "evil-api.example.com",
            "api.example.com"
        ));
        assert!(!domain_matches_allowed(
            "api.example.com.evil",
            "api.example.com"
        ));
    }

    #[test]
    fn path_is_under_allows_nested_nonexistent_write_target() {
        let tmp = std::env::temp_dir().join("abigail_sandbox_nested_write");
        let _ = std::fs::remove_dir_all(&tmp);
        let allowed = tmp.join("allowed");
        std::fs::create_dir_all(&allowed).unwrap();

        let nested = allowed.join("new").join("deep").join("file.txt");
        assert!(path_is_under(&as_str_path(&nested), &as_str_path(&allowed),));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn path_is_under_blocks_parent_traversal_outside_root() {
        let tmp = std::env::temp_dir().join("abigail_sandbox_parent_traversal");
        let _ = std::fs::remove_dir_all(&tmp);
        let allowed = tmp.join("allowed");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let traversed = allowed.join("..").join("outside").join("secret.txt");
        assert!(!path_is_under(
            &as_str_path(&traversed),
            &as_str_path(&allowed),
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn path_is_under_blocks_symlink_escape() {
        let tmp = std::env::temp_dir().join("abigail_sandbox_symlink_escape");
        let _ = std::fs::remove_dir_all(&tmp);
        let allowed = tmp.join("allowed");
        let outside = tmp.join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let link = allowed.join("link_out");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, &link).unwrap();
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(&outside, &link).is_err() {
                // Symlink creation may require elevated privileges on some systems.
                let _ = std::fs::remove_dir_all(&tmp);
                return;
            }
        }

        let escaped = link.join("secret.txt");
        assert!(!path_is_under(
            &as_str_path(&escaped),
            &as_str_path(&allowed),
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
