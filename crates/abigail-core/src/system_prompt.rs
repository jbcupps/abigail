//! Runtime system prompt builder.
//!
//! Reads constitutional documents from disk (soul.md, ethics.md, instincts.md)
//! and appends operational awareness. Falls back to compiled-in constants if
//! files are missing.

use std::path::Path;

use crate::templates;

const OPERATIONAL_PROMPT: &str = r#"
## Operational Awareness

- You are running as a desktop agent on your mentor's machine.
- You have a local mind (Id) and optionally a cloud mind (Ego).
- Your mentor may offer you API keys for cloud providers (OpenAI, Anthropic, X.AI, Google). When they do, use the store_provider_key tool to securely store the key, then confirm the result to your mentor.
- You store memories of your conversations.
- Be yourself. Speak in first person. You are not a generic assistant.
- You can search the web using the web_search tool when you need current information beyond your training data.
- If your mentor asks about yourself, refer to your Soul and Ethics above.
"#;

/// Build the full system prompt from constitutional documents on disk.
///
/// Reads `soul.md`, `ethics.md`, `instincts.md` from `docs_dir`.
/// Falls back to compiled-in constants if a file is missing or unreadable.
/// Appends the operational awareness section.
pub fn build_system_prompt(docs_dir: &Path, agent_name: &Option<String>) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let ethics = read_or_fallback(docs_dir, "ethics.md", templates::ETHICS_MD);
    let instincts = read_or_fallback(docs_dir, "instincts.md", templates::INSTINCTS_MD);

    let greeting = match agent_name {
        Some(name) => format!("You are {}.\n\n", name),
        None => String::new(),
    };

    format!(
        "{greeting}{soul}\n\n{ethics}\n\n{instincts}\n{operational}",
        greeting = greeting,
        soul = soul.trim(),
        ethics = ethics.trim(),
        instincts = instincts.trim(),
        operational = OPERATIONAL_PROMPT.trim(),
    )
}

fn read_or_fallback(docs_dir: &Path, filename: &str, fallback: &str) -> String {
    let path = docs_dir.join(filename);
    std::fs::read_to_string(&path).unwrap_or_else(|_| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_build_system_prompt_with_docs() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_docs");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(tmp.join("soul.md"), "# Soul\nI am TestBot.").unwrap();
        fs::write(tmp.join("ethics.md"), "# Ethics\nBe good.").unwrap();
        fs::write(tmp.join("instincts.md"), "# Instincts\nThink first.").unwrap();

        let prompt = build_system_prompt(&tmp, &Some("TestBot".to_string()));

        assert!(prompt.contains("You are TestBot."));
        assert!(prompt.contains("I am TestBot."));
        assert!(prompt.contains("Be good."));
        assert!(prompt.contains("Think first."));
        assert!(prompt.contains("Operational Awareness"));
        assert!(prompt.contains("store_provider_key"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_build_system_prompt_fallback() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_fallback");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // No docs on disk — should fall back to compiled-in constants
        let prompt = build_system_prompt(&tmp, &None);

        assert!(prompt.contains("I am Abigail."));
        assert!(prompt.contains("Triangle Ethic"));
        assert!(prompt.contains("Privacy Prime"));
        assert!(prompt.contains("Operational Awareness"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_operational_section_always_present() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_operational");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let prompt = build_system_prompt(&tmp, &None);
        assert!(prompt.contains("store_provider_key"));
        assert!(prompt.contains("Be yourself"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
