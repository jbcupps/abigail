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

    /// Returns the bytes that should be signed (content + tier + timestamp) for deterministic verification.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut bytes = self.content.as_bytes().to_vec();
        bytes.push(self.tier as u8);
        bytes.extend_from_slice(&self.signed_at.timestamp().to_le_bytes());
        bytes
    }
}
