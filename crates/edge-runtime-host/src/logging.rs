//! Logging host function implementation.
//!
//! This module provides the host-side implementation of the logging interface,
//! allowing guest components to emit structured logs that are captured by
//! the runtime.

use edge_runtime_core::store::{LogEntry, LogLevel, WorkerContext};
use tracing::{debug, error, info, warn};

/// Host implementation for the logging interface.
///
/// This struct provides the logging capabilities to guest components.
/// Logs are both:
/// 1. Stored in the [`WorkerContext`] for later retrieval
/// 2. Emitted via the `tracing` crate for observability
pub struct LoggingHost;

impl LoggingHost {
    /// Log a message at the specified level.
    ///
    /// This is the main entry point for the logging interface.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The worker context to store logs in
    /// * `level` - The log level
    /// * `message` - The log message
    pub fn log(ctx: &mut WorkerContext, level: LogLevel, message: &str) {
        // Store in context for later retrieval
        ctx.logs.push(LogEntry {
            level,
            message: message.to_string(),
            timestamp: std::time::Instant::now(),
        });

        // Also emit via tracing for observability
        let request_id = &ctx.request_id;
        match level {
            LogLevel::Debug => debug!(request_id, guest_log = true, "{}", message),
            LogLevel::Info => info!(request_id, guest_log = true, "{}", message),
            LogLevel::Warn => warn!(request_id, guest_log = true, "{}", message),
            LogLevel::Error => error!(request_id, guest_log = true, "{}", message),
        }
    }

    /// Convenience function for debug-level logging.
    pub fn log_debug(ctx: &mut WorkerContext, message: &str) {
        Self::log(ctx, LogLevel::Debug, message);
    }

    /// Convenience function for info-level logging.
    pub fn log_info(ctx: &mut WorkerContext, message: &str) {
        Self::log(ctx, LogLevel::Info, message);
    }

    /// Convenience function for warn-level logging.
    pub fn log_warn(ctx: &mut WorkerContext, message: &str) {
        Self::log(ctx, LogLevel::Warn, message);
    }

    /// Convenience function for error-level logging.
    pub fn log_error(ctx: &mut WorkerContext, message: &str) {
        Self::log(ctx, LogLevel::Error, message);
    }
}

/// Convert a numeric log level to [`LogLevel`].
///
/// This is used when receiving log levels from Wasm as integers.
///
/// # Arguments
///
/// * `level` - Numeric log level (0=debug, 1=info, 2=warn, 3=error)
///
/// # Returns
///
/// The corresponding [`LogLevel`], defaulting to Info for unknown values.
pub fn level_from_i32(level: i32) -> LogLevel {
    match level {
        0 => LogLevel::Debug,
        2 => LogLevel::Warn,
        3 => LogLevel::Error,
        _ => LogLevel::Info, // 1 and unknown values default to Info
    }
}

/// Convert a [`LogLevel`] to a numeric value.
pub fn level_to_i32(level: LogLevel) -> i32 {
    match level {
        LogLevel::Debug => 0,
        LogLevel::Info => 1,
        LogLevel::Warn => 2,
        LogLevel::Error => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logging_stores_in_context() {
        let mut ctx = WorkerContext::new("test-123".into());

        LoggingHost::log(&mut ctx, LogLevel::Info, "Hello");
        LoggingHost::log(&mut ctx, LogLevel::Error, "World");

        assert_eq!(ctx.logs.len(), 2);
        assert_eq!(ctx.logs[0].message, "Hello");
        assert_eq!(ctx.logs[0].level, LogLevel::Info);
        assert_eq!(ctx.logs[1].message, "World");
        assert_eq!(ctx.logs[1].level, LogLevel::Error);
    }

    #[test]
    fn test_convenience_functions() {
        let mut ctx = WorkerContext::new("test".into());

        LoggingHost::log_debug(&mut ctx, "debug");
        LoggingHost::log_info(&mut ctx, "info");
        LoggingHost::log_warn(&mut ctx, "warn");
        LoggingHost::log_error(&mut ctx, "error");

        assert_eq!(ctx.logs.len(), 4);
        assert_eq!(ctx.logs[0].level, LogLevel::Debug);
        assert_eq!(ctx.logs[1].level, LogLevel::Info);
        assert_eq!(ctx.logs[2].level, LogLevel::Warn);
        assert_eq!(ctx.logs[3].level, LogLevel::Error);
    }

    #[test]
    fn test_level_from_i32() {
        assert_eq!(level_from_i32(0), LogLevel::Debug);
        assert_eq!(level_from_i32(1), LogLevel::Info);
        assert_eq!(level_from_i32(2), LogLevel::Warn);
        assert_eq!(level_from_i32(3), LogLevel::Error);
        assert_eq!(level_from_i32(99), LogLevel::Info); // Unknown defaults to Info
    }

    #[test]
    fn test_level_to_i32() {
        assert_eq!(level_to_i32(LogLevel::Debug), 0);
        assert_eq!(level_to_i32(LogLevel::Info), 1);
        assert_eq!(level_to_i32(LogLevel::Warn), 2);
        assert_eq!(level_to_i32(LogLevel::Error), 3);
    }
}
