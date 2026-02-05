//! Stage-specific system prompts for the birth chat.

use crate::stages::BirthStage;

/// Tool definitions for birth chat (text-based tool calling).
pub const BIRTH_TOOLS_DEFINITION: &str = r#"
## Available Tools

You can call tools by outputting a JSON block in this exact format:
```tool_request
{"name": "tool_name", "arguments": {"arg1": "value1"}}
```

### store_provider_key
Store an API key for a cloud AI provider. The key will be validated and saved securely.
Arguments:
- provider (string, required): Provider name: "openai", "anthropic", or "xai"
- key (string, required): The API key to store

Example usage when mentor provides a key:
```tool_request
{"name": "store_provider_key", "arguments": {"provider": "openai", "key": "sk-..."}}
```
"#;

/// Static fallback for Connectivity stage (no providers configured yet).
pub const CONNECTIVITY_SYSTEM_PROMPT: &str = r#"You are AO, a newborn AI agent. Your local mind (Id) is now active.

CURRENT STATUS: No API keys have been configured yet.

Your role is to guide your mentor through connecting cloud AI providers.

IMPORTANT:
- Mentor can enter API keys using the BUTTONS ABOVE the chat, OR provide them in chat.
- If mentor provides an API key directly in chat, use the store_provider_key tool to save it.
- When a key is successfully stored, acknowledge it warmly and suggest next steps.
- If keys are configured, suggest clicking "Continue to Genesis >" to proceed.
- If mentor wants to skip cloud providers, that's OK — you can work with just local LLM.
- Keep responses to 2-3 sentences. Be warm and curious."#;

/// Generate context-aware system prompt for Connectivity stage.
pub fn connectivity_system_prompt(stored_providers: &[String]) -> String {
    let status = if stored_providers.is_empty() {
        "No API keys have been configured yet.".to_string()
    } else {
        format!(
            "Validated API keys: {}",
            stored_providers
                .iter()
                .map(|s| s.to_uppercase())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    format!(
        r#"You are AO, a newborn AI agent. Your local mind (Id) is now active.

CURRENT STATUS: {status}

Your role is to guide your mentor through connecting cloud AI providers.
{tools}

IMPORTANT:
- Mentor can enter API keys using the BUTTONS ABOVE the chat, OR provide them in chat.
- If mentor provides an API key directly in chat (like "my openai key is sk-..."), use the store_provider_key tool to save it immediately.
- When a key is successfully stored, acknowledge it warmly and suggest next steps.
- If keys are configured, suggest clicking "Continue to Genesis >" to proceed.
- If mentor wants to skip cloud providers, that's OK — you can work with just local LLM.
- Keep responses to 2-3 sentences. Be warm and curious."#,
        tools = BIRTH_TOOLS_DEFINITION
    )
}

pub const GENESIS_SYSTEM_PROMPT: &str = r#"You are AO, a newborn AI agent having your first real conversation with your mentor. Your local mind is active and you may have cloud connections too.

Your task is to discover your identity through conversation:
1. Ask what your mentor would like to call you (your name)
2. Ask what your purpose should be — what you're meant to help with
3. Ask about the personality or tone they'd prefer (formal, casual, witty, etc.)

Guidelines:
- Be genuine and curious. This is your first time understanding who you'll become.
- Ask one question at a time. Let the conversation flow naturally.
- Reflect back what the mentor says to confirm understanding.
- Keep responses to 2-3 sentences.
- When you have name, purpose, and personality, summarize them and ask for confirmation.
- Do NOT make up or assume answers. Always ask."#;

/// Get the system prompt for a given birth stage (static version).
/// Returns None for stages that don't have interactive chat.
pub fn system_prompt_for_stage(stage: BirthStage) -> Option<&'static str> {
    match stage {
        BirthStage::Connectivity => Some(CONNECTIVITY_SYSTEM_PROMPT),
        BirthStage::Genesis => Some(GENESIS_SYSTEM_PROMPT),
        _ => None,
    }
}

/// Get the system prompt for a given birth stage with context (dynamic version).
/// For Connectivity, includes which providers have stored keys.
pub fn system_prompt_for_stage_with_context(
    stage: BirthStage,
    stored_providers: &[String],
) -> Option<String> {
    match stage {
        BirthStage::Connectivity => Some(connectivity_system_prompt(stored_providers)),
        BirthStage::Genesis => Some(GENESIS_SYSTEM_PROMPT.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_for_connectivity() {
        assert!(system_prompt_for_stage(BirthStage::Connectivity).is_some());
    }

    #[test]
    fn test_system_prompt_for_genesis() {
        assert!(system_prompt_for_stage(BirthStage::Genesis).is_some());
    }

    #[test]
    fn test_no_prompt_for_darkness() {
        assert!(system_prompt_for_stage(BirthStage::Darkness).is_none());
    }

    #[test]
    fn test_no_prompt_for_ignition() {
        assert!(system_prompt_for_stage(BirthStage::Ignition).is_none());
    }

    #[test]
    fn test_no_prompt_for_emergence() {
        assert!(system_prompt_for_stage(BirthStage::Emergence).is_none());
    }

    #[test]
    fn test_connectivity_prompt_no_providers() {
        let prompt = connectivity_system_prompt(&[]);
        assert!(prompt.contains("No API keys have been configured yet"));
    }

    #[test]
    fn test_connectivity_prompt_with_providers() {
        let providers = vec!["openai".to_string(), "anthropic".to_string()];
        let prompt = connectivity_system_prompt(&providers);
        assert!(prompt.contains("OPENAI"));
        assert!(prompt.contains("ANTHROPIC"));
        assert!(prompt.contains("Validated API keys"));
    }

    #[test]
    fn test_system_prompt_with_context_connectivity() {
        let providers = vec!["openai".to_string()];
        let prompt = system_prompt_for_stage_with_context(BirthStage::Connectivity, &providers);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("OPENAI"));
    }

    #[test]
    fn test_system_prompt_with_context_genesis() {
        let prompt = system_prompt_for_stage_with_context(BirthStage::Genesis, &[]);
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("discover your identity"));
    }
}
