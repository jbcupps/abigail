//! Skill Instruction Registry — loads `registry.toml` and injects skill-specific
//! LLM instructions into the system prompt based on keyword matching.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Controls how instruction injection behaves during prompt assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptInjectionMode {
    /// Inject matching instructions directly into the prompt (default for Full mode).
    PerMessage,
    /// Skip all instruction injection (orchestrator mode — delegates to sub-agents).
    None,
    /// Select instructions by topic affinity for delegation to sub-agents.
    TopicAffinity,
}

/// A single skill entry deserialized from `registry.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInstructionEntry {
    pub id: String,
    pub instruction_file: String,
    pub keywords: Vec<String>,
    /// Optional topic tags for semantic affinity matching in delegation mode.
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

/// Top-level TOML structure: `[[skill]]` array-of-tables.
#[derive(Debug, Deserialize)]
struct RegistryFile {
    skill: Vec<SkillInstructionEntry>,
}

/// Caches registry entries and their loaded instruction content.
/// Stored in `AppState` and queried on each chat message.
pub struct InstructionRegistry {
    /// Maps skill id -> (entry, instruction markdown content)
    entries: HashMap<String, (SkillInstructionEntry, String)>,
}

impl InstructionRegistry {
    /// Parse `registry.toml` and read each referenced `.md` file from `instructions_dir`.
    /// Missing instruction files are logged and skipped.
    pub fn load(registry_path: &Path, instructions_dir: &Path) -> Self {
        let toml_content = match std::fs::read_to_string(registry_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "Failed to read registry.toml at {}: {}",
                    registry_path.display(),
                    e
                );
                return Self::empty();
            }
        };

        let registry_file: RegistryFile = match toml::from_str(&toml_content) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to parse registry.toml: {}", e);
                return Self::empty();
            }
        };

        let mut entries = HashMap::new();
        for entry in registry_file.skill {
            if !entry.enabled {
                tracing::debug!("Skipping disabled skill: {}", entry.id);
                continue;
            }
            let md_path = instructions_dir.join(&entry.instruction_file);
            match std::fs::read_to_string(&md_path) {
                Ok(content) => {
                    tracing::info!(
                        "Loaded instruction file for skill {}: {}",
                        entry.id,
                        entry.instruction_file
                    );
                    entries.insert(entry.id.clone(), (entry, content));
                }
                Err(e) => {
                    tracing::warn!(
                        "Missing instruction file for skill {}: {} ({})",
                        entry.id,
                        md_path.display(),
                        e
                    );
                }
            }
        }

        tracing::info!(
            "InstructionRegistry loaded {} skill instruction(s)",
            entries.len()
        );
        Self { entries }
    }

    /// Create an empty registry (no skills, no instructions).
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Return entries whose keywords match the user message (case-insensitive).
    /// Returns `Vec<(skill_id, instruction_content)>`.
    pub fn select_instructions(&self, user_message: &str) -> Vec<(&str, &str)> {
        let msg_lower = user_message.to_lowercase();
        let mut matched = Vec::new();
        for (id, (entry, content)) in &self.entries {
            if entry
                .keywords
                .iter()
                .any(|kw| msg_lower.contains(&kw.to_lowercase()))
            {
                matched.push((id.as_str(), content.as_str()));
            }
        }
        matched
    }

    /// Format matched instructions as a prompt section.
    /// Returns an empty string if no skills match.
    pub fn format_for_prompt(&self, user_message: &str) -> String {
        let matched = self.select_instructions(user_message);
        if matched.is_empty() {
            return String::new();
        }
        let mut section = String::from("\n\n## Skill-Specific Instructions\n\n");
        for (id, content) in &matched {
            section.push_str(&format!("<!-- skill: {} -->\n{}\n\n", id, content));
        }
        section
    }

    /// Like [`format_for_prompt`], but only includes instructions for skills
    /// whose IDs are present in `registered_skill_ids`. This prevents "phantom
    /// tool" hallucinations where the LLM sees instructions for skills that
    /// aren't actually loaded in the runtime.
    pub fn format_for_prompt_filtered(
        &self,
        user_message: &str,
        registered_skill_ids: &HashSet<String>,
    ) -> String {
        let matched = self.select_instructions(user_message);
        let filtered: Vec<_> = matched
            .into_iter()
            .filter(|(id, _)| registered_skill_ids.contains(*id))
            .collect();
        if filtered.is_empty() {
            return String::new();
        }
        let mut section = String::from("\n\n## Skill-Specific Instructions\n\n");
        for (id, content) in &filtered {
            section.push_str(&format!("<!-- skill: {} -->\n{}\n\n", id, content));
        }
        section
    }

    /// Like [`format_for_prompt_filtered`], but with budget limits to cap
    /// the total injection size. Intended for CLI orchestrator mode where
    /// prompt size must stay small.
    ///
    /// - `max_instructions`: maximum number of instruction files to inject
    /// - `max_bytes`: cumulative byte cap for all injected instruction content
    ///
    /// Matched instructions are sorted by keyword specificity: entries whose
    /// matching keyword has more words rank higher (multi-word keywords are
    /// more specific than single-word ones).
    pub fn format_for_prompt_budgeted(
        &self,
        user_message: &str,
        registered_skill_ids: &HashSet<String>,
        max_instructions: usize,
        max_bytes: usize,
    ) -> String {
        if max_instructions == 0 || max_bytes == 0 {
            return String::new();
        }

        let msg_lower = user_message.to_lowercase();
        let mut scored: Vec<(&str, &str, usize)> = Vec::new();

        for (id, (entry, content)) in &self.entries {
            if !registered_skill_ids.contains(id.as_str()) {
                continue;
            }
            // Find the best (most specific) matching keyword
            let best_specificity = entry
                .keywords
                .iter()
                .filter(|kw| msg_lower.contains(&kw.to_lowercase()))
                .map(|kw| kw.split_whitespace().count())
                .max();

            if let Some(specificity) = best_specificity {
                scored.push((id.as_str(), content.as_str(), specificity));
            }
        }

        if scored.is_empty() {
            return String::new();
        }

        // Sort by specificity descending (more words = more specific = higher priority)
        scored.sort_by(|a, b| b.2.cmp(&a.2));

        let mut section = String::from("\n\n## Skill-Specific Instructions\n\n");
        let header_len = section.len();
        let mut total_bytes = 0usize;

        for (count, (id, content, _)) in scored.iter().enumerate() {
            if count >= max_instructions {
                break;
            }
            let entry_text = format!("<!-- skill: {} -->\n{}\n\n", id, content);
            if total_bytes + entry_text.len() > max_bytes {
                break;
            }
            section.push_str(&entry_text);
            total_bytes += entry_text.len();
        }

        // If no entries fit within budget, return empty
        if section.len() == header_len {
            return String::new();
        }

        section
    }

    /// Select instructions suitable for delegation to a sub-agent.
    ///
    /// Returns `Vec<(skill_id, instruction_content)>` matched by either keyword
    /// or topic affinity. Unlike `format_for_prompt*`, this returns raw data so
    /// callers can inject it into a `JobSpec.system_context` or other target.
    ///
    /// Matching logic:
    /// 1. If a skill has `topics` and the message matches any topic (substring),
    ///    it is included (topic affinity).
    /// 2. Otherwise, falls back to keyword matching (same as `select_instructions`).
    pub fn select_instructions_for_delegation(
        &self,
        user_message: &str,
        registered_skill_ids: &HashSet<String>,
    ) -> Vec<(&str, &str)> {
        let msg_lower = user_message.to_lowercase();
        let mut matched = Vec::new();

        for (id, (entry, content)) in &self.entries {
            if !registered_skill_ids.contains(id.as_str()) {
                continue;
            }

            // Try topic affinity first (more semantic)
            let topic_hit = !entry.topics.is_empty()
                && entry
                    .topics
                    .iter()
                    .any(|t| msg_lower.contains(&t.to_lowercase()));

            // Fall back to keyword matching
            let keyword_hit = entry
                .keywords
                .iter()
                .any(|kw| msg_lower.contains(&kw.to_lowercase()));

            if topic_hit || keyword_hit {
                matched.push((id.as_str(), content.as_str()));
            }
        }

        matched
    }

    /// Format delegation instructions as a single string for injection into
    /// a sub-agent's system context.
    pub fn format_for_delegation(
        &self,
        user_message: &str,
        registered_skill_ids: &HashSet<String>,
    ) -> String {
        let matched = self.select_instructions_for_delegation(user_message, registered_skill_ids);
        if matched.is_empty() {
            return String::new();
        }

        let mut section = String::from("\n## Skill-Specific Instructions\n\n");
        for (id, content) in &matched {
            section.push_str(&format!("<!-- skill: {} -->\n{}\n\n", id, content));
        }
        section
    }

    /// List all loaded entries (for diagnostics).
    pub fn list_entries(&self) -> Vec<&SkillInstructionEntry> {
        self.entries.values().map(|(entry, _)| entry).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("abigail_instruction_registry_tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn setup_registry(
        test_name: &str,
        toml_content: &str,
        instruction_files: &[(&str, &str)],
    ) -> (PathBuf, InstructionRegistry) {
        let dir = test_dir(test_name);
        let registry_path = dir.join("registry.toml");
        fs::write(&registry_path, toml_content).unwrap();

        let instructions_dir = dir.join("instructions");
        fs::create_dir_all(&instructions_dir).unwrap();
        for (name, content) in instruction_files {
            fs::write(instructions_dir.join(name), content).unwrap();
        }

        let reg = InstructionRegistry::load(&registry_path, &instructions_dir);
        (dir, reg)
    }

    #[test]
    fn test_registry_loads_and_injects_instructions() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email", "inbox"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["search", "look up"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "loads_and_injects",
            toml,
            &[
                ("email.md", "# Email Instructions\nUse fetch_emails."),
                ("search.md", "# Search Instructions\nUse web_search."),
            ],
        );

        // Should match email
        let matches = reg.select_instructions("check my email");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "test.email");
        assert!(matches[0].1.contains("fetch_emails"));

        // Should match search
        let matches = reg.select_instructions("search for Rust tutorials");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "test.search");

        // Should match both
        let matches = reg.select_instructions("search my email inbox");
        assert_eq!(matches.len(), 2);

        // format_for_prompt should include section header
        let prompt = reg.format_for_prompt("check my email");
        assert!(prompt.contains("## Skill-Specific Instructions"));
        assert!(prompt.contains("test.email"));

        // No match should return empty
        let prompt = reg.format_for_prompt("tell me a joke");
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_empty_registry() {
        let reg = InstructionRegistry::empty();
        assert!(reg.list_entries().is_empty());
        assert!(reg.select_instructions("anything").is_empty());
        assert!(reg.format_for_prompt("anything").is_empty());
    }

    #[test]
    fn test_format_for_prompt_filtered_drops_unregistered() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "filtered_drops_unregistered",
            toml,
            &[
                ("email.md", "# Email\nUse fetch_emails."),
                ("search.md", "# Search\nUse web_search."),
            ],
        );

        // Both match on "email", but only test.email is registered
        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());

        let prompt = reg.format_for_prompt_filtered("check email", &registered);
        assert!(prompt.contains("test.email"));
        assert!(!prompt.contains("test.search"));
    }

    #[test]
    fn test_format_for_prompt_filtered_passes_registered() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "filtered_passes_registered",
            toml,
            &[("email.md", "# Email\nUse fetch_emails.")],
        );

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());

        let prompt = reg.format_for_prompt_filtered("check email", &registered);
        assert!(prompt.contains("test.email"));
        assert!(prompt.contains("## Skill-Specific Instructions"));
    }

    #[test]
    fn test_format_for_prompt_filtered_empty_registry() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "filtered_empty_registry",
            toml,
            &[("email.md", "# Email\nUse fetch_emails.")],
        );

        let registered = HashSet::new();
        let prompt = reg.format_for_prompt_filtered("check email", &registered);
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_format_for_prompt_budgeted_limits_count() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true

