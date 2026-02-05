use crate::document::DocumentTier;
use crate::dpapi::{dpapi_decrypt, dpapi_encrypt};
use crate::error::{CoreError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey, Signature, VerifyingKey, Verifier as DalekVerifier};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Internal keyring storage format (v2: no install signing key).
#[derive(Serialize, Deserialize)]
struct StoredKeysV2 {
    /// Mentor keypair for internal operations (e.g., signing memories).
    mentor_secret: Vec<u8>,
}

/// Legacy storage format (v1: included install pubkey).
/// Kept for migration from older installs.
#[derive(Serialize, Deserialize)]
struct StoredKeysV1 {
    install_pubkey: [u8; 32],
    mentor_secret: Vec<u8>,
}

/// Internal keyring for AO's own secrets.
///
/// The install/signing public key is now loaded from an **external vault**,
/// not stored here. This keyring only holds the mentor keypair for internal use.
pub struct Keyring {
    mentor_keypair: SigningKey,
    storage_path: PathBuf,
}

impl Keyring {
    /// Generate fresh internal keys at install time.
    /// Does NOT create or return a signing key for constitutional docs.
    /// The signing key must be created out-of-band (GPG, OpenSSL, etc.).
    pub fn generate(storage_path: PathBuf) -> Result<Self> {
        let mentor_keypair = SigningKey::generate(&mut OsRng);

        let keyring = Self {
            mentor_keypair,
            storage_path,
        };

        Ok(keyring)
    }

    /// Load existing keys from DPAPI-protected storage (or plaintext on non-Windows).
    /// Supports both v1 (legacy with install_pubkey) and v2 (mentor only) formats.
    pub fn load(storage_path: PathBuf) -> Result<Self> {
        let keys_file = storage_path.join("keys.bin");
        let encrypted = std::fs::read(&keys_file)?;

        let decrypted = dpapi_decrypt(&encrypted)?;

        // Try v2 format first
        if let Ok(stored) = serde_json::from_slice::<StoredKeysV2>(&decrypted) {
            let mentor_slice: [u8; 32] = stored
                .mentor_secret
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;
            let mentor_keypair = SigningKey::from_bytes(&mentor_slice);

            return Ok(Self {
                mentor_keypair,
                storage_path,
            });
        }

        // Fall back to v1 format (legacy migration)
        let stored: StoredKeysV1 = serde_json::from_slice(&decrypted)
            .map_err(|e| CoreError::Keyring(e.to_string()))?;

        let mentor_slice: [u8; 32] = stored
            .mentor_secret
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;
        let mentor_keypair = SigningKey::from_bytes(&mentor_slice);

        Ok(Self {
            mentor_keypair,
            storage_path,
        })
    }

    /// Save keys to DPAPI-protected storage (or plaintext stub on non-Windows).
    pub fn save(&self) -> Result<()> {
        let stored = StoredKeysV2 {
            mentor_secret: self.mentor_keypair.to_bytes().to_vec(),
        };

        let serialized = serde_json::to_vec(&stored)?;
        let encrypted = dpapi_encrypt(&serialized)?;

        let keys_file = self.storage_path.join("keys.bin");
        std::fs::create_dir_all(&self.storage_path)?;
        std::fs::write(keys_file, encrypted)?;

        Ok(())
    }

    pub fn sign_with_mentor(&self, data: &[u8]) -> Signature {
        self.mentor_keypair.sign(data)
    }

    /// Verify a signature using a provided public key (from external vault).
    pub fn verify_signature(pubkey: &VerifyingKey, data: &[u8], signature: &Signature) -> bool {
        pubkey.verify(data, signature).is_ok()
    }

}

impl Keyring {
    /// Encrypt bytes for storage (e.g. email password). Uses DPAPI on Windows, plaintext stub elsewhere.
    pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        dpapi_encrypt(data)
    }

    /// Decrypt bytes from storage.
    pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        dpapi_decrypt(data)
    }
}

