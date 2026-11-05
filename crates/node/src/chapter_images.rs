//! Single chapter previews reuse the Node's confined extraction/cache/worker.
use crate::trickplay::{QueueStatus, TrickplayManager, TrickplayRequest};
use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

pub async fn response(
    manager: &TrickplayManager,
    request: TrickplayRequest,
    http: Request,
) -> Response {
    if request.chapter_time.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_chapter_image"})),
        )
            .into_response();
    }
    if manager.check_native_cache(&request).await.is_some()
        && let Ok((opened, mime)) = manager.open_sheet_with_mime(&request, 0).await
        && let Ok(response) = crate::serve_opened(opened, &http, &mime).await
    {
        let (mut parts, body) = response.into_parts();
        parts.headers.insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        );
        parts.headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        );
        return Response::from_parts(parts, Body::new(body));
    }
    let (status, code) = match manager.enqueue(request) {
        QueueStatus::Queued => (StatusCode::ACCEPTED, "generating"),
        QueueStatus::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        QueueStatus::Failed => (StatusCode::SERVICE_UNAVAILABLE, "failed"),
        QueueStatus::Disabled => (StatusCode::NOT_FOUND, "disabled"),
    };
    (status, Json(json!({"status":code}))).into_response()
}

/// Only signed Core calls can supply these bounded generation parameters.
pub fn parameters(query: &str) -> Option<(f64, f64, u32, u8)> {
    let values: std::collections::HashMap<_, _> =
        query.split('&').filter_map(|p| p.split_once('=')).collect();
    let time = values.get("time")?.parse::<f64>().ok()?;
    let duration = values.get("duration")?.parse::<f64>().ok()?;
    let width = values.get("width")?.parse::<u32>().ok()?;
    let quality = values.get("quality")?.parse::<u8>().ok()?;
    (time.is_finite()
        && duration.is_finite()
        && time >= 0.0
        && time < duration
        && duration <= 86400.0
        && (160..=1920).contains(&width)
        && (1..=31).contains(&quality))
    .then_some((time, duration, width, quality))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chapter_parameters_are_bounded_and_not_synthetic_intervals() {
        assert_eq!(
            parameters("time=12.5&duration=100&width=320&quality=7"),
            Some((12.5, 100.0, 320, 7))
        );
        for query in [
            "time=NaN&duration=100&width=320&quality=7",
            "time=100&duration=100&width=320&quality=7",
            "time=0&duration=100&width=9999&quality=7",
            "time=0&duration=100&width=320&quality=0",
            "",
        ] {
            assert!(parameters(query).is_none());
        }
    }
}
