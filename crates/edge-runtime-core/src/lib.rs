//! Core Wasmtime runtime for edge-runtime.
//!
//! This crate provides the fundamental WebAssembly execution capabilities:
//! - [`WasmEngine`]: Configured Wasmtime engine with pooling allocator
//! - [`WorkerContext`]: Per-request execution context
//! - [`CompiledModule`]: Compiled WebAssembly module wrapper
//! - [`InstanceRunner`]: Instance lifecycle management
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                     WasmEngine                          │
//! │  (Shared across all requests, thread-safe)              │
//! │  - Pooling Allocator                                    │
//! │  - Compilation settings                                 │
//! └─────────────────────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │                   CompiledModule                        │
//! │  (Cached, shared across instances)                      │
//! │  - Pre-compiled machine code                            │
//! └─────────────────────────────────────────────────────────┘
//!                            │
//!                            ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │            Store<WorkerContext> + Instance              │
//! │  (Per-request, isolated)                                │
//! │  - Fuel metering                                        │
//! │  - Linear memory                                        │
//! │  - Logs and metrics                                     │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod engine;
pub mod instance;
pub mod module;
pub mod store;

pub use engine::WasmEngine;
pub use instance::{ExecutionResult, InstanceRunner};
pub use module::CompiledModule;
pub use store::{ExecutionMetrics, LogEntry, LogLevel, WorkerContext};
