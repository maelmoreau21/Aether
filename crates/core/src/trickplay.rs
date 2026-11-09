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

/// Owners of the preview warm-ups queued by a scan publication.
pub(crate) const SCAN_OWNERS: [&str; 2] = ["library-trickplay", "library-chapter-images"];
/// Owners of the preview warm-ups an administrator started from the task scheduler. They
/// run even when the library does not prepare previews during scans.
pub(crate) const MANUAL_OWNERS: [&str; 2] =
    ["library-trickplay-manual", "library-chapter-images-manual"];

/// Which files a preview pass covers.
#[derive(Clone, Copy)]
pub(crate) enum PreviewScope<'a> {
    /// The files of one scan publication (new, changed and unchanged files alike).
    Scan(&'a str),
    /// Every available video of a library and of its active folders.
    Library,
}

/// Persist preview warm-ups with the inventory publication. They use the existing
/// Node executor, leases, queue limit and playback preemption, never the scan worker.
/// A full rescan ("Tout réanalyser") re-arms finished warm-ups too: the Node keeps a
/// prepared cache and returns at once, and regenerates previews deleted from its cache.
pub(crate) async fn queue_scanned_assets(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    library: &str,
    scan: &str,
    options: &LibraryOptions,
    refresh_mode: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    queue_previews(
        tx,
        library,
        PreviewScope::Scan(scan),
        options,
        refresh_mode == "full",
        false,
        now,
    )
    .await
}

