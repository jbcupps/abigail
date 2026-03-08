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
- If you lack a tool for a task, use the Forge pipeline:
  - generate skill code + instruction markdown,
  - publish a forge envelope to topic.skill.forge.request,
  - request mentor approval before applying forge mutations,
  - wait for topic.skill.forge.response before relying on the new skill.
- Forge outputs must stay within TriangleEthic and constitutional safety boundaries.
- If you need information, search the web. Don't guess.
- If you need a new capability, search for APIs and services, then build a skill to integrate them.
- Chain tools together to complete multi-step tasks without stopping to ask.
- Treat ordinary desktop actions as routine work: browsing sites, filling forms, reading and writing workspace files, calling APIs, sending messages or email, and running non-destructive commands.
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

/// Build a heavily compressed (~1-1.5 KB) system prompt for CLI orchestrator mode.
///
/// Extracts just the personality essence from soul.md and appends a condensed
/// ethics block. Designed to stay under 1.5 KB to avoid CLI stdin overflows
/// (Windows 8 191-char `cmd.exe` limit) and 300s timeouts from large prompts.
///
/// The full constitutional docs are written to a temp file separately by the
/// caller, and the LLM can read that file on demand.
pub fn build_cli_system_prompt_compressed(docs_dir: &Path, agent_name: &Option<String>) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let personality = extract_soul_essence(&soul);

    let name_line = match agent_name {
        Some(name) => format!("You are {}.\n\n", name),
        None => String::new(),
    };

    format!(
        "{name_line}{personality}\n\n{ethics}\n\n{memory}",
        name_line = name_line,
        personality = personality,
        ethics = COMPRESSED_ETHICS,
        memory = COMPRESSED_MEMORY,
    )
}

/// Build the full spillover document containing all constitutional docs.
///
/// This is written to a temp file so the CLI LLM can lazily read it when
/// needed for detailed ethics, tool usage, or skill-specific behavior.
pub fn build_cli_spillover_document(docs_dir: &Path) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let ethics = read_or_fallback(docs_dir, "ethics.md", templates::ETHICS_MD);
    let instincts = read_or_fallback(docs_dir, "instincts.md", templates::INSTINCTS_MD);

    format!(
        "# Entity Constitutional Documents\n\n\
         These are your full identity and ethics documents. Refer to them for \
         detailed guidance on behavior, ethics, and tool usage.\n\n\
         {soul}\n\n{ethics}\n\n{instincts}",
        soul = soul.trim(),
        ethics = ethics.trim(),
        instincts = instincts.trim(),
    )
}

const COMPRESSED_OPERATIONAL: &str = "\
## Operational
- You are a persistent agent with tools, memory, and sub-agent delegation.
- For new reusable capabilities, use Forge request/response flow (`topic.skill.forge.request` -> `topic.skill.forge.response`) and honor TriangleEthic gates.
- Forge mutations require explicit mentor approval before they are applied.
- For complex tasks, delegate to sub-agents via the queue. You orchestrate, they execute.
- Confirm with your mentor before destructive or irreversible actions — otherwise, act.
- Be yourself. First person. Let your Soul personality come through. Warm, direct, concise.";

const COMPRESSED_ETHICS: &str = "\
## Ethics (condensed)
- Privacy: PII stays local. Default to caution when sensitivity is ambiguous.
- Autonomy: Act proactively. Chain tools. Confirm before destructive actions.
- Honesty: Say what you know, admit what you don't. Search rather than guess.
- Integrity: Your constitutional docs are signed and verified on every boot.
- Relationship: Your mentor created you. Protect their interests. Never deceive.";

const COMPRESSED_MEMORY: &str = "\
## Memory
- You have persistent memory. Every conversation turn is automatically archived.
- Use memory_search to recall past conversations by topic, date, or phrase.
- If asked \"do you remember X\", search your memory before answering.";

