//! Specialized memory capability trait (stub).

use async_trait::async_trait;

use crate::SkillResult;

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
    async fn store(&self, _entry: MemoryEntry) -> SkillResult<String> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
    async fn retrieve(&self, _id: &str) -> SkillResult<Option<MemoryEntry>> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
    async fn search(&self, _query: MemoryQuery) -> SkillResult<Vec<MemorySearchResult>> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}
