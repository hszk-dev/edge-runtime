//! HTTP outbound host function implementation.
//!
//! This module provides the host-side implementation of the HTTP outbound
//! interface, allowing guest components to make HTTP requests to external
//! services with security controls.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use reqwest::Client;
use tracing::{debug, info, warn};

use crate::Permissions;
use edge_runtime_common::{HostFunctionError, RuntimeError};

/// HTTP outbound host implementation.
///
/// This struct manages HTTP requests from guest components, providing:
/// - Permission checking against allowed hosts
/// - SSRF protection (blocking private addresses)
/// - Rate limiting per execution
/// - Request timeout enforcement
pub struct HttpOutboundHost {
    /// HTTP client (shared, connection pooled).
    client: Client,

    /// Permission configuration.
    permissions: Permissions,

    /// Request counter for rate limiting.
    request_count: AtomicU32,
}

/// HTTP request from guest code.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method.
    pub method: HttpMethod,
    /// Target URI.
    pub uri: String,
    /// Request headers.
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: Option<Vec<u8>>,
    /// Request timeout in milliseconds.
    pub timeout_ms: Option<u32>,
}

/// HTTP response to guest code.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
}

/// HTTP method enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
    Options,
}

impl HttpMethod {
    /// Convert to reqwest method.
    fn to_reqwest(self) -> reqwest::Method {
        match self {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Options => reqwest::Method::OPTIONS,
        }
    }
}

/// HTTP error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    /// Permission denied.
    PermissionDenied,
    /// Request timed out.
    Timeout,
    /// DNS resolution failed.
    DnsError,
    /// Connection failed.
    ConnectionFailed,
    /// TLS error.
    TlsError,
    /// Response body too large.
    BodyTooLarge,
    /// Rate limited.
    RateLimited,
    /// Other error.
    Other,
}

impl HttpOutboundHost {
    /// Create a new HTTP outbound host.
    ///
    /// # Arguments
    ///
    /// * `permissions` - Permission configuration for this execution
    pub fn new(permissions: Permissions) -> Self {
        // Create HTTP client with reasonable defaults
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .user_agent(concat!("edge-runtime/", env!("CARGO_PKG_VERSION"),))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            permissions,
            request_count: AtomicU32::new(0),
        }
    }

    /// Create with a custom HTTP client.
    pub fn with_client(client: Client, permissions: Permissions) -> Self {
        Self {
            client,
            permissions,
            request_count: AtomicU32::new(0),
        }
    }

    /// Perform an HTTP request.
    ///
    /// # Security
    ///
    /// This function performs the following security checks:
    /// 1. Verify the target URI is in the allowed hosts list
    /// 2. Block requests to private/internal networks (SSRF protection)
    /// 3. Enforce rate limiting
    ///
    /// # Arguments
    ///
    /// * `request` - The HTTP request to perform
    ///
    /// # Returns
    ///
    /// The HTTP response, or an error.
    pub async fn fetch(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        // Rate limit check
        let count = self.request_count.fetch_add(1, Ordering::SeqCst);
        if count >= self.permissions.max_http_requests {
            warn!(
                uri = %request.uri,
                count = count,
                max = self.permissions.max_http_requests,
                "HTTP rate limit exceeded"
            );
            return Err(HttpError::RateLimited);
        }

        // Permission check
        if !self.permissions.is_http_allowed(&request.uri) {
            warn!(
                uri = %request.uri,
                "HTTP request blocked: not in allowed hosts"
            );
            return Err(HttpError::PermissionDenied);
        }

        // SSRF protection
        if Permissions::is_private_address(&request.uri) {
            warn!(
                uri = %request.uri,
                "HTTP request blocked: private address"
            );
            return Err(HttpError::PermissionDenied);
        }

        debug!(
            method = ?request.method,
            uri = %request.uri,
            "Executing HTTP request"
        );

        // Build the request
        let mut req_builder = self
            .client
            .request(request.method.to_reqwest(), &request.uri);

        // Set timeout
        if let Some(timeout_ms) = request.timeout_ms {
            req_builder = req_builder.timeout(Duration::from_millis(timeout_ms.into()));
        }

        // Add headers
        for (key, value) in &request.headers {
            req_builder = req_builder.header(key, value);
        }

        // Add body
        if let Some(body) = request.body {
            req_builder = req_builder.body(body);
        }

        // Execute request
        let response = req_builder.send().await.map_err(|e| {
            if e.is_timeout() {
                HttpError::Timeout
            } else if e.is_connect() {
                HttpError::ConnectionFailed
            } else {
                HttpError::Other
            }
        })?;

        let status = response.status().as_u16();

        // Collect headers
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();

        // Read body (with size limit)
        let body = response.bytes().await.map_err(|_| HttpError::Other)?;

        // Check body size (10MB limit)
        if body.len() > 10 * 1024 * 1024 {
            return Err(HttpError::BodyTooLarge);
        }

        info!(
            uri = %request.uri,
            status = status,
            body_size = body.len(),
            "HTTP request completed"
        );

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }

    /// Convenience function for GET requests.
    pub async fn get(&self, uri: &str) -> Result<Vec<u8>, HttpError> {
        let response = self
            .fetch(HttpRequest {
                method: HttpMethod::Get,
                uri: uri.to_string(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            })
            .await?;

        Ok(response.body)
    }

    /// Get the number of requests made.
    pub fn request_count(&self) -> u32 {
        self.request_count.load(Ordering::SeqCst)
    }

    /// Reset the request counter.
    pub fn reset_count(&self) {
        self.request_count.store(0, Ordering::SeqCst);
    }
}

