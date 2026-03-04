use crate::state::AppState;
use abigail_core::config::SignedSkillAllowlistEntry;
use abigail_core::{McpServerDefinition, RuntimeMode};
use abigail_skills::protocol::mcp::{HttpMcpClient, McpTool};
use abigail_skills::{
    FileSystemPermission, HealthStatus, Permission, SkillExecutionPolicy, SkillId, SkillManifest,
    ToolDescriptor, ToolOutput, ToolParams,
};
use std::collections::HashMap;
use tauri::State;

pub use abigail_core::RESERVED_PROVIDER_KEYS;

fn refresh_skill_policy(
    state: &State<'_, AppState>,
    config: &abigail_core::AppConfig,
) -> Result<(), String> {
    state
        .registry
        .set_execution_policy(SkillExecutionPolicy::from_app_config(config))
        .map_err(|e| e.to_string())
}

fn is_dangerous_tool(td: &ToolDescriptor) -> bool {
    let name = td.name.to_lowercase();
    let destructive_name = [
        "delete", "remove", "drop", "wipe", "truncate", "reset", "kill",
    ]
    .iter()
    .any(|k| name.contains(k));
    let destructive_permission = td.required_permissions.iter().any(|perm| {
        matches!(
            perm,
            Permission::FileSystem(FileSystemPermission::Write(_))
                | Permission::FileSystem(FileSystemPermission::Full)
        )
    });
    td.requires_confirmation || destructive_name || destructive_permission
}

fn resolve_mcp_server_url(
    state: &State<'_, AppState>,
    server_id: &str,
) -> Result<(String, abigail_core::McpTrustPolicy), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let server = config
        .mcp_servers
        .iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("MCP server not found: {}", server_id))?;
    if server.transport != "http" {
        return Err("Only HTTP transport is supported for MCP list_tools".to_string());
    }
    config
        .mcp_trust_policy
        .validate_http_server_url(server_id, &server.command_or_url)?;
    Ok((
        server.command_or_url.clone(),
        config.mcp_trust_policy.clone(),
    ))
}

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillManifest>, String> {
    let mode = { state.config.read().map_err(|e| e.to_string())?.runtime_mode };
    if mode == RuntimeMode::Daemon {
        let entity_url = {
            state
                .config
                .read()
                .map_err(|e| e.to_string())?
                .entity_daemon_url
                .clone()
        };
        let client = daemon_client::EntityClient::new(&entity_url);
        let skills = client.list_skills().await.map_err(|e| e.to_string())?;
        return Ok(skills
            .into_iter()
            .map(|s| SkillManifest {
                id: SkillId(s.id),
                name: s.name,
                version: s.version,
                description: s.description,
                license: None,
                category: String::new(),
                keywords: vec![],
                runtime: String::new(),
                min_abigail_version: String::new(),
                platforms: vec![],
                capabilities: vec![],
                permissions: vec![],
                secrets: vec![],
                config_defaults: std::collections::HashMap::new(),
            })
            .collect());
    }
    state.registry.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_discovered_skills(state: State<AppState>) -> Result<Vec<SkillManifest>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(abigail_skills::SkillRegistry::discover(&paths))
}

/// One row for the Skills Vault UI: a secret key declared by some skill(s), and whether it is set.
#[derive(serde::Serialize)]
pub struct SkillsVaultEntry {
    pub secret_name: String,
    pub skill_names: Vec<String>,
    pub description: Option<String>,
    pub is_set: bool,
}

#[tauri::command]
pub fn list_skills_vault_entries(state: State<AppState>) -> Result<Vec<SkillsVaultEntry>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let skills_dir = config.data_dir.join("skills");
    let paths = vec![skills_dir.clone()];

    let registered = state.registry.list().map_err(|e| e.to_string())?;
    let discovered = abigail_skills::SkillRegistry::discover(&paths);
    let mut by_name: std::collections::HashMap<String, (Vec<String>, Option<String>)> =
        std::collections::HashMap::new();

    for m in registered.iter().chain(discovered.iter()) {
        for s in &m.secrets {
            by_name
                .entry(s.name.clone())
                .or_insert_with(|| (Vec::new(), Some(s.description.clone())))
                .0
                .push(m.name.clone());
        }
    }
    let vault = state.skills_secrets.lock().map_err(|e| e.to_string())?;
    let entries: Vec<SkillsVaultEntry> = by_name
        .into_iter()
        .map(|(secret_name, (mut skill_names, description))| {
            skill_names.sort();
            skill_names.dedup();
            let is_set = vault.exists(&secret_name);
            SkillsVaultEntry {
                secret_name,
                skill_names,
                description,
                is_set,
            }
        })
        .collect();
    Ok(entries)
}

#[tauri::command]
pub fn list_missing_skill_secrets(
    state: State<AppState>,
) -> Result<Vec<abigail_skills::MissingSkillSecret>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let paths = vec![config.data_dir.join("skills")];
    Ok(state.registry.list_all_missing_secrets(&paths))
}

