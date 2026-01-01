//! Per-request execution context and store management.
//!
//! This module provides:
//! - [`WorkerContext`]: Per-request state accessible from host functions
//! - [`LogEntry`] and [`LogLevel`]: Structured logging from guest code
//! - [`ExecutionMetrics`]: Performance metrics for each execution

use std::time::{Duration, Instant};

use wasmtime::Store;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::WasmEngine;
use edge_runtime_common::{ExecutionConfig, RuntimeError};

/// Per-request execution context.
///
/// This struct holds all state specific to a single WebAssembly execution.
/// It is created for each request and destroyed after the execution completes.
///
/// Host functions can access this context through the [`wasmtime::Caller`] API.
///
/// # Contents
///
/// - `wasi`: WASI context for system interface calls
/// - `table`: Resource table for component model resources
/// - `request_id`: Unique identifier for tracing
/// - `logs`: Collected log entries from guest code
/// - `metrics`: Execution performance metrics
pub struct WorkerContext {
    /// WASI context for system interface.
    wasi: WasiCtx,

    /// Resource table for component model.
    table: ResourceTable,

    /// Unique request identifier for tracing.
    pub request_id: String,

    /// Logs collected from guest code.
    pub logs: Vec<LogEntry>,

    /// Execution metrics.
    pub metrics: ExecutionMetrics,

    /// Execution start time.
    start_time: Instant,
}

/// A single log entry from guest code.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Log level (debug, info, warn, error).
    pub level: LogLevel,

    /// Log message content.
    pub message: String,

    /// Timestamp when the log was recorded.
    pub timestamp: Instant,
}

/// Log level for guest logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Debug-level messages.
    Debug,
    /// Informational messages.
    Info,
    /// Warning messages.
    Warn,
    /// Error messages.
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Execution performance metrics.
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetrics {
    /// Fuel consumed during execution.
    pub fuel_consumed: u64,

    /// Memory used in bytes.
    pub memory_used_bytes: usize,

    /// Total execution duration.
    pub duration: Option<Duration>,
}

impl WorkerContext {
    /// Create a new worker context with the given request ID.
    ///
    /// # Arguments
    ///
    /// * `request_id` - Unique identifier for this execution (for tracing)
    pub fn new(request_id: String) -> Self {
        let table = ResourceTable::new();

        // Build WASI context with minimal permissions
        // In production, this would be configured based on the function's manifest
        let wasi = WasiCtxBuilder::new()
            // Inherit stdout/stderr for debugging (can be disabled in production)
            .inherit_stdout()
            .inherit_stderr()
            .build();

        Self {
            wasi,
            table,
            request_id,
            logs: Vec::new(),
            metrics: ExecutionMetrics::default(),
            start_time: Instant::now(),
        }
    }

    /// Add a log entry.
    pub fn log(&mut self, level: LogLevel, message: String) {
        self.logs.push(LogEntry {
            level,
            message,
            timestamp: Instant::now(),
        });
    }

    /// Get elapsed time since execution started.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Finalize metrics after execution.
    pub fn finalize_metrics(&mut self) {
        self.metrics.duration = Some(self.start_time.elapsed());
    }
}

// Implement WasiView for component model integration
impl WasiView for WorkerContext {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

/// Create a new Wasmtime store with the given configuration.
///
/// # Arguments
///
/// * `engine` - The shared Wasmtime engine
/// * `config` - Execution configuration (fuel limits, etc.)
/// * `request_id` - Unique request identifier
///
/// # Errors
///
/// Returns an error if fuel cannot be set on the store.
pub fn create_store(
    engine: &WasmEngine,
    config: &ExecutionConfig,
    request_id: String,
) -> Result<Store<WorkerContext>, RuntimeError> {
    let context = WorkerContext::new(request_id);
    let mut store = Store::new(engine.inner(), context);

    // Set fuel limit if metering is enabled
    if config.fuel_metering {
        store
            .set_fuel(config.max_fuel)
            .map_err(|e| RuntimeError::invalid_config(format!("Failed to set fuel: {e}")))?;
    }

    // Set epoch deadline for timeout-based interruption
    // The deadline is relative to current epoch; use timeout_ms as ticks
    // (assuming 1 epoch increment per millisecond from background task)
    if engine.config().epoch_interruption {
        store.set_epoch_deadline(config.timeout_ms);
    }

    Ok(store)
}

/// Get remaining fuel from a store.
pub fn get_remaining_fuel(store: &Store<WorkerContext>) -> Option<u64> {
    store.get_fuel().ok()
}

/// Calculate fuel consumed.
pub fn calculate_fuel_consumed(initial_fuel: u64, store: &Store<WorkerContext>) -> u64 {
    let remaining = get_remaining_fuel(store).unwrap_or(0);
    initial_fuel.saturating_sub(remaining)
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_runtime_common::EngineConfig;

    #[test]
    fn test_worker_context_creation() {
        let ctx = WorkerContext::new("test-request-123".into());

        assert_eq!(ctx.request_id, "test-request-123");
        assert!(ctx.logs.is_empty());
        assert_eq!(ctx.metrics.fuel_consumed, 0);
    }

    #[test]
    fn test_worker_context_logging() {
        let mut ctx = WorkerContext::new("test".into());

        ctx.log(LogLevel::Info, "Hello".into());
        ctx.log(LogLevel::Error, "World".into());

        assert_eq!(ctx.logs.len(), 2);
        assert_eq!(ctx.logs[0].level, LogLevel::Info);
        assert_eq!(ctx.logs[0].message, "Hello");
        assert_eq!(ctx.logs[1].level, LogLevel::Error);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Debug.to_string(), "DEBUG");
        assert_eq!(LogLevel::Info.to_string(), "INFO");
        assert_eq!(LogLevel::Warn.to_string(), "WARN");
        assert_eq!(LogLevel::Error.to_string(), "ERROR");
    }

    #[test]
    fn test_store_creation() {
        let engine_config = EngineConfig {
            pooling_allocator: false, // Disable for simpler test
            ..Default::default()
        };
        let engine = WasmEngine::new(&engine_config).unwrap();
        let exec_config = ExecutionConfig::default();

        let store = create_store(&engine, &exec_config, "test-123".into());
        assert!(store.is_ok());
    }

    #[test]
    fn test_store_fuel() {
        let engine_config = EngineConfig {
            pooling_allocator: false,
            ..Default::default()
        };
        let engine = WasmEngine::new(&engine_config).unwrap();
        let exec_config = ExecutionConfig {
            max_fuel: 1000,
            fuel_metering: true,
            ..Default::default()
        };

        let store = create_store(&engine, &exec_config, "test".into()).unwrap();
        let remaining = get_remaining_fuel(&store);

        assert_eq!(remaining, Some(1000));
    }
}
