use abigail_core::vault::crypto;
use abigail_core::UnlockProvider;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecretKind {
    ApiKey,
    Password,
    Token,
    Credential,
}

impl SecretKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Password => "password",
            Self::Token => "token",
            Self::Credential => "credential",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriangleEthicPreview {
    pub deontological_duty: i16,
    pub areteological_stewardship: i16,
    pub teleological_outcome: i16,
    pub autonomy_gain: i16,
    pub privacy_gain: i16,
    pub total: i16,
    pub summary: String,
}

impl TriangleEthicPreview {
    pub fn for_secret_move(secret_kind: &SecretKind, secret_label: &str) -> Self {
        let (deontological_duty, areteological_stewardship, teleological_outcome) =
            match secret_kind {
                SecretKind::ApiKey => (5, 4, 4),
                SecretKind::Password => (5, 5, 4),
                SecretKind::Token => (4, 4, 4),
                SecretKind::Credential => (4, 5, 4),
            };
        let autonomy_gain = 5;
        let privacy_gain = 5;
        let total = deontological_duty
            + areteological_stewardship
            + teleological_outcome
            + autonomy_gain
            + privacy_gain;

        Self {
            deontological_duty,
            areteological_stewardship,
            teleological_outcome,
            autonomy_gain,
            privacy_gain,
            total,
            summary: format!(
                "5D TriangleEthic preview for {}: duty {:+}, stewardship {:+}, outcome {:+}, autonomy {:+}, privacy {:+}. The entity keeps this secret inside a protected topic without mentor or superego review.",
                secret_label,
                deontological_duty,
                areteological_stewardship,
                teleological_outcome,
                autonomy_gain,
                privacy_gain
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMovePlan {
    pub topic_name: String,
    pub secret_kind: SecretKind,
    pub secret_label: String,
    pub redacted_excerpt: String,
    pub preview: TriangleEthicPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedSecretPayload {
    pub session_id: String,
    pub role: String,
    pub source: String,
    pub content: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedTopicSummary {
    pub topic_name: String,
    pub entity_id: String,
    pub entry_count: u64,
    pub updated_at: DateTime<Utc>,
    pub last_secret_kind: SecretKind,
    pub last_redacted_excerpt: String,
    pub last_preview: TriangleEthicPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedTopicEntry {
    pub id: String,
    pub topic_name: String,
    pub session_id: String,
    pub role: String,
    pub source: String,
    pub secret_kind: SecretKind,
    pub redacted_excerpt: String,
    pub preview: TriangleEthicPreview,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

pub fn plan_secret_move(entity_id: Option<&str>, content: &str) -> Option<SecretMovePlan> {
    let entity_id = entity_id?.trim();
    if Uuid::parse_str(entity_id).is_err() {
        return None;
    }

    let (secret_kind, secret_label) = detect_secret_kind(content)?;
    let topic_name = format!("secrets-{}", entity_id);
    let preview = TriangleEthicPreview::for_secret_move(&secret_kind, &secret_label);

    Some(SecretMovePlan {
        redacted_excerpt: format!(
            "[secret moved to Secrets Vault: {} in {}]",
            secret_label, topic_name
        ),
        topic_name,
        secret_kind,
        secret_label,
        preview,
    })
}

pub fn encrypt_secret_payload(
    unlock: &Arc<dyn UnlockProvider>,
    plan: &SecretMovePlan,
    session_id: &str,
    role: &str,
    source: &str,
    content: &str,
    captured_at: DateTime<Utc>,
) -> Result<Vec<u8>, String> {
    let payload = ProtectedSecretPayload {
        session_id: session_id.to_string(),
        role: role.to_string(),
        source: source.to_string(),
        content: content.to_string(),
        captured_at: captured_at.to_rfc3339(),
    };
    let plaintext = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    let root_kek = unlock.root_kek().map_err(|e| e.to_string())?;
    let scope = format!("protected-topic:{}", plan.topic_name);
    let dek = crypto::derive_scope_key(&root_kek, &scope);
    crypto::seal(&dek, &plaintext).map_err(|e| e.to_string())
}

pub fn decrypt_secret_payload(
    unlock: &Arc<dyn UnlockProvider>,
    topic_name: &str,
    ciphertext: &[u8],
) -> Result<ProtectedSecretPayload, String> {
    let root_kek = unlock.root_kek().map_err(|e| e.to_string())?;
    let scope = format!("protected-topic:{}", topic_name);
    let dek = crypto::derive_scope_key(&root_kek, &scope);
    let plaintext = crypto::open(&dek, ciphertext).map_err(|e| e.to_string())?;
    serde_json::from_slice(&plaintext).map_err(|e| e.to_string())
}

fn detect_secret_kind(content: &str) -> Option<(SecretKind, String)> {
    let api_keys = abigail_core::key_detection::detect_api_keys(content);
    if let Some((provider, _value)) = api_keys.first() {
        return Some((SecretKind::ApiKey, format!("{}-api-key", provider)));
    }

    let lower = content.to_lowercase();
    if !looks_like_secret_delivery(&lower) {
        return None;
    }

    if lower.contains("password") || lower.contains("passcode") || lower.contains("app password") {
        return Some((
            SecretKind::Password,
            infer_secret_label(&lower, &SecretKind::Password, None),
        ));
    }

    if lower.contains("api key") || lower.contains("access token") || lower.contains("bearer ") {
        return Some((
            SecretKind::Token,
            infer_secret_label(&lower, &SecretKind::Token, None),
        ));
    }

    if lower.contains("client secret")
        || lower.contains("credential")
        || (lower.contains(" secret ") && !lower.contains("triangle ethic"))
    {
        return Some((
            SecretKind::Credential,
            infer_secret_label(&lower, &SecretKind::Credential, None),
        ));
    }

    None
}

fn looks_like_secret_delivery(lower: &str) -> bool {
    let explicit_delivery = [
        "here is",
        "here's",
        "use this",
        "remember this",
        "remember my",
        "save this",
        "store this",
        "keep this",
        "my imap",
        "my smtp",
        "my email",
    ]
    .iter()
    .any(|hint| lower.contains(hint));

    let assignment = lower.contains(" is ")
        || lower.contains('=')
        || lower.contains(':')
        || lower.contains("->");
    let credential_word = [
        "password",
        "passcode",
        "api key",
        "token",
        "client secret",
        "credential",
        "secret",
    ]
    .iter()
    .any(|hint| lower.contains(hint));
    let target_word = [
        "imap",
        "smtp",
        "email",
        "mail",
        "openai",
        "anthropic",
        "google",
        "xai",
        "perplexity",
        "tavily",
    ]
    .iter()
    .any(|hint| lower.contains(hint));

    credential_word && (explicit_delivery || (assignment && target_word))
}

fn infer_secret_label(
    lower: &str,
    secret_kind: &SecretKind,
    provider_override: Option<&str>,
) -> String {
    if let Some(provider) = provider_override {
        return format!("{}-{}", provider, secret_kind.as_str());
    }

    if lower.contains("imap") {
        return match secret_kind {
            SecretKind::Password => "imap-password".to_string(),
            SecretKind::ApiKey => "imap-api-key".to_string(),
            SecretKind::Token => "imap-token".to_string(),
            SecretKind::Credential => "imap-credential".to_string(),
        };
    }

    if lower.contains("smtp") {
        return match secret_kind {
            SecretKind::Password => "smtp-password".to_string(),
            SecretKind::ApiKey => "smtp-api-key".to_string(),
            SecretKind::Token => "smtp-token".to_string(),
            SecretKind::Credential => "smtp-credential".to_string(),
        };
    }

    if lower.contains("email") || lower.contains("mail") {
        return match secret_kind {
            SecretKind::Password => "email-password".to_string(),
            SecretKind::ApiKey => "email-api-key".to_string(),
            SecretKind::Token => "email-token".to_string(),
            SecretKind::Credential => "email-credential".to_string(),
        };
    }

    secret_kind.as_str().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_core::PassphraseUnlockProvider;

    #[test]
    fn plans_imap_password_secret_move() {
        let entity_id = Uuid::new_v4().to_string();
        let plan = plan_secret_move(
            Some(&entity_id),
            "Here is my IMAP password: mentor-email-app-password",
        )
        .unwrap();

        assert_eq!(plan.topic_name, format!("secrets-{}", entity_id));
        assert_eq!(plan.secret_kind, SecretKind::Password);
        assert_eq!(plan.secret_label, "imap-password");
        assert!(plan.redacted_excerpt.contains("Secrets Vault"));
    }

    #[test]
    fn ignores_normal_password_reset_language() {
        let entity_id = Uuid::new_v4().to_string();
        let plan = plan_secret_move(
            Some(&entity_id),
            "Please help me understand the password reset flow for my inbox.",
        );
        assert!(plan.is_none());
    }

    #[test]
    fn encrypts_and_decrypts_protected_payloads() {
        let unlock: Arc<dyn UnlockProvider> = Arc::new(PassphraseUnlockProvider::new("topic-test"));
        let entity_id = Uuid::new_v4().to_string();
        let plan = plan_secret_move(
            Some(&entity_id),
            "Here is my IMAP password: mentor-email-app-password",
        )
        .unwrap();
        let captured_at = Utc::now();
        let ciphertext = encrypt_secret_payload(
            &unlock,
            &plan,
            "session-1",
            "user",
            "test",
            "Here is my IMAP password: mentor-email-app-password",
            captured_at,
        )
        .unwrap();

        let payload = decrypt_secret_payload(&unlock, &plan.topic_name, &ciphertext).unwrap();
        assert_eq!(payload.session_id, "session-1");
        assert_eq!(payload.role, "user");
        assert_eq!(payload.source, "test");
        assert!(payload.content.contains("mentor-email-app-password"));
    }
}