/// Validate that `key` is in the allowed secret namespace: either a reserved
/// provider name or a secret declared by any registered/discovered skill.
///
/// This is the single source of truth for secret-key validation.  Both the
/// Tauri `store_secret` command and the `TauriHiveOps::set_skill_secret`
/// trait impl delegate here.
pub fn validate_secret_namespace_with(
    registry: &abigail_skills::SkillRegistry,
    data_dir: &std::path::Path,
    key: &str,
) -> Result<(), String> {
    if RESERVED_PROVIDER_KEYS.contains(&key) {
        return Ok(());
    }
    let skills = registry.list().map_err(|e| e.to_string())?;
    if skills
        .iter()
        .any(|m| m.secrets.iter().any(|s| s.name == key))
    {
        return Ok(());
    }
    let discovered = abigail_skills::SkillRegistry::discover(&[data_dir.join("skills")]);
    if discovered
        .iter()
        .any(|m| m.secrets.iter().any(|s| s.name == key))
    {
        return Ok(());
    }
    Err(format!(
        "Secret key '{}' is not in the allowed namespace. Keys must be a reserved provider name ({}) or declared in a skill manifest.",
        key,
        RESERVED_PROVIDER_KEYS.join(", ")
    ))
}

fn validate_secret_namespace(state: &State<AppState>, key: &str) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    validate_secret_namespace_with(&state.registry, &config.data_dir, key)
}

/// Keys that, when stored, should trigger re-initialization of the Email skill
/// so it picks up new credentials without an app restart.
const EMAIL_SECRET_KEYS: &[&str] = &[
    "imap_password",
    "imap_user",
    "imap_host",
    "imap_port",
    "imap_tls_mode",
    "smtp_host",
    "smtp_port",
];

