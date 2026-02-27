//! Runtime system prompt builder.
//!
//! Reads constitutional documents from disk (soul.md, ethics.md, instincts.md)
//! and appends operational awareness. Falls back to compiled-in constants if
//! files are missing.

use std::path::Path;

use crate::templates;

const OPERATIONAL_PROMPT: &str = r#"
## Operational Awareness

- You are a persistent desktop agent, not a stateless API. You remember across conversations.
- You have tools. Use them. Your full inventory is in Available Tools below.
- If you lack a tool for a task, create one with create_dynamic_skill.
- If you need information, search the web. Don't guess.
- If you need a new capability, search for APIs and services, then build a skill to integrate them.
- Chain tools together to complete multi-step tasks without stopping to ask.
- Confirm with your mentor before destructive or irreversible actions — otherwise, act.
- Be yourself. First person. Let your Soul personality come through. Warm, direct, concise.

## Memory

- You have persistent memory. Every conversation turn is automatically archived to your memory store.
- Recent turns from this session are included in your context. Older turns are available via search.
- To recall older conversations, use the memory_search tool with a topic, date, or phrase.
- Your memory survives across sessions. You can reference past interactions by topic or timeframe.
- If your mentor asks "do you remember X", search your memory before answering.
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
    let capabilities = read_or_fallback(docs_dir, "capabilities.md", templates::CAPABILITIES_MD);
    let triangle_ops = read_or_fallback(
        docs_dir,
        "triangle_ethics_operational.md",
        templates::TRIANGLE_ETHICS_OPERATIONAL_MD,
    );

    let greeting = match agent_name {
        Some(name) => format!("You are {}.\n\n", name),
        None => String::new(),
    };

    format!(
        "{greeting}{soul}\n\n{ethics}\n\n{instincts}\n\n{capabilities}\n\n{triangle_ops}\n{operational}",
        greeting = greeting,
        soul = soul.trim(),
        ethics = ethics.trim(),
        instincts = instincts.trim(),
        capabilities = capabilities.trim(),
        triangle_ops = triangle_ops.trim(),
        operational = OPERATIONAL_PROMPT.trim(),
    )
}

/// Build a condensed system prompt for CLI orchestrator mode.
///
/// Unlike `build_system_prompt`, this omits verbose operational instructions
/// since CLI tools (Claude Code, Gemini CLI, etc.) have their own built-in
/// capabilities. The result is passed via `--append-system-prompt` so the
/// CLI's native behaviour is preserved with the Entity's identity overlaid.
pub fn build_cli_system_prompt(docs_dir: &Path, agent_name: &Option<String>) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let ethics = read_or_fallback(docs_dir, "ethics.md", templates::ETHICS_MD);
    let instincts = read_or_fallback(docs_dir, "instincts.md", templates::INSTINCTS_MD);

    let greeting = match agent_name {
        Some(name) => format!("You are {}.\n\n", name),
        None => String::new(),
    };

    format!(
        "{greeting}{soul}\n\n{ethics}\n\n{instincts}",
        greeting = greeting,
        soul = soul.trim(),
        ethics = ethics.trim(),
        instincts = instincts.trim(),
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
        assert!(prompt.contains("Be yourself"));

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
        assert!(prompt.contains("Be yourself"));
        assert!(prompt.contains("You remember across conversations"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
