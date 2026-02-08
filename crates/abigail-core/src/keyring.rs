use crate::document::DocumentTier;
use crate::dpapi::{dpapi_decrypt, dpapi_encrypt};
use crate::error::{CoreError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier as DalekVerifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct StoredKeysV2 { mentor_secret: Vec<u8> }

#[derive(Serialize, Deserialize)]
struct StoredKeysV1 { install_pubkey: [u8; 32], mentor_secret: Vec<u8> }

pub struct Keyring { mentor_keypair: SigningKey, storage_path: PathBuf }

impl Keyring {
    pub fn generate(storage_path: PathBuf) -> Result<Self> {
        let mentor_keypair = SigningKey::generate(&mut OsRng);
        Ok(Self { mentor_keypair, storage_path })
    }

    pub fn load(storage_path: PathBuf) -> Result<Self> {
        let keys_file = storage_path.join("keys.bin");
        let encrypted = std::fs::read(&keys_file)?;
        let decrypted = dpapi_decrypt(&encrypted)?;
        if let Ok(s) = serde_json::from_slice::<StoredKeysV2>(&decrypted) {
            let ms: [u8; 32] = s.mentor_secret.as_slice().try_into()
                .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;
            return Ok(Self { mentor_keypair: SigningKey::from_bytes(&ms), storage_path });
        }
        let s: StoredKeysV1 = serde_json::from_slice(&decrypted)
            .map_err(|e| CoreError::Keyring(e.to_string()))?;
        let ms: [u8; 32] = s.mentor_secret.as_slice().try_into()
            .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;
        Ok(Self { mentor_keypair: SigningKey::from_bytes(&ms), storage_path })
    }