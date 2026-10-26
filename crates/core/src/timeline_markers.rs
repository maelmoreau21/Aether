//! Manual episode timeline corrections. Original media is never modified.
use crate::{
    auth::AuthState,
    database::{query, query_as},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::put,
};
use serde::Deserialize;
use serde_json::Value;

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/assets/{asset}/markers/{kind}",
            put(save).delete(clear),
        )
        .layer(DefaultBodyLimit::max(1024))
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
    if !matches!(kind, "intro" | "credits") || asset.len() > 128 {
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
