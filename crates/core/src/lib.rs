//! Local Core: persistent authentication and isolated synthetic media inventory.
pub mod auth;
#[path = "../../shared/src/brand.rs"]
mod brand;
mod catalog;
mod demo;
pub mod identity;
pub mod media;
pub mod node;
pub mod permissions;

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

fn media_routes(inventory: node::Inventory) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("bench.html")) }))
        .route("/api/v1/demo/media", get(demo::media))
        .route("/api/v1/catalog", get(catalog::list))
        .route("/api/v1/catalog/{id}", get(catalog::detail))
        .route("/api/v1/demo/subtitles/{name}", get(node::demo_subtitle))
        .route("/mse", get(|| async { Html(include_str!("mse.html")) }))
        .route("/shaka", get(|| async { Html(include_str!("shaka.html")) }))
        .route(
            "/audio-seek-policy.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("audio-seek-policy.js"),
                )
            }),
        )
        .route(
            "/shaka.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("shaka.js"),
                )
            }),
        )
        .route("/api/v1/experiment/shaka/{name}", get(node::shaka_content))
        .route(
            "/mse.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("mse.js"),
                )
            }),
        )
        .route("/api/v1/experiment/mse/{name}", get(node::mse_content))
        .route(
            "/api/v1/assets",
            get(|State(s): State<Arc<node::Inventory>>| async move { Json(s.descriptors()) }),
        )
        .route("/api/v1/assets/{id}/content", get(node::content))
        .with_state(Arc::new(inventory))
}

/// Production constructor: authentication precedes all media/resource handlers.
pub fn secure_app(inventory: node::Inventory, _port: u16, auth: auth::AuthState) -> Router {
    let origin = auth.origin().to_owned();
    let media = media_routes(inventory).route_layer(middleware::from_fn_with_state(
        auth.clone(),
        permissions::require_media,
    ));
    local_security(
        public_routes().merge(media).merge(auth::router(auth)),
        origin,
    )
}

/// Explicit compile-time isolated bench for synthetic fixture tests only.
/// The production binary never selects this constructor.
#[cfg(feature = "fixture-bench")]
pub fn app(inventory: node::Inventory, port: u16) -> Router {
    local_security(
        public_routes().merge(media_routes(inventory)),
        format!("http://127.0.0.1:{port}"),
    )
}

fn public_routes() -> Router {
    Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new("apps/web/build")
                .append_index_html_on_directories(true),
        )
        .route(
            "/api/v1/config",
            get(|| async {
                Json(serde_json::json!({"name": brand::DEFAULT_NAME, "mode":"localhost-fixtures"}))
            }),
        )
        .route(
            "/api/v1/health",
            get(|| async { Json(serde_json::json!({"status":"ok", "mode":"local-p0-bench"})) }),
        )
}

fn local_security(router: Router, origin: String) -> Router {
    let host = origin
        .parse::<axum::http::Uri>()
        .ok()
        .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
    router
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let host = host.clone();
            let origin = origin.clone();
            async move {
                // Preserve the loopback-only origin boundary for the local Core.
                if req.headers().get(header::HOST).and_then(|h| h.to_str().ok()) != host.as_deref()
                    || req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site")
                    || req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes())
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
