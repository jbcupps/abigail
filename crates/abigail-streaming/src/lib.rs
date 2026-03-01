//! Streaming trait boundary for Abigail's dual-layer messaging.
//!
//! Provides the `StreamBroker` trait that abstracts over messaging backends.
//! Phase 1 ships with `MemoryBroker` (tokio broadcast channels).
//! Phase 3 adds `IggyBroker` for persistent, multi-consumer streaming.

pub mod broker;
pub mod memory_broker;
pub mod types;

pub use broker::StreamBroker;
pub use memory_broker::MemoryBroker;
pub use types::{StreamMessage, SubscriptionHandle, TopicConfig};
