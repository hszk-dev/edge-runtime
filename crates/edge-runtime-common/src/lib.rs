//! Common types, errors, and utilities for edge-runtime.
//!
//! This crate provides shared functionality used across the edge-runtime workspace:
//! - Error types using `thiserror` for type-safe error handling
//! - Configuration structures for runtime settings
//! - Common type definitions

pub mod config;
pub mod error;

pub use config::{EngineConfig, ExecutionConfig, RuntimeConfig};
pub use error::{HostFunctionError, RuntimeError, WasiError};
