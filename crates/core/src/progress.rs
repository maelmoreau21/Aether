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
        .route("/api/v1/admin/activity", get(activity))
        .route(
            "/api/v1/account/playback/{id}",
            axum::routing::put(heartbeat).delete(stop),
        )
        .route("/api/v1/progress", get(list))
        .route("/api/v1/progress/{work}", get(read).put(write))
        .layer(DefaultBodyLimit::max(2048))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Heartbeat {
    work_id: String,
    position: f64,
    duration: f64,
    paused: bool,
}
async fn heartbeat(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Heartbeat>,
) -> Result<StatusCode, StatusCode> {
    if uuid::Uuid::parse_str(&id).is_err()
        || !input.position.is_finite()
        || !input.duration.is_finite()
        || input.position < 0.0
        || input.duration <= 0.0
        || input.duration > 604800.0
        || input.position > input.duration
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    authorized(&auth, &headers, &input.work_id, true).await?;
    let actor = auth.authenticate(&headers).await?;
    let mut tx = auth
        .store
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("DELETE FROM active_playbacks WHERE updated_at<unixepoch()-120")
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let count: i64 =
        query_scalar("SELECT COUNT(*) FROM active_playbacks WHERE session_hash=? AND id!=?")
            .bind(&actor.token_hash)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count >= 16 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let changed=query("INSERT INTO active_playbacks(id,session_hash,work_id,position_seconds,duration_seconds,paused,updated_at) VALUES(?,?,?,?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET work_id=excluded.work_id,position_seconds=excluded.position_seconds,duration_seconds=excluded.duration_seconds,paused=excluded.paused,updated_at=excluded.updated_at WHERE active_playbacks.session_hash=excluded.session_hash")
        .bind(id).bind(actor.token_hash).bind(input.work_id).bind(input.position).bind(input.duration).bind(if input.paused{1i64}else{0}).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if changed.rows_affected() == 0 {
        return Err(StatusCode::FORBIDDEN);
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn stop(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    query("DELETE FROM active_playbacks WHERE id=? AND session_hash=?")
        .bind(id)
        .bind(actor.token_hash)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn activity(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    let rows=crate::database::query("SELECT u.id AS user_id,COALESCE(u.display_name,u.sso_username,c.login,'Utilisateur') AS name,s.device_label,s.last_seen_at,a.id AS playback_id,w.title,a.position_seconds,a.duration_seconds,a.paused FROM sessions s JOIN users u ON u.id=s.user_id LEFT JOIN local_credentials c ON c.user_id=u.id LEFT JOIN active_playbacks a ON a.session_hash=s.token_hash AND a.updated_at>unixepoch()-45 LEFT JOIN inventory_works w ON w.id=a.work_id WHERE s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-120 AND u.disabled_at IS NULL ORDER BY s.last_seen_at DESC LIMIT 200")
        .fetch_all(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"sessions":rows.iter().map(|r|json!({"userId":r.get::<String,_>("user_id"),"name":r.get::<String,_>("name"),"device":r.get::<String,_>("device_label"),"lastSeenAt":r.get::<i64,_>("last_seen_at"),"playbackId":r.get::<Option<String>,_>("playback_id"),"title":r.get::<Option<String>,_>("title"),"position":r.get::<Option<f64>,_>("position_seconds"),"duration":r.get::<Option<f64>,_>("duration_seconds"),"paused":r.get::<Option<i64>,_>("paused").map(|v|v==1)})).collect::<Vec<_>>(),"activeWindowSeconds":120}),
    ))
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
    let found: Option<String> = query_scalar(
        "SELECT COALESCE( \
           (SELECT w.id FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id \
            WHERE w.id=? AND w.kind IN ('film','episode') AND l.enabled=1 AND n.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=? OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=?)) \
              AND gw.kind IN ('film','episode') AND gl.enabled=1 AND gn.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=gl.id AND p.user_id=?)) \
            ORDER BY gw.id LIMIT 1) \
         )",
    )
    .bind(work)
    .bind(identity.user.role.as_str())
    .bind(&identity.user.id)
    .bind(work)
    .bind(work)
    .bind(identity.user.role.as_str())
    .bind(&identity.user.id)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .flatten();
    if found.is_none() {
        return Err(StatusCode::FORBIDDEN);
    }
    if !crate::product::target_allowed(&auth.store, &identity.user.id, "work", work).await? {
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
    let requested_work = work.clone();
    let work = crate::product::canonical_work(&auth.store, &work).await?;
    let row = query("SELECT * FROM playback_progress WHERE user_id=? AND work_id=?")
        .bind(&user)
        .bind(&work)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(row) = row.as_ref() {
        let mut saved = value(row);
        saved["workId"] = json!(requested_work);
        return Ok(Json(saved));
    }
    let probe: Option<String> = query_scalar(
        "SELECT a.probe_json FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         WHERE (e.work_id=? OR e.work_id=? OR e.work_id IN (SELECT work_id FROM media_group_members WHERE group_id=?)) \
           AND a.available=1 \
         ORDER BY a.id LIMIT 1",
    )
    .bind(&requested_work)
    .bind(&work)
    .bind(&work)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        json!({"workId":requested_work,"position":0,"duration":duration,"watched":false,"revision":0}),
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
    let requested_work = work.clone();
    let work = crate::product::canonical_work(&auth.store, &work).await?;
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
        .map(|row| {
            let mut saved = value(row);
            saved["workId"] = json!(requested_work);
            Json(saved)
        })
        .ok_or(StatusCode::CONFLICT)
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let identity = auth.authenticate(&headers).await?;
    let role_str = if identity.user.role == Role::Administrator {
        "administrator"
    } else {
        "user"
    };
    let rows = query(
        "SELECT p.*, w.id AS resolved_work_id, COALESCE(json_extract(w.manual_json,'$.title'),w.title) AS title, w.kind, s.series_id \
         FROM playback_progress p \
         JOIN inventory_works w ON w.id = COALESCE( \
           (SELECT dw.id FROM inventory_works dw JOIN libraries dl ON dl.id=dw.library_id JOIN media_nodes dn ON dn.id=dl.node_id \
            WHERE dw.id=p.work_id AND dl.enabled=1 AND dn.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.library_id=dl.id AND lp.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=p.work_id OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=p.work_id)) \
              AND gl.enabled=1 AND gn.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.library_id=gl.id AND lp.user_id=?)) \
            ORDER BY gw.id LIMIT 1) \
         ) \
         LEFT JOIN inventory_episodes ep ON ep.work_id=w.id \
         LEFT JOIN inventory_seasons s ON s.id=ep.season_id \
         WHERE p.user_id=? \
         ORDER BY p.updated_at DESC, p.work_id LIMIT 100",
    )
    .bind(role_str)
    .bind(&identity.user.id)
    .bind(role_str)
    .bind(&identity.user.id)
    .bind(&identity.user.id)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut items: Vec<(String, Value)> = rows
        .iter()
        .map(|row| {
            let mut v = value(row);
            let resolved_id: String = row.get("resolved_work_id");
            v["title"] = json!(row.get::<String, _>("title"));
            v["kind"] = json!(row.get::<String, _>("kind"));
            v["seriesId"] = json!(row.get::<Option<String>, _>("series_id"));
            (resolved_id, v)
        })
        .collect();
    let mut allowed = Vec::new();
    for (resolved_id, item) in items.drain(..) {
        if crate::product::parental_allowed(&auth.store, &identity.user.id, &resolved_id).await? {
            allowed.push(item);
        }
    }
    Ok(Json(json!({"items":allowed})))
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    async fn call(
        app: &Router,
        path: &str,
        method: &str,
        cookie: &str,
        csrf: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(method)
                    .header("cookie", cookie)
                    .header("origin", "http://localhost")
                    .header("content-type", "application/json")
                    .header("x-csrf-token", csrf)
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    #[tokio::test]
    async fn activity_is_authenticated_scoped_expires_and_stops() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&root.path().join("activity.sqlite"))
            .await
            .unwrap();
        let administrator = store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("activity fixture secret".into()),
            )
            .await
            .unwrap();
        crate::libraries::initialize_local(&store).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','/fixture',1)").execute(&store.pool).await.unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,kind,external_ids_json) VALUES('film','films','film','Synthetic film','film','{}')").execute(&store.pool).await.unwrap();
        query("INSERT INTO users(id,role) VALUES('reader','user')")
            .execute(&store.pool)
            .await
            .unwrap();
        query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
            .execute(&store.pool)
            .await
            .unwrap();
        for (token, user) in [
            ("a".repeat(64), administrator),
            ("b".repeat(64), "reader".into()),
        ] {
            query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).bind(user).execute(&store.pool).await.unwrap();
        }
        let auth = AuthState::new(store.clone(), "http://localhost".into())
            .await
            .unwrap();
        let app = router(auth.clone()).merge(crate::auth::router(auth));
        let a = format!("media_session={}", "a".repeat(64));
        let b = format!("media_session={}", "b".repeat(64));
        let ac = call(&app, "/api/v1/auth/me", "GET", &a, "", Value::Null)
            .await
            .1["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned();
        let bc = call(&app, "/api/v1/auth/me", "GET", &b, "", Value::Null)
            .await
            .1["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            call(&app, "/api/v1/admin/activity", "GET", "", "", Value::Null)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, "/api/v1/admin/activity", "GET", &b, "", Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        let id = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/v1/account/playback/{id}");
        let body = json!({"workId":"film","position":12.0,"duration":120.0,"paused":false});
        assert_eq!(
            call(&app, &path, "PUT", &b, "", body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&app, &path, "PUT", &b, &bc, body.clone()).await.0,
            StatusCode::NO_CONTENT
        );
        let activity = call(&app, "/api/v1/admin/activity", "GET", &a, "", Value::Null)
            .await
            .1;
        let playback = activity["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["playbackId"] == id)
            .unwrap();
        assert_eq!(playback["title"], "Synthetic film");
        assert_eq!(playback["paused"], false);
        assert!(playback.get("session_hash").is_none());
        assert_eq!(
            call(&app, &path, "PUT", &a, &ac, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        call(&app, &path, "DELETE", &a, &ac, Value::Null).await;
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM active_playbacks")
                .fetch_one(&store.pool)
                .await
                .unwrap(),
            1
        );
        query("UPDATE active_playbacks SET updated_at=unixepoch()-46")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(
            call(&app, "/api/v1/admin/activity", "GET", &a, "", Value::Null)
                .await
                .1["sessions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["playbackId"].is_null())
        );
        assert_eq!(
            call(&app, &path, "DELETE", &b, &bc, Value::Null).await.0,
            StatusCode::NO_CONTENT
        );
        query("DELETE FROM library_permissions WHERE user_id='reader'")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            call(&app, &path, "PUT", &b, &bc, body).await.0,
            StatusCode::FORBIDDEN
        );
    }
}
