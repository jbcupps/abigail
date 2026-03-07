use abigail_core::{is_reserved_provider_key, SecretsVault, RESERVED_PROVIDER_KEYS};
use abigail_skills::{
    build_preloaded_skills, preloaded_secret_keys, DynamicApiSkill, HiveManagementSkill,
    HiveOperations, Skill, SkillFactory, SkillManifest, SkillRegistry,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub type SkillSecrets = Arc<Mutex<SecretsVault>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovedCapability {
    pub id: &'static str,
    pub reason: &'static str,
    pub secret_keys: &'static [&'static str],
    pub skill_ids: &'static [&'static str],
}

pub const REMOVED_EMAIL_SECRET_KEYS: &[&str] = &[
    "imap_user",
    "imap_password",
    "imap_host",
    "imap_port",
    "imap_tls_mode",
    "smtp_user",
    "smtp_password",
    "smtp_host",
    "smtp_port",
    "smtp_tls_mode",
];

pub const REMOVED_EMAIL_SKILL_IDS: &[&str] =
    &["com.abigail.skills.email", "com.abigail.skills.proton-mail"];

pub const REMOVED_EMAIL_CAPABILITY: RemovedCapability = RemovedCapability {
    id: "email_transport",
    reason: "IMAP/SMTP email transport is no longer supported in mainline Abigail; use Browser skill fallback instead.",
    secret_keys: REMOVED_EMAIL_SECRET_KEYS,
    skill_ids: REMOVED_EMAIL_SKILL_IDS,
};

pub const REMOVED_CAPABILITIES: &[RemovedCapability] = &[REMOVED_EMAIL_CAPABILITY];

pub const SUPPORTED_NATIVE_SKILL_IDS: &[&str] = &[
    "com.abigail.skills.browser",
    "com.abigail.skills.calendar",
    "com.abigail.skills.clipboard",
    "com.abigail.skills.code-analysis",
    "com.abigail.skills.database",
    "com.abigail.skills.document",
    "com.abigail.skills.filesystem",
    "com.abigail.skills.git",
    "com.abigail.skills.http",
    "com.abigail.skills.image",
    "com.abigail.skills.knowledge-base",
    "com.abigail.skills.notification",
    "com.abigail.skills.perplexity-search",
    "com.abigail.skills.shell",
    "com.abigail.skills.system-monitor",
    "com.abigail.skills.web-search",
];

pub fn supported_native_skill_ids() -> BTreeSet<String> {
    SUPPORTED_NATIVE_SKILL_IDS
        .iter()
        .map(|id| (*id).to_string())
        .collect()
}

pub fn supported_runtime_skill_ids() -> BTreeSet<String> {
    let mut ids = supported_native_skill_ids();
    ids.insert("builtin.hive_management".to_string());
    ids.insert("builtin.skill_factory".to_string());
    for skill in build_preloaded_skills(None) {
        ids.insert(skill.manifest().id.0.clone());
    }
    ids
}

pub fn removed_capability_for_secret_key(key: &str) -> Option<&'static RemovedCapability> {
    REMOVED_CAPABILITIES
        .iter()
        .find(|cap| cap.secret_keys.contains(&key))
}

pub fn is_removed_secret_key(key: &str) -> bool {
    removed_capability_for_secret_key(key).is_some()
}

pub fn removed_secret_key_error(key: &str) -> String {
    match removed_capability_for_secret_key(key) {
        Some(capability) => format!(
            "Secret key '{}' belongs to removed capability '{}'. {}",
            key, capability.id, capability.reason
        ),
        None => format!("Secret key '{}' is not supported.", key),
    }
}

pub fn validate_hive_secret_key(key: &str) -> Result<(), String> {
    if is_removed_secret_key(key) {
        return Err(removed_secret_key_error(key));
    }
    Ok(())
}

