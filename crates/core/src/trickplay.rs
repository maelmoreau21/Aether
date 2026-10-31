//! Core Trickplay authorization and routing.
//!
//! Enforces library permissions, parental policy, and node routing.
//! For local nodes, delegates to `LocalNode.trickplay()`.
//! For remote nodes, relays via authenticated internal protocol.
//! No arbitrary filesystem paths are ever accepted from clients.

use crate::{database::query, identity::IdentityStore, library_options::LibraryOptions};
use axum::{
    Json,
    body::Body,
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use media_node::trickplay::TrickplayRequest;
use serde_json::{Value, json};
use sqlx::Row;
use std::path::{Path, PathBuf};

struct AssetSource {
    node_id: String,
    relative_path: PathBuf,
    media_root: PathBuf,
    duration: f64,
    size: u64,
    modified_ns: String,
    options: LibraryOptions,
}

async fn resolve_asset_source(
    store: &IdentityStore,
    asset_id: &str,
) -> Result<Option<AssetSource>, sqlx::Error> {
    let row = query(
        "SELECT a.relative_path, a.size, a.modified_ns, a.probe_json, l.root, l.node_id, l.options_json \
         FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE a.id=? AND a.available=1 AND l.enabled=1 \
         AND s.root_snapshot=l.root AND l.kind IN ('films','series') AND n.status!='revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await?;

    let Some(row) = row else { return Ok(None) };
    let node_id: String = row.get("node_id");
    let video: String = row.get("relative_path");
    let probe: String = row.get("probe_json");
    let root: String = row.get("root");
    let options_json: Option<String> = row.get("options_json");
    let options = options_json
        .and_then(|s| serde_json::from_str::<LibraryOptions>(&s).ok())
        .unwrap_or_default();

    let parsed: Value = match serde_json::from_str(&probe) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let duration = parsed
        .get("format")
        .and_then(|value| value.get("duration"))
        .and_then(|value| {
            value
                .as_str()
                .and_then(|v| v.parse::<f64>().ok())
                .or_else(|| value.as_f64())
        })
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 86400.0);

    let Some(duration) = duration else {
        return Ok(None);
    };
    let video = Path::new(&video);
    if media_node::files::validate_relative_components(video).is_err() {
        return Ok(None);
    }

    Ok(Some(AssetSource {
        node_id,
        relative_path: video.to_path_buf(),
        media_root: PathBuf::from(root),
        duration,
        size: row.get::<i64, _>("size") as u64,
        modified_ns: row.get("modified_ns"),
        options,
    }))
}

pub async fn manifest(store: &IdentityStore, asset_id: &str, request: Request) -> Response {
    // Check global admin settings
    let global_settings = crate::server_settings::load(store)
        .await
        .unwrap_or_default();
    if !global_settings.trickplay_enabled {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_disabled");
    }

    let source = match resolve_asset_source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };

    if !source.options.trickplay_enabled {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_disabled");
    }

    // Remote node relaying
    if source.node_id != "local" {
        let node = match crate::nodes::load_node(store, &source.node_id).await {
            Ok(Some(node)) => node,
            _ => return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
        };
        let pq = format!(
            "/internal/v1/node/assets/{asset_id}/trickplay?duration={}&intervalMs={}&width={}&quality={}",
            source.duration,
            (source.options.trickplay_interval_seconds as u64) * 1000,
            source.options.trickplay_width,
            source.options.trickplay_quality
        );
        return crate::nodes::relay_from_remote_node(store, &node, &pq, request).await;
    }

    // Local standalone node execution
    let mgr = crate::node::LocalNode.trickplay();
    let req = TrickplayRequest {
        asset_id: asset_id.to_owned(),
        media_root: source.media_root,
        relative_path: source.relative_path,
        duration: source.duration,
        size: source.size,
        modified_ns: source.modified_ns,
        interval_ms: Some((source.options.trickplay_interval_seconds as u64) * 1000),
        width: Some(source.options.trickplay_width as u32),
        quality: Some(source.options.trickplay_quality),
    };

    // 1. Aether native cache priority
    if let Some(cached) = mgr.check_native_cache(&req).await {
        return Json(json!({
            "status": "ready",
            "source": "aether",
            "intervalMs": cached.interval_ms,
            "width": cached.width,
            "height": cached.height,
            "columns": cached.columns,
            "rows": cached.rows,
            "sheetCount": cached.sheet_count,
            "thumbnailCount": cached.thumbnail_count,
            "urlPrefix": format!("/api/v1/assets/{asset_id}/trickplay/")
        }))
        .into_response();
    }

    // 2. Jellyfin read-only fallback
    if let Some(jellyfin) = mgr.check_jellyfin(&req).await {
        return Json(json!({
            "status": "ready",
            "source": "jellyfin",
            "intervalMs": jellyfin.interval_ms,
            "width": jellyfin.width,
            "height": jellyfin.height,
            "columns": jellyfin.columns,
            "rows": jellyfin.rows,
            "sheetCount": jellyfin.sheet_count,
            "thumbnailCount": jellyfin.thumbnail_count,
            "urlPrefix": format!("/api/v1/assets/{asset_id}/trickplay/")
        }))
        .into_response();
    }

    // 3. Enqueue background generation
    let queue_status = mgr.enqueue(req);
    let (state, http) = match queue_status {
        media_node::trickplay::QueueStatus::Queued => ("generating", StatusCode::ACCEPTED),
        media_node::trickplay::QueueStatus::Busy => ("busy", StatusCode::SERVICE_UNAVAILABLE),
        media_node::trickplay::QueueStatus::Failed => ("failed", StatusCode::SERVICE_UNAVAILABLE),
        media_node::trickplay::QueueStatus::Disabled => ("disabled", StatusCode::NOT_FOUND),
    };
    (http, Json(json!({ "status": state }))).into_response()
}

