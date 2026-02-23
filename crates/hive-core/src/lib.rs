use serde::{Deserialize, Serialize};

pub const HIVE_API_VERSION_PREFIX: &str = "/v1";
pub const DEFAULT_HIVE_ADDR: &str = "127.0.0.1:7701";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEnvelope<T> {
    pub ok: bool,
    pub data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityStatus {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BirthPath {
    QuickStart,
    Direct,
    SoulCrystallization,
    SoulForge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecord {
    pub id: String,
    pub status: EntityStatus,
    pub birth_complete: bool,
    pub birth_path: Option<BirthPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartStopEntityRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirthEntityRequest {
    pub id: String,
    pub path: BirthPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiveStatus {
    pub service: String,
    pub api_version: String,
    pub managed_entities: usize,
}
