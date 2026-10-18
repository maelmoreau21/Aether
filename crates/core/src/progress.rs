//! User-owned progress keyed by editorial identity, shared across sessions and media copies.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/progress", get(list))
        .route("/api/v1/progress/{work}", get(read).put(write))
        .layer(DefaultBodyLimit::max(2048))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

async fn authorized(
    auth: &AuthState,
    headers: &HeaderMap,
    work: &str,
    mutation: bool,
) -> Result<String, StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if mutation {
        auth.require_csrf(headers, &identity)?;
    }
    let found: Option<String> = query_scalar("SELECT w.id FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE w.id=? AND w.kind IN ('film','episode') AND l.enabled=1 AND n.status != 'revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM library_permissions p WHERE p.library_id=l.id AND p.user_id=?))")
        .bind(work).bind(identity.user.role.as_str()).bind(&identity.user.id).fetch_optional(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if found.is_none() {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(identity.user.id)
}

fn value(row: &sqlx::any::AnyRow) -> Value {
    json!({"workId":row.get::<String,_>("work_id"), "position":row.get::<f64,_>("position_seconds"), "duration":row.get::<f64,_>("duration_seconds"), "watched":row.get::<i64,_>("watched") == 1, "revision":row.get::<i64,_>("revision"), "updatedAt":row.get::<i64,_>("updated_at")})
}

async fn read(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let user = authorized(&auth, &headers, &work, false).await?;
    let row = query("SELECT * FROM playback_progress WHERE user_id=? AND work_id=?")
        .bind(&user)
        .bind(&work)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(row) = row.as_ref() {
        return Ok(Json(value(row)));
    }
    let probe: Option<String> = query_scalar("SELECT a.probe_json FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE e.work_id=? AND a.available=1 ORDER BY a.id LIMIT 1")
        .bind(&work).fetch_optional(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let duration = probe
        .and_then(|p| serde_json::from_str::<media_contracts::media::Probe>(&p).ok())
        .and_then(|p| {
            p.format
                .get("duration")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<f64>().ok())
        })
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(0.0);
    Ok(Json(
        json!({"workId":work,"position":0,"duration":duration,"watched":false,"revision":0}),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    position: f64,
    duration: f64,
    watched: bool,
    expected_revision: i64,
}

async fn write(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<Json<Value>, StatusCode> {
    let user = authorized(&auth, &headers, &work, true).await?;
    if !input.position.is_finite()
        || !input.duration.is_finite()
        || input.duration <= 0.0
        || input.duration > 604800.0
        || input.position < 0.0
        || input.position > input.duration
        || input.expected_revision < 0
        || input.expected_revision == i64::MAX
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let now = media_contracts::protocol::unix_now();
    let row = if input.expected_revision == 0 {
        query("INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES(?,?,?,?,?,1,?) ON CONFLICT(user_id,work_id) DO NOTHING RETURNING *")
            .bind(&user).bind(&work).bind(input.position).bind(input.duration).bind(if input.watched {1i64} else {0}).bind(now as i64)
            .fetch_optional(&auth.store.pool).await
    } else {
        query("UPDATE playback_progress SET position_seconds=?,duration_seconds=?,watched=?,revision=revision+1,updated_at=? WHERE user_id=? AND work_id=? AND revision=? RETURNING *")
            .bind(input.position).bind(input.duration).bind(if input.watched {1i64} else {0}).bind(now as i64).bind(&user).bind(&work).bind(input.expected_revision)
            .fetch_optional(&auth.store.pool).await
    }.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    row.as_ref()
        .map(|row| Json(value(row)))
        .ok_or(StatusCode::CONFLICT)
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let identity = auth.authenticate(&headers).await?;
    let rows = query("SELECT p.*,w.title,w.kind,s.series_id FROM playback_progress p JOIN inventory_works w ON w.id=p.work_id JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id LEFT JOIN inventory_episodes ep ON ep.work_id=w.id LEFT JOIN inventory_seasons s ON s.id=ep.season_id WHERE p.user_id=? AND l.enabled=1 AND n.status != 'revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM library_permissions lp WHERE lp.library_id=l.id AND lp.user_id=?)) ORDER BY p.updated_at DESC,p.work_id LIMIT 100")
        .bind(&identity.user.id).bind(if identity.user.role == Role::Administrator {"administrator"} else {"user"}).bind(&identity.user.id).fetch_all(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let items: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut v = value(row);
            v["title"] = json!(row.get::<String, _>("title"));
            v["kind"] = json!(row.get::<String, _>("kind"));
            v["seriesId"] = json!(row.get::<Option<String>, _>("series_id"));
            v
        })
        .collect();
    Ok(Json(json!({"items":items})))
}
