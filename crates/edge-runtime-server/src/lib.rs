//! HTTP Server for edge-runtime.
//!
//! This crate provides the HTTP interface for executing WebAssembly functions
//! at the edge. It handles:
//!
//! - HTTP request routing
//! - Request/response transformation
//! - WebAssembly module execution
//! - Health and readiness checks
//! - Admin API for module management
//!
//! # Quick Start
//!
//! ```ignore
//! use edge_runtime_server::{EdgeServer, ServerConfig};
//! use edge_runtime_common::RuntimeConfig;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let runtime_config = RuntimeConfig::default();
//!     let server_config = ServerConfig::default();
//!
//!     let server = EdgeServer::new(&runtime_config, server_config)?;
//!     server.run().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod admin;
pub mod handler;
pub mod request;
pub mod response;
pub mod router;
pub mod server;
pub mod state;

pub use admin::{AdminState, build_admin_router};
pub use router::{AdminRouterConfig, build_router_with_admin};
pub use server::{EdgeServer, ServerConfig};
pub use state::AppState;