[[skill]]
id = "test.calendar"
instruction_file = "calendar.md"
keywords = ["email"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "budgeted_limits_count",
            toml,
            &[
                ("email.md", "# Email\nUse fetch_emails."),
                ("calendar.md", "# Calendar\nUse list_events."),
                ("search.md", "# Search\nUse web_search."),
            ],
        );

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());
        registered.insert("test.calendar".to_string());
        registered.insert("test.search".to_string());

        // All 3 match on "email", but budget limits to 1
        let prompt = reg.format_for_prompt_budgeted("check email", &registered, 1, 8192);
        assert!(prompt.contains("## Skill-Specific Instructions"));
        // Should contain exactly 1 skill
        let skill_count = prompt.matches("<!-- skill:").count();
        assert_eq!(skill_count, 1, "Expected 1 skill, got {}", skill_count);
    }

    #[test]
    fn test_format_for_prompt_budgeted_limits_bytes() {
        let toml = r#"
[[skill]]
id = "test.big"
instruction_file = "big.md"
keywords = ["data"]
enabled = true

[[skill]]
id = "test.small"
instruction_file = "small.md"
keywords = ["data"]
enabled = true
"#;
        // big.md is 200 bytes, small.md is tiny
        let big_content = "x".repeat(200);
        let (_dir, reg) = setup_registry(
            "budgeted_limits_bytes",
            toml,
            &[("big.md", &big_content), ("small.md", "tiny")],
        );

        let mut registered = HashSet::new();
        registered.insert("test.big".to_string());
        registered.insert("test.small".to_string());

        // Set byte cap to 100 — big.md won't fit but small.md will
        let prompt = reg.format_for_prompt_budgeted("data query", &registered, 10, 100);
        let skill_count = prompt.matches("<!-- skill:").count();
        assert!(
            skill_count <= 1,
            "Expected at most 1 skill within byte budget"
        );
    }

    #[test]
    fn test_format_for_prompt_budgeted_specificity_ordering() {
        let toml = r#"
[[skill]]
id = "test.generic"
instruction_file = "generic.md"
keywords = ["email"]
enabled = true

[[skill]]
id = "test.specific"
instruction_file = "specific.md"
keywords = ["check email inbox"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "budgeted_specificity",
            toml,
            &[
                ("generic.md", "# Generic\nGeneric handler."),
                ("specific.md", "# Specific\nSpecific handler."),
            ],
        );

        let mut registered = HashSet::new();
        registered.insert("test.generic".to_string());
        registered.insert("test.specific".to_string());

        // Both match, but limit to 1 — the multi-word keyword should win
        let prompt = reg.format_for_prompt_budgeted("check email inbox", &registered, 1, 8192);
        assert!(
            prompt.contains("test.specific"),
            "Multi-word keyword should rank higher"
        );
    }

    #[test]
    fn test_format_for_prompt_budgeted_zero_limits() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) =
            setup_registry("budgeted_zero", toml, &[("email.md", "# Email\nContent.")]);

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());

        assert!(reg
            .format_for_prompt_budgeted("email", &registered, 0, 8192)
            .is_empty());
        assert!(reg
            .format_for_prompt_budgeted("email", &registered, 10, 0)
            .is_empty());
    }

    #[test]
    fn test_registry_missing_instruction_file() {
        let toml = r#"
[[skill]]
id = "test.missing"
instruction_file = "nonexistent.md"
keywords = ["missing"]
enabled = true

[[skill]]
id = "test.present"
instruction_file = "present.md"
keywords = ["present"]
enabled = true
"#;
        let (_dir, reg) =
            setup_registry("missing_file", toml, &[("present.md", "# Present\nHere.")]);

        // Only the present one should load
        assert_eq!(reg.list_entries().len(), 1);
        assert!(reg.select_instructions("missing keyword").is_empty());
        assert_eq!(reg.select_instructions("present keyword").len(), 1);
    }

    #[test]
    fn test_select_instructions_for_delegation_keyword_match() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email", "inbox"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["search"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "delegation_keyword",
            toml,
            &[
                ("email.md", "# Email\nUse fetch_emails."),
                ("search.md", "# Search\nUse web_search."),
            ],
        );

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());
        registered.insert("test.search".to_string());

        let matched = reg.select_instructions_for_delegation("check email", &registered);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0, "test.email");
    }

    #[test]
    fn test_select_instructions_for_delegation_topic_match() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
