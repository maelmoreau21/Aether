//! Local Core: persistent authentication and isolated synthetic media inventory.

/// Every diagnostic the Core prints to stderr is also kept for the administration
/// log viewer (`server_log`). Defined before the modules so it shadows the standard
/// macro throughout the crate; output on stderr is unchanged.
macro_rules! eprintln {
    () => {
        $crate::server_log::record(String::new())
    };
    ($($arg:tt)*) => {
        $crate::server_log::record(format!($($arg)*))
    };
}

pub mod account;
pub mod acquisition;
pub mod admin_backups;
pub mod admin_users;
pub mod api_keys;
pub mod applications;
pub mod auth;
pub mod automation;
pub mod automation_execution;
pub mod automation_http;
pub mod avatars;
pub mod backup;
pub mod backup_bundle;
pub mod database;
pub mod dlna;
pub mod extensions;
pub mod groups;
pub mod oidc;
pub mod postgres_backup;
pub mod reports;
pub mod scheduler;
pub mod server_log;
pub mod server_settings;
pub mod system_memory;
pub mod system_task_queue;
pub mod system_tasks;
pub mod task_scheduler;
pub mod task_triggers;
use media_contracts::brand;
pub mod artwork;
pub mod assets;
mod catalog;
pub mod consolidation;
mod demo;
pub mod feedback;
pub mod identity;
mod image_import;
pub mod inventory;
pub mod libraries;
pub mod library_media_tasks;
pub mod library_options;
mod local_mounts;
pub mod media;
pub mod metadata_locale;
pub mod music;
pub mod node;
pub mod nodes;
pub mod notifications;
pub mod parties;
pub mod permissions;
pub mod playlists;
pub mod preferences;
pub mod product;
pub mod profiles;
pub mod progress;
mod provider_download;
pub mod recommendations;
pub mod remote;
pub mod scans;
pub mod statistics;
pub mod translations;
pub mod trickplay;
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
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};
use tower_http::set_header::SetResponseHeaderLayer;

fn media_routes(inventory: node::Inventory) -> Router {
    let is_bench = inventory.is_fixture_bench();
    let mut router = Router::new()
        .route("/api/v1/demo/media", get(demo::media))
        .route("/api/v1/catalog", get(catalog::list))
        .route("/api/v1/catalog/{id}", get(catalog::detail))
        .route("/api/v1/demo/subtitles/{name}", get(node::demo_subtitle))
        .route(
            "/mse",
            get(|| async { Html(include_str!("../experiments/mse.html")) }),
        )
        .route(
            "/shaka",
            get(|| async { Html(include_str!("../experiments/shaka.html")) }),
        )
        .route(
            "/audio-seek-policy.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../experiments/audio-seek-policy.js"),
                )
            }),
        )
        .route(
            "/shaka.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../experiments/shaka.js"),
                )
            }),
        )
        .route("/api/v1/experiment/shaka/{name}", get(node::shaka_content))
        .route(
            "/mse.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../experiments/mse.js"),
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
        router = router.route(
            "/",
            get(|| async { Html(include_str!("../experiments/bench.html")) }),
        );
    } else {
        router = router.route(
            "/bench",
            get(|| async { Html(include_str!("../experiments/bench.html")) }),
        );
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
        .route("/api/v1/assets/{id}/details", get(indexed_asset_details))
        .route("/api/v1/assets/{id}/trickplay", get(indexed_trickplay))
        .route(
            "/api/v1/assets/{id}/chapter-images",
            get(indexed_chapter_manifest),
        )
        .route(
            "/api/v1/assets/{id}/chapter-images/{index}",
            get(indexed_chapter_image).head(indexed_chapter_image),
        )
        .route(
            "/api/v1/assets/{id}/trickplay/{index}",
            get(indexed_trickplay_tile).head(indexed_trickplay_tile),
        )
        .route(
            "/api/v1/assets/{id}/image",
            get(indexed_image).head(indexed_image),
        )
        .route(
            "/api/v1/assets/{id}/hls",
            axum::routing::post(indexed_hls_grant),
        )
        .route(
            "/api/v1/assets/{id}/hls/{grant}/{component}",
            get(indexed_hls).head(indexed_hls),
        )
        .layer(axum::Extension(auth.clone()))
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
            .merge(statistics::router(auth.clone()))
            .merge(feedback::router(auth.clone()))
            .merge(recommendations::router(auth.clone()))
            .merge(notifications::router(auth.clone()))
            .merge(preferences::router(auth.clone()))
            .merge(account::router(auth.clone()))
            .merge(profiles::router(auth.clone()))
            .merge(applications::router(auth.clone()))
            .merge(acquisition::router(auth.clone()))
            .merge(reports::router(auth.clone()))
            .merge(music::router(auth.clone()))
            .merge(parties::router(auth.clone()))
            .merge(remote::router(auth.clone()))
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
            .merge(task_triggers::router(auth.clone()))
            // Operations queue (`/api/v1/admin/tasks/jobs`): without it the path fell through to
            // `/api/v1/admin/tasks/{id}` and the operations page answered 404.
            .merge(task_engine::router(auth.clone()))
            .merge(system_memory::router(auth.clone()))
            .merge(groups::router(auth.clone()))
            .merge(api_keys::router(auth.clone()))
            .merge(automation_http::router(auth.clone()))
            .merge(server_log::router(auth.clone()))
            .merge(timeline_markers::router(auth.clone()))
            .merge(media_tools::router(auth.clone()))
            .merge(extensions::router(auth.clone()))
            .merge(auth::router(auth))
            .merge(oidc),
        origin,
        Some(store_for_csp),
        allowed_origins,
    );
    Ok(health_routes(mode)
        .merge(protected)
        .layer(middleware::from_fn(server_log::trace_requests)))
}

