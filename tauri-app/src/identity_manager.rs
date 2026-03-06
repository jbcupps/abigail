//! Re-export from the standalone `abigail-identity` crate.
//!
//! All types and the `IdentityManager` struct now live in `crates/abigail-identity`.
//! This file exists only for backwards compatibility with existing `use crate::identity_manager::*`
//! imports throughout the Tauri app.
//!
//! Runtime verification note (paper Sections 22-27):
//! IdentityManager initialization now relies on the session-verified vault KEK
//! path in `abigail-identity` and will fail fast into recovery mode when
//! sentinel/key verification fails.

pub use abigail_identity::*;
