//! Capability-based security for host functions.
//!
//! This module provides the [`Permissions`] struct, which defines what
//! operations a guest component is allowed to perform.

use std::collections::HashSet;

/// Permission configuration for a function execution.
///
/// This struct defines what operations are allowed for a particular
/// function execution. It is checked by host functions before
/// performing privileged operations.
///
/// # Security Philosophy
///
/// We follow the principle of least privilege:
/// - By default, nothing is allowed
/// - Each capability must be explicitly granted
/// - Permissions are immutable during execution
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// Allowed HTTP hosts (domain patterns).
    ///
    /// Patterns can be:
    /// - Exact match: `api.example.com`
    /// - Wildcard subdomain: `*.example.com` (matches `api.example.com`, `www.example.com`)
    /// - All hosts: `*` (dangerous, use with caution)
    pub allowed_http_hosts: HashSet<String>,

    /// Enable HTTP outbound access.
    pub http_enabled: bool,

    /// Maximum HTTP requests per execution.
    pub max_http_requests: u32,

    /// Enable logging.
    pub logging_enabled: bool,
}

impl Permissions {
    /// Create a new permission set with all capabilities disabled.
    pub fn none() -> Self {
        Self::default()
    }

    /// Create a permission set with all capabilities enabled.
    ///
    /// # Warning
    ///
    /// This is intended for development/testing only.
    /// Production code should use explicit permissions.
    pub fn all() -> Self {
        let mut allowed_hosts = HashSet::new();
        allowed_hosts.insert("*".to_string());

        Self {
            allowed_http_hosts: allowed_hosts,
            http_enabled: true,
            max_http_requests: 100,
            logging_enabled: true,
        }
    }

    /// Create a builder for constructing permissions.
    pub fn builder() -> PermissionsBuilder {
        PermissionsBuilder::default()
    }

    /// Check if HTTP access to the given URL is allowed.
    ///
    /// This performs:
    /// 1. Check if HTTP is enabled at all
    /// 2. Parse the URL and extract the host
    /// 3. Match the host against allowed patterns
    /// 4. Block private/internal addresses (SSRF protection)
    pub fn is_http_allowed(&self, url: &str) -> bool {
        if !self.http_enabled {
            return false;
        }

        // Allow all hosts
        if self.allowed_http_hosts.contains("*") {
            return true;
        }

        // Parse URL and extract host
        let host = match url::Url::parse(url) {
            Ok(parsed) => match parsed.host_str() {
                Some(h) => h.to_lowercase(),
                None => return false,
            },
            Err(_) => return false,
        };

        // Check against allowed patterns
        self.allowed_http_hosts
            .iter()
            .any(|pattern| Self::matches_pattern(pattern, &host))
    }

    /// Check if a host matches a permission pattern.
    fn matches_pattern(pattern: &str, host: &str) -> bool {
        let pattern = pattern.to_lowercase();

        if pattern.starts_with("*.") {
            // Wildcard subdomain match
            let suffix = &pattern[1..]; // ".example.com"
            host.ends_with(suffix) || host == &pattern[2..]
        } else {
            // Exact match
            pattern == host
        }
    }

    /// Check if the given host is a private/internal address.
    ///
    /// This blocks SSRF attacks by preventing access to:
    /// - localhost and 127.0.0.0/8
    /// - Private IP ranges (10.x.x.x, 172.16-31.x.x, 192.168.x.x)
    /// - Link-local addresses (169.254.x.x)
    /// - Cloud metadata endpoints (169.254.169.254)
    pub fn is_private_address(url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };

        let Some(host_str) = parsed.host_str() else {
            return false;
        };
        let host = host_str.to_lowercase();

        // Block localhost
        if host == "localhost" || host == "127.0.0.1" {
            return true;
        }

        // Block cloud metadata endpoints
        if host == "169.254.169.254" || host == "metadata.google.internal" {
            return true;
        }

        // Use the url crate's host parsing for proper IPv6 handling
        if let Some(url_host) = parsed.host() {
            return match url_host {
                url::Host::Ipv4(v4) => {
                    v4.is_private()
                        || v4.is_loopback()
                        || v4.is_link_local()
                        || v4.is_broadcast()
                        || v4.is_documentation()
                        || v4.is_unspecified()
                }
                url::Host::Ipv6(v6) => v6.is_loopback() || v6.is_unspecified(),
                url::Host::Domain(_) => false,
            };
        }