async fn local_stream_priority(
    State(jobs): State<Arc<scans::ScanJobs>>,
    request: Request,
    next: Next,
) -> Response {
    media_node::trickplay::global_manager().set_active_streams(scans::LOCAL_STREAMS.clone());
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
    axum::extract::Path((id, index)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Response {
    if let Some(download) = index.strip_prefix("downloaded-") {
        return extensions::subtitles::content(
            &store,
            &id,
            download,
            request.method() == axum::http::Method::HEAD,
        )
        .await;
    }
    match index.parse::<usize>() {
        Ok(index) => inventory::subtitle(&store, &id, index, request).await,
        Err(_) => error(StatusCode::NOT_FOUND, "unknown_subtitle"),
    }
}

async fn indexed_prepared(
    axum::Extension(preparations): axum::Extension<Arc<media_node::packaging::Preparations>>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, component)): axum::extract::Path<(String, String)>,
    request: Request,
) -> Response {
    inventory::prepared(&store, &preparations, &id, &component, request).await
}

/// Issues the capability URL a native player or a cast device opens:
/// `{origin}/api/v1/assets/{id}/hls/{grant}/master.m3u8`.
async fn indexed_hls_grant(
    axum::Extension(auth): axum::Extension<auth::AuthState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    let Some(identity) = request.extensions().get::<auth::Authenticated>().cloned() else {
        return error(
            axum::http::StatusCode::UNAUTHORIZED,
            "authentication_required",
        );
    };
    if let Err(status) = auth.require_csrf(request.headers(), &identity) {
        return error(status, "forbidden");
    }
    match auth.issue_playback_grant(&identity, &id).await {
        Ok((grant, expires_at)) => {
            let url = format!(
                "{}/api/v1/assets/{id}/hls/{grant}/master.m3u8",
                auth.origin()
            );
            let mut response =
                Json(serde_json::json!({"url": url, "expiresAt": expires_at})).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Err(status) => error(status, "playback_grant_unavailable"),
    }
}

async fn indexed_hls(
    axum::Extension(preparations): axum::Extension<Arc<media_node::packaging::Preparations>>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, _grant, component)): axum::extract::Path<(String, String, String)>,
    request: Request,
) -> Response {
    if component == "master.m3u8" {
        return inventory::hls_master(&store, &id, request.method()).await;
    }
    if let Some((embedded, index, playlist)) = inventory::hls_subtitle_component(&component) {
        if playlist {
            let stem = component.trim_end_matches(".m3u8");
            return inventory::hls_subtitle_playlist(&store, &id, stem, request.method()).await;
        }
        // Same handlers, same library policy as the browser player.
        return if embedded {
            let prepared = format!("subtitle-{index}");
            inventory::prepared(&store, &preparations, &id, &prepared, request).await
        } else {
            inventory::subtitle(&store, &id, index, request).await
        };
    }
    if media_node::hls::request(&component).is_none() {
        return error(axum::http::StatusCode::NOT_FOUND, "hls_component_unknown");
    }
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

async fn indexed_trickplay(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request,
) -> Response {
    trickplay::manifest(&store, &id, request).await
}

async fn indexed_chapter_manifest(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    trickplay::chapter_manifest(&store, &id).await
}

async fn indexed_chapter_image(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, index)): axum::extract::Path<(String, usize)>,
    request: Request,
) -> Response {
    trickplay::chapter_image(&store, &id, index, request).await
}

async fn indexed_trickplay_tile(
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path((id, index)): axum::extract::Path<(String, usize)>,
    request: Request,
) -> Response {
    trickplay::tile(&store, &id, index, request).await
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

/// Technical sheet of one asset; Node-side paths only for administrators.
async fn indexed_asset_details(
    axum::Extension(identity): axum::Extension<auth::Authenticated>,
    axum::Extension(store): axum::Extension<identity::IdentityStore>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let administrator = identity.user.role == identity::Role::Administrator;
    match inventory::asset_details(&store, &id, administrator).await {
        Ok(Some(details)) => (
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(details),
        )
            .into_response(),
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
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
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
                "/setup",
                get(|| async { axum::response::Redirect::temporary("/app/setup/") }),
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

fn is_safe_csp_origin(origin: &str) -> bool {
    if !(origin.starts_with("http://") || origin.starts_with("https://")) {
        return false;
    }
    if origin.contains(|c: char| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                ';' | ',' | '\'' | '"' | '<' | '>' | '`' | '\\' | '(' | ')' | '*' | '?' | '#'
            )
    }) {
        return false;
    }
    if let Ok(parsed) = url::Url::parse(origin) {
        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return false;
        }
        if parsed.host_str().is_none() {
            return false;
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return false;
        }
        if parsed.path() != "" && parsed.path() != "/" {
            return false;
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return false;
        }
        true
    } else {
        false
    }
}

