use crate::document::DocumentTier;
use crate::error::{CoreError, Result};
use crate::vault::crypto;
use crate::vault::unlock::{HybridUnlockProvider, UnlockProvider};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier as DalekVerifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const KEYRING_SCOPE: &str = "keyring:identity";

#[derive(Serialize, Deserialize)]
struct StoredKeysV2 {
    mentor_secret: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct StoredKeysV1 {
    install_pubkey: [u8; 32],
    mentor_secret: Vec<u8>,
}

pub struct Keyring {
    mentor_keypair: SigningKey,
    storage_path: PathBuf,
    unlock: Arc<dyn UnlockProvider>,
}

impl Keyring {
    pub fn generate(storage_path: PathBuf) -> Result<Self> {
        let mentor_keypair = SigningKey::generate(&mut OsRng);
        Ok(Self {
            mentor_keypair,
            storage_path,
            unlock: Arc::new(HybridUnlockProvider::new()),
        })
    }

    pub fn load(storage_path: PathBuf) -> Result<Self> {
        let keys_file = storage_path.join("keys.vault");
        let unlock: Arc<dyn UnlockProvider> = Arc::new(HybridUnlockProvider::new());

        if keys_file.exists() {
            return Self::load_new_format(&keys_file, storage_path, unlock);
        }

        // Fallback: try legacy DPAPI keys.bin
        let legacy_file = storage_path.join("keys.bin");
        if legacy_file.exists() {
            return Self::load_legacy(&legacy_file, storage_path, unlock);
        }

        Err(CoreError::Keyring(format!(
            "No keyring found at {} (tried keys.vault and keys.bin)",
            storage_path.display()
        )))
    }

    fn load_new_format(
        keys_file: &Path,
        storage_path: PathBuf,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        let root_kek = unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, KEYRING_SCOPE);
        let envelope = std::fs::read(keys_file)?;
        let decrypted = crypto::open(&dek, &envelope)?;

        let stored: StoredKeysV2 =
            serde_json::from_slice(&decrypted).map_err(|e| CoreError::Keyring(e.to_string()))?;
        let mentor_slice: [u8; 32] = stored
            .mentor_secret
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;

        Ok(Self {
            mentor_keypair: SigningKey::from_bytes(&mentor_slice),
            storage_path,
            unlock,
        })
    }

    fn load_legacy(
        legacy_file: &Path,
        storage_path: PathBuf,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        use crate::dpapi::dpapi_decrypt;
        let encrypted = std::fs::read(legacy_file)?;
        let decrypted = dpapi_decrypt(&encrypted)?;

        let mentor_bytes = if let Ok(stored) = serde_json::from_slice::<StoredKeysV2>(&decrypted) {
            stored.mentor_secret
        } else {
            let stored: StoredKeysV1 = serde_json::from_slice(&decrypted)
                .map_err(|e| CoreError::Keyring(e.to_string()))?;
            stored.mentor_secret
        };

        let mentor_slice: [u8; 32] = mentor_bytes
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;

        let keyring = Self {
            mentor_keypair: SigningKey::from_bytes(&mentor_slice),
            storage_path,
            unlock,
        };

        // Auto-upgrade: save in new format so legacy file is no longer needed
        if let Err(e) = keyring.save() {
            tracing::warn!("Could not auto-upgrade keyring to new format: {}", e);
        } else {
            tracing::info!("Keyring auto-upgraded from legacy keys.bin to keys.vault");
        }

        Ok(keyring)
    }

    pub fn save(&self) -> Result<()> {
        let stored = StoredKeysV2 {
            mentor_secret: self.mentor_keypair.to_bytes().to_vec(),
        };
        let serialized = serde_json::to_vec(&stored)?;
        let root_kek = self.unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, KEYRING_SCOPE);
        let envelope = crypto::seal(&dek, &serialized)?;

        let keys_file = self.storage_path.join("keys.vault");
        std::fs::create_dir_all(&self.storage_path)?;
        std::fs::write(keys_file, envelope)?;
        Ok(())
    }

    pub fn sign_with_mentor(&self, data: &[u8]) -> Signature {
        self.mentor_keypair.sign(data)
    }

    pub fn verify_signature(pubkey: &VerifyingKey, data: &[u8], signature: &Signature) -> bool {
        pubkey.verify(data, signature).is_ok()
    }
}

impl Keyring {
    pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        let unlock = HybridUnlockProvider::new();
        let root_kek = unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, "keyring:encrypt");
        crypto::seal(&dek, data)
    }

    pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        let unlock = HybridUnlockProvider::new();
        let root_kek = unlock.root_kek()?;
        let dek = crypto::derive_scope_key(&root_kek, "keyring:encrypt");
        crypto::open(&dek, data)
    }
}

