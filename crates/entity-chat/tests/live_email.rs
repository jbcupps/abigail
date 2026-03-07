//! End-to-end integration test for the entity-chat email pipeline.
//!
//! Exercises the *exact* same code paths as both Tauri and entity-daemon
//! — no HTTP, no Tauri commands, just the `entity-chat` public API.
//!
//! **Env-gated**: all tests skip unless `ABIGAIL_IMAP_TEST=1`.
//!
//! Required env vars (see plan for details):
//!   ABIGAIL_IMAP_TEST, ABIGAIL_LLM_PROVIDER, ABIGAIL_LLM_API_KEY,
//!   ABIGAIL_IMAP_HOST, ABIGAIL_IMAP_PORT, ABIGAIL_IMAP_USER,
//!   ABIGAIL_IMAP_PASS, ABIGAIL_IMAP_TLS_MODE,
//!   ABIGAIL_SMTP_HOST, ABIGAIL_SMTP_PORT

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use abigail_router::{IdEgoRouter, RoutingMode};
use abigail_skills::manifest::SkillId;
use abigail_skills::skill::{Skill, SkillConfig};
use abigail_skills::{
    HiveManagementSkill, HiveOperations, InstructionRegistry, SkillExecutor, SkillRegistry,
};
use entity_chat::{
    augment_system_prompt, build_contextual_messages, build_tool_definitions, run_tool_use_loop,
};

const IMAP_INIT_TIMEOUT_SECS: u64 = 15;

// ---------------------------------------------------------------------------
// Env helpers
// ---------------------------------------------------------------------------

fn env_or_skip(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        eprintln!("Skipping: {} not set", key);
        String::new()
    })
}

fn is_live_test_enabled() -> bool {
    std::env::var("ABIGAIL_IMAP_TEST").is_ok_and(|v| v == "1")
}

struct TestEnv {
    provider: String,
    api_key: String,
    imap_host: String,
    imap_port: String,
    imap_user: String,
    imap_pass: String,
    imap_tls_mode: String,
    smtp_host: String,
    smtp_port: String,
}

impl TestEnv {
    fn load() -> Option<Self> {
        if !is_live_test_enabled() {
            eprintln!("ABIGAIL_IMAP_TEST not set to 1 — skipping live email tests");
            return None;
        }
        let api_key = env_or_skip("ABIGAIL_LLM_API_KEY");
        if api_key.is_empty() {
            return None;
        }
        Some(Self {
            provider: env_or_skip("ABIGAIL_LLM_PROVIDER"),
            api_key,
            imap_host: env_or_skip("ABIGAIL_IMAP_HOST"),
            imap_port: env_or_skip("ABIGAIL_IMAP_PORT"),
            imap_user: env_or_skip("ABIGAIL_IMAP_USER"),
            imap_pass: env_or_skip("ABIGAIL_IMAP_PASS"),
            imap_tls_mode: env_or_skip("ABIGAIL_IMAP_TLS_MODE"),
            smtp_host: env_or_skip("ABIGAIL_SMTP_HOST"),
            smtp_port: env_or_skip("ABIGAIL_SMTP_PORT"),
        })
    }
}

// ---------------------------------------------------------------------------
// TestHiveOps: in-memory HiveOperations
// ---------------------------------------------------------------------------

struct TestHiveOps {
    secrets: Arc<Mutex<HashMap<String, String>>>,
}

impl TestHiveOps {
    fn new(secrets: Arc<Mutex<HashMap<String, String>>>) -> Self {
        Self { secrets }
    }
}

#[async_trait::async_trait]
impl HiveOperations for TestHiveOps {
    async fn list_agents(&self) -> Result<Vec<abigail_skills::HiveAgentInfo>, String> {
        Ok(vec![abigail_skills::HiveAgentInfo {
            id: "test-entity-001".to_string(),
            name: "Test Entity".to_string(),
        }])
    }

    async fn load_agent(&self, _agent_id: &str) -> Result<(), String> {
        Ok(())
    }

    async fn create_agent(&self, _name: &str) -> Result<String, String> {
        Ok("test-entity-002".to_string())
    }