impl From<HttpError> for RuntimeError {
    fn from(err: HttpError) -> Self {
        match err {
            HttpError::PermissionDenied => {
                RuntimeError::HostFunction(HostFunctionError::PermissionDenied {
                    resource: "HTTP access".into(),
                })
            }
            HttpError::RateLimited => {
                RuntimeError::HostFunction(HostFunctionError::RateLimitExceeded {
                    operation: "HTTP request".into(),
                })
            }
            _ => RuntimeError::HostFunction(HostFunctionError::HttpRequestFailed {
                url: String::new(),
                status: 0,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_conversion() {
        assert_eq!(HttpMethod::Get.to_reqwest(), reqwest::Method::GET);
        assert_eq!(HttpMethod::Post.to_reqwest(), reqwest::Method::POST);
        assert_eq!(HttpMethod::Put.to_reqwest(), reqwest::Method::PUT);
        assert_eq!(HttpMethod::Delete.to_reqwest(), reqwest::Method::DELETE);
    }

    #[test]
    fn test_request_count() {
        let perms = Permissions::all();
        let host = HttpOutboundHost::new(perms);

        assert_eq!(host.request_count(), 0);

        // Simulate incrementing count
        host.request_count.fetch_add(1, Ordering::SeqCst);
        assert_eq!(host.request_count(), 1);

        host.reset_count();
        assert_eq!(host.request_count(), 0);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let perms = Permissions::builder()
            .allow_http_hosts(["httpbin.org"])
            .max_http_requests(0) // No requests allowed
            .build();

        let host = HttpOutboundHost::new(perms);

        let result = host
            .fetch(HttpRequest {
                method: HttpMethod::Get,
                uri: "https://httpbin.org/get".into(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            })
            .await;

        assert!(matches!(result, Err(HttpError::RateLimited)));
    }

    #[tokio::test]
    async fn test_permission_denied() {
        let perms = Permissions::builder()
            .allow_http_hosts(["allowed.com"])
            .max_http_requests(10)
            .build();

        let host = HttpOutboundHost::new(perms);

        let result = host
            .fetch(HttpRequest {
                method: HttpMethod::Get,
                uri: "https://blocked.com/path".into(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            })
            .await;

        assert!(matches!(result, Err(HttpError::PermissionDenied)));
    }

    #[tokio::test]
    async fn test_ssrf_blocked() {
        let perms = Permissions::all();
        let host = HttpOutboundHost::new(perms);

        // Should block localhost
        let result = host
            .fetch(HttpRequest {
                method: HttpMethod::Get,
                uri: "http://localhost:8080/".into(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            })
            .await;

        assert!(matches!(result, Err(HttpError::PermissionDenied)));

        // Should block private IPs
        let result = host
            .fetch(HttpRequest {
                method: HttpMethod::Get,
                uri: "http://192.168.1.1/".into(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            })
            .await;

        assert!(matches!(result, Err(HttpError::PermissionDenied)));
    }
}
