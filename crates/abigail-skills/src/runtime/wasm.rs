//! WASM/WASI runtime path for untrusted skills.
//!
//! This module provides the integration point for running skills inside a WebAssembly sandbox
//! (e.g. [Wasmtime](https://docs.wasmtime.dev/)) so that untrusted skill code cannot access
//! host resources except via declared capabilities. I/O (file, network, memory) must be
//! routed through the capability layer and [crate::sandbox::SkillSandbox::check_permission].
//!
//! To implement:
//! 1. Add optional dependency `wasmtime` (and `wasmtime-wasi`) under a `wasm` feature.
//! 2. Implement a loader that compiles a skill's WASM module once and reuses it.
//! 3. Implement a bridge from [crate::skill::Skill] (or a WASM-specific trait) that
//!    invokes exported tool functions and maps params/results to/from the WASM linear memory.
//! 4. Enforce [crate::sandbox::ResourceLimits] (timeout, memory cap) via Wasmtime config.
//!
//! Other runtimes (e.g. wasmer, wasm3) can be added behind additional features or
//! a runtime selector in skill manifest (e.g. `runtime = "wasmtime"`).

/// Placeholder for a WASM-based skill runtime.
///
/// When the `wasm` feature is enabled, this can hold a Wasmtime engine and store
/// so that skills built as WASM modules can be loaded and executed with strict
/// resource limits and capability checks.
#[derive(Debug)]
pub struct WasmRuntimeStub {
    _private: (),
}

impl WasmRuntimeStub {
    /// Create a stub; real implementation will initialize the WASM engine here.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for WasmRuntimeStub {
    fn default() -> Self {
        Self::new()
    }
}
