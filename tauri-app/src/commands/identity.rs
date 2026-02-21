use crate::identity_manager::{AgentIdentityInfo, IdentitySummary};
use crate::state::AppState;
use abigail_core::SecretsVault;
use tauri::State;

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
pub fn load_agent(state: State<AppState>, agent_id: String) -> Result<(), String> {
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

    crate::rebuild_router_with_superego(&state)?;

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
pub fn disconnect_agent(state: State<AppState>) -> Result<(), String> {
    let mut active = state.active_agent_id.write().map_err(|e| e.to_string())?;
    *active = None;

    let mut birth = state.birth.write().map_err(|e| e.to_string())?;
    *birth = None;

    Ok(())
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
