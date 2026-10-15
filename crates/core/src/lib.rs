//! Local fixture-only P0 bench and first P1 web integration. No product authentication.
#[path = "../../shared/src/brand.rs"]
mod brand;
mod catalog;
mod demo;
pub mod media;
pub mod node;

use axum::{
    Json, Router,
    extract::Request,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;
use tower_http::set_header::SetResponseHeaderLayer;

pub fn app(inventory: node::Inventory, port: u16) -> Router {
    let host = format!("127.0.0.1:{port}");
    Router::new()
        .route("/", get(|| async { Html(include_str!("bench.html")) }))
        .nest_service("/app", tower_http::services::ServeDir::new("apps/web/build").append_index_html_on_directories(true))
        .route("/api/v1/config", get(|| async { Json(serde_json::json!({"name": brand::DEFAULT_NAME, "mode":"localhost-fixtures"})) }))
        .route("/api/v1/demo/media", get(demo::media))
        .route("/api/v1/catalog", get(catalog::list))
        .route("/api/v1/catalog/{id}", get(catalog::detail))
        .route("/api/v1/demo/subtitles/{name}", get(node::demo_subtitle))
        .route("/mse", get(|| async { Html(include_str!("mse.html")) }))
        .route("/shaka", get(|| async { Html(include_str!("shaka.html")) }))
        .route("/audio-seek-policy.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("audio-seek-policy.js")) }))
        .route("/shaka.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("shaka.js")) }))
        .route("/api/v1/experiment/shaka/{name}", get(node::shaka_content))
        .route("/mse.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("mse.js")) }))
        .route("/api/v1/experiment/mse/{name}", get(node::mse_content))
        .route("/api/v1/health", get(|| async { Json(serde_json::json!({"status":"ok", "mode":"local-p0-bench"})) }))
        .route("/api/v1/assets", get(|State(s): State<Arc<node::Inventory>>| async move { Json(s.descriptors()) }))
        .route("/api/v1/assets/{id}/content", get(node::content))
        .with_state(Arc::new(inventory))
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let host = host.clone();
            async move {
                // No LAN listener, CORS or DNS rebinding access to this unauthenticated bench.
                if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != Some(&host)
                    || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
                    || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != format!("http://{host}").as_bytes())
                {
                    return StatusCode::FORBIDDEN.into_response();
                }
                next.run(req).await
            }
        }))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .layer(SetResponseHeaderLayer::overriding(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; script-src 'self'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'")))
        .route("/bench.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("bench.js")) }))
}

pub fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(serde_json::json!({"error":code}))).into_response()
}
