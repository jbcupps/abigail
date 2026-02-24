//! Integration test for PROOF-003: CLI scaffold -> skill discovery round-trip.
//!
//! Validates that `scaffold_skill` (dynamic type) produces a JSON file that
//! `DynamicApiSkill::discover` can load and register, completing the
//! scaffold-to-chat proof chain.

use abigail_skills::Skill;
use std::path::Path;

fn to_title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn chrono_now_stub() -> String {
    "2026-01-01T00:00:00Z".to_string()
}

fn scaffold_dynamic_in(dir: &Path, name: &str) -> anyhow::Result<()> {
    let skill_dir = dir.join(format!("skill-{}", name));
    std::fs::create_dir_all(&skill_dir)?;

    let skill_id = format!("custom.{}", name.replace('-', "_"));

    let toml_content = format!(
        r#"[skill]
id = "{skill_id}"
name = "{display_name}"
version = "0.1.0"
description = "TODO: describe what this skill does"
category = "General"

[runtime]
runtime = "DynamicApi"

[[permissions]]
permission = {{ Network = {{ Domains = ["api.example.com"] }} }}
reason = "API access"
optional = false
"#,
        skill_id = skill_id,
        display_name = to_title_case(name),
    );
    std::fs::write(skill_dir.join("skill.toml"), toml_content)?;

    let json_content = serde_json::to_string_pretty(&serde_json::json!({
        "id": skill_id,
        "name": to_title_case(name),
        "description": "TODO: describe what this skill does",
        "version": "0.1.0",
        "category": "General",
        "created_at": chrono_now_stub(),
        "tools": [
            {
                "name": "example_tool",
                "description": "TODO: describe this tool",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The input query"
                        }
                    },
                    "required": ["query"]
                },
                "method": "GET",
                "url_template": "https://api.example.com/v1/search?q={{query}}",
                "headers": {},
                "body_template": null,
                "response_extract": {
                    "result": "data.result"
                },
                "response_format": "Result: {{result}}"
            }
        ]
    }))?;
    std::fs::write(skill_dir.join(format!("{}.json", skill_id)), json_content)?;

    Ok(())
}

#[test]
fn scaffold_dynamic_produces_discoverable_json() {
    let tmp = std::env::temp_dir().join("abigail_cli_scaffold_disco");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    scaffold_dynamic_in(&tmp, "weather-api").unwrap();

    let skill_dir = tmp.join("skill-weather-api");
    assert!(skill_dir.join("skill.toml").exists());
    assert!(skill_dir.join("custom.weather_api.json").exists());

    let json_path = skill_dir.join("custom.weather_api.json");
    let content = std::fs::read_to_string(&json_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["id"], "custom.weather_api");
    assert_eq!(parsed["name"], "Weather Api");
    assert!(parsed["tools"].is_array());
    assert_eq!(parsed["tools"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["tools"][0]["name"], "example_tool");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn scaffold_then_discover_round_trip() {
    let tmp = std::env::temp_dir().join("abigail_cli_scaffold_roundtrip");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    scaffold_dynamic_in(&tmp, "test-tool").unwrap();

    let skill_dir = tmp.join("skill-test-tool");
    let json_path = skill_dir.join("custom.test_tool.json");
    assert!(json_path.exists());

    let flat_dir = tmp.join("entity_skills");
    std::fs::create_dir_all(&flat_dir).unwrap();
    std::fs::copy(&json_path, flat_dir.join("custom.test_tool.json")).unwrap();

    let skills = abigail_skills::DynamicApiSkill::discover(&flat_dir, None);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].manifest().id.0, "custom.test_tool");
    assert_eq!(skills[0].manifest().name, "Test Tool");

    let tools = skills[0].tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "example_tool");
    assert_eq!(tools[0].parameters["type"], "object");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn scaffold_then_register_and_build_tool_defs() {
    let tmp = std::env::temp_dir().join("abigail_cli_scaffold_defs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    scaffold_dynamic_in(&tmp, "my-api").unwrap();

    let skill_dir = tmp.join("skill-my-api");
    let json_path = skill_dir.join("custom.my_api.json");

    let entity_skills = tmp.join("entity_skills");
    std::fs::create_dir_all(&entity_skills).unwrap();
    std::fs::copy(&json_path, entity_skills.join("custom.my_api.json")).unwrap();

    let registry = abigail_skills::SkillRegistry::new();
    for skill in abigail_skills::DynamicApiSkill::discover(&entity_skills, None) {
        let id = skill.manifest().id.clone();
        registry.register(id, std::sync::Arc::new(skill)).unwrap();
    }

    let defs = entity_chat::build_tool_definitions(&registry);
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "custom.my_api::example_tool");

    let _ = std::fs::remove_dir_all(&tmp);
}