pub fn validate_secret_namespace(
    registry: &SkillRegistry,
    dynamic_skill_dirs: &[PathBuf],
    key: &str,
) -> Result<(), String> {
    let manifests = registry.list().map_err(|e| e.to_string())?;
    validate_secret_namespace_from_manifests(&manifests, dynamic_skill_dirs, key)
}

pub fn validate_secret_namespace_from_manifests(
    manifests: &[SkillManifest],
    dynamic_skill_dirs: &[PathBuf],
    key: &str,
) -> Result<(), String> {
    if is_reserved_provider_key(key) {
        return Ok(());
    }

    if is_removed_secret_key(key) {
        return Err(removed_secret_key_error(key));
    }

    if preloaded_secret_keys().iter().any(|allowed| allowed == key) {
        return Ok(());
    }

    if secret_declared_in_manifests(manifests, key) {
        return Ok(());
    }

    let discovered = SkillRegistry::discover(dynamic_skill_dirs);
    if secret_declared_in_manifests(&discovered, key) {
        return Ok(());
    }

    Err(format!(
        "Secret key '{}' is not in the allowed namespace. Keys must be a reserved provider name ({}) or declared in a supported skill manifest.",
        key,
        RESERVED_PROVIDER_KEYS.join(", ")
    ))
}

pub fn create_browser_skill(
    data_dir: PathBuf,
    entity_id: Option<String>,
    allow_local_network: bool,
) -> Arc<dyn Skill> {
    Arc::new(skill_browser::BrowserSkill::new_for_entity(
        skill_browser::BrowserSkill::default_manifest(),
        allow_local_network,
        data_dir,
        entity_id,
    ))
}

pub fn register_identity_bound_skills(
    registry: &Arc<SkillRegistry>,
    data_dir: PathBuf,
    entity_id: Option<String>,
    allow_local_network: bool,
) {
    let browser_id = skill_browser::BrowserSkill::default_manifest().id.clone();
    let browser_skill = create_browser_skill(data_dir, entity_id, allow_local_network);
    let _ = registry.unregister(&browser_id);
    if let Err(err) = registry.register(browser_id.clone(), browser_skill) {
        tracing::warn!("Failed to register {}: {}", browser_id.0, err);
    }
}

pub fn register_supported_native_skills(
    registry: &Arc<SkillRegistry>,
    entity_dir: &Path,
    allow_local_network: bool,
    skills_secrets: SkillSecrets,
) {
    let allowed_roots = default_allowed_roots(entity_dir);

    macro_rules! register_skill {
        ($skill:expr) => {{
            let skill = $skill;
            let skill_id = skill.manifest().id.clone();
            if let Err(err) = registry.register(skill_id.clone(), Arc::new(skill)) {
                tracing::warn!("Failed to register {}: {}", skill_id.0, err);
            }
        }};
    }

    register_skill!(skill_clipboard::ClipboardSkill::new(
        skill_clipboard::ClipboardSkill::default_manifest()
    ));
    register_skill!(skill_shell::ShellSkill::new(
        skill_shell::ShellSkill::default_manifest()
    ));
    register_skill!(skill_git::GitSkill::new(
        skill_git::GitSkill::default_manifest()
    ));
    register_skill!(skill_notification::NotificationSkill::new(
        skill_notification::NotificationSkill::default_manifest()
    ));
    register_skill!(skill_system_monitor::SystemMonitorSkill::new(
        skill_system_monitor::SystemMonitorSkill::default_manifest()
    ));
    register_skill!(skill_http::HttpSkill::new_with_local_network(
        skill_http::HttpSkill::default_manifest(),
        allow_local_network
    ));
    register_skill!(skill_calendar::CalendarSkill::new(
        skill_calendar::CalendarSkill::default_manifest(),
        entity_dir.to_path_buf()
    ));
    register_skill!(skill_knowledge_base::KnowledgeBaseSkill::new(
        skill_knowledge_base::KnowledgeBaseSkill::default_manifest(),
        entity_dir.to_path_buf()
    ));
    register_skill!(skill_filesystem::FilesystemSkill::new(
        skill_filesystem::FilesystemSkill::default_manifest(),
        allowed_roots.clone()
    ));
    register_skill!(skill_database::DatabaseSkill::new(
        skill_database::DatabaseSkill::default_manifest(),
        allowed_roots.clone()
    ));
    register_skill!(skill_code_analysis::CodeAnalysisSkill::new(
        skill_code_analysis::CodeAnalysisSkill::default_manifest(),
        allowed_roots.clone()
    ));
    register_skill!(skill_document::DocumentSkill::new(
        skill_document::DocumentSkill::default_manifest(),
        allowed_roots.clone()
    ));
    register_skill!(skill_image::ImageSkill::new(
        skill_image::ImageSkill::default_manifest(),
        allowed_roots
    ));
    register_skill!(skill_web_search::WebSearchSkill::with_secrets(
        skill_web_search::WebSearchSkill::default_manifest(),
        skills_secrets.clone()
    ));
    register_skill!(
        skill_perplexity_search::PerplexitySearchSkill::with_secrets(
            skill_perplexity_search::PerplexitySearchSkill::default_manifest(),
            skills_secrets
        )
    );
}