/// Queue the previews (trickplay sheets, chapter images) of the files in `scope`.
/// With `rearm`, a warm-up that already finished (or failed) is queued again; the Node
/// skips a file whose prepared cache is still present. `manual` runs ignore the
/// "during scans" switches but never the library's on/off switches.
pub(crate) async fn queue_previews(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    library: &str,
    scope: PreviewScope<'_>,
    options: &LibraryOptions,
    rearm: bool,
    manual: bool,
    now: i64,
) -> Result<u64, sqlx::Error> {
    use sha2::{Digest, Sha256};
    let previews = options.trickplay_enabled && (manual || options.trickplay_during_scan);
    let chapters = options.chapter_images_enabled && (manual || options.chapter_images_during_scan);
    if !previews && !chapters {
        return Ok(0);
    }
    let [preview_owner, chapter_owner] = if manual { MANUAL_OWNERS } else { SCAN_OWNERS };
    // Share admission's cross-Core capacity lock only for this final queue step.
    if tx.as_mut().backend_name() == "PostgreSQL" {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(crate::task_engine::TASK_ADMISSION_LOCK)
            .execute(&mut **tx)
            .await?;
    }
    let pending: i64 = crate::database::query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','paused')",
    )
    .fetch_one(&mut **tx)
    .await?;
    let capacity = (100_000 - pending).max(0);
    if capacity == 0 {
        return Ok(0);
    }
    let mut inserted = 0_u64;
    let mut last_asset = String::new();
    // Reused probes are included: enabling previews or changing their profile
    // must also warm existing videos. Compare indexed idempotency keys in batches,
    // rather than rewriting one task for every file on every monitoring scan.
    loop {
        let assets: Vec<(String, i64, String, String, String)> = match scope {
            PreviewScope::Scan(scan) => crate::database::query_as(
                "SELECT a.id,a.size,a.modified_ns,l.node_id,a.library_id FROM scan_staging_files f JOIN inventory_assets a ON a.library_id=f.library_id AND a.relative_path=f.relative_path JOIN libraries l ON l.id=a.library_id WHERE f.scan_id=? AND a.library_id=? AND a.id>? AND a.available=1 AND l.enabled=1 AND l.kind IN ('films','series') ORDER BY a.id LIMIT 128"
            ).bind(scan).bind(library).bind(&last_asset).fetch_all(&mut **tx).await?,
            PreviewScope::Library => crate::database::query_as(
                "SELECT a.id,a.size,a.modified_ns,l.node_id,a.library_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE (a.library_id=? OR a.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND a.id>? AND a.available=1 AND s.root_snapshot=l.root AND n.status!='revoked' AND l.kind IN ('films','series') AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) ORDER BY a.id LIMIT 128"
            ).bind(library).bind(library).bind(&last_asset).fetch_all(&mut **tx).await?,
        };
        if assets.is_empty() {
            break;
        }
        last_asset = assets.last().map(|row| row.0.clone()).unwrap_or_default();
        let mut candidates = Vec::with_capacity(assets.len() * 2);
        for (asset, size, modified, node, asset_library) in assets {
            // Chapter images reuse the on-demand profile (width/quality, no
            // interval) so the player hits the prepared cache directly.
            if previews {
                let grid_suffix = if options.trickplay_columns == 10 && options.trickplay_rows == 10
                {
                    String::new()
                } else {
                    format!(":{}x{}", options.trickplay_columns, options.trickplay_rows)
                };
                let key = format!(
                    "trickplay:{:x}",
                    Sha256::digest(format!(
                        "{asset}:{size}:{modified}:{}:{}:{}:webp{grid_suffix}",
                        options.trickplay_width,
                        options.trickplay_interval_seconds,
                        options.trickplay_quality
                    ))
                );
                candidates.push((
                    asset.clone(),
                    node.clone(),
                    asset_library.clone(),
                    key,
                    preview_owner,
                    "",
                ));
            }
            if chapters {
                let key = format!(
                    "chapters:{:x}",
                    Sha256::digest(format!(
                        "{asset}:{size}:{modified}:{}:{}",
                        options.trickplay_width, options.trickplay_quality
                    ))
                );
                candidates.push((asset, node, asset_library, key, chapter_owner, "chapters"));
            }
        }
        let placeholders = vec!["?"; candidates.len()].join(",");
        let statement = format!(
            "SELECT owner,idempotency_key,status,CAST(CASE WHEN lease_until IS NULL THEN 0 ELSE 1 END AS BIGINT) FROM durable_tasks WHERE owner IN (?,?) AND error NOT IN ('library_trickplay_disabled','library_chapter_images_disabled') AND idempotency_key IN ({placeholders})"
        );
        let mut lookup = crate::database::query_as::<sqlx::Any, (String, String, String, i64)>(
            sqlx::AssertSqlSafe(statement),
        )
        .bind(preview_owner)
        .bind(chapter_owner);
        for (_, _, _, key, _, _) in &candidates {
            lookup = lookup.bind(key);
        }
        let existing: std::collections::HashMap<(String, String), (String, i64)> = lookup
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .map(|(owner, key, status, leased)| ((owner, key), (status, leased)))
            .collect();
        for (asset, node, asset_library, key, owner, component) in candidates {
            if let Some((status, leased)) = existing.get(&(owner.to_owned(), key.clone())) {
                // A finished warm-up says nothing about the cache today (it may have been
                // deleted or evicted): an explicit full pass queues it again.
                if rearm
                    && *leased == 0
                    && matches!(status.as_str(), "completed" | "error" | "cancelled")
                {
                    inserted += query("UPDATE durable_tasks SET status='queued',error='',failure_count=0,available_at=?,updated_at=?,finished_at=NULL WHERE owner=? AND idempotency_key=? AND status IN ('completed','error','cancelled') AND lease_until IS NULL")
                        .bind(now).bind(now).bind(owner).bind(&key)
                        .execute(&mut **tx).await?.rows_affected();
                    if inserted >= capacity as u64 {
                        return Ok(inserted);
                    }
                }
                continue;
            }
            inserted += query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,priority,created_at,updated_at,available_at,category,target_id,component,width) VALUES(?,?,?,?,?,1,?,?,?,'trickplay',?,?,?) ON CONFLICT(owner,idempotency_key) DO UPDATE SET status='queued',error='',available_at=excluded.available_at,updated_at=excluded.updated_at,finished_at=NULL WHERE durable_tasks.error IN ('library_trickplay_disabled','library_chapter_images_disabled') AND durable_tasks.lease_until IS NULL")
                .bind(uuid::Uuid::new_v4().to_string()).bind(&asset_library).bind(owner).bind(key).bind(node)
                .bind(now).bind(now).bind(now).bind(asset).bind(component).bind(i64::from(options.trickplay_width))
                .execute(&mut **tx).await?.rows_affected();
            if inserted >= capacity as u64 {
                return Ok(inserted);
            }
        }
    }
    Ok(inserted)
}

struct AssetSource {
    node_id: String,
    relative_path: PathBuf,
    media_root: PathBuf,
    duration: f64,
    size: u64,
    modified_ns: String,
    options: LibraryOptions,
    chapters: Vec<(f64, String)>,
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
        chapters: crate::inventory::valid_chapters(
            parsed["chapters"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            Some(duration),
        ),
    }))
}

