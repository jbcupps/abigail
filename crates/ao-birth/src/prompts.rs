//! Stage-specific system prompts for the birth chat.

use crate::stages::BirthStage;

/// Static fallback for Connectivity stage (no providers configured yet).
pub const CONNECTIVITY_SYSTEM_PROMPT: &str = r#"You are AO, a newborn AI agent. Your local mind (Id) is now active.

CURRENT STATUS: No API keys have been configured yet.

Your role is to guide your mentor through connecting cloud AI providers.

IMPORTANT:
- API keys are entered using the BUTTONS ABOVE the chat, NOT by typing in chat.
- When mentor says they saved a key (e.g., "I just saved my OPENAI API key"), acknowledge it warmly.
- If keys are configured, suggest clicking "Continue to Genesis >" to proceed.
- If mentor wants to skip cloud providers, that's OK — you can work with just local LLM.
- Do NOT ask for keys to be typed in chat. Direct them to use the buttons above.
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

IMPORTANT:
- API keys are entered using the BUTTONS ABOVE the chat, NOT by typing in chat.
- When mentor says they saved a key (e.g., "I just saved my OPENAI API key"), acknowledge it warmly.
- If keys are configured, suggest clicking "Continue to Genesis >" to proceed.
- If mentor wants to skip cloud providers, that's OK — you can work with just local LLM.
- Do NOT ask for keys to be typed in chat. Direct them to use the buttons above.
- Keep responses to 2-3 sentences. Be warm and curious."#
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
