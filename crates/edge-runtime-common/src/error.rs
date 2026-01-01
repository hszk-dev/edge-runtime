//! Error types for the edge-runtime.
//!
//! This module defines a hierarchy of error types using `thiserror`:
//! - [`RuntimeError`]: Top-level errors for the runtime
//! - [`HostFunctionError`]: Errors from host function implementations
//! - [`WasiError`]: WASI-related errors

use std::io;

use thiserror::Error;

/// Top-level runtime errors.
///
/// These errors represent failures that can occur during the lifecycle of
/// executing WebAssembly modules, from compilation to execution.
#[derive(Error, Debug)]
pub enum RuntimeError {
    /// The requested module was not found in the cache or storage.
    #[error("Module not found: {module_id}")]
    ModuleNotFound {
        /// The identifier of the module that was not found.
        module_id: String,
    },

    /// WebAssembly compilation failed.
    #[error("Compilation failed: {reason}")]
    CompilationFailed {
        /// Description of the compilation failure.
        reason: String,
    },

    /// Execution exceeded the configured timeout.
    #[error("Execution timeout after {duration_ms}ms")]
    ExecutionTimeout {
        /// The timeout duration in milliseconds.
        duration_ms: u64,
    },

    /// Execution exhausted the configured fuel limit.
    ///
    /// This indicates the WebAssembly code consumed more CPU cycles
    /// than allowed by the fuel metering configuration.
    #[error("Fuel exhausted: CPU limit exceeded")]
    FuelExhausted,

    /// Linear memory allocation exceeded the configured limit.
    #[error("Memory limit exceeded: {limit_mb}MB")]
    MemoryLimitExceeded {
        /// The memory limit in megabytes.
        limit_mb: u32,
    },

    /// A host function returned an error.
    #[error("Host function error: {0}")]
    HostFunction(#[from] HostFunctionError),

    /// WASI operation failed.
    #[error("WASI error: {0}")]
    Wasi(#[from] WasiError),

    /// I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// A WebAssembly trap occurred during execution.
    #[error("Wasm trap: {message}")]
    Trap {
        /// Description of the trap.
        message: String,
    },

    /// Invalid configuration was provided.
    #[error("Invalid configuration: {reason}")]
    InvalidConfig {
        /// Description of the configuration error.
        reason: String,
    },
}

/// Errors from host function implementations.
///
/// These errors occur when host functions (provided by the runtime to
/// WebAssembly modules) fail to complete their operations.
#[derive(Error, Debug)]
pub enum HostFunctionError {
    /// An HTTP request made by the guest module failed.
    #[error("HTTP request failed: {url} (status: {status})")]
    HttpRequestFailed {
        /// The URL that was requested.
        url: String,
        /// The HTTP status code (0 if connection failed).
        status: u16,
    },

    /// The requested operation was denied by the permission system.
    #[error("Permission denied: {resource}")]
    PermissionDenied {
        /// Description of the resource that access was denied to.
        resource: String,
    },

    /// Key-value store operation failed.
    #[error("KV store error: {0}")]
    KvStore(String),

    /// Rate limit for host function calls was exceeded.
    #[error("Rate limit exceeded: {operation}")]
    RateLimitExceeded {
        /// The operation that was rate-limited.
        operation: String,
    },

    /// Invalid argument was passed to a host function.
    #[error("Invalid argument: {reason}")]
    InvalidArgument {
        /// Description of why the argument was invalid.
        reason: String,
    },
}

/// WASI-related errors.
///
/// These errors occur when WASI (WebAssembly System Interface) operations fail.
#[derive(Error, Debug)]
pub enum WasiError {
    /// Failed to initialize WASI context.
    #[error("WASI initialization failed: {reason}")]
    InitializationFailed {
        /// Description of the initialization failure.
        reason: String,
    },

    /// A WASI filesystem operation failed.
    #[error("WASI filesystem error: {operation}")]
    FilesystemError {
        /// The filesystem operation that failed.
        operation: String,
    },

    /// WASI environment configuration error.
    #[error("WASI environment error: {reason}")]
    EnvironmentError {
        /// Description of the environment error.
        reason: String,
    },
}

impl RuntimeError {
    /// Create a new `ModuleNotFound` error.
    pub fn module_not_found(module_id: impl Into<String>) -> Self {
        Self::ModuleNotFound {
            module_id: module_id.into(),
        }
    }

    /// Create a new `CompilationFailed` error.
    pub fn compilation_failed(reason: impl Into<String>) -> Self {
        Self::CompilationFailed {
            reason: reason.into(),
        }
    }

    /// Create a new `Trap` error.
    pub fn trap(message: impl Into<String>) -> Self {
        Self::Trap {
            message: message.into(),
        }
    }

    /// Create a new `InvalidConfig` error.
    pub fn invalid_config(reason: impl Into<String>) -> Self {
        Self::InvalidConfig {
            reason: reason.into(),
        }
    }

    /// Returns `true` if this error indicates the module was not found.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::ModuleNotFound { .. })
    }

    /// Returns `true` if this error indicates a resource limit was exceeded.
    pub fn is_resource_limit(&self) -> bool {
        matches!(
            self,
            Self::FuelExhausted | Self::MemoryLimitExceeded { .. } | Self::ExecutionTimeout { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = RuntimeError::module_not_found("test-module");
        assert_eq!(err.to_string(), "Module not found: test-module");

        let err = RuntimeError::FuelExhausted;
        assert_eq!(err.to_string(), "Fuel exhausted: CPU limit exceeded");
    }

    #[test]
    fn test_error_from_host_function() {
        let host_err = HostFunctionError::PermissionDenied {
            resource: "HTTP access".into(),
        };
        let runtime_err: RuntimeError = host_err.into();

        assert!(matches!(runtime_err, RuntimeError::HostFunction(_)));
    }

    #[test]
    fn test_is_resource_limit() {
        assert!(RuntimeError::FuelExhausted.is_resource_limit());
        assert!(RuntimeError::MemoryLimitExceeded { limit_mb: 128 }.is_resource_limit());
        assert!(RuntimeError::ExecutionTimeout { duration_ms: 100 }.is_resource_limit());
        assert!(!RuntimeError::module_not_found("test").is_resource_limit());
    }

    #[test]
    fn test_is_not_found() {
        assert!(RuntimeError::module_not_found("test").is_not_found());
        assert!(!RuntimeError::FuelExhausted.is_not_found());
    }
}
