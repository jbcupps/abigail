use crate::identity_manager::{AgentIdentityInfo, IdentitySummary};
use crate::state::AppState;
use abigail_core::SecretsVault;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupCheckResult {
    pub heartbeat_ok: bool,
    pub verification_ok: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn run_startup_checks(state: State<'_, AppState>) -> Result<StartupCheckResult, String> {
    let mut result = StartupCheckResult {
        heartbeat_ok: false,
        verification_ok: false,
        error: None,
    };

    // 1. LLM Heartbeat
    let router = {
        let r = state.router.read().map_err(|e| e.to_string())?;
        r.clone()
    };

    match router.heartbeat().await {
        Ok(_) => {
            result.heartbeat_ok = true;
        }
        Err(e) => {
            result.heartbeat_ok = false;
            result.error = Some(format!("LLM Heartbeat failed: {}", e));
            return Ok(result);
        }
    }

    // 2. Identity verification
    let active_id = {
        let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
        active.clone()
    };

    if let Some(agent_id) = active_id {
        match state.identity_manager.verify_agent(&agent_id) {
            Ok(_) => {
                result.verification_ok = true;
            }
            Err(e) => {
                result.verification_ok = false;
                result.error = Some(format!("Identity verification failed: {}", e));
                return Ok(result);
            }
        }
    } else {
        // If no active agent, verification is skipped but not failed
        result.verification_ok = true;
    }

    Ok(result)
}

#[tauri::command]
pub fn check_hive_status(state: State<AppState>) -> Result<bool, String> {
    Ok(state.identity_manager.has_agents()
        || state
            .identity_manager
            .data_root()
            .join("master.key")
            .exists())
}

#[tauri::command]
pub fn get_identities(state: State<AppState>) -> Result<Vec<AgentIdentityInfo>, String> {
    state.identity_manager.list_agents()
}

#[tauri::command]
pub fn get_active_agent(state: State<AppState>) -> Result<Option<String>, String> {
    let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
    Ok(active.clone())
}

#[tauri::command]
pub async fn load_agent(state: State<'_, AppState>, agent_id: String) -> Result<(), String> {
    tracing::info!("load_agent: loading agent {}", agent_id);

    let agent_config = state.identity_manager.load_agent(&agent_id)?;

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = agent_config;
    }

    {
        let config = state.config.read().map_err(|e| e.to_string())?;
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = SecretsVault::load(config.data_dir.clone())
            .unwrap_or_else(|_| SecretsVault::new(config.data_dir.clone()));
    }

    crate::rebuild_router(&state).await?;

    {
        let mut active = state.active_agent_id.write().map_err(|e| e.to_string())?;
        *active = Some(agent_id.clone());
    }

    Ok(())
}

#[tauri::command]
pub fn create_agent(state: State<AppState>, name: String) -> Result<String, String> {
    let (uuid, _agent_dir) = state.identity_manager.create_agent(&name)?;
    Ok(uuid)
}

#[tauri::command]
pub fn reset_birth(state: State<AppState>) -> Result<(), String> {
    let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
    let _agent_id = active.as_ref().ok_or("No active agent loaded")?;

    // We don't delete the whole agent, just its birth-related state in config and memory
    let mut config = state.config.write().map_err(|e| e.to_string())?;
    config.birth_complete = false;
    config.birth_stage = None;
    config
        .save(&config.config_path())
        .map_err(|e| e.to_string())?;

    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = None; // Force start_birth to be called again

    Ok(())
}

#[tauri::command]
pub fn delete_agent_identity(state: State<AppState>, agent_id: String) -> Result<(), String> {
    // Prevent deleting active agent
    {
        let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
        if let Some(id) = &*active {
            if id == &agent_id {
                return Err(
                    "Cannot delete the currently active agent. Disconnect first.".to_string(),
                );
            }
        }
    }

    state.identity_manager.delete_agent(&agent_id)
}

#[tauri::command]
pub fn archive_agent_identity(state: State<AppState>, agent_id: String) -> Result<String, String> {
    {
        let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
        if let Some(id) = &*active {
            if id == &agent_id {
                return Err("Cannot archive the currently active agent. Suspend first.".to_string());
            }
        }
    }

    state.identity_manager.archive_agent(&agent_id)
}

#[tauri::command]
pub fn suspend_agent(state: State<AppState>) -> Result<(), String> {
    let mut active = state.active_agent_id.write().map_err(|e| e.to_string())?;
    *active = None;

    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = None;

    Ok(())
}

