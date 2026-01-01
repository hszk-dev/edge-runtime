//! Shared application state.
//!
//! This module provides [`AppState`], which holds shared resources
//! across all HTTP request handlers.

use std::sync::Arc;

use dashmap::DashMap;

use edge_runtime_common::{ExecutionConfig, RuntimeConfig, RuntimeError};
use edge_runtime_core::{CompiledModule, InstanceRunner, WasmEngine};
use edge_runtime_host::{Permissions, create_instance_runner};

/// Shared state across all request handlers.
///
/// This struct is cloned for each request, so it uses `Arc` for shared data.
#[derive(Clone)]
pub struct AppState {
    /// Wasmtime engine (shared across all requests).
    engine: Arc<WasmEngine>,

    /// Instance runner with pre-registered host functions.
    runner: Arc<InstanceRunner>,

    /// Compiled module cache (module_id -> CompiledModule).
    modules: Arc<DashMap<String, Arc<CompiledModule>>>,

    /// Execution configuration.
    exec_config: ExecutionConfig,

    /// Default permissions for functions.
    default_permissions: Permissions,
}

impl AppState {
    /// Create new application state.
    ///
    /// # Arguments
    ///
    /// * `config` - Runtime configuration
    ///
    /// # Errors
    ///
    /// Returns an error if engine or runner creation fails.
    pub fn new(config: &RuntimeConfig) -> Result<Self, RuntimeError> {
        let engine = Arc::new(WasmEngine::new(&config.engine)?);
        let runner = Arc::new(create_instance_runner(Arc::new(engine.inner().clone()))?);

        Ok(Self {
            engine,
            runner,
            modules: Arc::new(DashMap::new()),
            exec_config: config.execution.clone(),
            default_permissions: Permissions::builder().enable_logging().build(),
        })
    }

    /// Get the Wasmtime engine.
    pub fn engine(&self) -> &WasmEngine {
        &self.engine
    }

    /// Get the instance runner.
    pub fn runner(&self) -> &InstanceRunner {
        &self.runner
    }

    /// Get the execution configuration.
    pub fn exec_config(&self) -> &ExecutionConfig {
        &self.exec_config
    }

    /// Get the default permissions.
    pub fn default_permissions(&self) -> &Permissions {
        &self.default_permissions
    }

    /// Load and cache a module from bytes.
    ///
    /// # Arguments
    ///
    /// * `module_id` - Unique identifier for the module
    /// * `wasm_bytes` - WebAssembly binary
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails.
    pub fn load_module(
        &self,
        module_id: &str,
        wasm_bytes: &[u8],
    ) -> Result<Arc<CompiledModule>, RuntimeError> {
        let compiled = CompiledModule::from_bytes(self.engine.inner(), wasm_bytes)?;
        let compiled = Arc::new(compiled);
        self.modules.insert(module_id.to_string(), compiled.clone());
        Ok(compiled)
    }

    /// Load and cache a module from WAT text.
    ///
    /// # Arguments
    ///
    /// * `module_id` - Unique identifier for the module
    /// * `wat` - WebAssembly text format source
    ///
    /// # Errors
    ///
    /// Returns an error if compilation fails.
    pub fn load_module_wat(
        &self,
        module_id: &str,
        wat: &str,
    ) -> Result<Arc<CompiledModule>, RuntimeError> {
        let compiled = CompiledModule::from_wat(self.engine.inner(), wat)?;
        let compiled = Arc::new(compiled);
        self.modules.insert(module_id.to_string(), compiled.clone());
        Ok(compiled)
    }

    /// Get a cached module.
    ///
    /// # Arguments
    ///
    /// * `module_id` - Module identifier
    ///
    /// # Returns
    ///
    /// The compiled module if found, or `None`.
    pub fn get_module(&self, module_id: &str) -> Option<Arc<CompiledModule>> {
        self.modules.get(module_id).map(|v| v.clone())
    }

    /// Remove a module from the cache.
    ///
    /// # Arguments
    ///
    /// * `module_id` - Module identifier
    ///
    /// # Returns
    ///
    /// The removed module if it existed.
    pub fn remove_module(&self, module_id: &str) -> Option<Arc<CompiledModule>> {
        self.modules.remove(module_id).map(|(_, v)| v)
    }

    /// List all cached module IDs.
    pub fn list_modules(&self) -> Vec<String> {
        self.modules.iter().map(|r| r.key().clone()).collect()
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("modules_count", &self.modules.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_creation() {
        let config = RuntimeConfig::default();
        let state = AppState::new(&config).unwrap();
        assert!(state.list_modules().is_empty());
    }

    #[test]
    fn test_load_module_wat() {
        let config = RuntimeConfig::default();
        let state = AppState::new(&config).unwrap();

        let wat = r#"(module (func (export "_start")))"#;
        let module = state.load_module_wat("test", wat).unwrap();
        assert!(!module.content_hash().is_empty());

        assert!(state.get_module("test").is_some());
        assert_eq!(state.list_modules(), vec!["test"]);
    }

    #[test]
    fn test_remove_module() {
        let config = RuntimeConfig::default();
        let state = AppState::new(&config).unwrap();

        let wat = r#"(module (func (export "_start")))"#;
        state.load_module_wat("test", wat).unwrap();

        let removed = state.remove_module("test");
        assert!(removed.is_some());
        assert!(state.get_module("test").is_none());
    }
}
