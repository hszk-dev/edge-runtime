//! Configuration structures for the edge-runtime.
//!
//! This module defines configuration options for various components:
//! - [`RuntimeConfig`]: Top-level configuration containing all settings
//! - [`EngineConfig`]: Wasmtime engine settings (pooling, caching)
//! - [`ExecutionConfig`]: Per-request execution limits (fuel, memory, timeout)

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Top-level runtime configuration.
///
/// This structure contains all configuration options for the edge-runtime.
/// It can be loaded from files (TOML, JSON) or environment variables.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuntimeConfig {
    /// Wasmtime engine configuration.
    #[serde(default)]
    pub engine: EngineConfig,

    /// Per-request execution configuration.
    #[serde(default)]
    pub execution: ExecutionConfig,
}

/// Wasmtime engine configuration.
///
/// These settings affect the global Wasmtime engine behavior,
/// including memory allocation strategy and compilation caching.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EngineConfig {
    /// Enable pooling allocator for high-performance instance creation.
    ///
    /// When enabled, memory is pre-allocated for a pool of instances,
    /// reducing instantiation time from ~1ms to ~10µs.
    #[serde(default = "defaults::pooling_allocator")]
    pub pooling_allocator: bool,

    /// Maximum concurrent instances in the pool.
    ///
    /// Only effective when `pooling_allocator` is enabled.
    #[serde(default = "defaults::max_instances")]
    pub max_instances: u32,

    /// Memory per instance slot in megabytes.
    ///
    /// This determines the maximum linear memory each instance can use.
    #[serde(default = "defaults::instance_memory_mb")]
    pub instance_memory_mb: u32,

    /// Enable caching of compiled modules.
    ///
    /// When enabled, compiled artifacts are cached to disk,
    /// speeding up subsequent loads of the same module.
    #[serde(default = "defaults::cache_compiled_modules")]
    pub cache_compiled_modules: bool,

    /// Directory for compiled module cache.
    ///
    /// Only effective when `cache_compiled_modules` is enabled.
    #[serde(default)]
    pub cache_dir: Option<String>,

    /// Enable epoch-based interruption.
    ///
    /// This allows interrupting long-running WebAssembly execution
    /// based on time rather than fuel consumption.
    #[serde(default = "defaults::epoch_interruption")]
    pub epoch_interruption: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            pooling_allocator: defaults::pooling_allocator(),
            max_instances: defaults::max_instances(),
            instance_memory_mb: defaults::instance_memory_mb(),
            cache_compiled_modules: defaults::cache_compiled_modules(),
            cache_dir: Some("./cache".into()),
            epoch_interruption: defaults::epoch_interruption(),
        }
    }
}

/// Per-request execution configuration.
///
/// These settings control resource limits for individual WebAssembly executions.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionConfig {
    /// Maximum fuel (CPU instructions) per request.
    ///
    /// Fuel metering provides deterministic CPU limiting.
    /// A typical simple function consumes ~1,000-10,000 fuel.
    /// Complex operations may consume millions.
    #[serde(default = "defaults::max_fuel")]
    pub max_fuel: u64,

    /// Execution timeout in milliseconds.
    ///
    /// This is a hard limit on execution time.
    #[serde(default = "defaults::timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum linear memory in megabytes.
    ///
    /// This limits the memory a single execution can allocate.
    #[serde(default = "defaults::max_memory_mb")]
    pub max_memory_mb: u32,

    /// Enable fuel metering.
    ///
    /// When enabled, CPU usage is tracked and limited by the `max_fuel` setting.
    #[serde(default = "defaults::fuel_metering")]
    pub fuel_metering: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_fuel: defaults::max_fuel(),
            timeout_ms: defaults::timeout_ms(),
            max_memory_mb: defaults::max_memory_mb(),
            fuel_metering: defaults::fuel_metering(),
        }
    }
}

impl ExecutionConfig {
    /// Get the timeout as a `Duration`.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

/// Default value functions for serde.
mod defaults {
    pub const fn pooling_allocator() -> bool {
        true
    }

    pub const fn max_instances() -> u32 {
        1000
    }

    pub const fn instance_memory_mb() -> u32 {
        64
    }

    pub const fn cache_compiled_modules() -> bool {
        true
    }

    pub const fn epoch_interruption() -> bool {
        true
    }

    pub const fn max_fuel() -> u64 {
        10_000_000
    }

    pub const fn timeout_ms() -> u64 {
        100
    }

    pub const fn max_memory_mb() -> u32 {
        128
    }

    pub const fn fuel_metering() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RuntimeConfig::default();

        assert!(config.engine.pooling_allocator);
        assert_eq!(config.engine.max_instances, 1000);
        assert_eq!(config.engine.instance_memory_mb, 64);
        assert!(config.engine.cache_compiled_modules);
        assert!(config.engine.epoch_interruption);

        assert_eq!(config.execution.max_fuel, 10_000_000);
        assert_eq!(config.execution.timeout_ms, 100);
        assert_eq!(config.execution.max_memory_mb, 128);
        assert!(config.execution.fuel_metering);
    }

    #[test]
    fn test_config_serialization() {
        let config = RuntimeConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: RuntimeConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            config.engine.max_instances,
            deserialized.engine.max_instances
        );
        assert_eq!(config.execution.max_fuel, deserialized.execution.max_fuel);
    }

    #[test]
    fn test_execution_timeout() {
        let config = ExecutionConfig {
            timeout_ms: 500,
            ..Default::default()
        };

        assert_eq!(config.timeout(), std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_partial_deserialization() {
        let json = r#"{"engine": {"max_instances": 500}}"#;
        let config: RuntimeConfig = serde_json::from_str(json).unwrap();

        // Explicitly set value
        assert_eq!(config.engine.max_instances, 500);
        // Default values for unspecified fields
        assert!(config.engine.pooling_allocator);
        assert_eq!(config.execution.max_fuel, 10_000_000);
    }
}
