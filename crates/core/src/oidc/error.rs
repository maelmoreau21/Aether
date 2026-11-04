//! HTTP error type and constructors shared by the OIDC handlers.
use axum::{Json, http::StatusCode};

pub(super) type Error = (StatusCode, Json<serde_json::Value>);
pub(super) fn deny(status: StatusCode, code: &str) -> Error {
    (status, Json(serde_json::json!({"error":code})))
}
pub(super) fn invalid() -> Error {
    deny(StatusCode::UNAUTHORIZED, "oidc_invalid_response")
}
pub(super) fn unavailable() -> Error {
    deny(StatusCode::BAD_GATEWAY, "oidc_provider_unavailable")
}
pub(super) fn db(_: sqlx::Error) -> Error {
    deny(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}
