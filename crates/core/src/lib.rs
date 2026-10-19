//! Local Core: persistent authentication and isolated synthetic media inventory.
pub mod admin_backups;
pub mod admin_users;
pub mod api_keys;
pub mod auth;
pub mod backup;
pub mod backup_bundle;
pub mod database;
pub mod groups;
pub mod oidc;
pub mod postgres_backup;
pub mod scheduler;
use media_contracts::brand;
mod catalog;
mod demo;
pub mod identity;
pub mod inventory;
pub mod libraries;
pub mod media;
pub mod node;
pub mod nodes;
pub mod permissions;
pub mod preferences;
pub mod progress;
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
    let is_bench = inventory.is_fixture_bench();
    let mut router = Router::new()
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
        .route(
            "/api/v1/assets/{id}/content",
            get(node::content).head(node::content),
        );
    if is_bench {
        router = router.route("/", get(|| async { Html(include_str!("bench.html")) }));
    } else {
        router = router.route("/bench", get(|| async { Html(include_str!("bench.html")) }));
    }
    router.with_state(Arc::new(inventory))
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
    let oidc = oidc::router(auth.clone())?;
    let root = inventory.root().to_owned();
    let origin = auth.origin().to_owned();
    let store_for_csp = auth.store.clone();
    let replay_window = Arc::new(media_contracts::protocol::ReplayWindow::new());
    let media = media_routes(inventory)
        .route(
            "/api/v1/assets/{id}/prepared/{component}",
            get(indexed_prepared).head(indexed_prepared),
        )
        .route(
            "/api/v1/assets/{id}/audio/{index}",
            get(indexed_audio).head(indexed_audio),
        )
        .route(
            "/api/v1/assets/{id}/subtitles/{index}",
            get(indexed_subtitle).head(indexed_subtitle),
        )
        .route("/api/v1/media/{id}", get(indexed_descriptor))
        .route(
            "/api/v1/assets/{id}/image",
            get(indexed_image).head(indexed_image),
        )
        .layer(axum::Extension(Arc::new(
            media_node::packaging::Preparations::default(),
        )))
        .layer(axum::Extension(auth.store.clone()))
        .route_layer(middleware::from_fn_with_state(
            permissions::MediaAccess {
                auth: auth.clone(),
                root: root.clone(),
            },
            permissions::require_media,
        ));
    let protected = local_security_with_store(
        public_routes(mode)
            .merge(media)
            .merge(scans::router(auth.clone(), root.clone(), jobs).await?)
            .merge(libraries::router(auth.clone(), root.clone()))
            .merge(nodes::router(auth.clone(), replay_window))
            .merge(progress::router(auth.clone()))
            .merge(preferences::router(auth.clone()))
            .merge(admin_users::router(auth.clone()))
            .merge(admin_backups::router(auth.clone(), root.clone()).await?)
            .merge(scheduler::router(auth.clone()).await?)
            .merge(groups::router(auth.clone()))
            .merge(api_keys::router(auth.clone()))
            .merge(auth::router(auth))
            .merge(oidc),
        origin,
        Some(store_for_csp),
    );
    Ok(health_routes(mode).merge(protected))
}

async fn indexed_subtitle(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, index)): axum::extract::Path<(String, usize)>,
    request: Request,
) -> Response {
    inventory::subtitle(&store, &id, index, request).await
}

async fn indexed_prepared(
    axum::Extension(preparations): axum::Extension<Arc<media_node::packaging::Preparations>>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, component)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Response {
    inventory::prepared(&store, &preparations, &id, &component, request).await
}

async fn indexed_audio(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, index)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Response {
    inventory::audio(&store, &id, &index, request).await
}

async fn indexed_image(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    inventory::image(&store, &id, request).await
}

