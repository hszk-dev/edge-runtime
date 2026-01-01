//! Wasmtime engine configuration and creation.
//!
//! The [`WasmEngine`] is the foundation of the runtime. It is:
//! - Thread-safe and shared across all requests
//! - Configured with pooling allocator for fast instantiation
//! - Set up with fuel metering and epoch interruption for resource limiting

use std::sync::Arc;

use tracing::{debug, info};
use wasmtime::{Config, Engine, InstanceAllocationStrategy, PoolingAllocationConfig};

use edge_runtime_common::{EngineConfig, RuntimeError};

/// Thread-safe WebAssembly engine wrapper.
///
/// This struct wraps a Wasmtime [`Engine`] configured for high-performance
/// serverless execution. The engine is designed to be shared across all
/// requests and contains no per-request state.
///
/// # Configuration
///
/// The engine is configured with:
/// - **Pooling Allocator**: Pre-allocates memory for instances, reducing
///   instantiation time from ~1ms to ~10µs
/// - **Fuel Metering**: Enables deterministic CPU limiting
/// - **Epoch Interruption**: Enables time-based interruption as a backup
/// - **Async Support**: Allows non-blocking host function execution
///
/// # Example
///
/// ```ignore
/// use edge_runtime_common::EngineConfig;
/// use edge_runtime_core::WasmEngine;
///
/// let config = EngineConfig::default();
/// let engine = WasmEngine::new(&config)?;
/// ```
#[derive(Clone)]
pub struct WasmEngine {
    engine: Arc<Engine>,
    config: EngineConfig,
}

impl WasmEngine {
    /// Create a new WebAssembly engine with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The Wasmtime configuration is invalid
    /// - The pooling allocator cannot be initialized
    /// - Cache configuration fails
    pub fn new(config: &EngineConfig) -> Result<Self, RuntimeError> {
        let mut wasmtime_config = Config::new();

        // Enable async support for non-blocking host functions
        wasmtime_config.async_support(true);

        // Enable fuel metering for deterministic CPU limiting
        wasmtime_config.consume_fuel(true);

        // Enable epoch-based interruption as a backup timeout mechanism
        if config.epoch_interruption {
            wasmtime_config.epoch_interruption(true);
        }

        // Enable Cranelift optimizations
        wasmtime_config.cranelift_opt_level(wasmtime::OptLevel::Speed);

        // Configure pooling allocator for high-performance instantiation
        if config.pooling_allocator {
            let pooling_config = Self::create_pooling_config(config);

            wasmtime_config
                .allocation_strategy(InstanceAllocationStrategy::Pooling(pooling_config));

            info!(
                max_instances = config.max_instances,
                instance_memory_mb = config.instance_memory_mb,
                "Pooling allocator enabled"
            );
        }

        // Enable module caching if configured
        if config.cache_compiled_modules {
            if let Some(ref cache_dir) = config.cache_dir {
                // Note: In production, you would configure the cache properly
                // For now, we just log that caching is requested
                debug!(cache_dir = %cache_dir, "Module caching configured");
            }
        }

        let engine = Engine::new(&wasmtime_config).map_err(|e| {
            RuntimeError::invalid_config(format!("Failed to create Wasmtime engine: {e}"))
        })?;

        info!("Wasmtime engine initialized");

        Ok(Self {
            engine: Arc::new(engine),
            config: config.clone(),
        })
    }

    /// Create pooling allocation configuration.
    fn create_pooling_config(config: &EngineConfig) -> PoolingAllocationConfig {
        let mut pooling = PoolingAllocationConfig::default();

        // Total number of component instances that can be allocated
        pooling.total_component_instances(config.max_instances);

        // Total number of core module instances
        pooling.total_core_instances(config.max_instances);

        // Total number of memories across all instances
        pooling.total_memories(config.max_instances);

        // Total number of tables across all instances
        pooling.total_tables(config.max_instances);

        // Maximum size of a single memory in bytes
        let max_memory_bytes = (config.instance_memory_mb as usize) * 1024 * 1024;
        pooling.max_memory_size(max_memory_bytes);

        pooling
    }

    /// Get a reference to the inner Wasmtime engine.
    pub fn inner(&self) -> &Engine {
        &self.engine
    }

    /// Get the engine configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Increment the epoch counter.
    ///
    /// This should be called periodically (e.g., every 1ms) to enable
    /// epoch-based interruption for long-running executions.
    pub fn increment_epoch(&self) {
        self.engine.increment_epoch();
    }

    /// Check if the pooling allocator is enabled.
    pub fn is_pooling_enabled(&self) -> bool {
        self.config.pooling_allocator
    }
}

impl std::fmt::Debug for WasmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmEngine")
            .field("pooling_allocator", &self.config.pooling_allocator)
            .field("max_instances", &self.config.max_instances)
            .field("instance_memory_mb", &self.config.instance_memory_mb)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation_default() {
        let config = EngineConfig::default();
        let engine = WasmEngine::new(&config);

        assert!(engine.is_ok());
        let engine = engine.unwrap();
        assert!(engine.is_pooling_enabled());
    }

    #[test]
    fn test_engine_creation_no_pooling() {
        let config = EngineConfig {
            pooling_allocator: false,
            ..Default::default()
        };
        let engine = WasmEngine::new(&config);

        assert!(engine.is_ok());
        let engine = engine.unwrap();
        assert!(!engine.is_pooling_enabled());
    }

    #[test]
    fn test_engine_epoch_increment() {
        let config = EngineConfig::default();
        let engine = WasmEngine::new(&config).unwrap();

        // Should not panic
        engine.increment_epoch();
        engine.increment_epoch();
    }

    #[test]
    fn test_engine_debug() {
        let config = EngineConfig::default();
        let engine = WasmEngine::new(&config).unwrap();

        let debug_str = format!("{engine:?}");
        assert!(debug_str.contains("WasmEngine"));
        assert!(debug_str.contains("pooling_allocator"));
    }
}
