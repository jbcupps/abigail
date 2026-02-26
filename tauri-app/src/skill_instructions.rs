//! Embedded skill instruction files seeded into `data_dir/skills/` on first run.
//!
//! Mirrors the constitutional-document bootstrap pattern: files are compiled
//! into the binary so packaged builds always have instructions available,
//! even before the user copies anything to disk.

use std::path::Path;

const REGISTRY_TOML: &str = include_str!("../../skills/registry.toml");

const INSTRUCTIONS: &[(&str, &str)] = &[
    (
        "skill_email_imap.md",
        include_str!("../../skills/instructions/skill_email_imap.md"),
    ),
    (
        "skill_web_search.md",
        include_str!("../../skills/instructions/skill_web_search.md"),
    ),
    (
        "skill_perplexity_search.md",
        include_str!("../../skills/instructions/skill_perplexity_search.md"),
    ),
    (
        "skill_filesystem.md",
        include_str!("../../skills/instructions/skill_filesystem.md"),
    ),
    (
        "skill_shell.md",
        include_str!("../../skills/instructions/skill_shell.md"),
    ),
    (
        "skill_http.md",
        include_str!("../../skills/instructions/skill_http.md"),
    ),
    (
        "skill_browser.md",
        include_str!("../../skills/instructions/skill_browser.md"),
    ),
    (
        "skill_git.md",
        include_str!("../../skills/instructions/skill_git.md"),
    ),
    (
        "skill_calendar.md",
        include_str!("../../skills/instructions/skill_calendar.md"),
    ),
    (
        "skill_knowledge_base.md",
        include_str!("../../skills/instructions/skill_knowledge_base.md"),
    ),
    (
        "skill_code_analysis.md",
        include_str!("../../skills/instructions/skill_code_analysis.md"),
    ),
    (
        "skill_image.md",
        include_str!("../../skills/instructions/skill_image.md"),
    ),
    (
        "skill_clipboard.md",
        include_str!("../../skills/instructions/skill_clipboard.md"),
    ),
    (
        "skill_system_monitor.md",
        include_str!("../../skills/instructions/skill_system_monitor.md"),
    ),
    (
        "skill_database.md",
        include_str!("../../skills/instructions/skill_database.md"),
    ),
    (
        "skill_document.md",
        include_str!("../../skills/instructions/skill_document.md"),
    ),
    (
        "skill_notification.md",
        include_str!("../../skills/instructions/skill_notification.md"),
    ),
    (
        "skill_troubleshooting.md",
        include_str!("../../skills/instructions/skill_troubleshooting.md"),
    ),
    (
        "skill_github_api.md",
        include_str!("../../skills/instructions/skill_github_api.md"),
    ),
    (
        "skill_slack.md",
        include_str!("../../skills/instructions/skill_slack.md"),
    ),
    (
        "skill_jira.md",
        include_str!("../../skills/instructions/skill_jira.md"),
    ),
];

/// Number of embedded instruction files (for testing/diagnostics).
pub const INSTRUCTION_COUNT: usize = INSTRUCTIONS.len();

/// Seed `registry.toml` and `instructions/*.md` into `data_dir/skills/`
/// when they are absent.  Existing files are never overwritten so user
/// customisations are preserved.
pub fn bootstrap_if_needed(data_dir: &Path) {
    let skills_dir = data_dir.join("skills");
    let registry_path = skills_dir.join("registry.toml");

    if registry_path.exists() {
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&skills_dir) {
        tracing::warn!("Failed to create skills dir {:?}: {}", skills_dir, e);
        return;
    }

    if let Err(e) = std::fs::write(&registry_path, REGISTRY_TOML) {
        tracing::warn!("Failed to write registry.toml: {}", e);
        return;
    }
    tracing::info!("Bootstrapped skills/registry.toml into {:?}", skills_dir);

    let instr_dir = skills_dir.join("instructions");
    if let Err(e) = std::fs::create_dir_all(&instr_dir) {
        tracing::warn!("Failed to create instructions dir: {}", e);
        return;
    }
    for (filename, content) in INSTRUCTIONS {
        let path = instr_dir.join(filename);
        if !path.exists() {
            if let Err(e) = std::fs::write(&path, content) {
                tracing::warn!("Failed to write {}: {}", filename, e);
            }
        }
    }
    tracing::info!(
        "Bootstrapped {} instruction file(s) into {:?}",
        INSTRUCTIONS.len(),
        instr_dir
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("abigail_bootstrap_tests")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bootstrap_creates_registry_and_instructions() {
        let tmp = tmp_dir("fresh");
        bootstrap_if_needed(&tmp);

        let registry_path = tmp.join("skills").join("registry.toml");
        assert!(registry_path.exists(), "registry.toml should be created");

        let instr_dir = tmp.join("skills").join("instructions");
        assert!(instr_dir.exists(), "instructions/ dir should be created");

        let count = std::fs::read_dir(&instr_dir).unwrap().count();
        assert_eq!(
            count, INSTRUCTION_COUNT,
            "all {} instruction files should be seeded",
            INSTRUCTION_COUNT
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bootstrap_is_idempotent() {
        let tmp = tmp_dir("idempotent");
        bootstrap_if_needed(&tmp);

        let registry_path = tmp.join("skills").join("registry.toml");
        std::fs::write(&registry_path, "# user-modified\n").unwrap();
        bootstrap_if_needed(&tmp);

        let after = std::fs::read_to_string(&registry_path).unwrap();
        assert_eq!(
            after, "# user-modified\n",
            "bootstrap should not overwrite existing registry.toml"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn instruction_registry_loads_after_bootstrap() {
        let tmp = tmp_dir("load");
        bootstrap_if_needed(&tmp);

        let skills_dir = tmp.join("skills");
        let reg = abigail_skills::InstructionRegistry::load(
            &skills_dir.join("registry.toml"),
            &skills_dir.join("instructions"),
        );

        let email_matches = reg.select_instructions("check my email inbox");
        assert!(
            !email_matches.is_empty(),
            "email keyword should match after bootstrap"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn embedded_registry_toml_is_valid() {
        assert!(
            !REGISTRY_TOML.is_empty(),
            "embedded registry.toml should not be empty"
        );
        assert!(
            REGISTRY_TOML.contains("com.abigail.skills.proton-mail"),
            "embedded registry.toml should contain proton-mail skill"
        );
    }
}