pub fn register_skill_factory(
    registry: &Arc<SkillRegistry>,
    skills_dir: PathBuf,
    skills_secrets: SkillSecrets,
) {
    let factory = SkillFactory::new(skills_dir)
        .with_registry(registry.clone())
        .with_secrets(skills_secrets);
    let skill_id = abigail_skills::SkillId("builtin.skill_factory".to_string());
    if let Err(err) = registry.register(skill_id.clone(), Arc::new(factory)) {
        tracing::warn!("Failed to register {}: {}", skill_id.0, err);
    }
}

pub fn register_hive_management_skill(
    registry: &Arc<SkillRegistry>,
    hive_ops: Arc<dyn HiveOperations>,
) {
    let skill = Arc::new(HiveManagementSkill::new(hive_ops));
    let skill_id = skill.manifest().id.clone();
    if let Err(err) = registry.register(skill_id.clone(), skill) {
        tracing::warn!("Failed to register {}: {}", skill_id.0, err);
    }
}

pub fn register_preloaded_skills(registry: &Arc<SkillRegistry>, skills_secrets: SkillSecrets) {
    for skill in build_preloaded_skills(Some(skills_secrets)) {
        let skill_id = skill.manifest().id.clone();
        if let Err(err) = registry.register(skill_id.clone(), Arc::new(skill)) {
            tracing::warn!("Failed to register preloaded skill {}: {}", skill_id.0, err);
        }
    }
}

pub fn register_dynamic_api_skills(
    registry: &Arc<SkillRegistry>,
    skills_dir: &Path,
    skills_secrets: SkillSecrets,
) -> usize {
    let dynamic_skills = DynamicApiSkill::discover(skills_dir, Some(skills_secrets));
    let count = dynamic_skills.len();
    for skill in dynamic_skills {
        let skill_id = skill.manifest().id.clone();
        if let Err(err) = registry.register(skill_id.clone(), Arc::new(skill)) {
            tracing::warn!("Failed to register dynamic skill {}: {}", skill_id.0, err);
        }
    }
    count
}

pub fn collect_declared_secret_keys(
    registry: &SkillRegistry,
    dynamic_skill_dirs: &[PathBuf],
) -> Result<BTreeSet<String>, String> {
    let mut keys = BTreeSet::new();

    for manifest in registry.list().map_err(|e| e.to_string())? {
        for secret in manifest.secrets {
            keys.insert(secret.name);
        }
    }

    for manifest in SkillRegistry::discover(dynamic_skill_dirs) {
        for secret in manifest.secrets {
            keys.insert(secret.name);
        }
    }

    for key in preloaded_secret_keys() {
        keys.insert(key);
    }

    for key in RESERVED_PROVIDER_KEYS {
        keys.insert((*key).to_string());
    }

    Ok(keys)
}