fn csp_origin_string(raw: &str) -> Option<String> {
    if !is_safe_csp_origin(raw) {
        return None;
    }
    let parsed = url::Url::parse(raw).ok()?;
    let host = parsed.host_str()?;
    let scheme = parsed.scheme();
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some(format!(" {scheme}://{host}{port}"))
}

const ALT_SVC_HEADER: axum::http::HeaderName = axum::http::HeaderName::from_static("alt-svc");

#[derive(Clone, Debug, PartialEq, Eq)]
enum AltSvcPolicy {
    Disabled,
    Always(HeaderValue),
    WhenHttps(HeaderValue),
}

fn is_https_url(raw: &str) -> bool {
    raw.trim()
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn default_h3_alt_svc(public_url: &str) -> HeaderValue {
    let port = public_url
        .parse::<axum::http::Uri>()
        .ok()
        .and_then(|uri| {
            let https = uri
                .scheme_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("https"));
            https.then(|| uri.port_u16().unwrap_or(443))
        })
        .unwrap_or(443);
    let value = format!("h3=\":{port}\"; ma=86400");
    HeaderValue::from_str(&value)
        .unwrap_or_else(|_| HeaderValue::from_static("h3=\":443\"; ma=86400"))
}

fn resolve_alt_svc_policy(
    configured: Option<&str>,
    origin: &str,
    media_public_url: Option<&str>,
) -> AltSvcPolicy {
    if let Some(raw) = configured {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("off") {
            return AltSvcPolicy::Disabled;
        }
        return match HeaderValue::from_str(trimmed) {
            Ok(val) => AltSvcPolicy::Always(val),
            Err(_) => AltSvcPolicy::Disabled,
        };
    }
    if is_https_url(origin) {
        return AltSvcPolicy::Always(default_h3_alt_svc(origin));
    }
    if let Some(public_url) = media_public_url.filter(|v| is_https_url(v)) {
        return AltSvcPolicy::Always(default_h3_alt_svc(public_url));
    }
    AltSvcPolicy::WhenHttps(HeaderValue::from_static("h3=\":443\"; ma=86400"))
}

fn normalize_authority(raw: &str, default_https: bool) -> &str {
    if default_https {
        raw.strip_suffix(":443").unwrap_or(raw)
    } else {
        raw.strip_suffix(":80").unwrap_or(raw)
    }
}

fn authority_matches(candidate: Option<&str>, expected: Option<&str>, default_https: bool) -> bool {
    match (candidate, expected) {
        (Some(cand), Some(exp)) => normalize_authority(cand, default_https)
            .eq_ignore_ascii_case(normalize_authority(exp, default_https)),
        _ => false,
    }
}