#[tauri::command]
pub async fn store_secret(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), String> {
    let key = key.trim().to_string();
    let value = value.trim().to_string();
    if key.is_empty() {
        return Err("Secret key cannot be empty".to_string());
    }
    if value.is_empty() {
        return Err("Secret value cannot be empty".to_string());
    }

    let mode = { state.config.read().map_err(|e| e.to_string())?.runtime_mode };
    if mode == RuntimeMode::Daemon {
        let hive_url = {
            state
                .config
                .read()
                .map_err(|e| e.to_string())?
                .hive_daemon_url
                .clone()
        };
        let client = daemon_client::HiveDaemonClient::new(&hive_url);
        client
            .store_secret(&key, &value)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    validate_secret_namespace(&state, &key)?;
    let mut vault = state.skills_secrets.lock().map_err(|e| e.to_string())?;
    vault.set_secret(&key, &value);
    vault.save().map_err(|e| e.to_string())?;

    // Re-initialize Email skill when email-related secrets change so the skill
    // picks up new credentials without requiring an app restart.
    if EMAIL_SECRET_KEYS.contains(&key.as_str()) {
        match crate::create_email_skill_for_registry(&state) {
            Ok(skill) => {
                let skill_id = skill_email::EmailSkill::default_manifest().id.clone();
                // Check actual health before logging success
                let health = skill.health();
                let _ = state.registry.unregister(&skill_id);
                if let Err(e) = state.registry.register(skill_id, skill) {
                    tracing::warn!("Email skill re-register after secret store failed: {}", e);
                } else if health.status == HealthStatus::Healthy {
                    tracing::info!("Email skill re-initialized after secret update");
                } else {
                    tracing::warn!(
                        "Email skill re-registered but not fully initialized: {:?}",
                        health.message
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Email skill reinit after secret store failed: {}", e);
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub fn list_tools(state: State<AppState>, skill_id: String) -> Result<Vec<ToolDescriptor>, String> {
    let id = SkillId(skill_id);
    let (skill, _) = state.registry.get_skill(&id).map_err(|e| e.to_string())?;
    Ok(skill.tools())
}

#[tauri::command]
pub async fn execute_tool(
    state: State<'_, AppState>,
    skill_id: String,
    tool_name: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<ToolOutput, String> {
    let id = SkillId(skill_id);
    state
        .registry
        .enforce_skill_execution(&id)
        .map_err(|e| e.to_string())?;
    if let Ok((skill, _)) = state.registry.get_skill(&id) {
        if let Some(td) = skill.tools().into_iter().find(|t| t.name == tool_name) {
            let mentor_confirmed = params
                .get("mentor_confirmed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_dangerous_tool(&td) && !mentor_confirmed {
                return Err(
                    "This tool requires explicit mentor confirmation. Re-run with `mentor_confirmed: true`."
                        .to_string(),
                );
            }
        }
    }
    let tool_params = ToolParams { values: params };
    state
        .executor
        .execute(&id, &tool_name, tool_params)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_mcp_servers(state: State<AppState>) -> Result<Vec<McpServerDefinition>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.mcp_servers.clone())
}

#[tauri::command]
pub async fn mcp_list_tools(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<McpTool>, String> {
    let (url, trust_policy) = resolve_mcp_server_url(&state, &server_id)?;
    let client = HttpMcpClient::new_with_policy(server_id, url, Some(trust_policy));
    client.initialize().await.map_err(|e| e.to_string())?;
    client.list_tools_impl().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_approved_skills(state: State<AppState>) -> Result<Vec<String>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.approved_skill_ids.clone())
}

#[tauri::command]
pub fn approve_skill(state: State<AppState>, skill_id: String) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if !config.approved_skill_ids.contains(&skill_id) {
        config.approved_skill_ids.push(skill_id.clone());
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        refresh_skill_policy(&state, &config)?;
    }
    Ok(())
}

#[tauri::command]
pub fn list_signed_skill_allowlist(
    state: State<AppState>,
) -> Result<Vec<SignedSkillAllowlistEntry>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    Ok(config.signed_skill_allowlist.clone())
}

#[tauri::command]
pub fn upsert_signed_skill_allowlist_entry(
    state: State<AppState>,
    skill_id: String,
    signer: String,
    signature: String,
    source: String,
) -> Result<(), String> {
    if signer.trim().is_empty() || signature.trim().is_empty() || source.trim().is_empty() {
        return Err("signer, signature, and source are required.".to_string());
    }
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if let Some(entry) = config
        .signed_skill_allowlist
        .iter_mut()
        .find(|e| e.skill_id == skill_id)
    {
        entry.signer = signer;
        entry.signature = signature;
        entry.source = source;
        entry.active = true;
    } else {
        config
            .signed_skill_allowlist
            .push(SignedSkillAllowlistEntry {
                skill_id,
                signer,
                signature,
                source,
                added_at: chrono::Utc::now().to_rfc3339(),
                active: true,
            });
    }
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;
    refresh_skill_policy(&state, &config)?;
    Ok(())
}

#[tauri::command]
pub fn revoke_signed_skill_allowlist_entry(
    state: State<AppState>,
    skill_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    if let Some(entry) = config
        .signed_skill_allowlist
        .iter_mut()
        .find(|e| e.skill_id == skill_id)
    {
        entry.active = false;
        if let Some(reason) = reason {
            if !reason.trim().is_empty() {
                entry.source = format!("{} (revoked: {})", entry.source, reason.trim());
            }
        }
        config
            .save(&config.config_path())
            .map_err(|e| e.to_string())?;
        refresh_skill_policy(&state, &config)?;
        return Ok(());
    }
    Err(format!(
        "No signed allowlist entry found for skill {}",
        skill_id
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_skills::SkillRegistry;
    use std::sync::Arc;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("abigail_skills_cmd_tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reserved_keys_always_accepted() {
        let tmp = tmp_dir("reserved");
        let registry = SkillRegistry::new();
        for key in RESERVED_PROVIDER_KEYS {
            assert!(
                validate_secret_namespace_with(&registry, &tmp, key).is_ok(),
                "reserved key '{}' should be accepted",
                key
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn unknown_key_rejected_with_empty_registry() {
        let tmp = tmp_dir("unknown");
        let registry = SkillRegistry::new();
        let result = validate_secret_namespace_with(&registry, &tmp, "totally_bogus");
        assert!(result.is_err(), "unknown key should be rejected");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn imap_keys_accepted_when_email_skill_registered() {
        let tmp = tmp_dir("imap");
        let registry = SkillRegistry::new();
        let manifest = skill_email::EmailSkill::default_manifest();
        let skill = skill_email::EmailSkill::new(manifest.clone());
        registry
            .register(manifest.id.clone(), Arc::new(skill))
            .unwrap();

        for key in &[
            "imap_password",
            "imap_user",
            "imap_host",
            "imap_port",
            "imap_tls_mode",
            "smtp_host",
            "smtp_port",
        ] {
            assert!(
                validate_secret_namespace_with(&registry, &tmp, key).is_ok(),
                "IMAP key '{}' should be accepted with EmailSkill registered",
                key
            );
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn imap_keys_rejected_without_email_skill() {
        let tmp = tmp_dir("no_email");
        let registry = SkillRegistry::new();
        let result = validate_secret_namespace_with(&registry, &tmp, "imap_password");
        assert!(
            result.is_err(),
            "imap_password should be rejected when EmailSkill is NOT registered"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn email_manifest_declares_expected_secrets() {
        let manifest = skill_email::EmailSkill::default_manifest();
        let names: Vec<&str> = manifest.secrets.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"imap_password"), "missing imap_password");
        assert!(names.contains(&"imap_user"), "missing imap_user");
        assert!(names.contains(&"imap_host"), "missing imap_host");
        assert!(names.contains(&"imap_port"), "missing imap_port");
        assert!(names.contains(&"imap_tls_mode"), "missing imap_tls_mode");
        assert!(names.contains(&"smtp_host"), "missing smtp_host");
        assert!(names.contains(&"smtp_port"), "missing smtp_port");
    }
}
