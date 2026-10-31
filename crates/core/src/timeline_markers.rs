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
        .route(
            "/api/v1/admin/seasons/{id}/analyze-markers",
            post(analyze_season_markers),
        )
        .route(
            "/api/v1/admin/works/{id}/analyze-markers",
            post(analyze_work_markers),
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
        let completed: i64 = query_scalar(
            "SELECT COUNT(*) FROM durable_tasks WHERE idempotency_key=? AND status='completed'",
        )
        .bind(&key)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(0);

        if completed > 0 {
            let event_key = format!("fingerprint-ready:{key}");
            let _ = crate::automation::emit(
                &mut tx,
                &crate::automation::EventInput {
                    key: &event_key,
                    kind: "fingerprint.ready",
                    library_id,
                    asset_id,
                    fingerprint: source_fp,
                    payload: &json!({"fingerprintKey": key, "stale": false}),
                },
                now,
            )
            .await;
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
         JOIN automation_observations obs ON obs.asset_id=a.id \
         WHERE ep.season_id=? AND a.available=1",
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
         JOIN automation_observations obs ON obs.asset_id=a.id \
         WHERE se.series_id=? AND a.available=1",
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
