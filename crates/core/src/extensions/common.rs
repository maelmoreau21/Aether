use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) type Error = (StatusCode, Json<Value>);

pub(super) fn storage_error(error: std::io::Error) -> Error {
    eprintln!(
        "Plugin storage operation failed ({:?}): {}",
        error.kind(),
        error
    );
    let (status, code) = match error.kind() {
        std::io::ErrorKind::PermissionDenied => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "plugin_storage_permission_denied",
        ),
        std::io::ErrorKind::AlreadyExists => (StatusCode::CONFLICT, "plugin_storage_conflict"),
        std::io::ErrorKind::InvalidData => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "plugin_storage_integrity_failed",
        ),
        std::io::ErrorKind::NotFound => {
            (StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_missing")
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_failed"),
    };
    err(status, code)
}

pub(super) fn err(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error": code})))
}

pub(super) fn db(_: sqlx::Error) -> Error {
    err(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.ends_with('.')
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_".contains(&c))
}

pub(super) fn http_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.parse::<std::net::IpAddr>().is_err()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}