async fn indexed_descriptor(
    axum::Extension(identity): axum::Extension<auth::Authenticated>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match inventory::descriptor(&store, &id, &identity.token_hash).await {
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
            get(move || async move {
                Json(serde_json::json!({
                    "status": "ok",
                    "mode": mode,
                    "version": media_contracts::version::current_version()
                }))
            }),
        )
        .route(
            "/internal/v1/health",
            get(move || async move {
                Json(serde_json::json!({
                    "status": "ok",
                    "mode": mode,
                    "protocolVersion": media_contracts::protocol::PROTOCOL_VERSION,
                    "version": media_contracts::version::current_version()
                }))
            }),
        )
        .route(
            "/internal/v1/ready",
            get(move || async move { Json(serde_json::json!({"ready":true, "mode":mode})) }),
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
    let mut router = Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new(configured_web_dist())
                .append_index_html_on_directories(true),
        )
        .route(
            "/api/v1/config",
            get(move || async move {
                Json(serde_json::json!({
                    "name": brand::DEFAULT_NAME,
                    "mode": mode,
                    "version": media_contracts::version::current_version()
                }))
            }),
        );
    if mode != "localhost-fixtures" {
        router = router
            .route(
                "/",
                get(|| async { axum::response::Redirect::temporary("/app/") }),
            )
            .route(
                "/favicon.ico",
                get(|| async { axum::response::Redirect::temporary("/app/favicon.svg") }),
            )
            .route(
                "/favicon.svg",
                get(|| async { axum::response::Redirect::temporary("/app/favicon.svg") }),
            );
    }
    router
}

#[cfg(feature = "fixture-bench")]
fn local_security(router: Router, origin: String) -> Router {
    local_security_with_store(router, origin, None)
}

fn local_security_with_store(
    router: Router,
    origin: String,
    store: Option<identity::IdentityStore>,
) -> Router {
    let host = origin
        .parse::<axum::http::Uri>()
        .ok()
        .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
    let internal_host = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .and_then(|u| u.parse::<axum::http::Uri>().ok())
        .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
    router
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let host = host.clone();
            let internal_host = internal_host.clone();
            let origin = origin.clone();
            let store = store.clone();
            async move {
                let is_internal = req.uri().path().starts_with("/internal/v1/");
                let has_internal_proto = req
                    .headers()
                    .get(media_contracts::protocol::HDR_PROTOCOL)
                    .and_then(|v| v.to_str().ok())
                    == Some(media_contracts::protocol::PROTOCOL_VERSION);
                let req_host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok());
                let host_ok = req_host == host.as_deref()
                    || (is_internal && has_internal_proto)
                    || (is_internal && internal_host.as_deref().is_some_and(|ih| req_host == Some(ih)));
                // Only the exact OIDC callback accepts a cross-site top-level GET.
                // Its single-use state is bound to an HttpOnly browser cookie.
                let oidc_callback = req.method() == axum::http::Method::GET
                    && req.uri().path() == "/api/v1/auth/oidc/callback"
                    && req.headers().get("sec-fetch-mode").is_none_or(|v| v == "navigate")
                    && req.headers().get("sec-fetch-dest").is_none_or(|v| v == "document");
                if !host_ok
                    || (!oidc_callback && req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site"))
                    || (!oidc_callback && req.headers().get(header::ORIGIN).is_some_and(|h| h.as_bytes() != origin.as_bytes()))
                {
                    return StatusCode::FORBIDDEN.into_response();
                }
                let mut res = next.run(req).await;
                let mut extra_origins = String::new();
                if let Some(store) = &store
                    && let Ok(urls) = sqlx::query_scalar::<_, String>(
                        "SELECT DISTINCT public_url FROM media_nodes WHERE kind='remote' AND status='connected' AND public_url IS NOT NULL AND public_url != ''",
                    )
                    .fetch_all(&store.pool)
                    .await
                {
                    for u in urls {
                        if let Ok(parsed) = url::Url::parse(&u)
                            && let Some(h) = parsed.host_str()
                        {
                            let scheme = parsed.scheme();
                            let origin_str = if let Some(p) = parsed.port() {
                                format!(" {scheme}://{h}:{p}")
                            } else {
                                format!(" {scheme}://{h}")
                            };
                            if !extra_origins.contains(&origin_str) {
                                extra_origins.push_str(&origin_str);
                            }
                        }
                    }
                }
                let csp = format!(
                    "default-src 'none'; script-src 'self'; img-src 'self'{extra_origins}; media-src 'self' blob:{extra_origins}; connect-src 'self'{extra_origins}; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'"
                );
                if let Ok(val) = HeaderValue::from_str(&csp) {
                    res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, val);
                }
                res
            }
        }))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
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
