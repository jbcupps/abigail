//! Live E2E probe — validates the desktop runtime skill/secrets/instruction
//! pipeline against a real (isolated) data directory and optional live IMAP
//! bridge.  Activated by setting `ABIGAIL_E2E_PROBE=1`.
//!
//! The probe reuses the *exact same* wiring code that `lib::run()` uses so
//! regressions in startup registration, namespace validation, or instruction
//! bootstrap are caught before the installer ever ships.

use abigail_core::SecretsVault;
use abigail_skills::manifest::SkillId;
use abigail_skills::{
    InstructionRegistry, ResourceLimits, Skill, SkillConfig, SkillExecutor, SkillRegistry,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct ProbeResult {
    passed: Vec<String>,
    failed: Vec<String>,
}

impl ProbeResult {
    fn new() -> Self {
        Self {
            passed: Vec::new(),
            failed: Vec::new(),
        }
    }
    fn pass(&mut self, name: &str) {
        eprintln!("  [PASS] {}", name);
        self.passed.push(name.to_string());
    }
    fn fail(&mut self, name: &str, reason: &str) {
        eprintln!("  [FAIL] {} — {}", name, reason);
        self.failed.push(format!("{}: {}", name, reason));
    }
    fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Returns `true` if the probe env var is set and the probe should run
/// instead of the normal GUI.
pub fn should_run() -> bool {
    std::env::var("ABIGAIL_E2E_PROBE").map_or(false, |v| v == "1")
}

/// Run the probe and call `std::process::exit` with the result.
pub fn run_and_exit() -> ! {
    eprintln!("=== Abigail E2E Probe ===\n");

    let mut result = ProbeResult::new();

    let tmp = std::env::temp_dir().join("abigail_e2e_probe");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("Failed to create probe temp dir");

    // 1. Bootstrap instruction files
    crate::skill_instructions::bootstrap_if_needed(&tmp);
    let registry_path = tmp.join("skills").join("registry.toml");
    if registry_path.exists() {
        result.pass("instruction_bootstrap");
    } else {
        result.fail("instruction_bootstrap", "registry.toml not created");
    }

    // 2. Load instruction registry and check email keyword match
    let instr_reg = {
        let skills_dir = tmp.join("skills");
        let reg_path = skills_dir.join("registry.toml");
        let instr_dir = skills_dir.join("instructions");
        if reg_path.exists() {
            InstructionRegistry::load(&reg_path, &instr_dir)
        } else {
            InstructionRegistry::empty()
        }
    };
    let email_matches = instr_reg.select_instructions("connect my email inbox");
    if !email_matches.is_empty() {
        result.pass("instruction_email_keyword");
    } else {
        result.fail(
            "instruction_email_keyword",
            "no instruction matched 'email' keyword after bootstrap",
        );
    }

    // 3. Build skill registry with EmailSkill registered
    let vault = Arc::new(Mutex::new(SecretsVault::new_custom(
        tmp.clone(),
        "skills.bin",
    )));
    let registry = Arc::new(SkillRegistry::with_secrets(vault.clone()));

    let hive_skill = abigail_skills::HiveManagementSkill::new(Arc::new(InMemoryHiveOps::default()));
    let _ = registry.register(
        SkillId("builtin.hive_management".to_string()),
        Arc::new(hive_skill),
    );

    let email_manifest = skill_email::EmailSkill::default_manifest();
    let email_id = email_manifest.id.clone();
    let email_skill = skill_email::EmailSkill::new(email_manifest);
    let _ = registry.register(email_id.clone(), Arc::new(email_skill));

    let skills = registry.list().unwrap_or_default();
    if skills.iter().any(|m| m.id == email_id) {
        result.pass("email_skill_registered");
    } else {
        result.fail("email_skill_registered", "EmailSkill not in registry");
    }

    // 4. Secret namespace validation
    let data_dir_for_validation = tmp.clone();
    let check = |key: &str| {
        crate::commands::skills::validate_secret_namespace_with(
            &registry,
            &data_dir_for_validation,
            key,
        )
    };

    if check("imap_password").is_ok() {
        result.pass("namespace_imap_password");
    } else {
        result.fail("namespace_imap_password", "imap_password rejected");
    }
    if check("imap_user").is_ok() {
        result.pass("namespace_imap_user");
    } else {
        result.fail("namespace_imap_user", "imap_user rejected");
    }
    if check("smtp_host").is_ok() {
        result.pass("namespace_smtp_host");
    } else {
        result.fail("namespace_smtp_host", "smtp_host rejected");
    }
    if check("openai").is_ok() {
        result.pass("namespace_reserved_openai");
    } else {
        result.fail("namespace_reserved_openai", "openai rejected");
    }
    if check("totally_bogus_key_xyz").is_err() {
        result.pass("namespace_rejects_unknown");
    } else {
        result.fail("namespace_rejects_unknown", "unknown key was accepted");
    }

    // 5. Tool-use round-trip: store_secret via executor
    let executor = SkillExecutor::new(registry.clone());
    let store_result = tokio_block(async {
        let mut params = abigail_skills::skill::ToolParams::new();
        params
            .values
            .insert("key".into(), serde_json::json!("imap_password"));
        params
            .values
            .insert("value".into(), serde_json::json!("probe_test_pw"));
        executor
            .execute(
                &SkillId("builtin.hive_management".to_string()),
                "store_secret",
                params,
            )
            .await
    });
    match store_result {
        Ok(output) if output.success => result.pass("store_secret_roundtrip"),
        Ok(output) => result.fail(
            "store_secret_roundtrip",
            &format!("returned success=false: {:?}", output.error),
        ),
        Err(e) => result.fail("store_secret_roundtrip", &e.to_string()),
    }

    // 6. Optional live IMAP bridge check (only when env vars are set)
    if std::env::var("ABIGAIL_IMAP_HOST").is_ok() {
        probe_live_imap(&mut result, &vault);
    } else {
        eprintln!("  [SKIP] live_imap (ABIGAIL_IMAP_HOST not set)");
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);

    // Summary
    eprintln!(
        "\n=== Probe complete: {} passed, {} failed ===",
        result.passed.len(),
        result.failed.len()
    );
    if result.ok() {
        eprintln!("RESULT: PASS");
        std::process::exit(0);
    } else {
        for f in &result.failed {
            eprintln!("  FAILURE: {}", f);
        }
        eprintln!("RESULT: FAIL");
        std::process::exit(1);
    }
}

fn probe_live_imap(result: &mut ProbeResult, vault: &Arc<Mutex<SecretsVault>>) {
    let host = std::env::var("ABIGAIL_IMAP_HOST").unwrap_or_default();
    let port = std::env::var("ABIGAIL_IMAP_PORT").unwrap_or_else(|_| "993".into());
    let user = std::env::var("ABIGAIL_IMAP_USER").unwrap_or_default();
    let pass = std::env::var("ABIGAIL_IMAP_PASS").unwrap_or_default();
    let tls = std::env::var("ABIGAIL_IMAP_TLS_MODE").unwrap_or_else(|_| "IMPLICIT".into());

    if host.is_empty() || user.is_empty() || pass.is_empty() {
        eprintln!("  [SKIP] live_imap (incomplete IMAP env vars)");
        return;
    }

    // Populate vault so EmailSkill can init
    {
        let mut v = vault.lock().unwrap();
        v.set_secret("imap_host", &host);
        v.set_secret("imap_port", &port);
        v.set_secret("imap_user", &user);
        v.set_secret("imap_password", &pass);
        v.set_secret("imap_tls_mode", &tls);
    }

    let mut skill = skill_email::EmailSkill::new(skill_email::EmailSkill::default_manifest());

    let mut values = HashMap::new();
    values.insert("imap_host".to_string(), serde_json::Value::String(host));
    values.insert(
        "imap_port".to_string(),
        serde_json::json!(port.parse::<u64>().unwrap_or(993)),
    );
    values.insert("imap_user".to_string(), serde_json::Value::String(user));
    values.insert("imap_tls_mode".to_string(), serde_json::Value::String(tls));

    let mut secrets = HashMap::new();
    secrets.insert("imap_password".to_string(), pass);

    let config = SkillConfig {
        values,
        secrets,
        limits: ResourceLimits::default(),
        permissions: vec![],
        stream_broker: None,
    };

    match tokio_block(async {
        tokio::time::timeout(std::time::Duration::from_secs(15), skill.initialize(config)).await
    }) {
        Ok(Ok(())) => result.pass("live_imap_init"),
        Ok(Err(e)) => result.fail("live_imap_init", &e.to_string()),
        Err(_) => result.fail("live_imap_init", "timed out after 15s"),
    }
}

fn tokio_block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(f)
}

// Minimal in-memory HiveOperations for probe (mirrors live_email.rs TestHiveOps)
#[derive(Default)]
struct InMemoryHiveOps {
    secrets: Mutex<HashMap<String, String>>,
}

#[async_trait::async_trait]
impl abigail_skills::HiveOperations for InMemoryHiveOps {
    async fn list_agents(&self) -> Result<Vec<abigail_skills::HiveAgentInfo>, String> {
        Ok(vec![])
    }
    async fn load_agent(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn create_agent(&self, _: &str) -> Result<String, String> {
        Ok("probe-entity".into())
    }
    async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn get_config_value(&self, _: &str) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }
    async fn set_config_value(&self, _: &str, _: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String> {
        let mut s = self.secrets.lock().map_err(|e| e.to_string())?;
        s.insert(key.to_string(), value.to_string());
        Ok(())
    }
    async fn get_skill_secret_names(&self) -> Result<Vec<String>, String> {
        let s = self.secrets.lock().map_err(|e| e.to_string())?;
        Ok(s.keys().cloned().collect())
    }
}