fn origin_matches_primary(origin_header: &str, primary_origin: &str) -> bool {
    if origin_header == primary_origin {
        return true;
    }
    let Ok(cand) = origin_header.parse::<axum::http::Uri>() else {
        return false;
    };
    let Ok(exp) = primary_origin.parse::<axum::http::Uri>() else {
        return false;
    };
    let (Some(cs), Some(es)) = (cand.scheme_str(), exp.scheme_str()) else {
        return false;
    };
    if !cs.eq_ignore_ascii_case(es) {
        return false;
    }
    let default_https = es.eq_ignore_ascii_case("https");
    authority_matches(
        cand.authority().map(|a| a.as_str()),
        exp.authority().map(|a| a.as_str()),
        default_https,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrustedProxyCidr {
    network: std::net::IpAddr,
    prefix: u8,
}

impl TrustedProxyCidr {
    fn parse(raw: &str) -> Option<Self> {
        let (address, prefix) = match raw.split_once('/') {
            Some((address, prefix)) => (address, Some(prefix.parse::<u8>().ok()?)),
            None => (raw, None),
        };
        let ip: std::net::IpAddr = address.trim().parse().ok()?;
        let network = ip.to_canonical();
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = prefix.unwrap_or(max);
        (prefix <= max).then_some(Self { network, prefix })
    }

    fn contains(&self, ip: std::net::IpAddr) -> bool {
        match (self.network, ip.to_canonical()) {
            (std::net::IpAddr::V4(network), std::net::IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(ip) & mask
            }
            (std::net::IpAddr::V6(network), std::net::IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

const DEFAULT_TRUSTED_PROXY_CIDRS: [&str; 7] = [
    "127.0.0.0/8",
    "::1/128",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "fc00::/7",
    "fe80::/10",
];

fn trusted_proxy_cidrs_from(raw: Option<&str>) -> Vec<TrustedProxyCidr> {
    let Some(raw) = raw.map(str::trim) else {
        return DEFAULT_TRUSTED_PROXY_CIDRS
            .iter()
            .filter_map(|c| TrustedProxyCidr::parse(c))
            .collect();
    };
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(TrustedProxyCidr::parse)
        .collect()
}

static TRUSTED_PROXY_CIDRS: std::sync::LazyLock<Vec<TrustedProxyCidr>> =
    std::sync::LazyLock::new(|| {
        trusted_proxy_cidrs_from(std::env::var("MEDIA_TRUSTED_PROXIES").ok().as_deref())
    });

fn request_is_https(
    headers: &axum::http::HeaderMap,
    uri: &axum::http::Uri,
    peer_ip: Option<std::net::IpAddr>,
) -> bool {
    request_is_https_with_trusted(headers, uri, peer_ip, &TRUSTED_PROXY_CIDRS)
}

fn request_is_https_with_trusted(
    headers: &axum::http::HeaderMap,
    uri: &axum::http::Uri,
    peer_ip: Option<std::net::IpAddr>,
    trusted: &[TrustedProxyCidr],
) -> bool {
    if uri
        .scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("https"))
    {
        return true;
    }
    // Only honour forwarded proto headers from a known trusted TCP proxy peer; direct
    // untrusted clients (or requests without peer identity) cannot trigger `Alt-Svc`.
    let Some(peer) = peer_ip else {
        return false;
    };
    if !trusted.iter().any(|cidr| cidr.contains(peer)) {
        return false;
    }
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        && proto.trim().eq_ignore_ascii_case("https")
    {
        return true;
    }
    if let Some(forwarded) = headers.get("forwarded").and_then(|v| v.to_str().ok()) {
        for element in forwarded.split(',') {
            for param in element.split(';') {
                if let Some((key, val)) = param.trim().split_once('=')
                    && key.trim().eq_ignore_ascii_case("proto")
                    && val.trim().trim_matches('"').eq_ignore_ascii_case("https")
                {
                    return true;
                }
            }
        }
    }
    false
}

fn strip_h3_forbidden_hop_by_hop_headers(headers: &mut axum::http::HeaderMap) {
    // RFC 9114 §4.2 forbids connection-specific headers in HTTP/3 (and RFC 9113 in HTTP/2).
    const FORBIDDEN: [&str; 5] = [
        "connection",
        "keep-alive",
        "proxy-connection",
        "transfer-encoding",
        "upgrade",
    ];
    for name in FORBIDDEN {
        headers.remove(name);
    }
}

fn finalize_http3_headers(res: &mut Response, is_https_req: bool, alt_svc_policy: &AltSvcPolicy) {
    if res.status() != StatusCode::SWITCHING_PROTOCOLS {
        strip_h3_forbidden_hop_by_hop_headers(res.headers_mut());
    }
    if is_https_req
        && let AltSvcPolicy::WhenHttps(val) = alt_svc_policy
        && !res.headers().contains_key("alt-svc")
    {
        res.headers_mut().insert(ALT_SVC_HEADER, val.clone());
    }
}

struct CachedCsp {
    cors_hosts: Vec<String>,
    node_origins: Vec<String>,
    header: Option<HeaderValue>,
}

fn build_csp_header(extra_origins: &str) -> Option<HeaderValue> {
    let csp = format!(
        "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; manifest-src 'self'; font-src 'self'; img-src 'self' data:{extra_origins}; media-src 'self' blob:{extra_origins}; connect-src 'self'{extra_origins}; style-src 'self'; style-src-attr 'unsafe-hashes' 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
    );
    HeaderValue::from_str(&csp).ok()
}

pub fn local_security_with_store(
    router: Router,
    origin: String,
    store: Option<identity::IdentityStore>,
    allowed_origins: Vec<String>,
) -> Router {
    let origin_uri = origin.parse::<axum::http::Uri>().ok();
    let origin_is_https = origin_uri
        .as_ref()
        .and_then(|uri| uri.scheme_str())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"));
    let host = origin_uri
        .as_ref()
        .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
    let internal_uri = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .and_then(|u| u.parse::<axum::http::Uri>().ok());
    let internal_is_https = internal_uri
        .as_ref()
        .and_then(|uri| uri.scheme_str())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"));
    let internal_host = internal_uri
        .as_ref()
        .and_then(|uri| uri.authority().map(|value| value.as_str().to_owned()));
    let configured_alt_svc = std::env::var("MEDIA_ALT_SVC").ok();
    let public_url_env = std::env::var("MEDIA_PUBLIC_URL").ok();
    let alt_svc_policy = resolve_alt_svc_policy(
        configured_alt_svc.as_deref(),
        &origin,
        public_url_env.as_deref(),
    );
    let mut base_extra_origins = String::new();
    for ao in &allowed_origins {
        if let Some(origin_str) = csp_origin_string(ao)
            && !base_extra_origins.contains(&origin_str)
        {
            base_extra_origins.push_str(&origin_str);
        }
    }
    let base_csp = build_csp_header(&base_extra_origins);
    let csp_cache = Arc::new(RwLock::new(CachedCsp {
        cors_hosts: Vec::new(),
        node_origins: Vec::new(),
        header: base_csp.clone(),
    }));
    let node_registry_warmed = Arc::new(AtomicBool::new(false));
    let layer_alt_svc = match &alt_svc_policy {
        AltSvcPolicy::Always(val) => Some(val.clone()),
        _ => None,
    };
    let router = router
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let host = host.clone();
            let internal_host = internal_host.clone();
            let origin = origin.clone();
            let store = store.clone();
            let allowed_origins = allowed_origins.clone();
            let alt_svc_policy = alt_svc_policy.clone();
            let base_extra_origins = base_extra_origins.clone();
            let base_csp = base_csp.clone();
            let csp_cache = csp_cache.clone();
            let node_registry_warmed = node_registry_warmed.clone();
            async move {
                let req_path = req.uri().path();
                let method = req.method();
                let peer_ip = req
                    .extensions()
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|info| info.0.ip());
                let is_https_req =
                    origin_is_https || request_is_https(req.headers(), req.uri(), peer_ip);

                let is_internal = req_path.starts_with("/internal/v1/");
                let has_internal_proto = req
                    .headers()
                    .get(media_contracts::protocol::HDR_PROTOCOL)
                    .and_then(|v| v.to_str().ok())
                    == Some(media_contracts::protocol::PROTOCOL_VERSION);
                let host_header = req.headers().get(header::HOST).and_then(|h| h.to_str().ok());
                let uri_authority = req.uri().authority().map(|a| a.as_str());
                // Support HTTP/2 and HTTP/3 `:authority` when `Host` is omitted, while
                // rejecting conflicting `Host` vs `:authority` smuggling.
                let authority_matches_request = match (host_header, uri_authority) {
                    (Some(h), Some(a)) => {
                        let public_ok = authority_matches(Some(h), Some(a), origin_is_https);
                        let mut internal_ok = false;
                        if is_internal {
                            internal_ok = authority_matches(Some(h), Some(a), internal_is_https);
                        }
                        public_ok || internal_ok
                    }
                    _ => true,
                };
                let req_host = host_header.or(uri_authority);
                let mut internal_host_ok = false;
                if is_internal
                    && let Some(ih) = internal_host.as_deref()
                {
                    internal_host_ok = authority_matches(req_host, Some(ih), internal_is_https);
                }
                let host_ok = authority_matches_request
                    && (authority_matches(req_host, host.as_deref(), origin_is_https)
                        || (is_internal && has_internal_proto)
                        || internal_host_ok);

                // 0. HLS capability URLs are opened by cast receivers on another origin
                // (Chromecast's page, AirPlay). They carry their own authorization and no
                // cookie is ever read for them, so any origin may read them, uncredentialed.
                if host_ok
                    && matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS)
                    && permissions::hls_grant(req_path).is_some()
                {
                    let mut res = if method == axum::http::Method::OPTIONS {
                        let mut res = StatusCode::NO_CONTENT.into_response();
                        res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, HEAD, OPTIONS"));
                        res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("range"));
                        res.headers_mut().insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
                        res
                    } else {
                        next.run(req).await
                    };
                    res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
                    res.headers_mut().insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("content-length, content-range"));
                    finalize_http3_headers(&mut res, is_https_req, &alt_svc_policy);
                    return res;
                }

                // 1. OIDC callback accepts a cross-site top-level GET (single-use state bound to HttpOnly cookie).
                let oidc_callback = method == axum::http::Method::GET
                    && req_path == "/api/v1/auth/oidc/callback"
                    && req.headers().get("sec-fetch-mode").is_none_or(|v| v == "navigate")
                    && req.headers().get("sec-fetch-dest").is_none_or(|v| v == "document");

                // 1b. A native application opens the sign-in in the phone's browser (a Custom
                // Tab launched by the app is a cross-site top-level navigation). It only
                // redirects to the provider; the handler validates challenge and redirect.
                let oidc_app_start = method == axum::http::Method::GET
                    && req_path == "/api/v1/auth/oidc/app/start"
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

                if let Some(s) = &store {
                    crate::server_settings::ensure_cors_origins_warm(s).await;
                }
                // 3. Check Origin header against primary and trusted external origins
                let origin_header = req
                    .headers()
                    .get(header::ORIGIN)
                    .and_then(|h| h.to_str().ok())
                    .map(|s| s.to_owned());
                let is_primary_origin = origin_header
                    .as_deref()
                    .is_some_and(|cand| origin_matches_primary(cand, &origin));
                let (is_explicit_cors_allowed, is_wildcard_cors_allowed) = store
                    .as_ref()
                    .and_then(|s| s.cors_origins.read().ok())
                    .map(|hosts| {
                        let has_wildcard = hosts.iter().any(|h| h == "*");
                        let matches_explicit = origin_header
                            .as_deref()
                            .is_some_and(|cand| hosts.iter().any(|h| h != "*" && h == cand));
                        (matches_explicit, has_wildcard)
                    })
                    .unwrap_or((false, false));
                let is_allowed_origin = origin_header
                    .as_deref()
                    .is_some_and(|cand| allowed_origins.iter().any(|ao| ao.as_str() == cand));
                let is_credentialed_external = is_allowed_origin || is_explicit_cors_allowed;
                let is_trusted_external = is_credentialed_external || is_wildcard_cors_allowed;
                let origin_matches = origin_header.is_none() || is_primary_origin || is_trusted_external;

                let is_cross_site = req.headers().get("sec-fetch-site").is_some_and(|h| h == "cross-site");

                let allowed = host_ok
                    && (
                        is_top_level_navigation
                        || oidc_callback
                        || oidc_app_start
                        || (!is_cross_site && origin_matches && (origin_header.is_none() || is_primary_origin))
                        || is_trusted_external
                    );

                if !allowed {
                    return StatusCode::FORBIDDEN.into_response();
                }

                if method == axum::http::Method::OPTIONS && origin_matches && origin_header.is_some() {
                    let mut res = StatusCode::NO_CONTENT.into_response();
                    let headers = res.headers_mut();
                    if is_primary_origin || is_credentialed_external {
                        if let Some(oh) = &origin_header
                            && let Ok(val) = HeaderValue::from_str(oh)
                        {
                            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, val);
                        }
                        headers.insert(
                            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                            HeaderValue::from_static("true"),
                        );
                    } else if is_wildcard_cors_allowed {
                        headers.insert(
                            header::ACCESS_CONTROL_ALLOW_ORIGIN,
                            HeaderValue::from_static("*"),
                        );
                    }
                    headers.insert(
                        header::ACCESS_CONTROL_ALLOW_METHODS,
                        HeaderValue::from_static("GET, POST, PUT, DELETE, PATCH, OPTIONS"),
                    );
                    headers.insert(
                        header::ACCESS_CONTROL_ALLOW_HEADERS,
                        HeaderValue::from_static("content-type, authorization, x-csrf-token, range, x-playback-session-id"),
                    );
                    headers.insert(
                        header::ACCESS_CONTROL_MAX_AGE,
                        HeaderValue::from_static("86400"),
                    );
                    finalize_http3_headers(&mut res, is_https_req, &alt_svc_policy);
                    return res;
                }

                let mut res = next.run(req).await;
                if is_primary_origin || is_credentialed_external {
                    if let Some(oh) = &origin_header
                        && let Ok(val) = HeaderValue::from_str(oh)
                    {
                        res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, val);
                    }
                    res.headers_mut().insert(
                        header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                        HeaderValue::from_static("true"),
                    );
                    res.headers_mut().insert(
                        header::ACCESS_CONTROL_EXPOSE_HEADERS,
                        HeaderValue::from_static("content-range, content-length, etag"),
                    );
                } else if is_wildcard_cors_allowed {
                    res.headers_mut().insert(
                        header::ACCESS_CONTROL_ALLOW_ORIGIN,
                        HeaderValue::from_static("*"),
                    );
                    res.headers_mut().insert(
                        header::ACCESS_CONTROL_EXPOSE_HEADERS,
                        HeaderValue::from_static("content-range, content-length, etag"),
                    );
                }
                let csp_header = if let Some(store) = &store {
                    if !node_registry_warmed.load(Ordering::Acquire)
                        && nodes::ensure_node_registry_warm(store).await.is_ok()
                    {
                        node_registry_warmed.store(true, Ordering::Release);
                    }
                    let node_origins = store.node_registry.connected_public_origins();
                    let (cached_hit, cors_hosts) = match store.cors_origins.read() {
                        Ok(hosts) => {
                            let hit = csp_cache.read().ok().and_then(|guard| {
                                (guard.cors_hosts.as_slice() == hosts.as_slice()
                                    && guard.node_origins.as_slice() == node_origins.as_slice())
                                .then(|| guard.header.clone())
                            });
                            if let Some(cached) = hit {
                                (Some(cached), Vec::new())
                            } else {
                                (None, hosts.clone())
                            }
                        }
                        Err(_) => (None, Vec::new()),
                    };
                    if let Some(cached) = cached_hit {
                        cached
                    } else {
                        let mut extra_origins = base_extra_origins.clone();
                        for host in &cors_hosts {
                            if host != "*"
                                && let Some(origin_str) = csp_origin_string(host)
                                && !extra_origins.contains(&origin_str)
                            {
                                extra_origins.push_str(&origin_str);
                            }
                        }
                        for origin_url in &node_origins {
                            if let Some(origin_str) = csp_origin_string(origin_url)
                                && !extra_origins.contains(&origin_str)
                            {
                                extra_origins.push_str(&origin_str);
                            }
                        }
                        let computed = build_csp_header(&extra_origins);
                        if let Ok(mut guard) = csp_cache.write() {
                            guard.cors_hosts = cors_hosts;
                            guard.node_origins = node_origins;
                            guard.header = computed.clone();
                        }
                        computed
                    }
                } else {
                    base_csp.clone()
                };
                if let Some(val) = csp_header {
                    res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, val);
                }
                finalize_http3_headers(&mut res, is_https_req, &alt_svc_policy);
                res
            }
        }))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin")))
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .route("/bench.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("../experiments/bench.js")) }));
    if let Some(alt_svc) = layer_alt_svc {
        let h3_layer = SetResponseHeaderLayer::if_not_present(ALT_SVC_HEADER, alt_svc);
        router.layer(h3_layer)
    } else {
        router
    }
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

    #[test]
    fn csp_origin_sanitizer_rejects_injections() {
        assert!(is_safe_csp_origin("http://localhost:3000"));
        assert!(is_safe_csp_origin("https://example.com"));
        assert!(is_safe_csp_origin("https://sub.domain.com:8443"));

        // Injections & malicious formats
        assert!(!is_safe_csp_origin("https://"));
        assert!(!is_safe_csp_origin("http://attacker.com; script-src *"));
        assert!(!is_safe_csp_origin("javascript:alert(1)"));
        assert!(!is_safe_csp_origin("data:text/html"));
        assert!(!is_safe_csp_origin("http://evil.com/'"));
        assert!(!is_safe_csp_origin("http://evil.com/\""));
        assert!(!is_safe_csp_origin("http://evil.com/with space"));
        assert!(!is_safe_csp_origin("https://evil.com\r\nInjected: yes"));
        assert!(!is_safe_csp_origin("ftp://example.com"));
        assert!(!is_safe_csp_origin("http://evil.com/path"));
        assert!(!is_safe_csp_origin("http://evil.com?query=1"));
        assert!(!is_safe_csp_origin("http://evil.com#hash"));
        assert!(!is_safe_csp_origin("http://user:pass@evil.com"));
        assert!(!is_safe_csp_origin("http://*"));
        assert!(!is_safe_csp_origin("http://evil.com(test)"));
        assert!(!is_safe_csp_origin("http://evil.com\\test"));

        assert_eq!(
            csp_origin_string("https://example.com/"),
            Some(" https://example.com".to_string())
        );
        assert_eq!(
            csp_origin_string("http://localhost:3000"),
            Some(" http://localhost:3000".to_string())
        );
        assert_eq!(csp_origin_string("https://"), None);
        assert_eq!(csp_origin_string("http://evil.com/path"), None);
    }

    #[tokio::test]
    async fn local_security_accepts_http3_authority_without_host_and_blocks_conflicts() {
        use axum::body::Body;
        use tower::ServiceExt as _;

        let inner = Router::new().route("/ping", get(|| async { "pong" }));
        let app =
            local_security_with_store(inner, "https://media.example.com".into(), None, Vec::new());

        // HTTP/2 & HTTP/3 `:authority` only (no Host header).
        let req = Request::builder()
            .uri("https://media.example.com/ping")
            .header(header::ORIGIN, "https://media.example.com:443")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("alt-svc").and_then(|v| v.to_str().ok()),
            Some("h3=\":443\"; ma=86400")
        );

        // Mismatched Host vs `:authority` smuggling attempt must be rejected.
        let conflict = Request::builder()
            .uri("https://media.example.com/ping")
            .header(header::HOST, "evil.attacker.com")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(conflict).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // Neither Host nor `:authority` must be rejected.
        let missing = Request::builder().uri("/ping").body(Body::empty()).unwrap();
        let res = app.oneshot(missing).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn local_security_alt_svc_and_strips_http3_hop_by_hop_headers() {
        use axum::body::Body;
        use tower::ServiceExt as _;

        assert_eq!(
            resolve_alt_svc_policy(Some("off"), "https://media.example.com", None),
            AltSvcPolicy::Disabled
        );
        assert_eq!(
            resolve_alt_svc_policy(Some(""), "https://media.example.com", None),
            AltSvcPolicy::Disabled
        );
        assert_eq!(
            resolve_alt_svc_policy(None, "https://media.example.com:8443", None),
            AltSvcPolicy::Always(HeaderValue::from_static("h3=\":8443\"; ma=86400"))
        );
        assert_eq!(
            resolve_alt_svc_policy(
                None,
                "http://127.0.0.1:3000",
                Some("https://edge.example.com:9443")
            ),
            AltSvcPolicy::Always(HeaderValue::from_static("h3=\":9443\"; ma=86400"))
        );

        let inner = Router::new().route(
            "/hop",
            get(|| async {
                let mut res = Response::new(Body::empty());
                let headers = res.headers_mut();
                headers.insert(
                    axum::http::HeaderName::from_static("connection"),
                    HeaderValue::from_static("keep-alive"),
                );
                headers.insert(
                    axum::http::HeaderName::from_static("keep-alive"),
                    HeaderValue::from_static("timeout=5"),
                );
                headers.insert(
                    axum::http::HeaderName::from_static("proxy-connection"),
                    HeaderValue::from_static("keep-alive"),
                );
                headers.insert(
                    axum::http::HeaderName::from_static("transfer-encoding"),
                    HeaderValue::from_static("chunked"),
                );
                headers.insert(
                    axum::http::HeaderName::from_static("upgrade"),
                    HeaderValue::from_static("h2c"),
                );
                res
            }),
        );
        let app =
            local_security_with_store(inner, "http://127.0.0.1:3000".into(), None, Vec::new());

        // Plain HTTP behind trusted reverse proxy signalling HTTPS triggers Alt-Svc & strips hop-by-hop headers.
        let mut proxied_https = Request::builder()
            .uri("/hop")
            .header(header::HOST, "127.0.0.1:3000")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        proxied_https
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                44321,
            ))));
        let res = app.clone().oneshot(proxied_https).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("alt-svc").and_then(|v| v.to_str().ok()),
            Some("h3=\":443\"; ma=86400")
        );
        for forbidden in [
            "connection",
            "keep-alive",
            "proxy-connection",
            "transfer-encoding",
            "upgrade",
        ] {
            assert!(!res.headers().contains_key(forbidden), "{forbidden} leaked");
        }

        // Untrusted TCP peer or missing ConnectInfo cannot spoof x-forwarded-proto/forwarded to emit Alt-Svc.
        let mut untrusted_https = Request::builder()
            .uri("/hop")
            .header(header::HOST, "127.0.0.1:3000")
            .header("x-forwarded-proto", "https")
            .header("forwarded", "proto=https")
            .body(Body::empty())
            .unwrap();
        untrusted_https
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [203, 0, 113, 9],
                44321,
            ))));
        let res = app.clone().oneshot(untrusted_https).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res.headers().contains_key("alt-svc"));

        let missing_peer_https = Request::builder()
            .uri("/hop")
            .header(header::HOST, "127.0.0.1:3000")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(missing_peer_https).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res.headers().contains_key("alt-svc"));

        // Plain HTTP without HTTPS proxy proto does not advertise h3 unless explicitly configured.
        let plain_http = Request::builder()
            .uri("/hop")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(plain_http).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res.headers().contains_key("alt-svc"));

        // MEDIA_TRUSTED_PROXIES=none rejects forwarded proto even from loopback; https:// URI still passes.
        let no_proxies = trusted_proxy_cidrs_from(Some("none"));
        let mut hdrs = axum::http::HeaderMap::new();
        hdrs.insert("x-forwarded-proto", "https".parse().unwrap());
        let http_uri: axum::http::Uri = "/hop".parse().unwrap();
        assert!(!request_is_https_with_trusted(
            &hdrs,
            &http_uri,
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
            &no_proxies,
        ));
        let https_uri: axum::http::Uri = "https://127.0.0.1:3000/hop".parse().unwrap();
        assert!(request_is_https_with_trusted(
            &hdrs,
            &https_uri,
            Some(std::net::IpAddr::from([203, 0, 113, 9])),
            &no_proxies,
        ));
    }

    #[tokio::test]
    async fn local_security_caches_csp_and_reflects_cors_changes() {
        use axum::body::Body;
        use tower::ServiceExt as _;

        let temp = tempfile::tempdir().unwrap();
        let store = identity::IdentityStore::open(&temp.path().join("csp.sqlite"))
            .await
            .unwrap();
        let inner = Router::new().route("/ping", get(|| async { "pong" }));
        let app = local_security_with_store(
            inner,
            "http://127.0.0.1:3000".into(),
            Some(store.clone()),
            vec!["https://static.example.com".into()],
        );

        let req1 = Request::builder()
            .uri("/ping")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(res1.status(), StatusCode::OK);
        let csp1 = res1
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .unwrap()
            .to_owned();
        assert!(csp1.contains("https://static.example.com"));

        // Second request hits cached CSP HeaderValue identically.
        let req2 = Request::builder()
            .uri("/ping")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res2 = app.clone().oneshot(req2).await.unwrap();
        let csp2 = res2
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok());
        assert_eq!(csp2, Some(csp1.as_str()));

        // Updating store.cors_origins invalidates the cached CSP header immediately.
        {
            let mut guard = store.cors_origins.write().unwrap();
            guard.push("https://cdn.example.com:8443".into());
        }
        let req3 = Request::builder()
            .uri("/ping")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res3 = app.oneshot(req3).await.unwrap();
        let csp3 = res3
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
            .unwrap();
        assert!(csp3.contains("https://static.example.com"));
        assert!(csp3.contains("https://cdn.example.com:8443"));
    }
}

pub mod media_tools;
pub mod task_engine;
mod task_execution;
pub mod timeline_markers;
