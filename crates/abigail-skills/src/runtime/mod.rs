//! Runtime backends for skill execution.
//!
//! Native skills run in-process. Untrusted or third-party skills can be run in a WASM/WASI
//! runtime for isolation; all file/network/memory I/O is then routed through capability layers
//! that enforce the sandbox (see [crate::sandbox]).

pub mod wasm;