topics = ["communication", "messaging"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["search"]
topics = ["research"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "delegation_topic",
            toml,
            &[
                ("email.md", "# Email\nUse fetch_emails."),
                ("search.md", "# Search\nUse web_search."),
            ],
        );

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());
        registered.insert("test.search".to_string());

        // "communication" should match via topics even though "communication" isn't a keyword
        let matched =
            reg.select_instructions_for_delegation("handle communication tasks", &registered);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0, "test.email");
    }

    #[test]
    fn test_select_instructions_for_delegation_filters_unregistered() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true

[[skill]]
id = "test.search"
instruction_file = "search.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "delegation_filters",
            toml,
            &[
                ("email.md", "# Email\nContent."),
                ("search.md", "# Search\nContent."),
            ],
        );

        // Only register test.email
        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());

        let matched = reg.select_instructions_for_delegation("email stuff", &registered);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].0, "test.email");
    }

    #[test]
    fn test_format_for_delegation() {
        let toml = r#"
[[skill]]
id = "test.email"
instruction_file = "email.md"
keywords = ["email"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "format_delegation",
            toml,
            &[("email.md", "# Email\nUse fetch_emails.")],
        );

        let mut registered = HashSet::new();
        registered.insert("test.email".to_string());

        let section = reg.format_for_delegation("check email", &registered);
        assert!(section.contains("## Skill-Specific Instructions"));
        assert!(section.contains("test.email"));
        assert!(section.contains("fetch_emails"));

        // No match returns empty
        let section = reg.format_for_delegation("unrelated query", &registered);
        assert!(section.is_empty());
    }

    #[test]
    fn test_topics_field_defaults_empty() {
        let toml = r#"
[[skill]]
id = "test.notopics"
instruction_file = "notopics.md"
keywords = ["test"]
enabled = true
"#;
        let (_dir, reg) = setup_registry(
            "topics_default",
            toml,
            &[("notopics.md", "# No topics\nContent.")],
        );

        let entries = reg.list_entries();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].topics.is_empty());
    }
}
