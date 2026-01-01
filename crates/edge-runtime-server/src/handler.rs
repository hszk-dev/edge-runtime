//! Request handlers for Wasm execution.
//!
//! This module provides HTTP handlers for executing WebAssembly functions
//! and managing the runtime.

use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use tracing::{error, info, instrument};
use uuid::Uuid;

use edge_runtime_common::RuntimeError;
use edge_runtime_core::ExecutionResult;
use edge_runtime_core::store::create_store;

use edge_runtime_core::store::LogEntry;

use crate::response::WasmHttpResponse;
use crate::state::AppState;

/// Convert log entries to JSON-serializable format.
fn logs_to_json(logs: &[LogEntry]) -> Vec<serde_json::Value> {
    logs.iter()
        .map(|l| {
            serde_json::json!({
                "level": l.level.to_string(),
                "message": l.message,
            })
        })
        .collect()
}

/// Execute a Wasm function for an HTTP request.
///
/// This handler:
/// 1. Looks up the module by function_id
/// 2. Creates a new execution store
/// 3. Executes the module's `_start` entry point
/// 4. Returns the execution result as an HTTP response
#[instrument(skip(state), fields(function_id = %function_id))]
pub async fn handle_function(
    State(state): State<AppState>,
    Path(function_id): Path<String>,
) -> impl IntoResponse {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();

    info!(
        request_id = %request_id,
        function_id = %function_id,
        "Handling function request"
    );

    // Get the module
    let module = match state.get_module(&function_id) {
        Some(m) => m,
        None => {
            error!(function_id = %function_id, "Function not found");
            return WasmHttpResponse::error(404, &format!("Function '{}' not found", function_id))
                .into_axum_response();
        }
    };

    // Create execution store
    let mut store = match create_store(state.engine(), state.exec_config(), request_id.clone()) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "Failed to create store");
            return WasmHttpResponse::error(500, "Internal server error").into_axum_response();
        }
    };

    // Execute the function
    let result = state
        .runner()
        .execute_core(&module, &mut store, "_start")
        .await;

    let duration = start.elapsed();

    match result {
        Ok(exec_result) => {
            let logs = &store.data().logs;
            let fuel_consumed = store.data().metrics.fuel_consumed;

            info!(
                request_id = %request_id,
                duration_ms = duration.as_millis(),
                fuel_consumed = fuel_consumed,
                log_count = logs.len(),
                "Request completed"
            );

            match exec_result {
                ExecutionResult::Success => {
                    let response_body = serde_json::json!({
                        "success": true,
                        "logs": logs_to_json(logs),
                        "metrics": {
                            "fuel_consumed": fuel_consumed,
                            "duration_ms": duration.as_millis(),
                        }
                    });

                    WasmHttpResponse::json(200, &response_body.to_string()).into_axum_response()
                }
                ExecutionResult::Trap { message, code } => {
                    let response_body = serde_json::json!({
                        "success": false,
                        "error": {
                            "type": "trap",
                            "message": message,
                            "code": code,
                        },
                        "logs": logs_to_json(logs),
                    });

                    WasmHttpResponse::json(500, &response_body.to_string()).into_axum_response()
                }
            }
        }
        Err(e) => {
            error!(
                request_id = %request_id,
                error = %e,
                duration_ms = duration.as_millis(),
                "Request failed"
            );
            error_to_response(e).into_axum_response()
        }
    }
}

/// Convert RuntimeError to HTTP response.
fn error_to_response(error: RuntimeError) -> WasmHttpResponse {
    match error {
        RuntimeError::ModuleNotFound { module_id } => {
            WasmHttpResponse::error(404, &format!("Module not found: {module_id}"))
        }
        RuntimeError::FuelExhausted => {
            WasmHttpResponse::error(429, "Execution limit exceeded: fuel exhausted")
        }
        RuntimeError::ExecutionTimeout { duration_ms } => {
            WasmHttpResponse::error(504, &format!("Execution timeout after {duration_ms}ms"))
        }
        RuntimeError::MemoryLimitExceeded { limit_mb } => {
            WasmHttpResponse::error(507, &format!("Memory limit exceeded: {limit_mb}MB"))
        }
        RuntimeError::HostFunction(host_err) => {
            WasmHttpResponse::error(500, &format!("Host function error: {host_err}"))
        }
        _ => WasmHttpResponse::error(500, "Internal server error"),
    }
}

/// Health check handler.
///
/// Returns 200 OK if the server is running.
pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Readiness check handler.
///
/// Returns 200 OK if the server is ready to accept requests.
pub async fn readiness_check(State(state): State<AppState>) -> impl IntoResponse {
    // Increment epoch to verify engine is responsive
    state.engine().inner().increment_epoch();

    let body = serde_json::json!({
        "status": "ready",
        "modules_loaded": state.list_modules().len(),
    });

    (StatusCode::OK, axum::Json(body))
}

/// List loaded modules.
pub async fn list_modules(State(state): State<AppState>) -> impl IntoResponse {
    let modules = state.list_modules();
    axum::Json(serde_json::json!({
        "modules": modules,
        "count": modules.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_to_response_not_found() {
        let err = RuntimeError::ModuleNotFound {
            module_id: "test".to_string(),
        };
        let resp = error_to_response(err);
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn test_error_to_response_fuel_exhausted() {
        let err = RuntimeError::FuelExhausted;
        let resp = error_to_response(err);
        assert_eq!(resp.status, 429);
    }

    #[test]
    fn test_error_to_response_timeout() {
        let err = RuntimeError::ExecutionTimeout { duration_ms: 5000 };
        let resp = error_to_response(err);
        assert_eq!(resp.status, 504);
    }
}
