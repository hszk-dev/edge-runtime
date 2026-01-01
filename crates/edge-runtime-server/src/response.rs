//! HTTP response conversion from Wasm execution.
//!
//! This module provides types and functions for converting WebAssembly
//! execution results into HTTP responses.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Response, StatusCode};

/// Wasm-compatible HTTP response structure.
///
/// This maps to the WIT `http-response` record defined in `wit/world.wit`.
#[derive(Debug, Clone)]
pub struct WasmHttpResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers as key-value pairs
    pub headers: Vec<(String, String)>,
    /// Response body
    pub body: Vec<u8>,
}

impl WasmHttpResponse {
    /// Create a simple text response.
    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![(
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string(),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    /// Create a JSON response.
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.as_bytes().to_vec(),
        }
    }

    /// Create an error response with JSON body.
    pub fn error(status: u16, message: &str) -> Self {
        let body = serde_json::json!({
            "error": message
        })
        .to_string();
        Self::json(status, &body)
    }

    /// Create an empty response with just a status code.
    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// Add a header to the response.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Convert to Axum response.
    pub fn into_axum_response(self) -> Response<Body> {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let mut response = Response::builder().status(status);

        for (name, value) in &self.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::try_from(value.as_str()),
            ) {
                response = response.header(name, value);
            }
        }

        response.body(Body::from(self.body)).unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))
                .unwrap()
        })
    }
}

impl Default for WasmHttpResponse {
    fn default() -> Self {
        Self::text(200, "OK")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_response() {
        let resp = WasmHttpResponse::text(200, "Hello, World!");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"Hello, World!");
        assert_eq!(
            resp.headers[0],
            (
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string()
            )
        );
    }

    #[test]
    fn test_json_response() {
        let resp = WasmHttpResponse::json(201, r#"{"id": 1}"#);
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body, br#"{"id": 1}"#);
        assert_eq!(
            resp.headers[0],
            ("content-type".to_string(), "application/json".to_string())
        );
    }

    #[test]
    fn test_error_response() {
        let resp = WasmHttpResponse::error(404, "Not found");
        assert_eq!(resp.status, 404);
        assert!(String::from_utf8_lossy(&resp.body).contains("Not found"));
    }

    #[test]
    fn test_with_header() {
        let resp = WasmHttpResponse::text(200, "OK")
            .with_header("X-Request-Id", "123")
            .with_header("X-Custom", "value");

        assert_eq!(resp.headers.len(), 3);
        assert_eq!(
            resp.headers[1],
            ("X-Request-Id".to_string(), "123".to_string())
        );
    }

    #[test]
    fn test_into_axum_response() {
        let resp = WasmHttpResponse::text(200, "Hello");
        let axum_resp = resp.into_axum_response();
        assert_eq!(axum_resp.status(), StatusCode::OK);
    }
}
