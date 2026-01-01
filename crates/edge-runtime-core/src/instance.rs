//! WebAssembly instance lifecycle management.
//!
//! This module provides [`InstanceRunner`], which handles the complete lifecycle
//! of executing WebAssembly code:
//!
//! 1. Link host functions with the module
//! 2. Instantiate the module with a fresh store
//! 3. Execute the entry point function
//! 4. Collect results and metrics

use std::sync::Arc;
use std::time::Instant;

use tracing::{debug, error, info, instrument, warn};
use wasmtime::component::Linker as ComponentLinker;
use wasmtime::{Engine, Linker, Store, Trap};

use crate::CompiledModule;
use crate::store::{WorkerContext, calculate_fuel_consumed, get_remaining_fuel};
use edge_runtime_common::RuntimeError;

/// Result of executing a WebAssembly module.
#[derive(Debug)]
pub enum ExecutionResult {
    /// Execution completed successfully.
    Success,

    /// Execution completed with a trap (runtime error).
    Trap {
        /// Description of the trap.
        message: String,
        /// Trap code if available.
        code: Option<String>,
    },
}

impl ExecutionResult {
    /// Returns `true` if execution was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, ExecutionResult::Success)
    }

    /// Returns `true` if execution trapped.
    pub fn is_trap(&self) -> bool {
        matches!(self, ExecutionResult::Trap { .. })
    }
}

/// Instance lifecycle manager.
///
/// This struct manages the execution of WebAssembly modules, including:
/// - Linking host functions
/// - Instantiating modules
/// - Executing entry points
/// - Collecting results and metrics
///
/// # Thread Safety
///
/// `InstanceRunner` is thread-safe and can be shared across multiple tasks.
/// Each execution uses its own [`Store`] for isolation.
pub struct InstanceRunner {
    engine: Arc<Engine>,
    linker: Linker<WorkerContext>,
    component_linker: ComponentLinker<WorkerContext>,
}

impl InstanceRunner {
    /// Create a new instance runner.
    ///
    /// # Arguments
    ///
    /// * `engine` - The Wasmtime engine
    pub fn new(engine: Arc<Engine>) -> Self {
        let linker = Linker::new(&engine);
        let component_linker = ComponentLinker::new(&engine);

        Self {
            engine,
            linker,
            component_linker,
        }
    }

    /// Get a mutable reference to the core module linker.
    ///
    /// Use this to register host functions for core modules.
    pub fn linker_mut(&mut self) -> &mut Linker<WorkerContext> {
        &mut self.linker
    }

    /// Get a mutable reference to the component linker.
    ///
    /// Use this to register host functions for components.
    pub fn component_linker_mut(&mut self) -> &mut ComponentLinker<WorkerContext> {
        &mut self.component_linker
    }

    /// Execute a core WebAssembly module.
    ///
    /// # Arguments
    ///
    /// * `module` - The compiled module to execute
    /// * `store` - The store containing execution context
    /// * `entry_point` - Name of the entry point function (e.g., "_start")
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Instantiation fails
    /// - Entry point is not found
    /// - Fuel is exhausted
    #[instrument(skip(self, module, store), fields(entry_point = %entry_point))]
    pub async fn execute_core(
        &self,
        module: &CompiledModule,
        store: &mut Store<WorkerContext>,
        entry_point: &str,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start = Instant::now();
        let initial_fuel = get_remaining_fuel(store).unwrap_or(0);

        debug!("Instantiating core module");

        // Instantiate the module
        let instance = self
            .linker
            .instantiate_async(&mut *store, module.as_core_module())
            .await
            .map_err(|e| RuntimeError::compilation_failed(format!("Instantiation failed: {e}")))?;

        debug!("Module instantiated, looking for entry point");

        // Get the entry point function
        let func = instance
            .get_typed_func::<(), ()>(&mut *store, entry_point)
            .map_err(|_| {
                RuntimeError::module_not_found(format!("Entry point '{entry_point}' not found"))
            })?;

        debug!("Executing entry point");

        // Execute the function
        let result = func.call_async(&mut *store, ()).await;

        // Calculate metrics
        let fuel_consumed = calculate_fuel_consumed(initial_fuel, store);
        store.data_mut().metrics.fuel_consumed = fuel_consumed;
        store.data_mut().finalize_metrics();

        let duration = start.elapsed();

        match result {
            Ok(()) => {
                info!(
                    duration_ms = duration.as_millis(),
                    fuel_consumed = fuel_consumed,
                    "Execution completed successfully"
                );
                Ok(ExecutionResult::Success)
            }
            Err(trap) => {
                let trap_info = extract_trap_info(&trap);

                // Check for fuel exhaustion
                if is_out_of_fuel(&trap) {
                    warn!(
                        duration_ms = duration.as_millis(),
                        fuel_consumed = fuel_consumed,
                        "Execution terminated: fuel exhausted"
                    );
                    return Err(RuntimeError::FuelExhausted);
                }

                error!(
                    duration_ms = duration.as_millis(),
                    fuel_consumed = fuel_consumed,
                    trap_message = %trap_info.0,
                    "Execution trapped"
                );

                Ok(ExecutionResult::Trap {
                    message: trap_info.0,
                    code: trap_info.1,
                })
            }
        }
    }

    /// Execute a WebAssembly component.
    ///
    /// This is the preferred execution method for Component Model modules.
    #[instrument(skip(self, component, store))]
    pub async fn execute_component(
        &self,
        component: &CompiledModule,
        store: &mut Store<WorkerContext>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start = Instant::now();
        let initial_fuel = get_remaining_fuel(store).unwrap_or(0);

        debug!("Instantiating component");

        // Instantiate the component
        let _instance = self
            .component_linker
            .instantiate_async(&mut *store, component.as_component())
            .await
            .map_err(|e| {
                RuntimeError::compilation_failed(format!("Component instantiation failed: {e}"))
            })?;

        // Calculate metrics
        let fuel_consumed = calculate_fuel_consumed(initial_fuel, store);
        store.data_mut().metrics.fuel_consumed = fuel_consumed;
        store.data_mut().finalize_metrics();

        let duration = start.elapsed();

        info!(
            duration_ms = duration.as_millis(),
            fuel_consumed = fuel_consumed,
            "Component instantiated"
        );

        // Note: Actual component execution would depend on the specific interface
        // This is a placeholder for the basic instantiation
        Ok(ExecutionResult::Success)
    }

    /// Get the engine reference.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

/// Extract human-readable trap information.
fn extract_trap_info(error: &wasmtime::Error) -> (String, Option<String>) {
    let message = error.to_string();

    // Try to get the trap code
    let code = error.downcast_ref::<Trap>().map(|trap| format!("{trap:?}"));

    (message, code)
}

/// Check if an error is due to fuel exhaustion.
fn is_out_of_fuel(error: &wasmtime::Error) -> bool {
    error
        .downcast_ref::<Trap>()
        .is_some_and(|trap| *trap == Trap::OutOfFuel)
}

impl std::fmt::Debug for InstanceRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceRunner").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_result_success() {
        let result = ExecutionResult::Success;
        assert!(result.is_success());
        assert!(!result.is_trap());
    }

    #[test]
    fn test_execution_result_trap() {
        let result = ExecutionResult::Trap {
            message: "unreachable".into(),
            code: Some("UnreachableCodeReached".into()),
        };
        assert!(!result.is_success());
        assert!(result.is_trap());
    }
}
