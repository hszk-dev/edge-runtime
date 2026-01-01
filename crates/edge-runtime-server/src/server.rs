//! HTTP server implementation.
//!
//! This module provides the main [`EdgeServer`] struct for running
//! the edge runtime HTTP server.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing::info;

use edge_runtime_common::{RuntimeConfig, RuntimeError};

use crate::router::build_router;
use crate::state::AppState;

/// Configuration for the HTTP server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind the server.
    pub bind_addr: SocketAddr,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Enable graceful shutdown on SIGTERM/SIGINT.
    pub graceful_shutdown: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".parse().unwrap(),
            request_timeout_secs: 30,
            graceful_shutdown: true,
        }
    }
}

impl ServerConfig {
    /// Create a new server config with custom bind address.
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Create a new server config with custom timeout.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.request_timeout_secs = secs;
        self
    }

    /// Get the request timeout as Duration.
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }
}

/// Edge Runtime HTTP server.
///
/// This is the main entry point for running the HTTP server.
///
/// # Example
///
/// ```ignore
/// use edge_runtime_server::{EdgeServer, ServerConfig};
/// use edge_runtime_common::RuntimeConfig;
///
/// let runtime_config = RuntimeConfig::default();
/// let server_config = ServerConfig::default();
///
/// let server = EdgeServer::new(&runtime_config, server_config)?;
///
/// // Load a module
/// server.state().load_module_wat("hello", r#"(module (func (export "_start")))"#)?;
///
/// // Run the server
/// server.run().await?;
/// ```
pub struct EdgeServer {
    /// Application state.
    state: AppState,
    /// Server configuration.
    config: ServerConfig,
}

impl EdgeServer {
    /// Create a new server instance.
    ///
    /// # Arguments
    ///
    /// * `runtime_config` - Configuration for the Wasmtime runtime
    /// * `server_config` - Configuration for the HTTP server
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime cannot be initialized.
    pub fn new(
        runtime_config: &RuntimeConfig,
        server_config: ServerConfig,
    ) -> Result<Self, RuntimeError> {
        let state = AppState::new(runtime_config)?;

        Ok(Self {
            state,
            config: server_config,
        })
    }

    /// Get a reference to the application state.
    ///
    /// Use this to load modules before starting the server.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Get the server configuration.
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Run the server until shutdown.
    ///
    /// This will block until the server is shut down via signal
    /// (SIGTERM/SIGINT) if graceful shutdown is enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot bind to the address.
    pub async fn run(self) -> Result<(), RuntimeError> {
        let app = build_router(self.state, self.config.request_timeout());

        let listener = TcpListener::bind(&self.config.bind_addr)
            .await
            .map_err(|e| RuntimeError::invalid_config(format!("Failed to bind: {e}")))?;

        info!(addr = %self.config.bind_addr, "Starting HTTP server");

        if self.config.graceful_shutdown {
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .map_err(|e| RuntimeError::invalid_config(format!("Server error: {e}")))?;
        } else {
            axum::serve(listener, app)
                .await
                .map_err(|e| RuntimeError::invalid_config(format!("Server error: {e}")))?;
        }

        info!("Server shutdown complete");
        Ok(())
    }

    /// Start the server and return a handle for testing.
    ///
    /// The server binds to an ephemeral port (127.0.0.1:0) and
    /// returns a handle that can be used to get the actual address
    /// and shut down the server.
    pub async fn start_test(runtime_config: &RuntimeConfig) -> Result<TestHandle, RuntimeError> {
        let state = AppState::new(runtime_config)?;
        let app = build_router(state.clone(), Duration::from_secs(30));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| RuntimeError::invalid_config(format!("Failed to bind: {e}")))?;

        let addr = listener
            .local_addr()
            .map_err(|e| RuntimeError::invalid_config(format!("Failed to get addr: {e}")))?;

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
        });

        Ok(TestHandle {
            addr,
            state,
            shutdown_tx: Some(shutdown_tx),
            handle,
        })
    }
}

/// Handle for a test server instance.
///
/// Use this to interact with and shut down a test server.
pub struct TestHandle {
    /// The address the server is bound to.
    addr: SocketAddr,
    /// Application state (for loading modules).
    state: AppState,
    /// Shutdown signal sender.
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Server task handle.
    handle: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl TestHandle {
    /// Get the server address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the server URL.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Get the application state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Shutdown the server gracefully.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

/// Wait for shutdown signal (SIGTERM or SIGINT).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    info!("Shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_addr.port(), 8080);
        assert_eq!(config.request_timeout_secs, 30);
        assert!(config.graceful_shutdown);
    }

    #[test]
    fn test_server_config_builder() {
        let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let config = ServerConfig::default()
            .with_bind_addr(addr)
            .with_timeout(60);

        assert_eq!(config.bind_addr.port(), 3000);
        assert_eq!(config.request_timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_server_creation() {
        let runtime_config = RuntimeConfig::default();
        let server_config = ServerConfig::default();
        let server = EdgeServer::new(&runtime_config, server_config);
        assert!(server.is_ok());
    }
}
