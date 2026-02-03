use crate::error::{CoreError, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey, Signature, VerifyingKey, Verifier as DalekVerifier};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct StoredKeys {
    install_pubkey: [u8; 32],
    mentor_secret: Vec<u8>,
}

pub struct Keyring {
    install_pubkey: VerifyingKey,
    mentor_keypair: SigningKey,
    storage_path: PathBuf,
}

impl Keyring {
    /// Generate fresh keys at install time. Returns (Keyring, install_signing_key).
    /// Caller MUST use install_signing_key to sign constitutional docs then discard it.
    pub fn generate(storage_path: PathBuf) -> Result<(Self, SigningKey)> {
        let install_signing = SigningKey::generate(&mut OsRng);
        let install_pubkey = install_signing.verifying_key();
        let mentor_keypair = SigningKey::generate(&mut OsRng);

        let keyring = Self {
            install_pubkey,
            mentor_keypair,
            storage_path,
        };

        Ok((keyring, install_signing))
    }

    /// Load existing keys from DPAPI-protected storage (or plaintext on non-Windows).
    pub fn load(storage_path: PathBuf) -> Result<Self> {
        let keys_file = storage_path.join("keys.bin");
        let encrypted = std::fs::read(&keys_file)?;

        let decrypted = Self::dpapi_decrypt(&encrypted)?;
        let stored: StoredKeys = serde_json::from_slice(&decrypted)
            .map_err(|e| CoreError::Keyring(e.to_string()))?;

        let install_pubkey = VerifyingKey::from_bytes(&stored.install_pubkey)
            .map_err(|e| CoreError::Crypto(e.to_string()))?;

        let mentor_slice: [u8; 32] = stored
            .mentor_secret
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::Crypto("Invalid mentor key length".into()))?;
        let mentor_keypair = SigningKey::from_bytes(&mentor_slice);

        Ok(Self {
            install_pubkey,
            mentor_keypair,
            storage_path,
        })
    }

    /// Save keys to DPAPI-protected storage (or plaintext stub on non-Windows).
    pub fn save(&self) -> Result<()> {
        let stored = StoredKeys {
            install_pubkey: self.install_pubkey.to_bytes(),
            mentor_secret: self.mentor_keypair.to_bytes().to_vec(),
        };

        let serialized = serde_json::to_vec(&stored)?;
        let encrypted = Self::dpapi_encrypt(&serialized)?;

        let keys_file = self.storage_path.join("keys.bin");
        std::fs::create_dir_all(&self.storage_path)?;
        std::fs::write(keys_file, encrypted)?;

        Ok(())
    }

    pub fn install_pubkey(&self) -> &VerifyingKey {
        &self.install_pubkey
    }

    pub fn sign_with_mentor(&self, data: &[u8]) -> Signature {
        self.mentor_keypair.sign(data)
    }

    pub fn verify_install_signature(&self, data: &[u8], signature: &Signature) -> bool {
        self.install_pubkey.verify(data, signature).is_ok()
    }

    #[cfg(windows)]
    fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>> {
        use windows::Win32::Foundation::*;
        use windows::Win32::Security::Cryptography::*;

        unsafe {
            let mut input_data = data.to_vec();
            let input = CRYPT_INTEGER_BLOB {
                cbData: input_data.len() as u32,
                pbData: input_data.as_mut_ptr(),
            };

            let mut output = CRYPT_INTEGER_BLOB::default();

            CryptProtectData(
                &input,
                None,
                None,
                None,
                None,
                Default::default(),
                &mut output,
            )
            .map_err(|e| CoreError::Crypto(format!("DPAPI encrypt failed: {}", e)))?;

            let result =
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(HLOCAL(output.pbData as *mut _));

            Ok(result)
        }
    }

    #[cfg(windows)]
    fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>> {
        use windows::Win32::Foundation::*;
        use windows::Win32::Security::Cryptography::*;

        unsafe {
            let mut input_data = data.to_vec();
            let input = CRYPT_INTEGER_BLOB {
                cbData: input_data.len() as u32,
                pbData: input_data.as_mut_ptr(),
            };

            let mut output = CRYPT_INTEGER_BLOB::default();

            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                Default::default(),
                &mut output,
            )
            .map_err(|e| CoreError::Crypto(format!("DPAPI decrypt failed: {}", e)))?;

            let result =
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(HLOCAL(output.pbData as *mut _));

            Ok(result)
        }
    }

    #[cfg(not(windows))]
    fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>> {
        tracing::warn!("DPAPI not available - using plaintext storage (dev only)");
        Ok(data.to_vec())
    }

    #[cfg(not(windows))]
    fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>> {
        tracing::warn!("DPAPI not available - plaintext storage (dev only)");
        Ok(data.to_vec())
    }
}

impl Keyring {
    /// Encrypt bytes for storage (e.g. email password). Uses DPAPI on Windows, plaintext stub elsewhere.
    pub fn encrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        Self::dpapi_encrypt(data)
    }

    /// Decrypt bytes from storage.
    pub fn decrypt_bytes(data: &[u8]) -> Result<Vec<u8>> {
        Self::dpapi_decrypt(data)
    }
}
