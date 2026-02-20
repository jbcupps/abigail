//! Preloaded integration skills — embedded DynamicApiSkill configs for
//! common third-party services (GitHub, Slack, Jira).
//!
//! These are compiled into the binary via `include_str!` (like constitutional
//! documents) and bootstrapped on first run or when the embedded version
//! exceeds the stored `preloaded_skills_version` in AppConfig.

use std::sync::{Arc, Mutex};

use abigail_auth::AuthMethod;
use abigail_core::SecretsVault;

use crate::dynamic::{extract_secret_keys, DynamicApiSkill, DynamicSkillConfig};

/// Current version of the preloaded skill definitions. Bump this whenever
/// the embedded JSON configs change so that existing installations re-bootstrap.
pub const PRELOADED_SKILLS_VERSION: u32 = 1;

// Embedded JSON configs — compiled into the binary.
const GITHUB_API_JSON: &str = include_str!("preloaded/github_api.json");
const SLACK_JSON: &str = include_str!("preloaded/slack.json");
const JIRA_JSON: &str = include_str!("preloaded/jira.json");

/// Auth metadata for a preloaded integration skill.
#[derive(Debug, Clone)]
pub struct PreloadedSkillAuth {
    /// Service identifier (matches the skill ID suffix, e.g. "github_api").
    pub service_id: String,
    /// How this service authenticates.
    pub auth_method: AuthMethod,
    /// Human-readable setup instructions for the user.
    pub setup_instructions: String,
    /// URL where the user can create credentials.
    pub setup_url: String,
}

/// Parse all embedded integration skill configs and pair them with auth metadata.
///
/// Returns `(DynamicSkillConfig, PreloadedSkillAuth)` for each integration.
/// Panics at startup if any embedded JSON is malformed (compile-time guarantee).
pub fn preloaded_integration_skills() -> Vec<(DynamicSkillConfig, PreloadedSkillAuth)> {
    vec![
        (
            serde_json::from_str::<DynamicSkillConfig>(GITHUB_API_JSON)
                .expect("embedded github_api.json is valid"),
            PreloadedSkillAuth {
                service_id: "github_api".to_string(),
                auth_method: AuthMethod::StaticToken {
                    secret_key: "github_token".to_string(),
                },
                setup_instructions: "Create a GitHub Personal Access Token (classic) with 'repo' scope.".to_string(),
                setup_url: "https://github.com/settings/tokens".to_string(),
            },
        ),
        (
            serde_json::from_str::<DynamicSkillConfig>(SLACK_JSON)
                .expect("embedded slack.json is valid"),
            PreloadedSkillAuth {
                service_id: "slack".to_string(),
                auth_method: AuthMethod::StaticToken {
                    secret_key: "slack_bot_token".to_string(),
                },
                setup_instructions: "Create a Slack App and install it to your workspace. Copy the Bot User OAuth Token (xoxb-...).".to_string(),
                setup_url: "https://api.slack.com/apps".to_string(),
            },
        ),
        (
            serde_json::from_str::<DynamicSkillConfig>(JIRA_JSON)
                .expect("embedded jira.json is valid"),
            PreloadedSkillAuth {
                service_id: "jira".to_string(),
                auth_method: AuthMethod::BasicAuth {
                    username_key: "jira_basic_auth".to_string(),
                    password_key: "jira_basic_auth".to_string(),
                },
                setup_instructions: "Create an Atlassian API token. Store your email as jira_email, API token as jira_api_token, and your Jira domain (e.g. mycompany.atlassian.net) as jira_domain. The jira_basic_auth value will be computed automatically.".to_string(),
                setup_url: "https://id.atlassian.com/manage-profile/security/api-tokens".to_string(),
            },
        ),
    ]
}

/// Build `DynamicApiSkill` instances from the embedded configs.
///
/// This is a convenience wrapper used during bootstrap. Skills that fail
/// validation are logged and skipped (should never happen with embedded configs).
pub fn build_preloaded_skills(secrets: Option<Arc<Mutex<SecretsVault>>>) -> Vec<DynamicApiSkill> {
    preloaded_integration_skills()
        .into_iter()
        .filter_map(|(config, _auth)| {
            match DynamicApiSkill::from_config(config.clone(), secrets.clone()) {
                Ok(skill) => Some(skill),
                Err(e) => {
                    tracing::error!("Failed to build preloaded skill '{}': {}", config.id, e);
                    None
                }
            }
        })
        .collect()
}