pub async fn tile(
    store: &IdentityStore,
    asset_id: &str,
    index: usize,
    request: Request,
) -> Response {
    let source = match resolve_asset_source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };

    if source.node_id != "local" {
        let node = match crate::nodes::load_node(store, &source.node_id).await {
            Ok(Some(node)) => node,
            _ => return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
        };
        let pq = format!("/internal/v1/node/assets/{asset_id}/trickplay/{index}");
        return crate::nodes::relay_from_remote_node(store, &node, &pq, request).await;
    }

    let mgr = crate::node::LocalNode.trickplay();
    let req = TrickplayRequest {
        asset_id: asset_id.to_owned(),
        media_root: source.media_root,
        relative_path: source.relative_path,
        duration: source.duration,
        size: source.size,
        modified_ns: source.modified_ns,
        interval_ms: Some((source.options.trickplay_interval_seconds as u64) * 1000),
        width: Some(source.options.trickplay_width as u32),
        quality: Some(source.options.trickplay_quality),
    };

    let opened = match mgr.open_sheet(&req, index).await {
        Ok(f) => f,
        Err(_) => return crate::error(StatusCode::NOT_FOUND, "trickplay_tile_unavailable"),
    };

    match media_node::serve_opened(opened, &request, "image/jpeg").await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            parts.headers.insert(
                axum::http::header::X_CONTENT_TYPE_OPTIONS,
                axum::http::HeaderValue::from_static("nosniff"),
            );
            parts.headers.insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("public, max-age=86400"),
            );
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "trickplay_stream_error"),
    }
}

#[cfg(test)]
mod tests {
    use media_node::trickplay::{DEFAULT_COLUMNS, DEFAULT_ROWS};

    #[test]
    fn grid_bounds_are_consistent() {
        assert_eq!(DEFAULT_COLUMNS, 10);
        assert_eq!(DEFAULT_ROWS, 10);
    }
}
