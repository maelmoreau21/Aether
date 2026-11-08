//! Episode timeline markers, automatic inspection, manual corrections and season-wide reanalysis.
use crate::{
    auth::AuthState,
    database::{begin_write, query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/assets/{asset}/markers", get(get_markers))
        .route(
            "/api/v1/admin/assets/{asset}/markers/{kind}",
            put(save).delete(clear),
        )
        .route("/api/v1/admin/seasons/{id}/markers", get(season_markers))
        .route(
            "/api/v1/admin/seasons/{id}/analyze-markers",
            post(analyze_season_markers),
        )
        .route(
            "/api/v1/admin/works/{id}/analyze-markers",
            post(analyze_work_markers),
        )
        .route(
            "/api/v1/admin/libraries/{id}/analyze-markers",
            post(analyze_library_markers),
        )
        .route(
            "/api/v1/admin/maintenance/clear-fingerprint-cache",
            post(clear_cache),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Correction {
    enabled: bool,
    start: Option<f64>,
    end: Option<f64>,
}

async fn target(
    auth: &AuthState,
    headers: &HeaderMap,
    asset: &str,
    kind: &str,
) -> Result<(String, f64), StatusCode> {
    if !matches!(kind, "intro" | "credits" | "recap" | "preview") || asset.len() > 128 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(headers, &actor)?;
    let row: Option<(String, String)> = query_as(
        "SELECT a.probe_json, w.kind FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND n.status!='revoked' AND s.root_snapshot=l.root")
        .bind(asset).fetch_optional(auth.store.pool()).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some((probe, work_kind)) = row else {
        return Err(StatusCode::NOT_FOUND);
    };
    if work_kind != "episode" {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let probe: Value =
        serde_json::from_str(&probe).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let duration = probe["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    Ok((actor.user.id, duration))
}

async fn get_markers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }

    type AutoMarkerRow = (String, f64, f64, f64, String, i64);
    type ManualMarkerRow = (String, Option<f64>, Option<f64>, i64, i64);

    let automatic: Vec<AutoMarkerRow> = query_as(
        "SELECT marker_type, start_seconds, end_seconds, confidence, extension_id, updated_at \
         FROM automatic_timeline_markers WHERE asset_id=? ORDER BY start_seconds",
    )
    .bind(&asset)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let manual: Vec<ManualMarkerRow> = query_as(
        "SELECT marker_type, start_seconds, end_seconds, enabled, updated_at \
         FROM manual_timeline_markers WHERE asset_id=? ORDER BY marker_type",
    )
    .bind(&asset)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let auto_list: Vec<Value> = automatic
        .into_iter()
        .map(|(mtype, start, end, conf, ext, upd)| {
            json!({
                "type": mtype,
                "start": start,
                "end": end,
                "confidence": conf,
                "extensionId": ext,
                "updatedAt": upd,
                "isManual": false,
            })
        })
        .collect();

    let manual_list: Vec<Value> = manual
        .into_iter()
        .map(|(mtype, start, end, enabled, upd)| {
            json!({
                "type": mtype,
                "start": start,
                "end": end,
                "enabled": enabled == 1,
                "updatedAt": upd,
                "isManual": true,
            })
        })
        .collect();

    Ok(Json(json!({
        "assetId": asset,
        "automatic": auto_list,
        "manual": manual_list,
    })))
}

/// Most recent task of one kind for an asset: its state and, when it failed, why.
async fn latest_task(
    auth: &AuthState,
    sql: &'static str,
    asset: &str,
) -> Result<Value, StatusCode> {
    let row: Option<(String, String, i64)> = query_as(sql)
        .bind(asset)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(match row {
        Some((status, error, updated)) => json!({
            "status": status,
            "error": if error.is_empty() { Value::Null } else { json!(error.chars().take(300).collect::<String>()) },
            "updatedAt": updated,
        }),
        None => json!({"status": "missing", "error": Value::Null, "updatedAt": Value::Null}),
    })
}

/// Season overview for the segment editor: every available episode file with its
/// automatic detections (usable or not), manual corrections, container chapters and
/// the state of its audio fingerprint and detection tasks.
async fn season_markers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(season_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if season_id.len() > 128 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    type EpisodeRow = (String, String, i64, String, String, String);
    let rows: Vec<EpisodeRow> = query_as(
        "SELECT a.id, w.id, ep.episode_number, w.title, a.probe_json, a.library_id \
         FROM inventory_episodes ep \
         JOIN inventory_works w ON w.id=ep.work_id \
         JOIN inventory_editions ed ON ed.work_id=ep.work_id \
         JOIN inventory_versions v ON v.edition_id=ed.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE ep.season_id=? AND a.available=1 AND s.root_snapshot=l.root AND n.status!='revoked' \
         AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) \
         ORDER BY ep.episode_number, a.id LIMIT 200",
    )
    .bind(&season_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut library_settings: std::collections::HashMap<String, (f64, bool)> =
        std::collections::HashMap::new();
    let mut episodes = Vec::with_capacity(rows.len());
    for (asset, work, number, title, probe, library) in rows {
        if !library_settings.contains_key(&library) {
            let encoded: Option<String> = query_scalar("SELECT COALESCE(parent.options_json,l.options_json) FROM libraries l LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 LEFT JOIN libraries parent ON parent.id=f.parent_id WHERE l.id=?")
                .bind(&library).fetch_optional(auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let options = encoded
                .and_then(|text| {
                    serde_json::from_str::<crate::library_options::LibraryOptions>(&text).ok()
                })
                .unwrap_or_default();
            library_settings.insert(
                library.clone(),
                (
                    f64::from(options.marker_min_confidence) / 100.0,
                    options.intro_detection_enabled,
                ),
            );
        }
        let (min_confidence, detection_enabled) = library_settings[&library];
        let probe: Value = serde_json::from_str(&probe).unwrap_or(Value::Null);
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.0);
        let chapters: Vec<Value> = probe["chapters"]
            .as_array()
            .map(|list| crate::inventory::valid_chapters(list, duration))
            .unwrap_or_default()
            .into_iter()
            .take(64)
            .map(|(start, title)| json!({"start": start, "title": title}))
            .collect();

        type AutoMarkerRow = (String, f64, f64, f64, String, i64);
        let automatic: Vec<AutoMarkerRow> = query_as(
            "SELECT marker_type, start_seconds, end_seconds, confidence, extension_id, updated_at \
             FROM automatic_timeline_markers WHERE asset_id=? ORDER BY confidence DESC",
        )
        .bind(&asset)
        .fetch_all(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        type ManualMarkerRow = (String, Option<f64>, Option<f64>, i64, i64);
        let manual: Vec<ManualMarkerRow> = query_as(
            "SELECT marker_type, start_seconds, end_seconds, enabled, updated_at \
             FROM manual_timeline_markers WHERE asset_id=? ORDER BY marker_type",
        )
        .bind(&asset)
        .fetch_all(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let fingerprint = latest_task(&auth, "SELECT status, error, updated_at FROM durable_tasks WHERE target_id=? AND category='analyze' AND component='fingerprint' ORDER BY updated_at DESC, id LIMIT 1", &asset).await?;
        let detection = latest_task(&auth, "SELECT t.status, t.error, t.updated_at FROM durable_tasks t JOIN automation_task_links link ON link.task_id=t.id WHERE t.target_id=? AND link.operation IN ('intro.detect','intro.parse_chapters','markers.detect') ORDER BY t.updated_at DESC, t.id LIMIT 1", &asset).await?;

        episodes.push(json!({
            "assetId": asset,
            "workId": work,
            "episodeNumber": number,
            "title": title,
            "duration": duration,
            "minConfidence": min_confidence,
            "detectionEnabled": detection_enabled,
            "chapters": chapters,
            "fingerprint": fingerprint,
            "detection": detection,
            "automatic": automatic.into_iter().map(|(kind, start, end, confidence, extension, updated)| json!({
                "type": kind,
                "start": start,
                "end": end,
                "confidence": confidence,
                "extensionId": extension,
                "updatedAt": updated,
                "usable": crate::inventory::usable_marker(start, end, confidence, min_confidence, duration),
            })).collect::<Vec<_>>(),
            "manual": manual.into_iter().map(|(kind, start, end, enabled, updated)| json!({
                "type": kind,
                "start": start,
                "end": end,
                "enabled": enabled == 1,
                "updatedAt": updated,
            })).collect::<Vec<_>>(),
        }));
    }

    let extensions: Vec<(String, i64, String)> =
        query_as("SELECT id, enabled, package_json FROM extensions ORDER BY id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let providers: Vec<Value> = extensions
        .into_iter()
        .filter_map(|(id, enabled, package)| {
            let package: media_extension_sdk::Package = serde_json::from_str(&package).ok()?;
            package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "playback.markers")
                .then(|| json!({"id": id, "name": package.manifest.name, "version": package.manifest.version, "enabled": enabled == 1}))
        })
        .collect();

    Ok(Json(json!({
        "seasonId": season_id,
        "providers": providers,
        "episodes": episodes,
    })))
}

async fn save(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((asset, kind)): Path<(String, String)>,
    Json(input): Json<Correction>,
) -> Result<StatusCode, StatusCode> {
    let (actor, duration) = target(&auth, &headers, &asset, &kind).await?;
    if input.enabled
        && !(matches!((input.start,input.end), (Some(start),Some(end)) if start.is_finite() && end.is_finite() && start>=0.0 && end>start && end<=duration && end-start<=600.0))
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if !input.enabled && (input.start.is_some() || input.end.is_some()) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    query("INSERT INTO manual_timeline_markers(asset_id,marker_type,start_seconds,end_seconds,enabled,updated_by,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(asset_id,marker_type) DO UPDATE SET start_seconds=excluded.start_seconds,end_seconds=excluded.end_seconds,enabled=excluded.enabled,updated_by=excluded.updated_by,updated_at=excluded.updated_at")
        .bind(&asset).bind(&kind).bind(input.start).bind(input.end).bind(i64::from(input.enabled)).bind(&actor).bind(media_contracts::protocol::unix_now())
        .execute(auth.store.pool()).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((asset, kind)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    target(&auth, &headers, &asset, &kind).await?;
    query("DELETE FROM manual_timeline_markers WHERE asset_id=? AND marker_type=?")
        .bind(&asset)
        .bind(&kind)
        .execute(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn queue_episodes_fingerprints(
    auth: &AuthState,
    episodes: &[(String, String, String, String)],
) -> Result<usize, StatusCode> {
    let now = media_contracts::protocol::unix_now();
    let mut conn = auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut queued = 0;
    for (asset_id, library_id, node_id, source_fp) in episodes {
        let key = format!(
            "fp:{}",
            crate::automation::hash(&(&asset_id, &source_fp, "chromaprint-v1"))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        );
        let task_id = uuid::Uuid::new_v4().to_string();
        let inserted = query(
            "INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,component,priority,created_at,updated_at,available_at) \
             VALUES(?,?,?,?,?,'analyze',?,'fingerprint',20,?,?,?) ON CONFLICT(owner,idempotency_key) DO UPDATE SET status='queued',failure_count=0,finished_at=NULL,updated_at=excluded.updated_at WHERE status IN ('error','cancelled')",
        )
        .bind(&task_id)
        .bind(library_id)
        .bind("system")
        .bind(&key)
        .bind(node_id)
        .bind(asset_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if inserted.rows_affected() > 0 {
            queued += 1;
        }

        // Emit an event to trigger immediate marker detection if fingerprint is ready
        let completed: Option<String> = query_scalar(
            "SELECT id FROM durable_tasks WHERE owner='system' AND idempotency_key=? AND target_id=? AND component='fingerprint' AND status='completed'",
        )
        .bind(&key)
        .bind(asset_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(completed) = completed {
            // An explicit reanalysis must bypass the ordinary once-per-source
            // event deduplication, while reusing the Node's completed extraction.
            let event_key = format!("marker-reanalysis:{}", uuid::Uuid::new_v4());
            crate::automation::emit(
                &mut tx,
                &crate::automation::EventInput {
                    key: &event_key,
                    kind: "manual",
                    library_id,
                    asset_id,
                    fingerprint: source_fp,
                    payload: &json!({"fingerprintTaskId": completed, "reanalyzeMarkers": true, "stale": false}),
                },
                now,
            )
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::task_engine::WAKE.notify_one();
    Ok(queued)
}

async fn analyze_season_markers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(season_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(&headers, &actor)?;

    let episodes: Vec<(String, String, String, String)> = query_as(
        "SELECT a.id, a.library_id, l.node_id, obs.fingerprint \
         FROM inventory_episodes ep \
         JOIN inventory_editions ed ON ed.work_id=ep.work_id \
         JOIN inventory_versions v ON v.edition_id=ed.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         JOIN automation_observations obs ON obs.asset_id=a.id \
         WHERE ep.season_id=? AND a.available=1 AND s.root_snapshot=l.root AND n.status!='revoked' \
         AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))",
    )
    .bind(&season_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count = episodes.len();
    let queued = queue_episodes_fingerprints(&auth, &episodes).await?;

    Ok(Json(json!({
        "status": "queued",
        "seasonId": season_id,
        "episodesCount": count,
        "tasksQueued": queued,
    })))
}

async fn analyze_work_markers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(&headers, &actor)?;

    let episodes: Vec<(String, String, String, String)> = query_as(
        "SELECT a.id, a.library_id, l.node_id, obs.fingerprint \
         FROM inventory_episodes ep \
         JOIN inventory_seasons se ON se.id=ep.season_id \
         JOIN inventory_editions ed ON ed.work_id=ep.work_id \
         JOIN inventory_versions v ON v.edition_id=ed.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         JOIN automation_observations obs ON obs.asset_id=a.id \
         WHERE se.series_id=? AND a.available=1 AND s.root_snapshot=l.root AND n.status!='revoked' \
         AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))",
    )
    .bind(&work_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count = episodes.len();
    let queued = queue_episodes_fingerprints(&auth, &episodes).await?;

    Ok(Json(json!({
        "status": "queued",
        "seriesId": work_id,
        "episodesCount": count,
        "tasksQueued": queued,
    })))
}

/// Every episode of a series library (and of its active folders): fingerprints are queued
/// again where they failed, and episodes already fingerprinted are re-detected at once.
async fn analyze_library_markers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(library_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(&headers, &actor)?;
    if library_id.len() > 128 {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let episodes: Vec<(String, String, String, String)> = query_as(
        "SELECT a.id, a.library_id, l.node_id, obs.fingerprint \
         FROM inventory_episodes ep \
         JOIN inventory_editions ed ON ed.work_id=ep.work_id \
         JOIN inventory_versions v ON v.edition_id=ed.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         JOIN automation_observations obs ON obs.asset_id=a.id \
         WHERE (a.library_id=? OR a.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) \
         AND a.available=1 AND s.root_snapshot=l.root AND n.status!='revoked' \
         AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) \
         ORDER BY a.id LIMIT 20000",
    )
    .bind(&library_id)
    .bind(&library_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count = episodes.len();
    let queued = queue_episodes_fingerprints(&auth, &episodes).await?;

    Ok(Json(json!({
        "status": "queued",
        "libraryId": library_id,
        "episodesCount": count,
        "tasksQueued": queued,
    })))
}

async fn clear_cache(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    auth.require_csrf(&headers, &actor)?;

    let freed = media_node::tasks::clear_fingerprint_cache()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "status": "cleared",
        "bytesFreed": freed,
    })))
}
