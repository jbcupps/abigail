//! Hive: the single authority for secret resolution and provider construction.
//!
//! Entities and the router request providers through the Hive — never touching
//! vault internals directly.

pub mod hive;
pub mod model_registry;
pub mod provider_registry;

pub use hive::{detect_cli_providers_full, is_binary_on_path, BuiltProviders, Hive, HiveConfig};
pub use model_registry::ModelRegistry;
pub use provider_registry::{ProviderKind, ProviderRegistry};
