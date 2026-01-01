//! HTTP request conversion for Wasm execution.
//!
//! This module provides types and functions for converting HTTP requests
//! into a format suitable for passing to WebAssembly modules.

use axum::http::Request;
use bytes::Bytes;

/// Wasm-compatible HTTP request structure.
///
/// This maps to the WIT `http-request` record defined in `wit/world.wit`.
#[derive(Debug, Clone)]
pub struct WasmHttpRequest {
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Request URI
    pub uri: String,
    /// Request headers as key-value pairs
    pub headers: Vec<(String, String)>,
    /// Optional request body
    pub body: Option<Vec<u8>>,
}

impl WasmHttpRequest {
    /// Create a new empty request.
    pub fn new(method: &str, uri: &str) -> Self {
        Self {
            method: method.to_string(),
            uri: uri.to_string(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Convert from Axum request parts.
    ///
    /// # Arguments
    ///
    /// * `req` - The HTTP request (headers and metadata)
    /// * `body` - The request body as bytes
    pub fn from_axum<B>(req: &Request<B>, body: Bytes) -> Self {
        let method = req.method().to_string();
        let uri = req.uri().to_string();

        let headers: Vec<(String, String)> = req
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.to_string(), v.to_string()))
            })
            .collect();

        let body = if body.is_empty() {
            None
        } else {
            Some(body.to_vec())
        };

        Self {
            method,
            uri,
            headers,
            body,
        }
    }

    /// Get a header value by name (case-insensitive).
    pub fn get_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    /// Get the Content-Type header.
    pub fn content_type(&self) -> Option<&str> {
        self.get_header("content-type")
    }

    /// Check if the request has a JSON content type.
    pub fn is_json(&self) -> bool {
        self.content_type()
            .is_some_and(|ct| ct.contains("application/json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request as HttpRequest};

    #[test]
    fn test_new_request() {
        let req = WasmHttpRequest::new("GET", "/api/test");
        assert_eq!(req.method, "GET");
        assert_eq!(req.uri, "/api/test");
        assert!(req.headers.is_empty());
        assert!(req.body.is_none());
    }

    #[test]
    fn test_from_axum() {
        let http_req = HttpRequest::builder()
            .method(Method::POST)
            .uri("/api/users")
            .header("Content-Type", "application/json")
            .header("X-Request-Id", "123")
            .body(())
            .unwrap();

        let body = Bytes::from(r#"{"name": "test"}"#);
        let req = WasmHttpRequest::from_axum(&http_req, body);

        assert_eq!(req.method, "POST");
        assert_eq!(req.uri, "/api/users");
        assert_eq!(req.headers.len(), 2);
        assert!(req.body.is_some());
    }

    #[test]
    fn test_get_header() {
        let mut req = WasmHttpRequest::new("GET", "/");
        req.headers
            .push(("Content-Type".to_string(), "application/json".to_string()));

        assert_eq!(req.get_header("content-type"), Some("application/json"));
        assert_eq!(req.get_header("Content-Type"), Some("application/json"));
        assert!(req.get_header("X-Missing").is_none());
    }

    #[test]
    fn test_is_json() {
        let mut req = WasmHttpRequest::new("POST", "/");
        assert!(!req.is_json());

        req.headers
            .push(("Content-Type".to_string(), "application/json".to_string()));
        assert!(req.is_json());

        req.headers[0].1 = "application/json; charset=utf-8".to_string();
        assert!(req.is_json());
    }
}
