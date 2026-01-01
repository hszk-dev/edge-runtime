//! HTTP router configuration.
//!
//! This module provides functions to build the Axum router with all
//! necessary routes and middleware.

use std::time::Duration;

use axum::Router;
use axum::routing::{any, get, post};
use tower_http::cors::{Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::handler::{handle_function, health_check, list_modules, readiness_check};
use crate::state::AppState;

/// Build the main application router.
///
/// Routes:
/// - `POST /functions/:function_id` - Execute a function with request body
/// - `GET /functions/:function_id` - Execute a function without body
/// - `GET /health` - Health check
/// - `GET /ready` - Readiness check
/// - `GET /modules` - List loaded modules
pub fn build_router(state: AppState, request_timeout: Duration) -> Router {
    // Function execution routes
    let function_routes = Router::new()
        // POST /functions/:function_id - Execute with request body
        .route("/functions/{function_id}", post(handle_function))
        // GET /functions/:function_id - Execute without body
        .route("/functions/{function_id}", get(handle_function))
        // ANY /invoke/:function_id - Simplified invoke endpoint
        .route("/invoke/{function_id}", any(handle_function));

    // Health and monitoring routes
    let health_routes = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/modules", get(list_modules));

    // Combine all routes
    Router::new()
        .merge(function_routes)
        .merge(health_routes)
        // Add middleware layers
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(request_timeout))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use edge_runtime_common::RuntimeConfig;
    use tower::util::ServiceExt;

    async fn setup_router() -> Router {
        let config = RuntimeConfig::default();
        let state = AppState::new(&config).unwrap();
        build_router(state, Duration::from_secs(30))
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = setup_router().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_readiness_check() {
        let app = setup_router().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_modules_empty() {
        let app = setup_router().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/modules")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_function_not_found() {
        let app = setup_router().await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/functions/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