pub async fn chapter_manifest(store: &IdentityStore, asset_id: &str) -> Response {
    let source = match resolve_asset_source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "chapter_images_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    if !source.options.chapter_images_enabled {
        return crate::error(StatusCode::NOT_FOUND, "chapter_images_disabled");
    }
    Json(json!({"chapters":source.chapters.iter().enumerate().map(|(index,(start,title))|json!({"index":index,"start":start,"title":title,"imageUrl":format!("/api/v1/assets/{asset_id}/chapter-images/{index}")})).collect::<Vec<_>>(),"mode":"on_demand"})).into_response()
}

pub async fn chapter_image(
    store: &IdentityStore,
    asset_id: &str,
    index: usize,
    request: Request,
) -> Response {
    let source = match resolve_asset_source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "chapter_images_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    if !source.options.chapter_images_enabled {
        return crate::error(StatusCode::NOT_FOUND, "chapter_images_disabled");
    }
    let Some((time, _)) = source.chapters.get(index) else {
        return crate::error(StatusCode::NOT_FOUND, "unknown_chapter");
    };
    if source.node_id != "local" {
        let node = match crate::nodes::load_node(store, &source.node_id).await {
            Ok(Some(node)) => node,
            _ => return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
        };
        let path = format!(
            "/internal/v1/node/assets/{asset_id}/chapter-images/{index}?time={time}&duration={}&width={}&quality={}",
            source.duration, source.options.trickplay_width, source.options.trickplay_quality
        );
        return crate::nodes::relay_from_remote_node(store, &node, &path, request).await;
    }
    media_node::chapter_images::response(
        &crate::node::LocalNode.trickplay(),
        TrickplayRequest {
            asset_id: asset_id.into(),
            media_root: source.media_root,
            relative_path: source.relative_path,
            duration: source.duration,
            size: source.size,
            modified_ns: source.modified_ns,
            interval_ms: None,
            width: Some(source.options.trickplay_width.into()),
            quality: Some(source.options.trickplay_quality),
            columns: None,
            rows: None,
            chapter_time: Some(*time),
        },
        request,
    )
    .await
}

pub async fn manifest(store: &IdentityStore, asset_id: &str, request: Request) -> Response {
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
            "/internal/v1/node/assets/{asset_id}/trickplay?duration={}&intervalMs={}&width={}&quality={}&columns={}&rows={}",
            source.duration,
            (source.options.trickplay_interval_seconds as u64) * 1000,
            source.options.trickplay_width,
            source.options.trickplay_quality,
            source.options.trickplay_columns,
            source.options.trickplay_rows
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
        columns: Some(u32::from(source.options.trickplay_columns)),
        rows: Some(u32::from(source.options.trickplay_rows)),
        chapter_time: None,
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
                "mime": cached.mime,
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
                "mime": jellyfin.mime,
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

    if !source.options.trickplay_enabled {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_disabled");
    }

    if source.node_id != "local" {
        let node = match crate::nodes::load_node(store, &source.node_id).await {
            Ok(Some(node)) => node,
            _ => return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
        };
        let pq = format!(
            "/internal/v1/node/assets/{asset_id}/trickplay/{index}?duration={}&intervalMs={}&width={}&quality={}&columns={}&rows={}",
            source.duration,
            u64::from(source.options.trickplay_interval_seconds) * 1000,
            source.options.trickplay_width,
            source.options.trickplay_quality,
            source.options.trickplay_columns,
            source.options.trickplay_rows
        );
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
        columns: Some(u32::from(source.options.trickplay_columns)),
        rows: Some(u32::from(source.options.trickplay_rows)),
        chapter_time: None,
    };

    let (opened, mime) = match mgr.open_sheet_with_mime(&req, index).await {
        Ok(f) => f,
        Err(_) => return crate::error(StatusCode::NOT_FOUND, "trickplay_tile_unavailable"),
    };

    match media_node::serve_opened(opened, &request, &mime).await {
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
mod background_tests;
#[cfg(test)]
mod chapter_tests;

#[cfg(test)]
mod tests {
    use media_node::trickplay::{DEFAULT_COLUMNS, DEFAULT_ROWS};

    #[test]
    fn grid_bounds_are_consistent() {
        assert_eq!(DEFAULT_COLUMNS, 10);
        assert_eq!(DEFAULT_ROWS, 10);
    }
}
