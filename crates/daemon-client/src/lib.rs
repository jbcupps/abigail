//! HTTP clients for hive-daemon and entity-daemon.
//!
//! Used by the Tauri desktop app in Daemon runtime mode and by the
//! daemon test harness.

mod entity;
mod hive;

pub use entity::{ChatStreamEvent, EntityClient};
pub use hive::HiveDaemonClient;
