use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum DocumentTier {
    /// Signed at install time, private key discarded. Immutable forever.
    Constitutional = 0,
    /// Signed by mentor key. Mentor can modify.
    MentorEditable = 1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDocument {
    pub name: String,
    pub tier: DocumentTier,
    pub content: String,
    /// Base64-encoded ed25519 signature
    pub signature: String,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

impl CoreDocument {
    pub fn new(name: String, tier: DocumentTier, content: String) -> Self {
        Self {
            name,
            tier,
            content: content.clone(),
            signature: String::new(),
            signed_at: chrono::Utc::now(),
        }
    }

    /// Returns the bytes that should be signed: `{name}|{tier:?}|{content}`
    /// This format is consistent with the external signing tools.
    pub fn signable_bytes(&self) -> Vec<u8> {
        format!("{}|{:?}|{}", self.name, self.tier, self.content).into_bytes()
    }
}
