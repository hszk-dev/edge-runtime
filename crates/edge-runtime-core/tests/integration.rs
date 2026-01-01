//! Integration tests for edge-runtime-core.
//!
//! These tests verify the complete execution pipeline:
//! - WAT compilation to module
//! - Store creation with fuel metering
//! - Host function registration
//! - Instance execution
//! - Result and metrics collection

use std::sync::Arc;

use edge_runtime_common::{EngineConfig, ExecutionConfig};
use edge_runtime_core::store::{LogLevel, create_store};
use edge_runtime_core::{CompiledModule, ExecutionResult, InstanceRunner, WasmEngine};
use edge_runtime_host::linker::register_all;

// ============================================================================
// Test: Basic Execution
// ============================================================================

#[tokio::test]
async fn test_basic_execution() {
    let wat = r#"
        (module
            (func (export "_start"))
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    let exec_config = ExecutionConfig::default();
    let mut store = create_store(&engine, &exec_config, "test-basic".into()).unwrap();

    let result = runner
        .execute_core(&compiled, &mut store, "_start")
        .await
        .unwrap();

    assert!(result.is_success());
}

// ============================================================================
// Test: Fuel Consumption
// ============================================================================

#[tokio::test]
async fn test_fuel_consumption() {
    let wat = r#"
        (module
            (func (export "_start")
                (local $i i32)
                (local.set $i (i32.const 0))
                (block $break
                    (loop $continue
                        (local.set $i (i32.add (local.get $i) (i32.const 1)))
                        (br_if $continue (i32.lt_u (local.get $i) (i32.const 100)))
                    )
                )
            )
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    let exec_config = ExecutionConfig {
        max_fuel: 100_000,
        fuel_metering: true,
        ..Default::default()
    };
    let mut store = create_store(&engine, &exec_config, "test-fuel".into()).unwrap();

    let result = runner
        .execute_core(&compiled, &mut store, "_start")
        .await
        .unwrap();

    assert!(result.is_success());
    assert!(
        store.data().metrics.fuel_consumed > 0,
        "Expected fuel to be consumed, got 0"
    );
}

// ============================================================================
// Test: Fuel Exhaustion
// ============================================================================

#[tokio::test]
async fn test_fuel_exhaustion() {
    let wat = r#"
        (module
            (func (export "_start")
                (loop $forever
                    (br $forever)
                )
            )
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    // Very low fuel to trigger exhaustion quickly
    let exec_config = ExecutionConfig {
        max_fuel: 1000,
        fuel_metering: true,
        ..Default::default()
    };
    let mut store = create_store(&engine, &exec_config, "test-exhaustion".into()).unwrap();

    let result = runner.execute_core(&compiled, &mut store, "_start").await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        edge_runtime_common::RuntimeError::FuelExhausted
    ));
}

// ============================================================================
// Test: Host Function Logging
// ============================================================================

#[tokio::test]
async fn test_host_function_logging() {
    let wat = r#"
        (module
            (import "env" "log" (func $log (param i32 i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "Hello from Wasm")

            (func (export "_start")
                (call $log (i32.const 1) (i32.const 0) (i32.const 15))
            )
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let mut runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    // Register all host functions
    register_all(runner.linker_mut()).unwrap();

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    let exec_config = ExecutionConfig::default();
    let mut store = create_store(&engine, &exec_config, "test-logging".into()).unwrap();

    let result = runner
        .execute_core(&compiled, &mut store, "_start")
        .await
        .unwrap();

    assert!(result.is_success());

    // Verify log was captured
    let logs = &store.data().logs;
    assert_eq!(logs.len(), 1, "Expected 1 log entry, got {}", logs.len());
    assert_eq!(logs[0].message, "Hello from Wasm");
    assert_eq!(logs[0].level, LogLevel::Info);
}

// ============================================================================
// Test: Trap Handling
// ============================================================================

#[tokio::test]
async fn test_trap_unreachable() {
    let wat = r#"
        (module
            (func (export "_start")
                unreachable
            )
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    let exec_config = ExecutionConfig::default();
    let mut store = create_store(&engine, &exec_config, "test-trap".into()).unwrap();

    let result = runner
        .execute_core(&compiled, &mut store, "_start")
        .await
        .unwrap();

    assert!(result.is_trap(), "Expected trap, got {result:?}");
    if let ExecutionResult::Trap { message, code } = result {
        // Wasmtime returns "UnreachableCodeReached" as the trap code
        assert!(
            message.contains("wasm backtrace") || code.as_deref() == Some("UnreachableCodeReached"),
            "Expected wasm trap, got message: {message} (code: {code:?})"
        );
    }
}

// ============================================================================
// Test: Multiple Logs
// ============================================================================

#[tokio::test]
async fn test_multiple_logs() {
    let wat = r#"
        (module
            (import "env" "log" (func $log (param i32 i32 i32)))
            (memory (export "memory") 1)
            (data (i32.const 0) "First message")
            (data (i32.const 20) "Second message")
            (data (i32.const 40) "Error message")

            (func (export "_start")
                ;; Log at Info level (1)
                (call $log (i32.const 1) (i32.const 0) (i32.const 13))
                ;; Log at Debug level (0)
                (call $log (i32.const 0) (i32.const 20) (i32.const 14))
                ;; Log at Error level (3)
                (call $log (i32.const 3) (i32.const 40) (i32.const 13))
            )
        )
    "#;

    let engine_config = EngineConfig {
        pooling_allocator: false,
        epoch_interruption: false,
        ..Default::default()
    };
    let engine = WasmEngine::new(&engine_config).unwrap();
    let mut runner = InstanceRunner::new(Arc::new(engine.inner().clone()));

    // Register all host functions
    register_all(runner.linker_mut()).unwrap();

    let compiled = CompiledModule::from_wat(engine.inner(), wat).unwrap();

    let exec_config = ExecutionConfig::default();
    let mut store = create_store(&engine, &exec_config, "test-multi-log".into()).unwrap();

    let result = runner
        .execute_core(&compiled, &mut store, "_start")
        .await
        .unwrap();

    assert!(result.is_success());

    let logs = &store.data().logs;
    assert_eq!(logs.len(), 3);

    assert_eq!(logs[0].message, "First message");
    assert_eq!(logs[0].level, LogLevel::Info);

    assert_eq!(logs[1].message, "Second message");
    assert_eq!(logs[1].level, LogLevel::Debug);

    assert_eq!(logs[2].message, "Error message");
    assert_eq!(logs[2].level, LogLevel::Error);
}
