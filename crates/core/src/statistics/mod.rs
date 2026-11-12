//! Opt-in, local playback analytics. Collection never participates in media delivery.
mod aggregate;
mod calculation;
mod insights;
mod reports;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wrapped_tests;
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use calculation::{Event, Kind, State as PlaybackState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;

const LOCK: i64 = 0x535441544953;
fn now() -> i64 {
    media_contracts::protocol::unix_now()
}
type Outcome<T> = Result<T, StatusCode>;
fn db(error: sqlx::Error) -> StatusCode {
    #[cfg(test)]
    eprintln!("statistics fixture storage error: {error}");
    let _ = error;
    StatusCode::INTERNAL_SERVER_ERROR
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/playback/statistics", get(collection))
        .route("/api/v1/playback/sessions", post(start))
        .route("/api/v1/playback/sessions/{id}/events", post(events))
        .route(
            "/api/v1/admin/statistics/settings",
            get(read_settings).put(save_settings),
        )
        .route("/api/v1/admin/statistics/purge", post(purge))
        .route("/api/v1/admin/statistics", get(reports::admin))
        .route(
            "/api/v1/account/statistics",
            get(reports::personal).delete(erase),
        )
        .route(
            "/api/v1/admin/statistics/export",
            get(reports::admin_export),
        )
        .route(
            "/api/v1/account/statistics/export",
            get(reports::personal_export),
        )
        .route(
            "/api/v1/admin/statistics/sessions/{id}",
            get(reports::admin_detail),
        )
        .route(
            "/api/v1/account/statistics/sessions/{id}",
            get(reports::personal_detail),
        )
        .route("/api/v1/admin/statistics/insights", get(insights::admin))
        .route(
            "/api/v1/account/statistics/insights",
            get(insights::personal),
        )
        .route(
            "/api/v1/admin/statistics/users/{id}",
            get(insights::profile),
        )
        .route(
            "/api/v1/account/statistics/wrapped",
            get(insights::personal_wrapped),
        )
        .route(
            "/api/v1/admin/statistics/wrapped/{user}",
            get(insights::admin_wrapped),
        )
        .route(
            "/api/v1/admin/statistics/wrapped-settings",
            get(insights::read_wrapped_settings).put(insights::save_wrapped_settings),
        )
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

#[derive(Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Settings {
    enabled: i64,
    event_retention_days: i64,
    summary_retention_days: i64,
    first_enabled_at: Option<i64>,
    generation: i64,
}
async fn settings(store: &IdentityStore) -> Outcome<Settings> {
    query_as("SELECT enabled,event_retention_days,summary_retention_days,first_enabled_at,generation FROM statistics_settings WHERE singleton=1")
        .fetch_one(&store.pool).await.map_err(db)
}
async fn users_can_view(store: &IdentityStore) -> Outcome<bool> {
    query_scalar::<_, i64>("SELECT users_can_view FROM statistics_visibility WHERE singleton=1")
        .fetch_one(&store.pool)
        .await
        .map(|value| value != 0)
        .map_err(db)
}
async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Outcome<Authenticated> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(actor)
}
async fn collection(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<Json<Value>> {
    let actor = auth.authenticate(&headers).await?;
    let s = settings(&auth.store).await?;
    let visible = users_can_view(&auth.store).await?;
    Ok(Json(
        json!({"enabled":s.enabled!=0,"usersCanView":visible,"canView":s.enabled!=0 && (visible || actor.user.role==Role::Administrator),"firstEnabledAt":s.first_enabled_at,
        "eventRetentionDays":s.event_retention_days,"summaryRetentionDays":s.summary_retention_days,
        "heartbeatSeconds":15,"timeoutSeconds":90}),
    ))
}
async fn read_settings(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<Json<Value>> {
    administrator(&auth, &headers, false).await?;
    let s = settings(&auth.store).await?;
    Ok(Json(
        json!({"enabled":s.enabled!=0,"usersCanView":users_can_view(&auth.store).await?,"eventRetentionDays":s.event_retention_days,
        "summaryRetentionDays":s.summary_retention_days,"firstEnabledAt":s.first_enabled_at}),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    enabled: bool,
    #[serde(default)]
    users_can_view: bool,
    event_retention_days: i64,
    summary_retention_days: i64,
}
async fn save_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<SettingsInput>,
) -> Outcome<StatusCode> {
    administrator(&auth, &headers, true).await?;
    if !(1..=3650).contains(&input.event_retention_days)
        || !(0..=36500).contains(&input.summary_retention_days)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let was: i64 = query_scalar("SELECT enabled FROM statistics_settings WHERE singleton=1")
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let epoch = now();
    if (was != 0) != input.enabled {
        if input.enabled {
            query("INSERT INTO statistics_collection_periods(id,started_at) VALUES(?,?)")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(epoch)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        } else {
            query("UPDATE statistics_collection_periods SET ended_at=? WHERE ended_at IS NULL")
                .bind(epoch)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            query("UPDATE statistics_sessions SET ended_at=updated_at,end_reason='collection_disabled' WHERE ended_at IS NULL").execute(&mut *tx).await.map_err(db)?;
        }
    }
    query("UPDATE statistics_settings SET enabled=?,event_retention_days=?,summary_retention_days=?,first_enabled_at=COALESCE(first_enabled_at,?),generation=generation+? WHERE singleton=1")
        .bind(i64::from(input.enabled)).bind(input.event_retention_days).bind(input.summary_retention_days)
        .bind(if input.enabled {Some(epoch)} else {None}).bind(i64::from((was!=0)!=input.enabled))
        .execute(&mut *tx).await.map_err(db)?;
    query("UPDATE statistics_visibility SET users_can_view=? WHERE singleton=1")
        .bind(i64::from(input.users_can_view))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    id: String,
    media_id: String,
    music: bool,
}
#[derive(sqlx::FromRow)]
struct Media {
    media_id: String,
    library_id: String,
    node_id: String,
    kind: String,
    title: String,
    artist: String,
    album: String,
}
async fn resolve(auth: &AuthState, actor: &Authenticated, id: &str, music: bool) -> Outcome<Media> {
    if id.is_empty() || id.len() > 160 {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !crate::product::access_now(&auth.store, &actor.user.id, now()).await? {
        return Err(StatusCode::FORBIDDEN);
    }
    let role = actor.user.role.as_str();
    let found: Option<Media> = if music {
        query_as(
            "SELECT t.id AS media_id,t.library_id,l.node_id,'music' AS \
                kind,t.title,t.artist,t.album \
                FROM music_tracks t \
                JOIN libraries l ON l.id=t.library_id \
                JOIN media_nodes n ON n.id=l.node_id \
                WHERE t.id=? AND t.available=1 AND t.root_snapshot=l.root AND l.enabled=1 \
                AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 \
                FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) \
                AND NOT EXISTS(SELECT 1 FROM parental_policies p WHERE p.user_id=? \
                AND p.allow_unrated=0)",
        )
        .bind(id)
        .bind(role)
        .bind(&actor.user.id)
        .bind(&actor.user.id)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(db)?
    } else {
        query_as("SELECT w.id AS media_id,s.library_id,l.node_id,w.kind,COALESCE(json_extract(w.manual_json,'$.title'),w.title) AS title,'' AS artist,'' AS album \
                FROM inventory_assets a \
                JOIN inventory_sources s ON s.id=a.source_id \
                JOIN inventory_versions v ON v.id=s.version_id \
                JOIN inventory_editions e ON e.id=v.edition_id \
                JOIN inventory_works w ON w.id=e.work_id \
                JOIN libraries l ON l.id=s.library_id \
                JOIN media_nodes n ON n.id=l.node_id \
                WHERE a.id=? AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 \
                AND n.status!='revoked' AND w.kind IN ('film','episode') AND (?='administrator' \
                OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
                AND p.user_id=?))")
            .bind(id).bind(role).bind(&actor.user.id).fetch_optional(&auth.store.pool).await.map_err(db)?
    };
    let media = found.ok_or(StatusCode::FORBIDDEN)?;
    if !music
        && !crate::product::parental_allowed(&auth.store, &actor.user.id, &media.media_id).await?
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(media)
}
async fn start(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Outcome<Json<Value>> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    if uuid::Uuid::parse_str(&input.id).is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let media = resolve(&auth, &actor, &input.media_id, input.music).await?;
    let canonical = if input.music {
        media.media_id.clone()
    } else {
        crate::product::canonical_work(&auth.store, &media.media_id).await?
    };
    let device: Option<String> =
        query_scalar("SELECT device_label FROM sessions WHERE token_hash=?")
            .bind(&actor.token_hash)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(db)?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let (enabled, generation): (i64, i64) =
        query_as("SELECT enabled,generation FROM statistics_settings WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if enabled == 0 {
        return Err(StatusCode::CONFLICT);
    }
    let epoch = now();
    query("UPDATE statistics_sessions SET ended_at=updated_at,end_reason='interrupted' WHERE ended_at IS NULL AND updated_at<?").bind(epoch-90).execute(&mut *tx).await.map_err(db)?;
    if let Some(row) =
        query("SELECT user_id,owner_hash,asset_id,ended_at FROM statistics_sessions WHERE id=?")
            .bind(&input.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
    {
        if row.get::<String, _>("user_id") != actor.user.id
            || row.get::<String, _>("owner_hash") != actor.token_hash
            || row.get::<String, _>("asset_id") != input.media_id
        {
            return Err(StatusCode::FORBIDDEN);
        }
        if row.get::<Option<i64>, _>("ended_at").is_some() {
            return Err(StatusCode::GONE);
        }
    } else {
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM statistics_sessions WHERE user_id=? AND ended_at IS NULL",
        )
        .bind(&actor.user.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        let recent: i64 = query_scalar(
            "SELECT COUNT(*) FROM statistics_sessions WHERE user_id=? AND started_at>?",
        )
        .bind(&actor.user.id)
        .bind(epoch - 60)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if count >= 16 || recent >= 60 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        query("INSERT INTO statistics_sessions(id,user_id,owner_hash,generation,media_id,canonical_id,asset_id,library_id,node_id,kind,title,artist,album,device,started_at,updated_at,state_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&input.id).bind(&actor.user.id).bind(&actor.token_hash).bind(generation).bind(media.media_id).bind(canonical).bind(&input.media_id)
            .bind(media.library_id).bind(media.node_id).bind(media.kind).bind(media.title).bind(media.artist).bind(media.album)
            .bind(device.unwrap_or_else(||"Appareil inconnu".into()).chars().filter(|c|!c.is_control()).take(160).collect::<String>())
            .bind(epoch).bind(epoch).bind(serde_json::to_string(&PlaybackState::default()).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?)
            .execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Json(
        json!({"id":input.id,"heartbeatSeconds":15,"timeoutSeconds":90}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    events: Vec<Event>,
}
async fn events(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(batch): Json<Batch>,
) -> Outcome<Json<Value>> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    if batch.events.is_empty()
        || batch.events.len() > 100
        || batch.events.iter().any(|e| !e.valid())
        || batch
            .events
            .windows(2)
            .any(|e| e[0].sequence >= e[1].sequence)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let row = query("SELECT asset_id,kind,user_id,owner_hash FROM statistics_sessions WHERE id=?")
        .bind(&id)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(db)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if row.get::<String, _>("user_id") != actor.user.id
        || row.get::<String, _>("owner_hash") != actor.token_hash
    {
        return Err(StatusCode::FORBIDDEN);
    }
    resolve(
        &auth,
        &actor,
        &row.get::<String, _>("asset_id"),
        row.get::<String, _>("kind") == "music",
    )
    .await?;
    let mut sources = std::collections::HashMap::new();
    for event in &batch.events {
        if let Some(source_id) = &event.source_media_id
            && !sources.contains_key(source_id)
        {
            let source = resolve(
                &auth,
                &actor,
                source_id,
                row.get::<String, _>("kind") == "music",
            )
            .await?;
            sources.insert(source_id.clone(), source);
        }
    }
    let epoch = now();
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let (enabled, generation): (i64, i64) =
        query_as("SELECT enabled,generation FROM statistics_settings WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if enabled == 0 {
        return Err(StatusCode::CONFLICT);
    }
    let row = query("SELECT * FROM statistics_sessions WHERE id=?")
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut last: i64 = row.get("last_sequence");
    let updated: i64 = row.get("updated_at");
    if row.get::<i64, _>("generation") != generation {
        return Err(StatusCode::GONE);
    }
    // An identical retried final batch remains a successful no-op, even after closure.
    if batch.events.last().is_some_and(|e| e.sequence <= last) {
        return Ok(Json(json!({"acceptedThrough":last})));
    }
    if row.get::<Option<i64>, _>("ended_at").is_some() || updated < epoch - 90 {
        return Err(StatusCode::GONE);
    }
    let mut state: PlaybackState = serde_json::from_str(&row.get::<String, _>("state_json"))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let started: i64 = row.get("started_at");
    let mut end: Option<i64> = None;
    let mut reason = String::new();
    for event in batch
        .events
        .iter()
        .filter(|e| e.sequence > row.get::<i64, _>("last_sequence"))
    {
        if event.sequence != last + 1
            || end.is_some()
            || event.elapsed_ms > (epoch - started + 30) as f64 * 1000.0
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let previous_ms = state
            .previous
            .as_ref()
            .map_or(event.elapsed_ms, |e| e.elapsed_ms);
        let dimensions = [
            (
                "audio",
                state
                    .audio
                    .as_ref()
                    .map_or("und", |t| t.language.as_str())
                    .to_owned(),
            ),
            (
                "subtitles",
                state
                    .subtitle
                    .as_ref()
                    .map_or("off", |t| t.language.as_str())
                    .to_owned(),
            ),
            (
                "method",
                if state.method.is_empty() {
                    "unknown".into()
                } else {
                    state.method.clone()
                },
            ),
        ];
        let gained = state.apply(event).map_err(|_| StatusCode::BAD_REQUEST)?;
        if let Some(source) = event
            .source_media_id
            .as_ref()
            .and_then(|id| sources.get(id))
        {
            let canonical = if source.kind == "music" {
                source.media_id.clone()
            } else {
                crate::product::canonical_work(&auth.store, &source.media_id).await?
            };
            if canonical != row.get::<String, _>("canonical_id") {
                return Err(StatusCode::BAD_REQUEST);
            }
            query("UPDATE statistics_sessions SET asset_id=?,media_id=?,library_id=?,node_id=? WHERE id=?")
                .bind(&event.source_media_id).bind(&source.media_id).bind(&source.library_id).bind(&source.node_id).bind(&id).execute(&mut *tx).await.map_err(db)?;
        }
        let occurred = (started + (event.elapsed_ms / 1000.0) as i64).min(epoch);
        query("INSERT INTO statistics_events(session_id,sequence,occurred_at,received_at,event_json) VALUES(?,?,?,?,?)")
            .bind(&id).bind(event.sequence).bind(occurred).bind(epoch).bind(serde_json::to_string(event).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?)
            .execute(&mut *tx).await.map_err(db)?;
        if gained > 0.0 {
            for (dimension, label) in dimensions {
                query("INSERT INTO statistics_dimensions(session_id,dimension,label,seconds) VALUES(?,?,?,?) ON CONFLICT(session_id,dimension,label) DO UPDATE SET seconds=statistics_dimensions.seconds+excluded.seconds")
                    .bind(&id).bind(dimension).bind(label).bind(gained).execute(&mut *tx).await.map_err(db)?;
            }
            // Split observed time across UTC midnight without attributing it to ingestion time.
            let a = started as f64 + previous_ms / 1000.0;
            let b = started as f64 + event.elapsed_ms / 1000.0;
            let day = (a as i64 / 86400) * 86400;
            let boundary = (day + 86400) as f64;
            let first = if b > a {
                gained * ((b.min(boundary) - a) / (b - a)).clamp(0.0, 1.0)
            } else {
                gained
            };
            for (date, seconds) in [(day, first), (day + 86400, gained - first)] {
                if seconds > 0.0 {
                    query("INSERT INTO statistics_daily(session_id,day,seconds) VALUES(?,?,?) ON CONFLICT(session_id,day) DO UPDATE SET seconds=statistics_daily.seconds+excluded.seconds")
                    .bind(&id).bind(date).bind(seconds).execute(&mut *tx).await.map_err(db)?;
                }
            }
        }
        if matches!(event.kind, Kind::Stop | Kind::Ended) {
            end = Some(epoch);
            reason = if event.kind == Kind::Ended {
                "ended"
            } else {
                event.stop_reason.as_deref().unwrap_or("stopped")
            }
            .into();
        }
        last = event.sequence;
    }
    query("UPDATE statistics_sessions SET updated_at=?,ended_at=?,end_reason=?,state_json=?,seconds=?,covered_seconds=?,duration=?,completed=?,last_sequence=? WHERE id=?")
        .bind(epoch).bind(end).bind(reason).bind(serde_json::to_string(&state).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?)
        .bind(state.seconds).bind(state.covered_seconds).bind(state.duration).bind(i64::from(state.completed())).bind(last).bind(&id)
        .execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"acceptedThrough":last})))
}

/// Bounded cleanup, also run by the existing scheduled maintenance task.
pub async fn maintain(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let epoch = now();
    let mut conn = store.pool.acquire().await?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK).await?;
    let (events,summaries):(i64,i64)=query_as("SELECT event_retention_days,summary_retention_days FROM statistics_settings WHERE singleton=1").fetch_one(&mut *tx).await?;
    query("UPDATE statistics_sessions SET ended_at=updated_at,end_reason='interrupted' WHERE id IN (SELECT id FROM statistics_sessions WHERE ended_at IS NULL AND updated_at<? ORDER BY updated_at LIMIT 500)")
        .bind(epoch-90).execute(&mut *tx).await?;
    let deleted=query("DELETE FROM statistics_events WHERE (session_id,sequence) IN (SELECT session_id,sequence FROM statistics_events WHERE received_at<? ORDER BY received_at LIMIT 5000)")
        .bind(epoch-events*86400).execute(&mut *tx).await?.rows_affected();
    if summaries > 0 {
        query("DELETE FROM statistics_sessions WHERE id IN (SELECT id FROM statistics_sessions WHERE ended_at IS NOT NULL AND started_at<? ORDER BY started_at LIMIT 500)")
        .bind(epoch-summaries*86400).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(deleted)
}
async fn purge(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<Json<Value>> {
    administrator(&auth, &headers, true).await?;
    Ok(Json(
        json!({"eventsRemoved":maintain(&auth.store).await.map_err(db)?,"batchLimit":5000}),
    ))
}
async fn erase(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    query("DELETE FROM statistics_sessions WHERE user_id=?")
        .bind(&actor.user.id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
