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
//!
//! # Security Model
//!
//! All host functions are subject to security controls:
//!
//! 1. **Permissions**: Each function execution has an associated permission set
//!    that determines what operations are allowed.
//! 2. **Rate Limiting**: HTTP requests are rate-limited per execution.
//! 3. **SSRF Protection**: Private/internal network addresses are blocked.

pub mod http_outbound;
pub mod logging;
pub mod permissions;

pub use http_outbound::HttpOutboundHost;
pub use logging::LoggingHost;
pub use permissions::Permissions;
