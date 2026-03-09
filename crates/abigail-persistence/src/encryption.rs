use abigail_core::vault::crypto;
use abigail_core::{HybridUnlockProvider, UnlockProvider};
use std::sync::Arc;

use crate::client::{EntityScope, PersistenceError, Result};

#[derive(Clone)]
pub struct ScopedCipher {
    unlock: Arc<dyn UnlockProvider>,
    scope: EntityScope,
}

impl ScopedCipher {
    pub fn new(scope: EntityScope) -> Self {
        Self {
            unlock: Arc::new(HybridUnlockProvider::new()),
            scope,
        }
    }

    fn key(&self) -> Result<[u8; 32]> {
        let root = self
            .unlock
            .root_kek()
            .map_err(|e| PersistenceError::Join(e.to_string()))?;
        Ok(crypto::derive_scope_key(
            &root,
            &format!("persistence:{}", self.scope.label()),
        ))
    }

    pub fn encrypt_bytes(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(crypto::seal(&self.key()?, bytes).map_err(|e| PersistenceError::Join(e.to_string()))?)
    }

    pub fn decrypt_bytes(&self, envelope: &[u8]) -> Result<Vec<u8>> {
        Ok(crypto::open(&self.key()?, envelope)
            .map_err(|e| PersistenceError::Join(e.to_string()))?)
    }

    pub fn encrypt_json<T: serde::Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(value)?;
        self.encrypt_bytes(&bytes)
    }

    pub fn decrypt_json<T: serde::de::DeserializeOwned>(&self, envelope: &[u8]) -> Result<T> {
        let bytes = self.decrypt_bytes(envelope)?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