    async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
        Ok(Some("test-entity-001".to_string()))
    }

    async fn get_config_value(&self, _key: &str) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }

    async fn set_config_value(&self, _key: &str, _value: serde_json::Value) -> Result<(), String> {
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

// ---------------------------------------------------------------------------
// Helpers: build the pipeline components
// ---------------------------------------------------------------------------

fn build_router(env: &TestEnv) -> IdEgoRouter {
    let provider = if env.provider.is_empty() {
        None
    } else {
        Some(env.provider.as_str())
    };
    IdEgoRouter::new(
        None,
        provider,
        Some(env.api_key.clone()),
        None,
        RoutingMode::EgoPrimary,
    )
}

fn build_skill_registry(hive_ops: Arc<dyn HiveOperations>) -> Arc<SkillRegistry> {
    let registry = SkillRegistry::new();
    let hive_skill = HiveManagementSkill::new(hive_ops);
    registry
        .register(
            SkillId("builtin.hive_management".to_string()),
            Arc::new(hive_skill),
        )
        .expect("Failed to register HiveManagementSkill");
    Arc::new(registry)
}

fn build_instruction_registry() -> InstructionRegistry {
    let skills_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("skills");
    let registry_path = skills_dir.join("registry.toml");
    let instructions_dir = skills_dir.join("instructions");
    if registry_path.exists() {
        InstructionRegistry::load(&registry_path, &instructions_dir)
    } else {
        InstructionRegistry::empty()
    }
}

fn build_skill_config_from_vault(secrets: &HashMap<String, String>) -> SkillConfig {
    let mut values = HashMap::new();
    let mut secret_map = HashMap::new();
    for (k, v) in secrets {
        match k.as_str() {
            "imap_host" | "imap_tls_mode" | "smtp_host" => {
                values.insert(k.clone(), serde_json::Value::String(v.clone()));
            }
            "imap_port" | "smtp_port" => {
                if let Ok(n) = v.parse::<u64>() {
                    values.insert(k.clone(), serde_json::json!(n));
                } else {
                    values.insert(k.clone(), serde_json::Value::String(v.clone()));
                }
            }
            _ => {
                secret_map.insert(k.clone(), v.clone());
            }
        }
    }
    SkillConfig {
        values,
        secrets: secret_map,
        limits: Default::default(),
        permissions: vec![],
        stream_broker: None,
    }
}

const BASE_SYSTEM_PROMPT: &str = "\
You are Abigail, a Sovereign Entity assistant. You help the user manage their \
digital life. You have tools available and should use them when appropriate. \
When the user provides credentials or configuration details, store them using \
the store_secret tool from builtin.hive_management.";

// ---------------------------------------------------------------------------
// Turn 1: Credential setup
// ---------------------------------------------------------------------------

#[tokio::test]
#[allow(deprecated)]
async fn turn1_credential_setup() {
    let Some(env) = TestEnv::load() else {
        return;
    };

    let vault: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let hive_ops: Arc<dyn HiveOperations> = Arc::new(TestHiveOps::new(vault.clone()));
    let registry = build_skill_registry(hive_ops);
    let executor = SkillExecutor::new(registry.clone());
    let instruction_reg = build_instruction_registry();
    let router = build_router(&env);

    let user_message = format!(
        "Please set up my email. IMAP: host {host}, port {port}, user {user}, \
         password {pass}, security {tls}. SMTP: host {smtp_host}, port {smtp_port}.",
        host = env.imap_host,
        port = env.imap_port,
        user = env.imap_user,
        pass = env.imap_pass,
        tls = env.imap_tls_mode,
        smtp_host = env.smtp_host,
        smtp_port = env.smtp_port,
    );

    let augmented_prompt = augment_system_prompt(
        BASE_SYSTEM_PROMPT,
        &registry,
        &instruction_reg,
        &user_message,
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = build_contextual_messages(&augmented_prompt, None, &user_message);
    let tools = build_tool_definitions(&registry);

    eprintln!("--- Turn 1: sending credential setup message ---");
    eprintln!(
        "Router: has_ego={}, provider={:?}",
        router.has_ego(),
        router.ego_provider_name()
    );
    eprintln!(
        "Tools available: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    assert!(
        router.has_ego(),
        "Router must have Ego configured — check ABIGAIL_LLM_PROVIDER and ABIGAIL_LLM_API_KEY"
    );

    // Quick sanity check: call the router directly to see if the provider works
    let test_msgs = vec![abigail_capabilities::cognitive::Message::new(
        "user",
        "say hello",
    )];
    match router.route(test_msgs).await {
        Ok(r) => eprintln!(
            "Router sanity check OK: {:?}",
            &r.content[..r.content.len().min(80)]
        ),
        Err(e) => eprintln!("Router sanity check FAILED: {}", e),
    }

    let result = run_tool_use_loop(&router, &executor, messages, tools)
        .await
        .expect("Tool-use loop failed");

    eprintln!("Turn 1 response: {}", result.content);
    eprintln!(
        "Turn 1 tool calls: {:?}",
        result
            .tool_calls_made
            .iter()
            .map(|r| format!("{}::{} (ok={})", r.skill_id, r.tool_name, r.success))
            .collect::<Vec<_>>()
    );

    // Assert: store_secret was called at least once
    let store_calls: Vec<_> = result
        .tool_calls_made
        .iter()
        .filter(|r| r.tool_name == "store_secret")
        .collect();
    assert!(
        !store_calls.is_empty(),
        "LLM should have called store_secret at least once"
    );
    assert!(
        store_calls.iter().all(|r| r.success),
        "All store_secret calls should succeed"
    );

    // Assert: vault now contains the critical keys
    let secrets = vault.lock().unwrap();
    eprintln!("Vault contents: {:?}", secrets.keys().collect::<Vec<_>>());
    assert!(
        secrets.contains_key("imap_user"),
        "Vault should contain imap_user"
    );
    assert!(
        secrets.contains_key("imap_password"),
        "Vault should contain imap_password"
    );
}

// ---------------------------------------------------------------------------
// Turn 2: Fetch emails
// ---------------------------------------------------------------------------

#[tokio::test]
async fn turn2_fetch_emails() {
    let Some(env) = TestEnv::load() else {
        return;
    };

    // Pre-populate the vault as if Turn 1 succeeded
    let mut initial_secrets = HashMap::new();
    initial_secrets.insert("imap_user".to_string(), env.imap_user.clone());
    initial_secrets.insert("imap_password".to_string(), env.imap_pass.clone());
    initial_secrets.insert("imap_host".to_string(), env.imap_host.clone());
    initial_secrets.insert("imap_port".to_string(), env.imap_port.clone());
    initial_secrets.insert("imap_tls_mode".to_string(), env.imap_tls_mode.clone());
    initial_secrets.insert("smtp_host".to_string(), env.smtp_host.clone());
    initial_secrets.insert("smtp_port".to_string(), env.smtp_port.clone());

    let vault: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(initial_secrets.clone()));
    let hive_ops: Arc<dyn HiveOperations> = Arc::new(TestHiveOps::new(vault.clone()));
    let registry = build_skill_registry(hive_ops);

    // Initialize EmailSkill with a timeout (IMAP server may be unavailable)
    let mut email_skill = skill_email::EmailSkill::new(skill_email::EmailSkill::default_manifest());
    let skill_config = build_skill_config_from_vault(&initial_secrets);
    let init_result = tokio::time::timeout(
        std::time::Duration::from_secs(IMAP_INIT_TIMEOUT_SECS),
        email_skill.initialize(skill_config),
    )
    .await;
    match init_result {
        Ok(Ok(())) => eprintln!("EmailSkill initialized successfully"),
        Ok(Err(e)) => {
            eprintln!("Skipping Turn 2 — EmailSkill init failed: {e}");
            return;
        }
        Err(_) => {
            eprintln!(
                "Skipping Turn 2 — IMAP init timed out after {}s (is the mail bridge running?)",
                IMAP_INIT_TIMEOUT_SECS
            );
            return;
        }
    }

    registry
        .register(
            SkillId("com.abigail.skills.email".to_string()),
            Arc::new(email_skill),
        )
        .expect("Failed to register email skill");

    let executor = SkillExecutor::new(registry.clone());
    let instruction_reg = build_instruction_registry();
    let router = build_router(&env);

    let user_message = "Check my email and find any messages from jbcupps";
    let augmented_prompt = augment_system_prompt(
        BASE_SYSTEM_PROMPT,
        &registry,
        &instruction_reg,
        user_message,
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = build_contextual_messages(&augmented_prompt, None, user_message);
    let tools = build_tool_definitions(&registry);

    eprintln!("--- Turn 2: fetch emails ---");
    eprintln!(
        "Tools available: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let result = run_tool_use_loop(&router, &executor, messages, tools)
        .await
        .expect("Tool-use loop failed");

    eprintln!("Turn 2 response: {}", result.content);
    eprintln!(
        "Turn 2 tool calls: {:?}",
        result
            .tool_calls_made
            .iter()
            .map(|r| format!("{}::{} (ok={})", r.skill_id, r.tool_name, r.success))
            .collect::<Vec<_>>()
    );

    // Assert: fetch_emails was called
    let fetch_calls: Vec<_> = result
        .tool_calls_made
        .iter()
        .filter(|r| r.tool_name == "fetch_emails")
        .collect();
    assert!(
        !fetch_calls.is_empty(),
        "LLM should have called fetch_emails"
    );

    // Assert: LLM response mentions jbcupps (found an email from that sender)
    let content_lower = result.content.to_lowercase();
    assert!(
        content_lower.contains("jbcupps"),
        "Response should mention 'jbcupps' — got: {}",
        result.content
    );
}

// ---------------------------------------------------------------------------
// Turn 3: Send email
// ---------------------------------------------------------------------------

#[tokio::test]
async fn turn3_send_email() {
    let Some(env) = TestEnv::load() else {
        return;
    };

    // Pre-populate the vault
    let mut initial_secrets = HashMap::new();
    initial_secrets.insert("imap_user".to_string(), env.imap_user.clone());
    initial_secrets.insert("imap_password".to_string(), env.imap_pass.clone());
    initial_secrets.insert("imap_host".to_string(), env.imap_host.clone());
    initial_secrets.insert("imap_port".to_string(), env.imap_port.clone());
    initial_secrets.insert("imap_tls_mode".to_string(), env.imap_tls_mode.clone());
    initial_secrets.insert("smtp_host".to_string(), env.smtp_host.clone());
    initial_secrets.insert("smtp_port".to_string(), env.smtp_port.clone());

    let vault: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(initial_secrets.clone()));
    let hive_ops: Arc<dyn HiveOperations> = Arc::new(TestHiveOps::new(vault.clone()));
    let registry = build_skill_registry(hive_ops);

    // Initialize EmailSkill with a timeout
    let mut email_skill = skill_email::EmailSkill::new(skill_email::EmailSkill::default_manifest());
    let skill_config = build_skill_config_from_vault(&initial_secrets);
    let init_result = tokio::time::timeout(
        std::time::Duration::from_secs(IMAP_INIT_TIMEOUT_SECS),
        email_skill.initialize(skill_config),
    )
    .await;
    match init_result {
        Ok(Ok(())) => eprintln!("EmailSkill initialized successfully"),
        Ok(Err(e)) => {
            eprintln!("Skipping Turn 3 — EmailSkill init failed: {e}");
            return;
        }
        Err(_) => {
            eprintln!(
                "Skipping Turn 3 — IMAP init timed out after {}s (is the mail bridge running?)",
                IMAP_INIT_TIMEOUT_SECS
            );
            return;
        }
    }

    registry
        .register(
            SkillId("com.abigail.skills.email".to_string()),
            Arc::new(email_skill),
        )
        .expect("Failed to register email skill");

    let executor = SkillExecutor::new(registry.clone());
    let instruction_reg = build_instruction_registry();
    let router = build_router(&env);

    let user_message =
        "Send a test email to jbcupps@gmail.com with subject 'Abigail integration test' \
         and body 'This email was sent by the Abigail entity-chat integration test.'";
    let augmented_prompt = augment_system_prompt(
        BASE_SYSTEM_PROMPT,
        &registry,
        &instruction_reg,
        user_message,
        &entity_chat::RuntimeContext::default(),
        entity_chat::PromptMode::Full,
    );
    let messages = build_contextual_messages(&augmented_prompt, None, user_message);
    let tools = build_tool_definitions(&registry);

    eprintln!("--- Turn 3: send email ---");
    eprintln!(
        "Tools available: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let result = run_tool_use_loop(&router, &executor, messages, tools)
        .await
        .expect("Tool-use loop failed");

    eprintln!("Turn 3 response: {}", result.content);
    eprintln!(
        "Turn 3 tool calls: {:?}",
        result
            .tool_calls_made
            .iter()
            .map(|r| format!("{}::{} (ok={})", r.skill_id, r.tool_name, r.success))
            .collect::<Vec<_>>()
    );

    // Assert: send_email was called
    let send_calls: Vec<_> = result
        .tool_calls_made
        .iter()
        .filter(|r| r.tool_name == "send_email")
        .collect();
    assert!(!send_calls.is_empty(), "LLM should have called send_email");
    assert!(
        send_calls.iter().any(|r| r.success),
        "At least one send_email call should succeed"
    );
}
