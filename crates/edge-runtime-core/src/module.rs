//! WebAssembly module compilation and caching.
//!
//! This module provides [`CompiledModule`], a wrapper around Wasmtime's [`Module`]
//! that handles compilation, serialization, and deserialization of WebAssembly modules.
//!
//! # Compilation Strategies
//!
//! - **JIT**: Compile from Wasm bytes at runtime (slower cold start)
//! - **AOT**: Pre-compile and serialize to disk (fast cold start)
//!
//! For production edge workloads, AOT compilation is recommended.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::time::Instant;

use tracing::{debug, info, instrument};
use wasmtime::component::Component;
use wasmtime::{Engine, Module};

use edge_runtime_common::RuntimeError;

/// A compiled WebAssembly module.
///
/// This struct wraps a Wasmtime [`Module`] or [`Component`] with additional metadata
/// for caching and debugging purposes.
///
/// # Thread Safety
///
/// `CompiledModule` is thread-safe and can be shared across multiple instances.
/// The underlying Wasmtime module is also thread-safe.
#[derive(Clone)]
pub struct CompiledModule {
    /// The compiled Wasmtime module.
    inner: ModuleKind,

    /// SHA256-like hash of the original Wasm bytes.
    content_hash: String,

    /// When this module was compiled.
    compiled_at: Instant,
}

/// The kind of compiled module (Core Module or Component).
#[derive(Clone)]
enum ModuleKind {
    /// A core WebAssembly module.
    Core(Module),
    /// A WebAssembly component (Component Model).
    Component(Component),
}

impl CompiledModule {
    /// Compile a core module from WebAssembly bytes.
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine to use for compilation
    /// * `bytes` - The raw WebAssembly bytes
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails (e.g., invalid Wasm).
    #[instrument(skip(engine, bytes), fields(bytes_len = bytes.len()))]
    pub fn from_bytes(engine: &Engine, bytes: &[u8]) -> Result<Self, RuntimeError> {
        let start = Instant::now();

        // Validate Wasm magic number
        Self::validate_wasm_header(bytes)?;

        let module = Module::new(engine, bytes).map_err(|e| {
            RuntimeError::compilation_failed(format!("Core module compilation failed: {e}"))
        })?;

        let content_hash = compute_hash(bytes);
        let duration = start.elapsed();

        info!(
            content_hash = %content_hash,
            duration_ms = duration.as_millis(),
            "Core module compiled"
        );

        Ok(Self {
            inner: ModuleKind::Core(module),
            content_hash,
            compiled_at: Instant::now(),
        })
    }

    /// Compile a component from WebAssembly component bytes.
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine to use for compilation
    /// * `bytes` - The raw WebAssembly component bytes
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails.
    #[instrument(skip(engine, bytes), fields(bytes_len = bytes.len()))]
    pub fn from_component_bytes(engine: &Engine, bytes: &[u8]) -> Result<Self, RuntimeError> {
        let start = Instant::now();

        Self::validate_wasm_header(bytes)?;

        let component = Component::new(engine, bytes).map_err(|e| {
            RuntimeError::compilation_failed(format!("Component compilation failed: {e}"))
        })?;

        let content_hash = compute_hash(bytes);
        let duration = start.elapsed();

        info!(
            content_hash = %content_hash,
            duration_ms = duration.as_millis(),
            "Component compiled"
        );

        Ok(Self {
            inner: ModuleKind::Component(component),
            content_hash,
            compiled_at: Instant::now(),
        })
    }

    /// Load a pre-compiled module from disk.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it deserializes pre-compiled machine code.
    /// Only load artifacts that were compiled by the same version of Wasmtime.
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine (must match compilation settings)
    /// * `path` - Path to the pre-compiled artifact (`.cwasm`)
    ///
    /// # Errors
    ///
    /// Returns an error if the artifact cannot be loaded or is incompatible.
    #[allow(unsafe_code)]
    #[instrument(skip(engine, path))]
    pub fn from_precompiled(engine: &Engine, path: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let path = path.as_ref();
        let start = Instant::now();

        // SAFETY: We trust artifacts compiled by our AOT pipeline
        let module = unsafe { Module::deserialize_file(engine, path) }.map_err(|e| {
            RuntimeError::compilation_failed(format!(
                "Failed to load precompiled module from {}: {e}",
                path.display()
            ))
        })?;

        // Extract hash from filename convention: {hash}.cwasm
        let content_hash = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let duration = start.elapsed();

        debug!(
            path = %path.display(),
            content_hash = %content_hash,
            duration_us = duration.as_micros(),
            "Precompiled module loaded"
        );

        Ok(Self {
            inner: ModuleKind::Core(module),
            content_hash,
            compiled_at: Instant::now(),
        })
    }

    /// Load a pre-compiled component from disk.
    #[allow(unsafe_code)]
    #[instrument(skip(engine, path))]
    pub fn from_precompiled_component(
        engine: &Engine,
        path: impl AsRef<Path>,
    ) -> Result<Self, RuntimeError> {
        let path = path.as_ref();
        let start = Instant::now();

        let component = unsafe { Component::deserialize_file(engine, path) }.map_err(|e| {
            RuntimeError::compilation_failed(format!(
                "Failed to load precompiled component from {}: {e}",
                path.display()
            ))
        })?;

        let content_hash = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let duration = start.elapsed();

        debug!(
            path = %path.display(),
            duration_us = duration.as_micros(),
            "Precompiled component loaded"
        );

        Ok(Self {
            inner: ModuleKind::Component(component),
            content_hash,
            compiled_at: Instant::now(),
        })
    }