// ============================================================================
// Master Key Generation (for Hive identity signing)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterKeyResult {
    pub master_key_path: PathBuf,
}

pub fn generate_master_key(data_dir: &Path) -> Result<MasterKeyResult> {
    let signing_key = SigningKey::generate(&mut OsRng);
    std::fs::create_dir_all(data_dir)?;

    let master_key_path = data_dir.join("master.key");
    let stored = MasterKeyStored {
        secret: signing_key.to_bytes().to_vec(),
    };
    let serialized = serde_json::to_vec(&stored)?;

    let unlock = HybridUnlockProvider::new();
    let root_kek = unlock.root_kek()?;
    let dek = crypto::derive_scope_key(&root_kek, "keyring:master");
    let envelope = crypto::seal(&dek, &serialized)?;
    std::fs::write(&master_key_path, envelope)?;
    Ok(MasterKeyResult { master_key_path })
}

pub fn load_master_key(path: &Path) -> Result<SigningKey> {
    let data = std::fs::read(path)?;

    // Try new envelope format first
    let unlock = HybridUnlockProvider::new();
    if let Ok(root_kek) = unlock.root_kek() {
        let dek = crypto::derive_scope_key(&root_kek, "keyring:master");
        if let Ok(decrypted) = crypto::open(&dek, &data) {
            let stored: MasterKeyStored = serde_json::from_slice(&decrypted)
                .map_err(|e| CoreError::Keyring(e.to_string()))?;
            let key_bytes: [u8; 32] = stored
                .secret
                .as_slice()
                .try_into()
                .map_err(|_| CoreError::Crypto("Invalid master key length".into()))?;
            return Ok(SigningKey::from_bytes(&key_bytes));
        }
    }

    // Fallback: try legacy DPAPI format
    let decrypted = crate::dpapi::dpapi_decrypt(&data)?;
    let stored: MasterKeyStored =
        serde_json::from_slice(&decrypted).map_err(|e| CoreError::Keyring(e.to_string()))?;
    let key_bytes: [u8; 32] = stored
        .secret
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::Crypto("Invalid master key length".into()))?;
    Ok(SigningKey::from_bytes(&key_bytes))
}

pub fn sign_agent_key(master_key: &SigningKey, agent_pubkey: &VerifyingKey) -> Vec<u8> {
    let signature = master_key.sign(agent_pubkey.as_bytes());
    signature.to_bytes().to_vec()
}

pub fn verify_agent_signature(
    master_pubkey: &VerifyingKey,
    agent_pubkey: &VerifyingKey,
    signature_bytes: &[u8],
) -> bool {
    let Ok(signature) = Signature::from_slice(signature_bytes) else {
        return false;
    };
    master_pubkey
        .verify(agent_pubkey.as_bytes(), &signature)
        .is_ok()
}

#[derive(Serialize, Deserialize)]
struct MasterKeyStored {
    secret: Vec<u8>,
}

// ============================================================================
// External Signing Keypair Generation (for constitutional document signing)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalKeypairResult {
    pub private_key_base64: String,
    pub public_key_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureMetadata {
    pub signature: String,
    pub tier: DocumentTier,
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

pub fn generate_external_keypair(data_dir: &Path) -> Result<ExternalKeypairResult> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    std::fs::create_dir_all(data_dir)?;
    let pubkey_path = data_dir.join("external_pubkey.bin");
    std::fs::write(&pubkey_path, verifying_key.to_bytes())?;
    let private_key_base64 = BASE64.encode(signing_key.to_bytes());
    Ok(ExternalKeypairResult {
        private_key_base64,
        public_key_path: pubkey_path,
    })
}

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

pub fn sign_constitutional_documents(signing_key: &SigningKey, docs_dir: &Path) -> Result<()> {
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
        let sig_meta = sign_document(
            signing_key,
            doc_name,
            &content,
            DocumentTier::Constitutional,
        );
        let sig_path = docs_dir.join(format!("{}.sig", doc_name));
        let json = serde_json::to_string_pretty(&sig_meta)?;
        std::fs::write(&sig_path, json)?;
    }
    Ok(())
}

pub fn parse_private_key(base64_key: &str) -> Result<SigningKey> {
    let bytes = BASE64
        .decode(base64_key)
        .map_err(|e| CoreError::Crypto(format!("Invalid base64 private key: {}", e)))?;
    let key_bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| CoreError::Crypto("Private key must be exactly 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&key_bytes))
}
