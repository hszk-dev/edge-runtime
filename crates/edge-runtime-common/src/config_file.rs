//! Configuration file structures for the edge-runtime.
//!
//! This module defines structures for TOML configuration files:
//! - [`ConfigFile`]: Top-level configuration file structure
//! - [`ServerConfigFile`]: HTTP server settings
//! - [`AdminConfig`]: Admin API settings
//! - [`ModuleEntry`]: Pre-loaded module definition

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::RuntimeConfig;

/// Top-level configuration file structure.
///
/// This structure represents a complete TOML configuration file
/// that can be loaded at startup.
///
/// # Example
///
/// ```toml
/// [runtime.engine]
/// pooling_allocator = true
/// max_instances = 1000
///
/// [runtime.execution]
/// max_fuel = 10_000_000
/// timeout_ms = 100
///
/// [server]
/// bind_addr = "0.0.0.0:8080"
/// request_timeout_secs = 30
///
/// [admin]
/// enabled = true
/// token = "your-secret-token"
/// prefix = "/admin"
///
/// [[modules]]
/// id = "hello"
/// path = "./modules/hello.wasm"
/// ```
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConfigFile {
    /// Runtime configuration (engine + execution settings).
    #[serde(default)]
    pub runtime: RuntimeConfig,

    /// HTTP server configuration.
    #[serde(default)]
    pub server: ServerConfigFile,

    /// Admin API configuration.
    #[serde(default)]
    pub admin: AdminConfig,

    /// Modules to load at startup.
    #[serde(default)]
    pub modules: Vec<ModuleEntry>,
}

impl ConfigFile {
    /// Load configuration from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigFileError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| ConfigFileError::Io {
            path: path.as_ref().display().to_string(),
            source: e,
        })?;

        Self::from_toml(&content)
    }

    /// Parse configuration from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as TOML.
    pub fn from_toml(content: &str) -> Result<Self, ConfigFileError> {
        toml::from_str(content).map_err(|e| ConfigFileError::Parse {
            message: e.to_string(),
        })
    }
}

/// HTTP server configuration from config file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfigFile {
    /// Bind address (e.g., "0.0.0.0:8080").
    #[serde(default = "defaults::bind_addr")]
    pub bind_addr: String,

    /// Request timeout in seconds.
    #[serde(default = "defaults::request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Enable graceful shutdown.
    #[serde(default = "defaults::graceful_shutdown")]
    pub graceful_shutdown: bool,
}

impl Default for ServerConfigFile {
    fn default() -> Self {
        Self {
            bind_addr: defaults::bind_addr(),
            request_timeout_secs: defaults::request_timeout_secs(),
            graceful_shutdown: defaults::graceful_shutdown(),
        }
    }
}

/// Admin API configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdminConfig {
    /// Enable Admin API.
    #[serde(default)]
    pub enabled: bool,

    /// Authentication token (required when enabled).
    ///
    /// Clients must include this token in the `X-Admin-Token` header.
    pub token: Option<String>,

    /// URL prefix for Admin API endpoints.
    #[serde(default = "defaults::admin_prefix")]
    pub prefix: String,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: None,
            prefix: defaults::admin_prefix(),
        }
    }
}

impl AdminConfig {
    /// Check if Admin API is properly configured.
    ///
    /// Returns `true` if enabled and token is set.
    pub fn is_configured(&self) -> bool {
        self.enabled && self.token.is_some()
    }
}

/// A module entry to load at startup.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleEntry {
    /// Unique identifier for the module.
    ///
    /// This ID is used in the `/functions/:id` endpoint.
    pub id: String,

    /// Path to the WebAssembly module file.
    pub path: String,
}

/// Configuration file errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigFileError {
    /// Failed to read configuration file.
    #[error("Failed to read config file '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse configuration file.
    #[error("Failed to parse config file: {message}")]
    Parse { message: String },
}

/// Default value functions for serde.
mod defaults {
    pub fn bind_addr() -> String {
        "0.0.0.0:8080".to_string()
    }

    pub const fn request_timeout_secs() -> u64 {
        30
    }

    pub const fn graceful_shutdown() -> bool {
        true
    }

    pub fn admin_prefix() -> String {
        "/admin".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_file() {
        let config = ConfigFile::default();

        assert_eq!(config.server.bind_addr, "0.0.0.0:8080");
        assert_eq!(config.server.request_timeout_secs, 30);
        assert!(config.server.graceful_shutdown);
        assert!(!config.admin.enabled);
        assert!(config.admin.token.is_none());
        assert_eq!(config.admin.prefix, "/admin");
        assert!(config.modules.is_empty());
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
            [server]
            bind_addr = "127.0.0.1:3000"
        "#;

        let config = ConfigFile::from_toml(toml).unwrap();

        assert_eq!(config.server.bind_addr, "127.0.0.1:3000");
        // Defaults applied
        assert_eq!(config.server.request_timeout_secs, 30);
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
            [runtime.engine]
            pooling_allocator = true
            max_instances = 500

            [runtime.execution]
            max_fuel = 5_000_000
            timeout_ms = 50

            [server]
            bind_addr = "0.0.0.0:9000"
            request_timeout_secs = 60
            graceful_shutdown = false

            [admin]
            enabled = true
            token = "secret-token"
            prefix = "/api/admin"

            [[modules]]
            id = "hello"
            path = "./hello.wasm"

            [[modules]]
            id = "echo"
            path = "./echo.wasm"
        "#;

        let config = ConfigFile::from_toml(toml).unwrap();

        assert_eq!(config.runtime.engine.max_instances, 500);
        assert_eq!(config.runtime.execution.max_fuel, 5_000_000);
        assert_eq!(config.server.bind_addr, "0.0.0.0:9000");
        assert_eq!(config.server.request_timeout_secs, 60);
        assert!(!config.server.graceful_shutdown);
        assert!(config.admin.enabled);
        assert_eq!(config.admin.token, Some("secret-token".to_string()));
        assert_eq!(config.admin.prefix, "/api/admin");
        assert_eq!(config.modules.len(), 2);
        assert_eq!(config.modules[0].id, "hello");
        assert_eq!(config.modules[1].path, "./echo.wasm");
    }

    #[test]
    fn test_admin_config_is_configured() {
        let mut admin = AdminConfig::default();
        assert!(!admin.is_configured());

        admin.enabled = true;
        assert!(!admin.is_configured());

        admin.token = Some("token".to_string());
        assert!(admin.is_configured());
    }

    #[test]
    fn test_parse_invalid_toml() {
        let invalid = "this is not valid toml [";
        let result = ConfigFile::from_toml(invalid);
        assert!(result.is_err());
    }
}
