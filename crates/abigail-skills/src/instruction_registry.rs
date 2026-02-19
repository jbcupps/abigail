//! Skill Instruction Registry — loads `registry.toml` and injects skill-specific
//! LLM instructions into the system prompt based on keyword matching.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A single skill entry deserialized from `registry.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillInstructionEntry {
    pub id: String,
    pub instruction_file: String,
    pub keywords: Vec<String>,
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

    /// List all loaded entries (for diagnostics).
    pub fn list_entries(&self) -> Vec<&SkillInstructionEntry> {
        self.entries.values().map(|(entry, _)| entry).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