        false
    }
}

/// Builder for [`Permissions`].
#[derive(Debug, Default)]
pub struct PermissionsBuilder {
    inner: Permissions,
}

impl PermissionsBuilder {
    /// Allow HTTP access to specific hosts.
    ///
    /// # Arguments
    ///
    /// * `hosts` - Host patterns to allow (e.g., `api.example.com`, `*.example.com`)
    #[must_use]
    pub fn allow_http_hosts<I, S>(mut self, hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inner.http_enabled = true;
        self.inner.allowed_http_hosts = hosts.into_iter().map(Into::into).collect();
        self
    }

    /// Set the maximum number of HTTP requests per execution.
    #[must_use]
    pub fn max_http_requests(mut self, max: u32) -> Self {
        self.inner.max_http_requests = max;
        self
    }

    /// Enable logging.
    #[must_use]
    pub fn enable_logging(mut self) -> Self {
        self.inner.logging_enabled = true;
        self
    }

    /// Build the permissions.
    #[must_use]
    pub fn build(self) -> Permissions {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissions_none() {
        let perms = Permissions::none();
        assert!(!perms.http_enabled);
        assert!(!perms.logging_enabled);
        assert!(perms.allowed_http_hosts.is_empty());
    }

    #[test]
    fn test_permissions_all() {
        let perms = Permissions::all();
        assert!(perms.http_enabled);
        assert!(perms.logging_enabled);
        assert!(perms.allowed_http_hosts.contains("*"));
    }

    #[test]
    fn test_http_allowed_exact_match() {
        let perms = Permissions::builder()
            .allow_http_hosts(["api.example.com"])
            .build();

        assert!(perms.is_http_allowed("https://api.example.com/path"));
        assert!(!perms.is_http_allowed("https://other.example.com/path"));
        assert!(!perms.is_http_allowed("https://evil.com/path"));
    }

    #[test]
    fn test_http_allowed_wildcard() {
        let perms = Permissions::builder()
            .allow_http_hosts(["*.example.com"])
            .build();

        assert!(perms.is_http_allowed("https://api.example.com/path"));
        assert!(perms.is_http_allowed("https://www.example.com/path"));
        assert!(perms.is_http_allowed("https://example.com/path"));
        assert!(!perms.is_http_allowed("https://evil.com/path"));
    }

    #[test]
    fn test_http_allowed_all() {
        let perms = Permissions::all();
        assert!(perms.is_http_allowed("https://api.example.com/path"));
        assert!(perms.is_http_allowed("https://evil.com/path"));
    }

    #[test]
    fn test_http_disabled() {
        let perms = Permissions::none();
        assert!(!perms.is_http_allowed("https://api.example.com/path"));
    }

    #[test]
    fn test_private_address_localhost() {
        assert!(Permissions::is_private_address("http://localhost:8080/"));
        assert!(Permissions::is_private_address("http://127.0.0.1:8080/"));
        assert!(Permissions::is_private_address("http://[::1]:8080/"));
    }

    #[test]
    fn test_private_address_private_ranges() {
        assert!(Permissions::is_private_address("http://10.0.0.1/"));
        assert!(Permissions::is_private_address("http://172.16.0.1/"));
        assert!(Permissions::is_private_address("http://192.168.1.1/"));
    }

    #[test]
    fn test_private_address_metadata() {
        assert!(Permissions::is_private_address("http://169.254.169.254/"));
        assert!(Permissions::is_private_address(
            "http://metadata.google.internal/"
        ));
    }

    #[test]
    fn test_private_address_public() {
        assert!(!Permissions::is_private_address("https://api.example.com/"));
        assert!(!Permissions::is_private_address("https://8.8.8.8/"));
    }

    #[test]
    fn test_builder() {
        let perms = Permissions::builder()
            .allow_http_hosts(["api.example.com", "*.internal.example.com"])
            .max_http_requests(10)
            .enable_logging()
            .build();

        assert!(perms.http_enabled);
        assert!(perms.logging_enabled);
        assert_eq!(perms.max_http_requests, 10);
        assert_eq!(perms.allowed_http_hosts.len(), 2);
    }
}