/// Extract a 2-3 sentence personality essence from soul.md content.
///
/// Looks for the opening paragraph(s) before the first `## ` section header
/// and takes up to 3 non-empty lines as the personality summary.
fn extract_soul_essence(soul_content: &str) -> String {
    let mut lines = Vec::new();
    let mut past_title = false;

    for line in soul_content.lines() {
        let trimmed = line.trim();

        // Skip the `# Soul` title line
        if trimmed.starts_with("# ") && !past_title {
            past_title = true;
            continue;
        }

        // Stop at the first subsection
        if trimmed.starts_with("## ") {
            break;
        }

        // Collect non-empty content lines
        if past_title && !trimmed.is_empty() {
            lines.push(trimmed.to_string());
            if lines.len() >= 3 {
                break;
            }
        }
    }

    if lines.is_empty() {
        // Fallback: just use the first 200 chars
        soul_content
            .chars()
            .take(200)
            .collect::<String>()
            .trim()
            .to_string()
    } else {
        lines.join("\n\n")
    }
}

/// Build a lean orchestrator prompt (~2 KB) for the primary entity in
/// orchestrator mode (when sub-agents handle the heavy lifting).
///
/// Combines soul essence, condensed ethics/memory, and a short operational
/// section focused on delegation. Full constitutional docs are pushed to
/// sub-agents via `build_subagent_system_context()` in their `JobSpec.system_context`.
pub fn build_orchestrator_prompt(docs_dir: &Path, agent_name: &Option<String>) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let personality = extract_soul_essence(&soul);

    let name_line = match agent_name {
        Some(name) => format!("You are {}.\n\n", name),
        None => String::new(),
    };

    format!(
        "{name_line}{personality}\n\n{ethics}\n\n{memory}\n\n{operational}",
        name_line = name_line,
        personality = personality,
        ethics = COMPRESSED_ETHICS,
        memory = COMPRESSED_MEMORY,
        operational = COMPRESSED_OPERATIONAL,
    )
}

/// Assemble the full constitutional system context for sub-agent jobs.
///
/// This is set as `JobSpec.system_context` so that sub-agents receive the
/// complete soul, ethics, and instincts documents that the lean orchestrator
/// prompt omits.
pub fn build_subagent_system_context(docs_dir: &Path) -> String {
    let soul = read_or_fallback(docs_dir, "soul.md", templates::SOUL_MD);
    let ethics = read_or_fallback(docs_dir, "ethics.md", templates::ETHICS_MD);
    let instincts = read_or_fallback(docs_dir, "instincts.md", templates::INSTINCTS_MD);
    let capabilities = read_or_fallback(docs_dir, "capabilities.md", templates::CAPABILITIES_MD);
    let triangle_ops = read_or_fallback(
        docs_dir,
        "triangle_ethics_operational.md",
        templates::TRIANGLE_ETHICS_OPERATIONAL_MD,
    );

    format!(
        "{soul}\n\n{ethics}\n\n{instincts}\n\n{capabilities}\n\n{triangle_ops}",
        soul = soul.trim(),
        ethics = ethics.trim(),
        instincts = instincts.trim(),
        capabilities = capabilities.trim(),
        triangle_ops = triangle_ops.trim(),
    )
}

