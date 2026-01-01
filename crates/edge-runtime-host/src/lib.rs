//! Host functions implementation for edge-runtime.
//!
//! This crate provides the host-side implementations of the interfaces
//! defined in the WIT files. Guest WebAssembly components import these
//! interfaces to interact with the outside world.
//!
//! # Interfaces
//!
//! - [`logging`]: Structured logging from guest code
//! - [`http_outbound`]: Outbound HTTP requests with security controls
//! - [`permissions`]: Capability-based security configuration
//! - [`linker`]: Host function registration for Wasmtime linkers
//!
//! # Security Model
//!
//! All host functions are subject to security controls:
//!
//! 1. **Permissions**: Each function execution has an associated permission set
//!    that determines what operations are allowed.
//! 2. **Rate Limiting**: HTTP requests are rate-limited per execution.
//! 3. **SSRF Protection**: Private/internal network addresses are blocked.
//!
//! # Quick Start
//!
//! Use [`create_instance_runner`] to create an [`InstanceRunner`] with all
//! standard host functions pre-registered:
//!
//! ```ignore
//! use edge_runtime_host::create_instance_runner;
//!
//! let runner = create_instance_runner(engine)?;
//! ```

pub mod http_outbound;
pub mod linker;
pub mod logging;
pub mod permissions;

pub use http_outbound::HttpOutboundHost;
pub use logging::LoggingHost;
pub use permissions::Permissions;

use std::sync::Arc;

use edge_runtime_common::RuntimeError;
use edge_runtime_core::InstanceRunner;
use wasmtime::Engine;

/// Create an [`InstanceRunner`] with all standard host functions registered.
///
/// This is a convenience function that creates an `InstanceRunner` and
/// registers all host functions from this crate.
///
/// # Arguments
///
/// * `engine` - The Wasmtime engine
///
/// # Errors
///
/// Returns an error if host function registration fails.
pub fn create_instance_runner(engine: Arc<Engine>) -> Result<InstanceRunner, RuntimeError> {
    let mut runner = InstanceRunner::new(engine);
    linker::register_all(runner.linker_mut())?;
    Ok(runner)
}
