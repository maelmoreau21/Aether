//! Local Core: persistent authentication and isolated synthetic media inventory.
pub mod account;
pub mod admin_backups;
pub mod admin_users;
pub mod api_keys;
pub mod auth;
pub mod automation;
pub mod automation_execution;
pub mod automation_http;
pub mod avatars;
pub mod backup;
pub mod backup_bundle;
pub mod database;
pub mod extension_data_lifecycle;
pub mod extension_files;
pub mod extension_runtime;
pub mod extensions;
pub mod groups;
pub mod oidc;
pub mod postgres_backup;
pub mod scheduler;
pub mod server_settings;
pub mod system_memory;
pub mod system_task_queue;
pub mod system_tasks;
use media_contracts::brand;
pub mod artwork;
pub mod assets;
mod catalog;
pub mod consolidation;
mod demo;
pub mod feedback;
pub mod identity;
pub mod inventory;
pub mod libraries;
pub mod library_options;
mod local_mounts;
pub mod media;
pub mod music;
pub mod node;
pub mod nodes;
pub mod permissions;
pub mod playlists;
pub mod preferences;
pub mod product;
#[cfg(test)]
mod product_tests;
pub mod progress;
pub mod scans;
pub mod user_media_policy;

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
        )
        .route(
            "/api/v1/assets/{id}/download",
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
    secure_app_with_jobs_and_web_dist(inventory, _port, auth, jobs, configured_web_dist()).await
}

/// Build the authenticated router with an explicit static asset directory.
/// Used by integration tests to supply disposable web fixtures.
#[doc(hidden)]
pub async fn secure_app_with_web_dist(
    inventory: node::Inventory,
    port: u16,
    auth: auth::AuthState,
    web_dist: std::path::PathBuf,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    secure_app_with_jobs_and_web_dist(
        inventory,
        port,
        auth,
        Arc::new(scans::ScanJobs::default()),
        web_dist,
    )
    .await
}

async fn secure_app_with_jobs_and_web_dist(
    inventory: node::Inventory,
    _port: u16,
    auth: auth::AuthState,
    jobs: Arc<scans::ScanJobs>,
    web_dist: std::path::PathBuf,
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
    extensions::initialize(&auth).await?;
    let root = inventory.root().to_owned();
    let origin = auth.origin().to_owned();
    let allowed_origins = auth.allowed_origins().to_vec();
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
        .layer(axum::Extension(jobs.preparations.clone()))
        .layer(axum::Extension(auth.store.clone()))
        .layer(middleware::from_fn_with_state(
            jobs.clone(),
            local_stream_priority,
        ))
        .route_layer(middleware::from_fn_with_state(
            permissions::MediaAccess {
                auth: auth.clone(),
                root: root.clone(),
            },
            permissions::require_media,
        ));
    let protected = local_security_with_store(
        public_routes(mode, web_dist)
            .merge(media)
            .merge(scans::router(auth.clone(), root.clone(), jobs).await?)
            .merge(libraries::router(auth.clone(), root.clone()))
            .merge(nodes::router(auth.clone(), replay_window))
            .merge(progress::router(auth.clone()))
            .merge(feedback::router(auth.clone()))
            .merge(preferences::router(auth.clone()))
            .merge(account::router(auth.clone()))
            .merge(music::router(auth.clone()))
            .merge(playlists::router(auth.clone()))
            .merge(product::router(auth.clone()))
            .merge(artwork::router(auth.clone()))
            .merge(user_media_policy::router(auth.clone()))
            .merge(avatars::router(auth.clone()))
            .merge(server_settings::router(auth.clone()))
            .merge(admin_users::router(auth.clone()))
            .merge(admin_backups::router(auth.clone(), root.clone()).await?)
            .merge(scheduler::router(auth.clone()).await?)
            .merge(system_tasks::router(auth.clone(), root.clone()).await?)
            .merge(system_memory::router(auth.clone()))
            .merge(groups::router(auth.clone()))
            .merge(api_keys::router(auth.clone()))
            .merge(automation_http::router(auth.clone()))
            .merge(timeline_markers::router(auth.clone()))
            .merge(extensions::router(auth.clone()))
            .merge(auth::router(auth))
            .merge(oidc),
        origin,
        Some(store_for_csp),
        allowed_origins,
    );
    Ok(health_routes(mode).merge(protected))
}

