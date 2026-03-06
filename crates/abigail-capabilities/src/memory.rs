//! Specialized memory capability traits (stubs).

use async_trait::async_trait;

#[derive(Debug, Clone, Copy)]
pub enum MemoryType {
    KeyValue,
    Vector,
    Graph,
    Document,
    Hybrid,
}

#[derive(Debug, Clone)]
pub struct MemorySystemInfo {
    pub id: String,
    pub memory_type: MemoryType,
}

#[derive(Debug, Clone)]
pub struct MemoryEntry;

#[derive(Debug, Clone)]
pub struct MemoryQuery;

#[derive(Debug, Clone)]
pub struct MemorySearchResult;

#[async_trait]
pub trait SpecializedMemoryCapability: Send + Sync {
    fn info(&self) -> MemorySystemInfo;
    async fn store(&self, entry: MemoryEntry) -> anyhow::Result<String>;
    async fn retrieve(&self, id: &str) -> anyhow::Result<Option<MemoryEntry>>;
    async fn search(&self, query: MemoryQuery) -> anyhow::Result<Vec<MemorySearchResult>>;
}
