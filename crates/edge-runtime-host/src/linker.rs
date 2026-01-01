//! Host function registration for Wasmtime linkers.
//!
//! This module provides functions to register host functions on Wasmtime linkers,
//! enabling WebAssembly modules to call into the host runtime.

use edge_runtime_common::RuntimeError;
use edge_runtime_core::store::WorkerContext;
use tracing::warn;
use wasmtime::{Caller, Linker};

use crate::logging::{LoggingHost, level_from_i32};

/// Register all standard host functions on a core module linker.
///
/// This registers the following host functions:
/// - `env::log` - Logging function for guest code
///
/// # Arguments
///
/// * `linker` - The Wasmtime linker to register functions on
///
/// # Errors
///
/// Returns an error if function registration fails.
pub fn register_all(linker: &mut Linker<WorkerContext>) -> Result<(), RuntimeError> {
    register_logging(linker)?;
    Ok(())
}

/// Register the logging host function.
///
/// Registers `env::log(level: i32, ptr: i32, len: i32)` which allows guest
/// code to emit logs at various levels (debug, info, warn, error).
///
/// # Memory Protocol
///
/// The guest passes:
/// - `level`: Log level (0=debug, 1=info, 2=warn, 3=error)
/// - `ptr`: Pointer to the message string in guest memory
/// - `len`: Length of the message in bytes (UTF-8)
pub fn register_logging(linker: &mut Linker<WorkerContext>) -> Result<(), RuntimeError> {
    linker
        .func_wrap(
            "env",
            "log",
            |mut caller: Caller<'_, WorkerContext>, level: i32, ptr: i32, len: i32| {
                // Validate pointer and length are non-negative
                if ptr < 0 || len < 0 {
                    warn!(
                        ptr = ptr,
                        len = len,
                        "Invalid pointer or length (negative value)"
                    );
                    return;
                }

                let Some(memory) = caller
                    .get_export("memory")
                    .and_then(wasmtime::Extern::into_memory)
                else {
                    warn!("Memory export not found in guest module");
                    return;
                };

                // Read message from guest memory and convert to owned String
                // to avoid borrow checker issues with caller.data_mut()
                #[allow(clippy::cast_sign_loss)]
                let message = {
                    let data = memory.data(&caller);
                    let start = ptr as usize;
                    let Some(end) = start.checked_add(len as usize) else {
                        warn!(ptr = ptr, len = len, "Pointer + length overflow");
                        return;
                    };

                    // Bounds check
                    if end > data.len() {
                        warn!(
                            start = start,
                            end = end,
                            memory_size = data.len(),
                            "Memory access out of bounds"
                        );
                        return;
                    }

                    std::str::from_utf8(&data[start..end])
                        .unwrap_or("<invalid utf8>")
                        .to_string()
                };

                LoggingHost::log(caller.data_mut(), level_from_i32(level), &message);
            },
        )
        .map_err(|e| {
            RuntimeError::invalid_config(format!("Failed to register log function: {e}"))
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use edge_runtime_common::EngineConfig;
    use edge_runtime_core::WasmEngine;

    #[test]
    fn test_register_logging() {
        let config = EngineConfig::default();
        let engine = WasmEngine::new(&config).unwrap();
        let mut linker = Linker::new(engine.inner());

        let result = register_logging(&mut linker);
        assert!(result.is_ok());
    }

    #[test]
    fn test_register_all() {
        let config = EngineConfig::default();
        let engine = WasmEngine::new(&config).unwrap();
        let mut linker = Linker::new(engine.inner());

        let result = register_all(&mut linker);
        assert!(result.is_ok());
    }
}
