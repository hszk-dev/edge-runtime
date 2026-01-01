//! Admin API handlers for runtime management.
//!
//! This module provides HTTP handlers for managing the runtime,
//! including module upload, deletion, and inspection.
//!
//! # Authentication
//!
//! All Admin API endpoints require the `X-Admin-Token` header
//! to match the configured admin token.
//!
//! # Endpoints
//!
//! - `POST /admin/modules` - Upload a new module
//! - `GET /admin/modules` - List all modules (detailed)
//! - `GET /admin/modules/:id` - Get module info
//! - `DELETE /admin/modules/:id` - Delete a module

use axum::{
    Extension, Json, Router,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
};
use axum_extra::extract::Multipart;
use serde::Serialize;
use tracing::{info, instrument, warn};

use crate::state::AppState;

/// Admin API state containing app state and auth token.
#[derive(Clone)]
pub struct AdminState {
    /// Application state (module cache, engine, etc.).
    pub app_state: AppState,
    /// Expected admin token for authentication.
    pub admin_token: String,
}

/// Module information for API responses.
#[derive(Serialize)]
pub struct ModuleInfo {
    /// Module ID.
    pub id: String,
    /// Content hash of the original Wasm bytes.
    pub content_hash: String,
    /// Whether this is a Component Model component.
    pub is_component: bool,
}

/// Build the Admin API router.
///
/// Returns a router that uses Extension to pass the admin state,
/// allowing it to be nested into routers with different state types.
///
/// # Arguments
///
/// * `admin_state` - Admin state containing app state and auth token
pub fn build_admin_router(admin_state: AdminState) -> Router<AppState> {
    Router::new()
        .route("/modules", post(upload_module))
        .route("/modules", get(list_modules_admin))
        .route("/modules/:id", get(get_module_info))
        .route("/modules/:id", delete(delete_module))
        .layer(Extension(admin_state))
}

/// Verify the admin token from request headers.
fn verify_token(headers: &HeaderMap, expected: &str) -> Result<(), (StatusCode, &'static str)> {
    match headers.get("X-Admin-Token") {
        Some(token) => {
            if token.to_str().unwrap_or("") == expected {
                Ok(())
            } else {
                Err((StatusCode::UNAUTHORIZED, "Invalid admin token"))
            }
        }
        None => Err((StatusCode::UNAUTHORIZED, "Missing X-Admin-Token header")),
    }
}