    /// Serialize the compiled module for AOT caching.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn serialize(&self) -> Result<Vec<u8>, RuntimeError> {
        match &self.inner {
            ModuleKind::Core(module) => module.serialize().map_err(|e| {
                RuntimeError::compilation_failed(format!("Module serialization failed: {e}"))
            }),
            ModuleKind::Component(component) => component.serialize().map_err(|e| {
                RuntimeError::compilation_failed(format!("Component serialization failed: {e}"))
            }),
        }
    }

    /// Get the content hash of the original Wasm bytes.
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    /// Get when this module was compiled.
    pub fn compiled_at(&self) -> Instant {
        self.compiled_at
    }

    /// Check if this is a component (vs core module).
    pub fn is_component(&self) -> bool {
        matches!(self.inner, ModuleKind::Component(_))
    }

    /// Get the inner core module.
    ///
    /// # Panics
    ///
    /// Panics if this is a component, not a core module.
    pub fn as_core_module(&self) -> &Module {
        match &self.inner {
            ModuleKind::Core(module) => module,
            ModuleKind::Component(_) => panic!("Expected core module, got component"),
        }
    }

    /// Get the inner component.
    ///
    /// # Panics
    ///
    /// Panics if this is a core module, not a component.
    pub fn as_component(&self) -> &Component {
        match &self.inner {
            ModuleKind::Component(component) => component,
            ModuleKind::Core(_) => panic!("Expected component, got core module"),
        }
    }

    /// Compile a core module from WAT (WebAssembly Text Format).
    ///
    /// This is primarily for testing purposes.
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine to use for compilation
    /// * `wat` - The WAT source code
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails.
    #[instrument(skip(engine, wat))]
    pub fn from_wat(engine: &Engine, wat: &str) -> Result<Self, RuntimeError> {
        let start = Instant::now();

        let module = Module::new(engine, wat).map_err(|e| {
            RuntimeError::compilation_failed(format!("WAT compilation failed: {e}"))
        })?;

        let content_hash = compute_hash(wat.as_bytes());
        let duration = start.elapsed();

        info!(
            content_hash = %content_hash,
            duration_ms = duration.as_millis(),
            "WAT module compiled"
        );

        Ok(Self {
            inner: ModuleKind::Core(module),
            content_hash,
            compiled_at: Instant::now(),
        })
    }

    /// Validate WebAssembly header (magic number).
    fn validate_wasm_header(bytes: &[u8]) -> Result<(), RuntimeError> {
        if bytes.len() < 8 {
            return Err(RuntimeError::compilation_failed(
                "Invalid Wasm: file too small",
            ));
        }

        // Check magic number: \0asm
        if &bytes[0..4] != b"\0asm" {
            return Err(RuntimeError::compilation_failed(
                "Invalid Wasm: bad magic number",
            ));
        }

        Ok(())
    }
}

impl std::fmt::Debug for CompiledModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledModule")
            .field("content_hash", &self.content_hash)
            .field("is_component", &self.is_component())
            .finish_non_exhaustive()
    }
}

/// Compute a hash of the given bytes.
fn compute_hash(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasmEngine;
    use edge_runtime_common::EngineConfig;

    // Minimal valid Wasm module (empty module)
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    #[test]
    fn test_validate_wasm_header_valid() {
        assert!(CompiledModule::validate_wasm_header(MINIMAL_WASM).is_ok());
    }

    #[test]
    fn test_validate_wasm_header_too_small() {
        let result = CompiledModule::validate_wasm_header(&[0x00, 0x61]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_wasm_header_bad_magic() {
        let bad_wasm = &[0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let result = CompiledModule::validate_wasm_header(bad_wasm);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_hash() {
        let hash1 = compute_hash(b"hello");
        let hash2 = compute_hash(b"hello");
        let hash3 = compute_hash(b"world");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 16); // 64-bit hex
    }

    #[test]
    fn test_module_compilation() {
        let engine_config = EngineConfig {
            pooling_allocator: false,
            ..Default::default()
        };
        let engine = WasmEngine::new(&engine_config).unwrap();

        let module = CompiledModule::from_bytes(engine.inner(), MINIMAL_WASM);
        assert!(module.is_ok());

        let module = module.unwrap();
        assert!(!module.is_component());
        assert!(!module.content_hash().is_empty());
    }

    #[test]
    fn test_module_debug() {
        let engine_config = EngineConfig {
            pooling_allocator: false,
            ..Default::default()
        };
        let engine = WasmEngine::new(&engine_config).unwrap();
        let module = CompiledModule::from_bytes(engine.inner(), MINIMAL_WASM).unwrap();

        let debug_str = format!("{module:?}");
        assert!(debug_str.contains("CompiledModule"));
        assert!(debug_str.contains("content_hash"));
    }
}
