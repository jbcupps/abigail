//! Integration tests for entity-daemon skill discovery, registration, and
//! execution through the shared `entity-chat` engine.
//!
//! These tests validate PROOF-002 from the Skills Chat Proof checklist:
//! - DynamicApiSkill discovery from JSON files
//! - Built-in and factory skills registered
//! - build_tool_definitions produces correct qualified names
//! - SkillExecutor handles success and failure paths

use abigail_skills::manifest::SkillId;
use abigail_skills::skill::ToolParams;
use abigail_skills::{DynamicApiSkill, Skill, SkillExecutor, SkillFactory, SkillRegistry};
use std::sync::Arc;

fn sample_dynamic_config() -> serde_json::Value {
    serde_json::json!({
        "id": "dynamic.test_api",
        "name": "Test API",
        "description": "A test dynamic API skill",
        "version": "0.1.0",
        "category": "Test",
        "created_at": "2026-01-01T00:00:00Z",
        "tools": [{
            "name": "get_data",
            "description": "Fetch test data",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            },
            "method": "GET",
            "url_template": "https://httpbin.org/get?q={{query}}",
            "headers": {},
            "body_template": null,
            "response_extract": { "result": "args.q" },
            "response_format": "Result: {{result}}"
        }]
    })
}

#[test]
fn dynamic_skill_discovery_from_directory() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_discover");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config = sample_dynamic_config();
    let json = serde_json::to_string_pretty(&config).unwrap();
    std::fs::write(tmp.join("dynamic.test_api.json"), &json).unwrap();

    let skills = DynamicApiSkill::discover(&tmp, None);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].manifest().id.0, "dynamic.test_api");
    assert_eq!(skills[0].manifest().name, "Test API");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn discovered_skill_registered_and_listed() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_register");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config = sample_dynamic_config();
    std::fs::write(
        tmp.join("dynamic.test_api.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    let registry = SkillRegistry::new();
    let skills = DynamicApiSkill::discover(&tmp, None);
    for skill in skills {
        let skill_id = skill.manifest().id.clone();
        registry.register(skill_id, Arc::new(skill)).unwrap();
    }

    let listed = registry.list().unwrap();
    assert!(
        listed.iter().any(|m| m.id.0 == "dynamic.test_api"),
        "discovered skill should appear in registry list"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_tool_definitions_includes_discovered_skills() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_defs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let config = sample_dynamic_config();
    std::fs::write(
        tmp.join("dynamic.test_api.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();

    let registry = SkillRegistry::new();
    let skills = DynamicApiSkill::discover(&tmp, None);
    for skill in skills {
        let skill_id = skill.manifest().id.clone();
        registry.register(skill_id, Arc::new(skill)).unwrap();
    }

    let defs = entity_chat::build_tool_definitions(&registry);
    assert!(
        !defs.is_empty(),
        "should produce at least one tool definition"
    );
    assert!(
        defs.iter().any(|d| d.name == "dynamic.test_api::get_data"),
        "qualified tool name should be skill_id::tool_name, got: {:?}",
        defs.iter().map(|d| &d.name).collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn skill_factory_registers_and_lists_tools() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_factory");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let registry = SkillRegistry::new();
    let factory = SkillFactory::new(tmp.clone());
    registry
        .register(
            SkillId("builtin.skill_factory".to_string()),
            Arc::new(factory),
        )
        .unwrap();

    let defs = entity_chat::build_tool_definitions(&registry);
    let factory_tools: Vec<&str> = defs
        .iter()
        .filter(|d| d.name.starts_with("builtin.skill_factory::"))
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        factory_tools.contains(&"builtin.skill_factory::author_skill"),
        "factory should expose author_skill, got: {:?}",
        factory_tools
    );
    assert!(
        factory_tools.contains(&"builtin.skill_factory::delete_skill"),
        "factory should expose delete_skill, got: {:?}",
        factory_tools
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn skill_factory_author_creates_files() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_author");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let registry = Arc::new(SkillRegistry::new());
    let factory = SkillFactory::new(tmp.clone());
    registry
        .register(
            SkillId("builtin.skill_factory".to_string()),
            Arc::new(factory),
        )
        .unwrap();
    let executor = SkillExecutor::new(registry);

    let params = ToolParams::new()
        .with("id", "custom.greeter")
        .with("name", "Greeter")
        .with("description", "Says hello")
        .with("script_content", "print('hello')")
        .with("script_filename", "main.py")
        .with("how_to_use_md", "# Greeter\nJust say hello.");

    let result = executor
        .execute(
            &SkillId("builtin.skill_factory".to_string()),
            "author_skill",
            params,
        )
        .await
        .unwrap();
    assert!(result.success);

    let skill_dir = tmp.join("custom.greeter");
    assert!(
        skill_dir.join("skill.toml").exists(),
        "skill.toml should be created"
    );
    assert!(
        skill_dir.join("main.py").exists(),
        "script should be created"
    );
    assert!(
        skill_dir.join("how-to-use.md").exists(),
        "how-to-use.md should be created"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test]
async fn executor_returns_error_for_missing_tool() {
    let registry = Arc::new(SkillRegistry::new());
    let factory = SkillFactory::new(std::env::temp_dir());
    registry
        .register(
            SkillId("builtin.skill_factory".to_string()),
            Arc::new(factory),
        )
        .unwrap();
    let executor = SkillExecutor::new(registry);

    let result = executor
        .execute(
            &SkillId("builtin.skill_factory".to_string()),
            "nonexistent_tool",
            ToolParams::new(),
        )
        .await;
    assert!(result.is_err(), "should fail for unknown tool");
    assert!(
        result.unwrap_err().to_string().contains("Unknown tool"),
        "error should mention unknown tool"
    );
}

#[tokio::test]
async fn executor_returns_error_for_missing_skill() {
    let registry = Arc::new(SkillRegistry::new());
    let executor = SkillExecutor::new(registry);

    let result = executor
        .execute(
            &SkillId("nonexistent.skill".to_string()),
            "some_tool",
            ToolParams::new(),
        )
        .await;
    assert!(result.is_err(), "should fail for missing skill");
}

#[test]
fn empty_skills_dir_yields_no_dynamic_skills() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_empty");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let skills = DynamicApiSkill::discover(&tmp, None);
    assert!(skills.is_empty(), "empty dir should yield no skills");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn invalid_json_skipped_during_discovery() {
    let tmp = std::env::temp_dir().join("abigail_daemon_integ_invalid");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    std::fs::write(tmp.join("broken.json"), "{ not valid json }").unwrap();
    let valid_config = sample_dynamic_config();
    std::fs::write(
        tmp.join("dynamic.test_api.json"),
        serde_json::to_string_pretty(&valid_config).unwrap(),
    )
    .unwrap();

    let skills = DynamicApiSkill::discover(&tmp, None);
    assert_eq!(skills.len(), 1, "should skip invalid JSON, load valid one");
    assert_eq!(skills[0].manifest().id.0, "dynamic.test_api");

    let _ = std::fs::remove_dir_all(&tmp);
}
