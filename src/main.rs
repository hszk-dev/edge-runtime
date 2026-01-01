//! Edge Runtime CLI entry point.
//!
//! This is the main entry point for running the edge runtime HTTP server.

use std::net::SocketAddr;

use anyhow::Context;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use edge_runtime_common::RuntimeConfig;
use edge_runtime_server::{EdgeServer, ServerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,edge_runtime=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Edge Runtime");

    // Load configuration
    let runtime_config = RuntimeConfig::default();

    // Parse server address from environment or use default
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .context("Invalid BIND_ADDR format. Expected format: 'host:port' (e.g., '0.0.0.0:8080')")?;

    let server_config = ServerConfig::default().with_bind_addr(bind_addr);

    info!(bind_addr = %bind_addr, "Configuration loaded");

    // Create and run server
    let server = EdgeServer::new(&runtime_config, server_config)?;

    info!("Server initialized. Available endpoints:");
    info!("  GET  /health              - Health check");
    info!("  GET  /ready               - Readiness check");
    info!("  GET  /modules             - List loaded modules");
    info!("  GET  /functions/:id       - Execute function (no body)");
    info!("  POST /functions/:id       - Execute function (with body)");

    server.run().await?;

    Ok(())
}
