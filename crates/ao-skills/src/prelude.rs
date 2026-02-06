//! Prelude for skill development.

pub use crate::channel::{SkillEvent, TriggerDescriptor, TriggerFrequency, TriggerPriority};
pub use crate::manifest::{
    CapabilityDescriptor, Permission, SecretDescriptor, SkillId, SkillManifest,
};
pub use crate::sandbox::ResourceLimits;
pub use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolMetadata, ToolOutput, ToolParams,
};
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
pub use std::collections::HashMap;