async fn local_stream_priority(
    State(jobs): State<Arc<scans::ScanJobs>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let is_prepared_or_audio = path.contains("/prepared/") || path.contains("/audio/");
    let streamed = path.contains("/content") || is_prepared_or_audio;
    let _demand =
        streamed.then(|| media_node::server::ActiveStreamGuard::enter(&scans::LOCAL_STREAMS));
    if is_prepared_or_audio {
        if let Ok(tokens) = jobs.cancellations.lock() {
            for token in tokens.values() {
                token.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(250);
        while jobs
            .cancellations
            .lock()
            .map(|tokens| !tokens.is_empty())
            .unwrap_or(false)
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    let response = next.run(request).await;
    if streamed {
        media_node::server::track_stream_body(response, &scans::LOCAL_STREAMS)
    } else {
        response
    }
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
    axum::Extension(visible): axum::Extension<permissions::VisibleLibraries>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    match inventory::descriptor_for_user(
        &store,
        &id,
        &identity.token_hash,
        Some(&identity.user.id),
        Some(&visible.0),
    )
    .await
    {
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
        public_routes("localhost-fixtures", configured_web_dist()).merge(media_routes(inventory)),
        format!("http://127.0.0.1:{port}"),
    ))
}

/// Static assets are built at this path relative to the Core working directory.
pub fn configured_web_dist() -> std::path::PathBuf {
    let raw = std::path::PathBuf::from("apps/web/build");
    raw.canonicalize().unwrap_or(raw)
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

fn public_routes(mode: &'static str, web_dist: std::path::PathBuf) -> Router {
    let mut router = Router::new()
        .nest_service(
            "/app",
            tower_http::services::ServeDir::new(web_dist).append_index_html_on_directories(true),
        )
        .route(
            "/api/v1/config",
            get(move || async move {
                let v = media_contracts::version::current_version_info();
                Json(serde_json::json!({
                    "name": brand::DEFAULT_NAME,
                    "mode": mode,
                    "version": v.version,
                    "channel": v.channel,
                    "commit": v.commit,
                    "build_date": v.build_date,
                    "release_url": v.release_url,
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
    local_security_with_store(router, origin, None, Vec::new())
}

fn local_security_with_store(
    router: Router,
    origin: String,
    store: Option<identity::IdentityStore>,
    allowed_origins: Vec<String>,
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
            let allowed_origins = allowed_origins.clone();
            async move {
                let req_path = req.uri().path();
                let method = req.method();

                let is_internal = req_path.starts_with("/internal/v1/");
                let has_internal_proto = req
                    .headers()
                    .get(media_contracts::protocol::HDR_PROTOCOL)
                    .and_then(|v| v.to_str().ok())
                    == Some(media_contracts::protocol::PROTOCOL_VERSION);
                let req_host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok());
                let host_ok = req_host == host.as_deref()
                    || (is_internal && has_internal_proto)
                    || (is_internal && internal_host.as_deref().is_some_and(|ih| req_host == Some(ih)));

                // 1. OIDC callback accepts a cross-site top-level GET (single-use state bound to HttpOnly cookie).
                let oidc_callback = method == axum::http::Method::GET
                    && req_path == "/api/v1/auth/oidc/callback"
                    && req.headers().get("sec-fetch-mode").is_none_or(|v| v == "navigate")
                    && req.headers().get("sec-fetch-dest").is_none_or(|v| v == "document");

                // 2. Safe top-level web navigation to frontend routes (/, /app, /app/*, /favicon*)
                // When an end-user navigates from an external portal (like Authentik) or third-party link,
                // browsers send sec-fetch-mode: navigate (and/or sec-fetch-dest: document).
                let is_web_route = req_path == "/"
                    || req_path.starts_with("/app")
                    || req_path.starts_with("/favicon");
                let is_top_level_navigation = (method == axum::http::Method::GET || method == axum::http::Method::HEAD)
                    && is_web_route
                    && req.headers().get("sec-fetch-mode").is_some_and(|v| v == "navigate");

                // 3. Check Origin header against primary and trusted external origins
                let origin_header = req.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok());
                let is_primary_origin = origin_header == Some(origin.as_str());
                let is_trusted_external = origin_header
                    .is_some_and(|cand| allowed_origins.iter().any(|ao| ao.as_str() == cand));
                let origin_matches = origin_header.is_none() || is_primary_origin || is_trusted_external;

                let is_cross_site = req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site");

                let allowed = host_ok
                    && (
                        is_top_level_navigation
                        || oidc_callback
                        || (!is_cross_site && origin_matches && (origin_header.is_none() || is_primary_origin))
                        || is_trusted_external
                    );

                if !allowed {
                    return StatusCode::FORBIDDEN.into_response();
                }
                let mut res = next.run(req).await;
                let mut extra_origins = String::new();
                for ao in &allowed_origins {
                    let origin_str = format!(" {ao}");
                    if !extra_origins.contains(&origin_str) {
                        extra_origins.push_str(&origin_str);
                    }
                }
                if let Some(store) = &store {
                    let _ = nodes::ensure_node_registry_warm(store).await;
                    for origin_url in store.node_registry.connected_public_origins() {
                        let origin_str = format!(" {origin_url}");
                        if !extra_origins.contains(&origin_str) {
                            extra_origins.push_str(&origin_str);
                        }
                    }
                }
                let csp = format!(
                    "default-src 'none'; script-src 'self'; worker-src 'self'; manifest-src 'self'; img-src 'self' data:{extra_origins}; media-src 'self' blob:{extra_origins}; connect-src 'self'{extra_origins}; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; frame-ancestors 'none'"
                );
                if let Ok(val) = HeaderValue::from_str(&csp) {
                    res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, val);
                }
                res
            }
        }))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
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
    fn configured_web_dist_uses_build_directory() {
        let expected = std::path::PathBuf::from("apps/web/build");
        let expected = expected.canonicalize().unwrap_or(expected);
        assert_eq!(configured_web_dist(), expected);
    }
}

pub mod extension_storage;

pub mod task_engine;
mod task_execution;
pub mod timeline_markers;

pub mod extension_data;
pub mod extension_objects;
