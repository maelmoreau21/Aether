//! Local Core: persistent authentication and isolated synthetic media inventory.
pub mod auth;
pub mod backup;
use media_contracts::brand;
mod catalog;
mod demo;
pub mod identity;
pub mod inventory;
pub mod libraries;
pub mod media;
pub mod node;
pub mod permissions;
pub mod scans;

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
pub async fn secure_app(
    inventory: node::Inventory,
    _port: u16,
    auth: auth::AuthState,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    secure_app_with_jobs(inventory, _port, auth, Arc::new(scans::ScanJobs::default())).await
}

pub async fn secure_app_with_jobs(
    inventory: node::Inventory,
    _port: u16,
    auth: auth::AuthState,
    jobs: Arc<scans::ScanJobs>,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let mode = if inventory.is_fixture_bench() {
        "localhost-fixtures"
    } else {
        "standalone-local"
    };
    if inventory.is_fixture_bench() {
        libraries::initialize(&auth.store, inventory.root()).await?;
    } else {
        libraries::initialize_local(&auth.store).await?;
    }
    let root = inventory.root().to_owned();
    let origin = auth.origin().to_owned();
    let media = media_routes(inventory)
        .route("/api/v1/media/{id}", get(indexed_descriptor))
        .route("/api/v1/assets/{id}/image", get(indexed_image))
        .layer(axum::Extension(auth.store.clone()))
        .route_layer(middleware::from_fn_with_state(
            permissions::MediaAccess {
                auth: auth.clone(),
                root: root.clone(),
            },
            permissions::require_media,
        ));
    let protected = local_security(
        public_routes(mode)
            .merge(media)
            .merge(scans::router(auth.clone(), root.clone(), jobs).await?)
            .merge(libraries::router(auth.clone(), root))
            .merge(auth::router(auth)),
        origin,
    );
    Ok(health_routes(mode).merge(protected))
}

async fn indexed_image(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    inventory::image(&store, &id, request).await
}

async fn indexed_descriptor(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match inventory::descriptor(&store, &id).await {
        Ok(Some(media)) => Json(media).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "unknown_media"),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    }
}

/// Explicit compile-time isolated bench for synthetic fixture tests only.
/// The production binary never selects this constructor.
#[cfg(feature = "fixture-bench")]
pub fn app(inventory: node::Inventory, port: u16) -> Router {
    health_routes("localhost-fixtures").merge(local_security(
        public_routes("localhost-fixtures").merge(media_routes(inventory)),
        format!("http://127.0.0.1:{port}"),
    ))
}

/// Helper to resolve web dist path from an optional string slice.
pub fn configured_web_dist_from(val: Option<&str>) -> std::path::PathBuf {
    let raw = match val {
        Some(s) if !s.trim().is_empty() => std::path::PathBuf::from(s.trim()),
        _ => std::path::PathBuf::from("apps/web/build"),
    };
    raw.canonicalize().unwrap_or(raw)
}

/// Resolve the static web distribution directory from `MEDIA_WEB_DIST`.
/// Defaults to `"apps/web/build"` if unset or empty/whitespace.
/// Best-effort canonicalizes if the directory exists on disk.
pub fn configured_web_dist() -> std::path::PathBuf {
    configured_web_dist_from(std::env::var("MEDIA_WEB_DIST").ok().as_deref())
}

fn health_routes(mode: &'static str) -> Router {
    Router::new()
        .route(
            "/api/v1/health",
            get(move || async move { Json(serde_json::json!({"status":"ok", "mode":mode})) }),
        )
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}

fn public_routes(mode: &'static str) -> Router {
    Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new(configured_web_dist())
                .append_index_html_on_directories(true),
        )
        .route(
            "/api/v1/config",
            get(move || async move {
                Json(serde_json::json!({"name": brand::DEFAULT_NAME, "mode":mode}))
            }),
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
    (
        status,
        Json(media_contracts::errors::ApiErrorBody::new(code)),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_web_dist_defaults_when_unset_or_empty() {
        let expected = std::path::PathBuf::from("apps/web/build");
        let expected = expected.canonicalize().unwrap_or(expected);

        assert_eq!(configured_web_dist_from(None), expected);
        assert_eq!(configured_web_dist_from(Some("")), expected);
        assert_eq!(configured_web_dist_from(Some("   \t  ")), expected);
    }

    #[test]
    fn configured_web_dist_custom_path() {
        let temp = tempfile::tempdir().unwrap();
        let custom = temp.path().join("dist");
        std::fs::create_dir_all(&custom).unwrap();
        let canonical_custom = custom.canonicalize().unwrap();

        assert_eq!(
            configured_web_dist_from(Some(custom.to_str().unwrap())),
            canonical_custom
        );
    }

    #[test]
    fn configured_web_dist_nonexistent_path_falls_back_to_raw() {
        let nonexistent = "nonexistent/custom/path";
        assert_eq!(
            configured_web_dist_from(Some(nonexistent)),
            std::path::PathBuf::from(nonexistent)
        );
    }
}