#[tauri::command]
pub fn disconnect_agent(state: State<AppState>) -> Result<(), String> {
    suspend_agent(state)
}

#[tauri::command]
pub fn save_recovery_key(state: State<AppState>, private_key: String) -> Result<String, String> {
    let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
    let agent_id = active.as_ref().ok_or("No active agent loaded")?;

    state
        .identity_manager
        .save_recovery_key(agent_id, &private_key)
}

#[tauri::command]
pub fn migrate_legacy_identity(state: State<AppState>) -> Result<Option<String>, String> {
    state.identity_manager.migrate_legacy_identity()
}

#[tauri::command]
pub fn check_existing_identity(state: State<AppState>) -> Result<Option<IdentitySummary>, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;

    let has_signed_identity = config.data_dir.join("external_pubkey.bin").exists()
        && config.docs_dir.join("soul.md.sig").exists()
        && config.docs_dir.join("ethics.md.sig").exists()
        && config.docs_dir.join("instincts.md.sig").exists();

    if !config.birth_complete && !has_signed_identity {
        return Ok(None);
    }

    let name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "Unknown".to_string());

    let birth_date = config.birth_timestamp.clone().unwrap_or_else(|| {
        let soul_path = config.docs_dir.join("soul.md");
        if let Ok(meta) = std::fs::metadata(&soul_path) {
            if let Ok(mtime) = meta.modified() {
                return chrono::DateTime::<chrono::Utc>::from(mtime)
                    .format("%Y-%m-%d")
                    .to_string();
            }
        }
        "Unknown".to_string()
    });

    let db_path = config.db_path.clone();
    let has_memories = db_path.exists();
    let has_signatures = config.docs_dir.join("soul.md.sig").exists()
        && config.docs_dir.join("ethics.md.sig").exists()
        && config.docs_dir.join("instincts.md.sig").exists();

    Ok(Some(IdentitySummary {
        name,
        birth_date,
        data_path: config.data_dir.to_string_lossy().to_string(),
        has_memories,
        has_signatures,
    }))
}

#[tauri::command]
pub fn archive_identity(state: State<AppState>) -> Result<String, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    let agent_name = config
        .agent_name
        .clone()
        .unwrap_or_else(|| "agent".to_string());
    drop(config);

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let safe_name = agent_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    let backup_name = format!("{}_{}", timestamp, safe_name);
    let backups_dir = data_dir.join("backups");
    let backup_path = backups_dir.join(&backup_name);

    std::fs::create_dir_all(&backup_path)
        .map_err(|e| format!("Failed to create backup dir: {}", e))?;

    let files_to_move = [
        "config.json",
        "abigail_seed.db",
        "abigail_seed.db-wal",
        "abigail_seed.db-shm",
        "secrets.bin",
        "keys.bin",
        "external_pubkey.bin",
    ];

    for file in &files_to_move {
        let src = data_dir.join(file);
        if src.exists() {
            let dst = backup_path.join(file);
            std::fs::rename(&src, &dst).map_err(|e| format!("Failed to move {}: {}", file, e))?;
        }
    }

    let docs_src = data_dir.join("docs");
    if docs_src.exists() {
        let docs_dst = backup_path.join("docs");
        std::fs::rename(&docs_src, &docs_dst).map_err(|e| format!("Failed to move docs: {}", e))?;
    }

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = abigail_core::AppConfig::default_paths();
    }

    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = abigail_core::SecretsVault::new(data_dir.clone());
    }

    tracing::info!("Identity archived");
    Ok(backup_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn wipe_identity(state: State<AppState>) -> Result<(), String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let data_dir = config.data_dir.clone();
    drop(config);

    let files_to_delete = [
        "config.json",
        "abigail_seed.db",
        "abigail_seed.db-wal",
        "abigail_seed.db-shm",
        "secrets.bin",
        "keys.bin",
        "external_pubkey.bin",
    ];

    for file in &files_to_delete {
        let path = data_dir.join(file);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Failed to delete {}: {}", file, e))?;
        }
    }

    let docs_path = data_dir.join("docs");
    if docs_path.exists() {
        std::fs::remove_dir_all(&docs_path).map_err(|e| format!("Failed to delete docs: {}", e))?;
    }

    {
        let mut config = state.config.write().map_err(|e| e.to_string())?;
        *config = abigail_core::AppConfig::default_paths();
    }

    {
        let mut vault = state.secrets.lock().map_err(|e| e.to_string())?;
        *vault = abigail_core::SecretsVault::new(data_dir);
    }

    tracing::warn!("Identity wiped - all data deleted");
    Ok(())
}
