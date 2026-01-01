//! Common types, errors, and utilities for edge-runtime.
//!
//! This crate provides shared functionality used across the edge-runtime workspace:
//! - Error types using `thiserror` for type-safe error handling
//! - Configuration structures for runtime settings
//! - Configuration file structures for TOML-based configuration
//! - Common type definitions

pub mod config;
pub mod config_file;
pub mod error;

pub use config::{EngineConfig, ExecutionConfig, RuntimeConfig};
pub use config_file::{AdminConfig, ConfigFile, ConfigFileError, ModuleEntry, ServerConfigFile};
pub use error::{HostFunctionError, RuntimeError, WasiError};