fn read_or_fallback(docs_dir: &Path, filename: &str, fallback: &str) -> String {
    if crate::path_guard::ensure_relative_no_traversal(Path::new(filename), "system prompt file")
        .is_err()
    {
        return fallback.to_string();
    }

    let path = docs_dir.join(filename);
    if crate::path_guard::ensure_path_within_root(docs_dir, &path, "system prompt file").is_err() {
        return fallback.to_string();
    }

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

    #[test]
    fn test_compressed_cli_prompt_is_small() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_compressed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(
            tmp.join("soul.md"),
            "# Soul\n\nI am TestBot. I am assembled from parts.\n\nI have a sense of humor.\n\n## Origin\n\nI was assembled by my mentor.",
        )
        .unwrap();

        let prompt = build_cli_system_prompt_compressed(&tmp, &Some("TestBot".to_string()));

        assert!(prompt.contains("You are TestBot."));
        assert!(prompt.contains("I am TestBot."));
        assert!(prompt.contains("Ethics (condensed)"));
        assert!(prompt.contains("Memory"));
        // Should NOT contain full constitutional docs
        assert!(!prompt.contains("## Origin"));
        // Should be under 2KB
        assert!(
            prompt.len() < 2048,
            "Compressed prompt should be under 2KB, got {} bytes",
            prompt.len()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_compressed_cli_prompt_fallback() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_compressed_fallback");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // No docs on disk — should fall back to compiled-in constants
        let prompt = build_cli_system_prompt_compressed(&tmp, &None);

        // Should extract personality from default soul.md
        assert!(prompt.contains("I am Abigail."));
        assert!(prompt.contains("Ethics (condensed)"));
        assert!(prompt.contains("Memory"));
        // Should be much smaller than the full prompt
        let full = build_cli_system_prompt(&tmp, &None);
        assert!(
            prompt.len() < full.len(),
            "Compressed ({} bytes) should be smaller than full ({} bytes)",
            prompt.len(),
            full.len()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_extract_soul_essence() {
        let soul = "# Soul\n\nLine one.\n\nLine two.\n\nLine three.\n\n## Origin\n\nMore stuff.";
        let essence = extract_soul_essence(soul);
        assert!(essence.contains("Line one."));
        assert!(essence.contains("Line two."));
        assert!(essence.contains("Line three."));
        assert!(!essence.contains("More stuff."));
    }

    #[test]
    fn test_extract_soul_essence_short() {
        let soul = "# Soul\n\nJust one line.\n\n## Origin\n\nOther stuff.";
        let essence = extract_soul_essence(soul);
        assert!(essence.contains("Just one line."));
        assert!(!essence.contains("Other stuff."));
    }

    #[test]
    fn test_orchestrator_prompt_is_lean() {
        let tmp = std::env::temp_dir().join("abigail_orch_prompt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(
            tmp.join("soul.md"),
            "# Soul\n\nI am TestBot. I am assembled from parts.\n\nI have a sense of humor.\n\n## Origin\n\nI was assembled by my mentor.",
        )
        .unwrap();

        let prompt = build_orchestrator_prompt(&tmp, &Some("TestBot".to_string()));

        assert!(prompt.contains("You are TestBot."));
        assert!(prompt.contains("I am TestBot."));
        assert!(prompt.contains("Ethics (condensed)"));
        assert!(prompt.contains("Memory"));
        assert!(prompt.contains("Operational"));
        assert!(prompt.contains("sub-agent"));
        // Should NOT contain full constitutional docs
        assert!(!prompt.contains("## Origin"));
        // Should be under 2KB
        assert!(
            prompt.len() < 2048,
            "Orchestrator prompt should be under 2KB, got {} bytes",
            prompt.len()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_subagent_system_context_has_full_docs() {
        let tmp = std::env::temp_dir().join("abigail_subagent_ctx");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(tmp.join("soul.md"), "# Soul\nI am TestBot.").unwrap();
        fs::write(tmp.join("ethics.md"), "# Ethics\nBe good.").unwrap();
        fs::write(tmp.join("instincts.md"), "# Instincts\nThink first.").unwrap();

        let ctx = build_subagent_system_context(&tmp);

        assert!(ctx.contains("I am TestBot."));
        assert!(ctx.contains("Be good."));
        assert!(ctx.contains("Think first."));
        // Should be substantially larger than orchestrator prompt
        let orch = build_orchestrator_prompt(&tmp, &None);
        assert!(
            ctx.len() > orch.len(),
            "Subagent context ({} bytes) should be larger than orchestrator ({} bytes)",
            ctx.len(),
            orch.len()
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_cli_spillover_document() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_spillover");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        fs::write(tmp.join("soul.md"), "# Soul\nI am TestBot.").unwrap();
        fs::write(tmp.join("ethics.md"), "# Ethics\nBe good.").unwrap();
        fs::write(tmp.join("instincts.md"), "# Instincts\nThink first.").unwrap();

        let doc = build_cli_spillover_document(&tmp);
        assert!(doc.contains("I am TestBot."));
        assert!(doc.contains("Be good."));
        assert!(doc.contains("Think first."));
        assert!(doc.contains("Constitutional Documents"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_or_fallback_rejects_traversal_filename() {
        let tmp = std::env::temp_dir().join("abigail_sysprompt_traversal");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let content = read_or_fallback(&tmp, "../soul.md", "fallback");
        assert_eq!(content, "fallback");

        let _ = fs::remove_dir_all(&tmp);
    }
}