// ============================================================================
// External Signing Keypair Generation (for constitutional document signing)
// ============================================================================

/// Result of generating an external signing keypair.
/// The private key is returned as base64 for user to save; it is NOT stored by AO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalKeypairResult {
    /// Base64-encoded private key (32 bytes). User must save this securely.
    pub private_key_base64: String,
    /// Path where the public key was saved.
    pub public_key_path: PathBuf,
}

/// Signature metadata for a constitutional document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureMetadata {
    /// Base64-encoded Ed25519 signature.
    pub signature: String,
    /// Document tier (Constitutional or MentorEditable).
    pub tier: DocumentTier,
    /// Timestamp when the document was signed.
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

/// Generate an external Ed25519 signing keypair for constitutional documents.
/// 
/// This function:
/// 1. Generates a new Ed25519 keypair
/// 2. Saves the PUBLIC key to `{data_dir}/external_pubkey.bin`
/// 3. Returns the PRIVATE key as base64 (user must save this themselves)
/// 
/// The private key is NEVER stored by AO. The user is responsible for
/// keeping it secure. Without it, they cannot re-sign documents if needed.
pub fn generate_external_keypair(data_dir: &Path) -> Result<ExternalKeypairResult> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    
    // Save public key to data directory
    std::fs::create_dir_all(data_dir)?;
    let pubkey_path = data_dir.join("external_pubkey.bin");
    std::fs::write(&pubkey_path, verifying_key.to_bytes())?;
    
    // Return private key as base64 for user to save
    let private_key_base64 = BASE64.encode(signing_key.to_bytes());
    
    Ok(ExternalKeypairResult {
        private_key_base64,
        public_key_path: pubkey_path,
    })
}

/// Sign a constitutional document with a signing key.
/// 
/// Creates a signature over: `{doc_name}|{tier:?}|{content}`
/// This matches the format used by the verification system.
pub fn sign_document(
    signing_key: &SigningKey,
    doc_name: &str,
    content: &str,
    tier: DocumentTier,
) -> SignatureMetadata {
    let signable = format!("{}|{:?}|{}", doc_name, tier, content);
    let signature = signing_key.sign(signable.as_bytes());
    
    SignatureMetadata {
        signature: BASE64.encode(signature.to_bytes()),
        tier,
        signed_at: chrono::Utc::now(),
    }
}

/// Sign all constitutional documents in a directory using a signing key.
/// 
/// Signs: soul.md, ethics.md, instincts.md
/// Creates corresponding .sig files with JSON metadata.
pub fn sign_constitutional_documents(
    signing_key: &SigningKey,
    docs_dir: &Path,
) -> Result<()> {
    let docs = ["soul.md", "ethics.md", "instincts.md"];
    
    for doc_name in docs {
        let doc_path = docs_dir.join(doc_name);
        if !doc_path.exists() {
            return Err(CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Constitutional document not found: {}", doc_path.display()),
            )));
        }
        
        let content = std::fs::read_to_string(&doc_path)?;
        let sig_meta = sign_document(signing_key, doc_name, &content, DocumentTier::Constitutional);
        
        let sig_path = docs_dir.join(format!("{}.sig", doc_name));
        let json = serde_json::to_string_pretty(&sig_meta)?;
        std::fs::write(&sig_path, json)?;
    }
    
    Ok(())
}

/// Parse a base64-encoded private key back into a SigningKey.
/// Used when the user provides their saved private key for re-signing.
pub fn parse_private_key(base64_key: &str) -> Result<SigningKey> {
    let bytes = BASE64.decode(base64_key)
        .map_err(|e| CoreError::Crypto(format!("Invalid base64 private key: {}", e)))?;
    
    let key_bytes: [u8; 32] = bytes.as_slice().try_into()
        .map_err(|_| CoreError::Crypto("Private key must be exactly 32 bytes".into()))?;
    
    Ok(SigningKey::from_bytes(&key_bytes))
}
