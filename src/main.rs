//! Edge Runtime CLI entry point.
//!
//! This is the main entry point for running the edge runtime HTTP server.
//!
//! # Usage
//!
//! ```bash
//! # Start with default settings
//! edge-runtime
//!
//! # Load a specific module
//! edge-runtime --wasm ./hello.wasm
//!
//! # Load modules from a directory
//! edge-runtime --modules-dir ./modules/
//!
//! # Use a configuration file
//! edge-runtime --config ./edge-runtime.toml
//!
//! # Enable Admin API
//! edge-runtime --enable-admin --admin-token secret
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use edge_runtime_common::{AdminConfig, ConfigFile, RuntimeConfig, ServerConfigFile};
use edge_runtime_server::{EdgeServer, ServerConfig};

/// Edge Runtime - High-density serverless edge runtime
#[derive(Parser, Debug)]
#[command(name = "edge-runtime")]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Path to TOML configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Load a single Wasm module at startup
    #[arg(long, value_name = "FILE")]
    wasm: Option<PathBuf>,

    /// Directory to scan for .wasm files at startup
    #[arg(long, value_name = "DIR")]
    modules_dir: Option<PathBuf>,

    /// Port to bind (overrides config/env)
    #[arg(short, long, value_name = "PORT", env = "PORT")]
    port: Option<u16>,

    /// Bind address (overrides config/env)
    #[arg(short, long, value_name = "ADDR", env = "BIND_ADDR")]
    bind: Option<String>,

    /// Admin API token (required if admin API is enabled)
    #[arg(long, env = "ADMIN_TOKEN")]
    admin_token: Option<String>,

    /// Enable admin API
    #[arg(long)]
    enable_admin: bool,
}

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

    // Parse CLI arguments
    let cli = Cli::parse();

    // Build configuration from CLI, config file, and defaults
    let (runtime_config, server_config, admin_config) = build_config(&cli)?;

    info!(bind_addr = %server_config.bind_addr, "Configuration loaded");

    // Create server
    let mut server = EdgeServer::new(&runtime_config, server_config.clone())?;

    // Enable Admin API if configured
    if admin_config.is_configured() {
        server = server.with_admin(
            admin_config.prefix.clone(),
            admin_config.token.clone().unwrap(),
        );
    }

    // Load modules from CLI options
    load_modules_from_cli(&cli, server.state())?;

    // Log admin API status
    if admin_config.is_configured() {
        info!(prefix = %admin_config.prefix, "Admin API enabled");
    }

    info!("Server initialized. Available endpoints:");
    info!("  GET  /health              - Health check");
    info!("  GET  /ready               - Readiness check");
    info!("  GET  /modules             - List loaded modules");
    info!("  GET  /functions/:id       - Execute function (no body)");
    info!("  POST /functions/:id       - Execute function (with body)");

    if admin_config.is_configured() {
        info!("Admin API endpoints (requires X-Admin-Token header):");
        info!(
            "  POST   {}/modules      - Upload module",
            admin_config.prefix
        );
        info!(
            "  GET    {}/modules      - List modules (detailed)",
            admin_config.prefix
        );
        info!(
            "  GET    {}/modules/:id  - Get module info",
            admin_config.prefix
        );
        info!(
            "  DELETE {}/modules/:id  - Delete module",
            admin_config.prefix
        );
    }

    server.run().await?;

    Ok(())
}

/// Build configuration from CLI arguments, config file, and defaults.
///
/// Priority: CLI > Environment Variables > Config File > Defaults
fn build_config(cli: &Cli) -> anyhow::Result<(RuntimeConfig, ServerConfig, AdminConfig)> {
    // 1. Load config file if specified
    let config_file = if let Some(path) = &cli.config {
        info!(path = ?path, "Loading configuration file");
        ConfigFile::from_file(path).context("Failed to load config file")?
    } else {
        ConfigFile::default()
    };

    // 2. RuntimeConfig from config file
    let runtime_config = config_file.runtime;

    // 3. ServerConfig: CLI > config file > defaults
    let bind_addr = resolve_bind_addr(cli, &config_file.server)?;
    let server_config = ServerConfig::default()
        .with_bind_addr(bind_addr)
        .with_timeout(config_file.server.request_timeout_secs);

    // 4. AdminConfig: CLI > config file
    let admin_config = AdminConfig {
        enabled: cli.enable_admin || config_file.admin.enabled,
        token: cli.admin_token.clone().or(config_file.admin.token),
        prefix: config_file.admin.prefix,
    };

    Ok((runtime_config, server_config, admin_config))
}

/// Resolve bind address from CLI, environment, or config file.
fn resolve_bind_addr(cli: &Cli, server_config: &ServerConfigFile) -> anyhow::Result<SocketAddr> {
    // CLI --bind is highest priority
    if let Some(bind) = &cli.bind {
        return bind.parse().context("Invalid --bind address");
    }

    // CLI --port with default IP
    if let Some(port) = cli.port {
        return Ok(SocketAddr::from(([0, 0, 0, 0], port)));
    }

    // Config file
    server_config
        .bind_addr
        .parse()
        .context("Invalid bind_addr in config")
}

/// Load modules from CLI options.
fn load_modules_from_cli(cli: &Cli, state: &edge_runtime_server::AppState) -> anyhow::Result<()> {
    // Load from config file modules (already loaded in build_config)
    // This will be handled when we integrate with the full config loading

    // Load from --wasm option
    if let Some(wasm_path) = &cli.wasm {
        let id = wasm_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default");
        let bytes = std::fs::read(wasm_path)
            .with_context(|| format!("Failed to read module: {}", wasm_path.display()))?;
        state.load_module(id, &bytes)?;
        info!(id = %id, path = ?wasm_path, "Loaded module from --wasm");
    }

    // Load from --modules-dir option
    if let Some(dir) = &cli.modules_dir {
        if !dir.is_dir() {
            anyhow::bail!("--modules-dir path is not a directory: {}", dir.display());
        }

        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read modules directory: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "wasm") {
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("Failed to read module: {}", path.display()))?;
                state.load_module(id, &bytes)?;
                info!(id = %id, path = ?path, "Loaded module from --modules-dir");
            }
        }
    }

    Ok(())
}
