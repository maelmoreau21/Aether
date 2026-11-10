//! Per-file maintenance of the media tools of a detail page: what was generated for a file
//! (preview thumbnails, chapters, intro/credit segments) and the actions that rebuild it.
//!
//! Nothing here reads or writes the media itself: previews are rebuilt from the prepared cache
//! of the Node that holds the file, and every action goes through the same durable tasks as a
//! library scan. Administrators only; mutations also need the CSRF token.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query_as},
    identity::Role,
    trickplay::{AssetSource, PreviewScope, queue_previews, resolve_asset_source},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use media_node::trickplay::TrickplayRequest;
use serde_json::{Value, json};

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/assets/{asset}/media-status", get(status))
        .route(
            "/api/v1/admin/assets/{asset}/regenerate-trickplay",
            post(regenerate_trickplay),
        )
        .route(
            "/api/v1/admin/assets/{asset}/refresh-chapters",
            post(refresh_chapters),
        )
        .layer(DefaultBodyLimit::max(1024))
        .with_state(auth)
}

async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    asset: &str,
    mutation: bool,
) -> Result<AssetSource, StatusCode> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    if asset.is_empty() || asset.len() > 128 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    resolve_asset_source(&auth.store, asset)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)
}

/// The request the Node identifies this file's sheets (or one chapter image) by.
fn request(asset: &str, source: &AssetSource, chapter_time: Option<f64>) -> TrickplayRequest {
    let options = &source.options;
    TrickplayRequest {
        asset_id: asset.to_owned(),
        media_root: source.media_root.clone(),
        relative_path: source.relative_path.clone(),
        duration: source.duration,
        size: source.size,
        modified_ns: source.modified_ns.clone(),
        interval_ms: chapter_time
            .is_none()
            .then(|| u64::from(options.trickplay_interval_seconds) * 1000),
        width: Some(u32::from(options.trickplay_width)),
        quality: Some(options.trickplay_quality),
        columns: chapter_time
            .is_none()
            .then(|| u32::from(options.trickplay_columns)),
        rows: chapter_time
            .is_none()
            .then(|| u32::from(options.trickplay_rows)),
        chapter_time,
    }
}

/// State of the latest preview warm-up of a file: `running`, `failed`, `done` or `none`.
async fn warm_up(
    auth: &AuthState,
    asset: &str,
    component: &str,
) -> Result<&'static str, StatusCode> {
    let row: Option<(String,)> = query_as(
        "SELECT status FROM durable_tasks WHERE target_id=? AND category='trickplay' AND component=? ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(asset)
    .bind(component)
    .fetch_optional(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(match row.as_ref().map(|(status,)| status.as_str()) {
        Some("queued" | "running" | "paused") => "running",
        Some("error") => "failed",
        Some("completed") => "done",
        _ => "none",
    })
}

async fn status(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let source = administrator(&auth, &headers, &asset, false).await?;
    let options = &source.options;

    let task = warm_up(&auth, &asset, "").await?;
    let (state, thumbnails) = if !options.trickplay_enabled {
        ("disabled", None)
    } else if source.node_id == "local" {
        // The local Node's prepared cache is authoritative: a finished task says nothing
        // about a cache that was since deleted.
        let prepared = crate::node::LocalNode
            .trickplay()
            .check_native_cache(&request(&asset, &source, None))
            .await;
        match (prepared, task) {
            (Some(cache), _) => ("generated", Some(cache.thumbnail_count)),
            (None, "running") => ("running", None),
            (None, "failed") => ("failed", None),
            (None, _) => ("missing", None),
        }
    } else {
        // A remote Node's cache is not visible from the Core: only the warm-up tells.
        match task {
            "done" => ("generated", None),
            "running" => ("running", None),
            "failed" => ("failed", None),
            _ => ("missing", None),
        }
    };

    let chapter_images = if options.chapter_images_enabled {
        warm_up(&auth, &asset, "chapters").await?
    } else {
        "disabled"
    };

    let (automatic, manual): (i64, i64) = (
        crate::database::query_scalar(
            "SELECT COUNT(*) FROM automatic_timeline_markers WHERE asset_id=?",
        )
        .bind(&asset)
        .fetch_one(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        crate::database::query_scalar(
            "SELECT COUNT(*) FROM manual_timeline_markers WHERE asset_id=?",
        )
        .bind(&asset)
        .fetch_one(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    Ok(Json(json!({
        "assetId": asset,
        "trickplay": {
            "enabled": options.trickplay_enabled,
            "status": state,
            "width": options.trickplay_width,
            "intervalSeconds": options.trickplay_interval_seconds,
            "thumbnailCount": thumbnails,
            // Whether this Core can delete the prepared cache itself before rebuilding it.
            "canPurge": source.node_id == "local",
        },
        "chapters": {
            "count": source.chapters.len(),
            "imagesEnabled": options.chapter_images_enabled,
            "images": chapter_images,
        },
        "segments": {
            "detectionEnabled": options.intro_detection_enabled,
            "automatic": automatic,
            // A correction is never touched by an analysis: it counts as locked.
            "manual": manual,
            "locked": manual,
        },
    })))
}

/// Queue the previews of one file again. `sheets` and `chapters` choose what is rebuilt; the
/// library's own on/off switches still apply.
async fn requeue(
    auth: &AuthState,
    asset: &str,
    source: &AssetSource,
    sheets: bool,
    chapters: bool,
) -> Result<u64, StatusCode> {
    let library: String =
        crate::database::query_scalar("SELECT library_id FROM inventory_assets WHERE id=?")
            .bind(asset)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?;
    let mut options = source.options.clone();
    options.trickplay_enabled &= sheets;
    options.chapter_images_enabled &= chapters;
    let now = media_contracts::protocol::unix_now();
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let queued = queue_previews(
        &mut tx,
        &library,
        PreviewScope::Asset(asset),
        &options,
        true,
        true,
        now,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    commit(tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::task_engine::WAKE.notify_one();
    Ok(queued)
}

/// Delete the prepared sheets of this file and queue them again. On a remote Node the cache
/// cannot be deleted from here: the warm-up is queued again and the Node keeps a valid cache.
async fn regenerate_trickplay(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let source = administrator(&auth, &headers, &asset, true).await?;
    if !source.options.trickplay_enabled {
        return Err(StatusCode::CONFLICT);
    }
    let purged = if source.node_id == "local" {
        crate::node::LocalNode
            .trickplay()
            .purge(&request(&asset, &source, None))
            .await
    } else {
        false
    };
    let queued = requeue(&auth, &asset, &source, true, false).await?;
    Ok(Json(
        json!({"status": "queued", "assetId": asset, "purged": purged, "tasksQueued": queued}),
    ))
}

/// Read the chapters of the file again (marker detection) and rebuild their images. The chapter
/// list comes from the file's last analysis: a library rescan re-reads the container itself.
async fn refresh_chapters(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let source = administrator(&auth, &headers, &asset, true).await?;
    let reread = crate::timeline_markers::reread_chapters(&auth, &asset).await?;
    let mut purged = 0_u32;
    let mut queued = 0;
    if source.options.chapter_images_enabled {
        if source.node_id == "local" {
            let manager = crate::node::LocalNode.trickplay();
            for (time, _) in &source.chapters {
                if manager.purge(&request(&asset, &source, Some(*time))).await {
                    purged += 1;
                }
            }
        }
        queued = requeue(&auth, &asset, &source, false, true).await?;
    }
    Ok(Json(json!({
        "status": "queued",
        "assetId": asset,
        "chaptersReread": reread,
        "imagesPurged": purged,
        "tasksQueued": queued,
    })))
}
