use std::collections::HashMap;

use tokio::sync::RwLock;

use crate::types::TokenInfo;

/// In-memory token cache keyed by service ID.
///
/// Holds resolved credentials so providers aren't called on every request.
/// No disk persistence — refresh tokens go in SecretsVault, this is purely
/// a runtime cache.
pub struct TokenCache {
    entries: RwLock<HashMap<String, TokenInfo>>,
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Get a cached credential if it exists and hasn't expired.
    pub async fn get(&self, service_id: &str) -> Option<TokenInfo> {
        let cache = self.entries.read().await;
        let entry = cache.get(service_id)?;
        if entry.is_expired() {
            None
        } else {
            Some(entry.clone())
        }
    }

    /// Store a credential in the cache.
    pub async fn put(&self, service_id: &str, info: TokenInfo) {
        let mut cache = self.entries.write().await;
        cache.insert(service_id.to_string(), info);
    }

    /// Remove a cached credential.
    pub async fn remove(&self, service_id: &str) {
        let mut cache = self.entries.write().await;
        cache.remove(service_id);
    }

    /// Clear all cached credentials.
    pub async fn clear(&self) {
        let mut cache = self.entries.write().await;
        cache.clear();
    }

    /// Number of cached entries (including possibly expired ones).
    pub async fn len(&self) -> usize {
        let cache = self.entries.read().await;
        cache.len()
    }

    /// Whether the cache is empty.
    pub async fn is_empty(&self) -> bool {
        let cache = self.entries.read().await;
        cache.is_empty()
    }
}
