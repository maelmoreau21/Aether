use super::*;
use crate::auth::AuthState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, header},
    routing::{get, put},
};

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .merge(repositories::router())
        .merge(collections::router())
        .merge(subtitles::router())
        .route("/api/v1/admin/extensions", get(list).post(install))
        .route(
            "/api/v1/admin/extensions/storage/health",
            get(storage_health),
        )
        .route(
            "/api/v1/admin/extensions/publishers",
            get(publishers).post(trust),
        )
        .route(
            "/api/v1/admin/extensions/publishers/{id}",
            axum::routing::delete(revoke_publisher),
        )
        .route("/api/v1/admin/extensions/runs", get(runs))
        .route("/api/v1/admin/extensions/audit", get(audit_log))
        .route(
            "/api/v1/admin/extensions/images/collect",
            axum::routing::post(image_collection::collect),
        )
        .route(
            "/api/v1/admin/extensions/images/candidates",
            axum::routing::post(image_collection::search_candidates),
        )
        .route(
            "/api/v1/admin/extensions/{id}/image-preview",
            get(adapters::preview_image),
        )
        .route(
            "/api/v1/works/{work}/people/{key}/image",
            get(person_images::read),
        )
        .route(
            "/api/v1/admin/extensions/identify/search",
            axum::routing::post(identify::search),
        )
        .route(
            "/api/v1/admin/extensions/identify/apply",
            axum::routing::post(identify::apply),
        )
        .route(
            "/api/v1/admin/artwork/rotate",
            axum::routing::post(artwork_rotation::rotate),
        )
        .route(
            "/api/v1/admin/artwork/jobs",
            get(super::artwork_jobs::list).post(super::artwork_jobs::create),
        )
        .route(
            "/api/v1/admin/artwork/jobs/{id}",
            axum::routing::put(super::artwork_jobs::control),
        )
        .route(
            "/api/v1/admin/artwork/collect-batch",
            axum::routing::post(image_collection::collect_batch),
        )
        .route(
            "/api/v1/admin/artwork/works",
            get(image_collection::list_works),
        )
        .route(
            "/api/v1/admin/extensions/{id}",
            put(configure).delete(uninstall),
        )
        .route(
            "/api/v1/admin/extensions/{id}/upgrade",
            axum::routing::post(upgrade),
        )
        .route(
            "/api/v1/admin/extensions/{id}/rollback",
            axum::routing::post(updates::rollback),
        )
        .route(
            "/api/v1/admin/extensions/{id}/settings",
            get(get_settings).put(put_settings),
        )
        .route(
            "/api/v1/admin/extensions/{id}/run",
            axum::routing::post(run),
        )
        .route(
            "/api/v1/admin/extensions/{id}/apply",
            axum::routing::post(apply_metadata),
        )
        .route("/api/v1/admin/extensions/{id}/works", get(works))
        .route(
            "/api/v1/admin/extensions/{id}/image",
            axum::routing::post(adapters::import_image),
        )
        .route(
            "/api/v1/account/avatar/extensions",
            get(adapters::avatar_extensions),
        )
        .route(
            "/api/v1/account/avatar/extensions/{id}",
            axum::routing::post(adapters::avatar_operation),
        )
        // A signed package carries its module in base64, beside its manifest and key.
        .layer(DefaultBodyLimit::max(
            media_extension_sdk::MAX_MODULE * 4 / 3 + 262_144,
        ))
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            ),
        )
        .with_state(auth)
}

/// Recover stale invocation history without interrupting another active Core.
pub async fn initialize(auth: &AuthState) -> Result<(), sqlx::Error> {
    run_ownership::recover(&auth.store).await?;
    Ok(())
}