/// Get the secret keys required by all preloaded integration skills.
pub fn preloaded_secret_keys() -> Vec<String> {
    let mut all_keys = Vec::new();
    for (config, _auth) in preloaded_integration_skills() {
        all_keys.extend(extract_secret_keys(&config));
    }
    all_keys.sort();
    all_keys.dedup();
    all_keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill::Skill;

    #[test]
    fn test_all_embedded_jsons_parse() {
        let skills = preloaded_integration_skills();
        assert_eq!(skills.len(), 3, "Expected 3 preloaded integration skills");
    }

    #[test]
    fn test_github_api_config() {
        let skills = preloaded_integration_skills();
        let (config, auth) = &skills[0];

        assert_eq!(config.id, "dynamic.github_api");
        assert_eq!(config.name, "GitHub API");
        assert_eq!(config.tools.len(), 3);
        assert_eq!(config.tools[0].name, "github_list_repos");
        assert_eq!(config.tools[1].name, "github_list_issues");
        assert_eq!(config.tools[2].name, "github_create_issue");

        assert_eq!(auth.service_id, "github_api");
        assert!(matches!(auth.auth_method, AuthMethod::StaticToken { .. }));
    }

    #[test]
    fn test_slack_config() {
        let skills = preloaded_integration_skills();
        let (config, auth) = &skills[1];

        assert_eq!(config.id, "dynamic.slack");
        assert_eq!(config.tools.len(), 2);
        assert_eq!(config.tools[0].name, "slack_send_message");
        assert_eq!(config.tools[1].name, "slack_list_channels");

        assert_eq!(auth.service_id, "slack");
        assert!(matches!(auth.auth_method, AuthMethod::StaticToken { .. }));
    }

    #[test]
    fn test_jira_config() {
        let skills = preloaded_integration_skills();
        let (config, auth) = &skills[2];

        assert_eq!(config.id, "dynamic.jira");
        assert_eq!(config.tools.len(), 2);
        assert_eq!(config.tools[0].name, "jira_search_issues");
        assert_eq!(config.tools[1].name, "jira_create_issue");

        assert_eq!(auth.service_id, "jira");
        assert!(matches!(auth.auth_method, AuthMethod::BasicAuth { .. }));
    }

    #[test]
    fn test_all_validate_as_dynamic_api_skill() {
        let skills = build_preloaded_skills(None);
        assert_eq!(skills.len(), 3, "All 3 preloaded skills should validate");

        let ids: Vec<&str> = skills.iter().map(|s| s.manifest().id.0.as_str()).collect();
        assert!(ids.contains(&"dynamic.github_api"));
        assert!(ids.contains(&"dynamic.slack"));
        assert!(ids.contains(&"dynamic.jira"));
    }

    #[test]
    fn test_extracted_secret_keys() {
        let skills = preloaded_integration_skills();

        let github_keys = extract_secret_keys(&skills[0].0);
        assert!(github_keys.contains(&"github_token".to_string()));

        let slack_keys = extract_secret_keys(&skills[1].0);
        assert!(slack_keys.contains(&"slack_bot_token".to_string()));

        let jira_keys = extract_secret_keys(&skills[2].0);
        assert!(jira_keys.contains(&"jira_basic_auth".to_string()));
        assert!(jira_keys.contains(&"jira_domain".to_string()));
    }

    #[test]
    fn test_preloaded_secret_keys_aggregated() {
        let keys = preloaded_secret_keys();
        assert!(keys.contains(&"github_token".to_string()));
        assert!(keys.contains(&"slack_bot_token".to_string()));
        assert!(keys.contains(&"jira_basic_auth".to_string()));
        assert!(keys.contains(&"jira_domain".to_string()));
    }

    #[test]
    fn test_expected_tool_count() {
        let skills = preloaded_integration_skills();
        let total_tools: usize = skills.iter().map(|(c, _)| c.tools.len()).sum();
        assert_eq!(total_tools, 7, "GitHub(3) + Slack(2) + Jira(2) = 7 tools");
    }
}