/// Upload a new module.
///
/// # Request
///
/// `POST /admin/modules`
///
/// Content-Type: `multipart/form-data`
///
/// Fields:
/// - `id` (optional): Module ID (defaults to filename without extension)
/// - `file` or `wasm` or `module`: The WebAssembly binary
///
/// # Response
///
/// ```json
/// {
///   "id": "hello",
///   "content_hash": "abc123...",
///   "message": "Module uploaded successfully"
/// }
/// ```
#[instrument(skip(admin_state, headers, multipart))]
pub async fn upload_module(
    Extension(admin_state): Extension<AdminState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> impl IntoResponse {
    if let Err(e) = verify_token(&headers, &admin_state.admin_token) {
        return e.into_response();
    }

    let (module_id, wasm_bytes) = match extract_module_from_multipart(multipart).await {
        Ok(result) => result,
        Err(msg) => {
            warn!(error = msg, "Failed to extract module from request");
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
    };

    match admin_state.app_state.load_module(&module_id, &wasm_bytes) {
        Ok(module) => {
            info!(id = %module_id, hash = %module.content_hash(), "Module uploaded");
            Json(serde_json::json!({
                "id": module_id,
                "content_hash": module.content_hash(),
                "message": "Module uploaded successfully"
            }))
            .into_response()
        }
        Err(e) => {
            warn!(id = %module_id, error = %e, "Module compilation failed");
            (StatusCode::BAD_REQUEST, format!("Compilation failed: {e}")).into_response()
        }
    }
}

/// Delete a module.
///
/// # Request
///
/// `DELETE /admin/modules/:id`
///
/// # Response
///
/// ```json
/// {
///   "id": "hello",
///   "message": "Module deleted successfully"
/// }
/// ```
#[instrument(skip(admin_state, headers))]
pub async fn delete_module(
    Extension(admin_state): Extension<AdminState>,
    headers: HeaderMap,
    Path(module_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_token(&headers, &admin_state.admin_token) {
        return e.into_response();
    }

    match admin_state.app_state.remove_module(&module_id) {
        Some(_) => {
            info!(id = %module_id, "Module deleted");
            Json(serde_json::json!({
                "id": module_id,
                "message": "Module deleted successfully"
            }))
            .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            format!("Module not found: {module_id}"),
        )
            .into_response(),
    }
}

/// Get module information.
///
/// # Request
///
/// `GET /admin/modules/:id`
///
/// # Response
///
/// ```json
/// {
///   "id": "hello",
///   "content_hash": "abc123...",
///   "is_component": false
/// }
/// ```
#[instrument(skip(admin_state, headers))]
pub async fn get_module_info(
    Extension(admin_state): Extension<AdminState>,
    headers: HeaderMap,
    Path(module_id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = verify_token(&headers, &admin_state.admin_token) {
        return e.into_response();
    }

    match admin_state.app_state.get_module(&module_id) {
        Some(module) => Json(ModuleInfo {
            id: module_id,
            content_hash: module.content_hash().to_string(),
            is_component: module.is_component(),
        })
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            format!("Module not found: {module_id}"),
        )
            .into_response(),
    }
}

/// List all modules (detailed).
///
/// # Request
///
/// `GET /admin/modules`
///
/// # Response
///
/// ```json
/// {
///   "modules": [
///     {
///       "id": "hello",
///       "content_hash": "abc123...",
///       "is_component": false
///     }
///   ],
///   "count": 1
/// }
/// ```
#[instrument(skip(admin_state, headers))]
pub async fn list_modules_admin(
    Extension(admin_state): Extension<AdminState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(e) = verify_token(&headers, &admin_state.admin_token) {
        return e.into_response();
    }

    let modules: Vec<ModuleInfo> = admin_state
        .app_state
        .list_modules()
        .into_iter()
        .filter_map(|id| {
            admin_state.app_state.get_module(&id).map(|m| ModuleInfo {
                id,
                content_hash: m.content_hash().to_string(),
                is_component: m.is_component(),
            })
        })
        .collect();

    let count = modules.len();

    Json(serde_json::json!({
        "modules": modules,
        "count": count
    }))
    .into_response()
}

/// Extract module ID and bytes from multipart form data.
async fn extract_module_from_multipart(
    mut multipart: Multipart,
) -> Result<(String, Vec<u8>), &'static str> {
    let mut module_id: Option<String> = None;
    let mut wasm_bytes: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "id" | "module_id" => {
                module_id = Some(field.text().await.map_err(|_| "Invalid id field")?);
            }
            "file" | "wasm" | "module" => {
                // Extract module ID from filename if not explicitly set
                if module_id.is_none() {
                    if let Some(filename) = field.file_name() {
                        module_id = Some(
                            std::path::Path::new(filename)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("uploaded")
                                .to_string(),
                        );
                    }
                }
                wasm_bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|_| "Failed to read file")?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    match (module_id, wasm_bytes) {
        (Some(id), Some(bytes)) => Ok((id, bytes)),
        (None, Some(_)) => Err("Missing module id"),
        (_, None) => Err("Missing wasm file"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_token_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Admin-Token", "secret".parse().unwrap());

        let result = verify_token(&headers, "secret");
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_token_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Admin-Token", "wrong".parse().unwrap());

        let result = verify_token(&headers, "secret");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_verify_token_missing() {
        let headers = HeaderMap::new();

        let result = verify_token(&headers, "secret");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::UNAUTHORIZED);
    }
}
