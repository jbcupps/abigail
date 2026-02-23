//! Re-export from the standalone `abigail-identity` crate.
//!
//! All types and the `IdentityManager` struct now live in `crates/abigail-identity`.
//! This file exists only for backwards compatibility with existing `use crate::identity_manager::*`
//! imports throughout the Tauri app.

pub use abigail_identity::*;
