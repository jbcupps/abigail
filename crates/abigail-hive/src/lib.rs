//! Hive: the single authority for secret resolution and provider construction.
//!
//! Entities and the router request providers through the Hive — never touching
//! vault internals directly.

pub mod hive;
pub mod provider_registry;

pub use hive::{BuiltProviders, Hive, HiveConfig};
pub use provider_registry::{ProviderKind, ProviderRegistry};