fn secret_declared_in_manifests(manifests: &[SkillManifest], key: &str) -> bool {
    manifests
        .iter()
        .any(|manifest| manifest.secrets.iter().any(|secret| secret.name == key))
}

fn default_allowed_roots(entity_dir: &Path) -> Vec<PathBuf> {
    let mut allowed_roots = vec![entity_dir.to_path_buf(), std::env::temp_dir()];
    if let Some(docs_dir) =
        directories::UserDirs::new().and_then(|dirs| dirs.document_dir().map(|d| d.to_path_buf()))
    {
        allowed_roots.push(docs_dir.join("Abigail"));
    }
    allowed_roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn removed_email_secret_keys_are_rejected() {
        for key in REMOVED_EMAIL_SECRET_KEYS {
            let err = validate_secret_namespace_from_manifests(&[], &[], key)
                .expect_err("removed email key should be rejected");
            assert!(err.contains("email_transport"));
        }
    }

    #[test]
    fn supported_native_skill_ids_are_bootstrapped_in_instruction_registry() {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf();
        let registry_path = workspace_root.join("skills").join("registry.toml");

        let contents = std::fs::read_to_string(&registry_path).expect("read registry.toml");
        let parsed: toml::Value = toml::from_str(&contents).expect("parse registry.toml");
        let actual: BTreeSet<String> = parsed
            .get("skill")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("id").and_then(|value| value.as_str()))
            .map(|id| id.to_string())
            .collect();

        for skill_id in supported_native_skill_ids() {
            assert!(
                actual.contains(&skill_id),
                "supported native skill '{}' is missing from skills/registry.toml",
                skill_id
            );
        }
    }

    #[test]
    fn runtime_skill_inventory_matches_shared_registration() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("create data dir");

        let registry = Arc::new(SkillRegistry::new());
        let skills_secrets = Arc::new(Mutex::new(SecretsVault::new_custom(
            data_dir.clone(),
            "skills.bin",
        )));

        register_hive_management_skill(&registry, Arc::new(TestHiveOps::default()));
        register_identity_bound_skills(&registry, data_dir.clone(), Some("entity-1".into()), false);
        register_supported_native_skills(&registry, &data_dir, false, skills_secrets.clone());
        register_skill_factory(&registry, data_dir.join("skills"), skills_secrets.clone());
        register_preloaded_skills(&registry, skills_secrets);

        let ids: BTreeSet<String> = registry
            .list()
            .expect("list skills")
            .into_iter()
            .map(|manifest| manifest.id.0)
            .collect();

        let expected = supported_runtime_skill_ids();
        for skill_id in expected {
            assert!(
                ids.contains(&skill_id),
                "shared bootstrap did not register expected skill '{}'",
                skill_id
            );
        }
    }

    #[derive(Default)]
    struct TestHiveOps {
        secrets: Mutex<HashMap<String, String>>,
    }

    #[async_trait::async_trait]
    impl HiveOperations for TestHiveOps {
        async fn list_agents(&self) -> Result<Vec<abigail_skills::HiveAgentInfo>, String> {
            Ok(vec![])
        }

        async fn load_agent(&self, _agent_id: &str) -> Result<(), String> {
            Ok(())
        }

        async fn create_agent(&self, _name: &str) -> Result<String, String> {
            Ok("entity-1".to_string())
        }

        async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
            Ok(Some("entity-1".to_string()))
        }

        async fn get_config_value(&self, _key: &str) -> Result<serde_json::Value, String> {
            Ok(serde_json::Value::Null)
        }

        async fn set_config_value(
            &self,
            _key: &str,
            _value: serde_json::Value,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String> {
            let mut secrets = self.secrets.lock().map_err(|e| e.to_string())?;
            secrets.insert(key.to_string(), value.to_string());
            Ok(())
        }

        async fn get_skill_secret_names(&self) -> Result<Vec<String>, String> {
            let secrets = self.secrets.lock().map_err(|e| e.to_string())?;
            Ok(secrets.keys().cloned().collect())
        }
    }
}
