//! Authenticated playlist sharing never grants access to the underlying library.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
type Error = StatusCode;
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/playlists/policy", get(policy))
        .route(
            "/api/v1/admin/playlists/policy",
            get(policy).put(set_policy),
        )
        .route("/api/v1/playlists/recipients", get(recipients))
        .route(
            "/api/v1/playlists/{id}/settings",
            axum::routing::put(settings),
        )
        .route(
            "/api/v1/playlists/{id}/music/{track}",
            axum::routing::put(add_music).delete(remove_music),
        )
        .route("/api/v1/playlists/{id}/order", axum::routing::put(reorder))
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
pub async fn flags(auth: &AuthState) -> Result<(i64, i64), Error> {
    crate::database::query_as(
        "SELECT allow_public,allow_sharing FROM playlist_policy WHERE singleton=1",
    )
    .fetch_one(&auth.store.pool)
    .await
    .map_err(|_| Error::INTERNAL_SERVER_ERROR)
}
async fn policy(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers).await?;
    let (public, sharing) = flags(&auth).await?;
    Ok(Json(
        json!({"allowPublic":public!=0,"allowSharing":sharing!=0}),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    allow_public: bool,
    allow_sharing: bool,
}
async fn set_policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Policy>,
) -> Result<StatusCode, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    if actor.user.role != Role::Administrator {
        return Err(Error::FORBIDDEN);
    }
    query("UPDATE playlist_policy SET allow_public=?,allow_sharing=? WHERE singleton=1")
        .bind(i64::from(input.allow_public))
        .bind(i64::from(input.allow_sharing))
        .execute(&auth.store.pool)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn access(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = auth.authenticate(headers).await?;
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    let count:i64=query_scalar("SELECT COUNT(*) FROM personal_lists p WHERE p.id=? AND (p.user_id=? OR (?=0 AND ((p.visibility='public' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_public=1)) OR (p.visibility='shared' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_sharing=1) AND EXISTS(SELECT 1 FROM playlist_shares s WHERE s.list_id=p.id AND s.user_id=?)))))").bind(id).bind(&actor.user.id).bind(i64::from(mutation)).bind(&actor.user.id).fetch_one(&auth.store.pool).await.map_err(|_|Error::INTERNAL_SERVER_ERROR)?;
    if count != 1 {
        return Err(Error::NOT_FOUND);
    }
    Ok(actor)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    name: String,
    visibility: String,
    #[serde(default)]
    user_ids: Vec<String>,
}
async fn settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Settings>,
) -> Result<StatusCode, Error> {
    let actor = access(&auth, &headers, &id, true).await?;
    if input.name.trim().is_empty()
        || input.name.chars().count() > 100
        || input.name.chars().any(char::is_control)
        || !matches!(input.visibility.as_str(), "private" | "shared" | "public")
        || input.user_ids.len() > 50
    {
        return Err(Error::BAD_REQUEST);
    }
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    // Lock the policy row too, so admin revocation and publication serialize.
    query("UPDATE playlist_policy SET singleton=singleton WHERE singleton=1")
        .execute(&mut *tx)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    let (public, sharing): (i64, i64) = crate::database::query_as(
        "SELECT allow_public,allow_sharing FROM playlist_policy WHERE singleton=1",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    if (input.visibility == "public" && public == 0)
        || (input.visibility == "shared" && sharing == 0)
    {
        return Err(Error::FORBIDDEN);
    }
    query("UPDATE personal_lists SET name=?,visibility=? WHERE id=? AND user_id=?")
        .bind(input.name.trim())
        .bind(&input.visibility)
        .bind(&id)
        .bind(&actor.user.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    query("DELETE FROM playlist_shares WHERE list_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    if input.visibility == "shared" {
        for user in input.user_ids {
            let exists: i64 =
                query_scalar("SELECT COUNT(*) FROM users WHERE id=? AND disabled_at IS NULL")
                    .bind(&user)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
            if exists != 1 {
                return Err(Error::BAD_REQUEST);
            }
            query(
                "INSERT INTO playlist_shares(list_id,user_id) VALUES(?,?) ON CONFLICT DO NOTHING",
            )
            .bind(&id)
            .bind(user)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
        }
    }
    tx.commit()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    q: String,
}
async fn recipients(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(search): Query<Search>,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    if flags(&auth).await?.1 == 0 {
        return Err(Error::FORBIDDEN);
    }
    if search.q.chars().count() < 2 || search.q.len() > 100 {
        return Err(Error::BAD_REQUEST);
    }
    let term = format!(
        "%{}%",
        search
            .q
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows=query("SELECT u.id,COALESCE(u.display_name,c.login,u.id) AS name FROM users u LEFT JOIN local_credentials c ON c.user_id=u.id WHERE u.disabled_at IS NULL AND u.id!=? AND lower(COALESCE(u.display_name,c.login,u.id)) LIKE ? ESCAPE '\\' ORDER BY name,u.id LIMIT 20").bind(actor.user.id).bind(term).fetch_all(&auth.store.pool).await.map_err(|_|Error::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"users":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name")})).collect::<Vec<_>>()}),
    ))
}
pub async fn insert_item(auth: &AuthState, id: &str, item: &str, music: bool) -> Result<(), Error> {
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    let exists_sql = if music {
        "SELECT COUNT(*) FROM playlist_music_items WHERE list_id=? AND track_id=?"
    } else {
        "SELECT COUNT(*) FROM personal_list_items WHERE list_id=? AND work_id=?"
    };
    let exists: i64 = query_scalar(exists_sql)
        .bind(id)
        .bind(item)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        let (count,position):(i64,i64)=crate::database::query_as("SELECT COUNT(*),COALESCE(MAX(position),-1)+1 FROM (SELECT position FROM personal_list_items WHERE list_id=? UNION ALL SELECT position FROM playlist_music_items WHERE list_id=?) entries").bind(id).bind(id).fetch_one(&mut *tx).await.map_err(|_|Error::INTERNAL_SERVER_ERROR)?;
        if count >= 1000 {
            return Err(Error::CONFLICT);
        }
        let sql = if music {
            "INSERT INTO playlist_music_items(list_id,track_id,position) VALUES(?,?,?)"
        } else {
            "INSERT INTO personal_list_items(list_id,work_id,position) VALUES(?,?,?)"
        };
        query(sql)
            .bind(id)
            .bind(item)
            .bind(position)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    }
    tx.commit()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    Ok(())
}
async fn add_music(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, track)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    access(&auth, &headers, &id, true).await?;
    crate::music::detail(&auth, &headers, &track).await?;
    insert_item(&auth, &id, &track, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_music(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, track)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    access(&auth, &headers, &id, true).await?;
    query("DELETE FROM playlist_music_items WHERE list_id=? AND track_id=?")
        .bind(id)
        .bind(track)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: String,
    id: String,
}
async fn reorder(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(entries): Json<Vec<Entry>>,
) -> Result<StatusCode, Error> {
    access(&auth, &headers, &id, true).await?;
    if entries.len() > 2000 {
        return Err(Error::BAD_REQUEST);
    }
    let mut seen = std::collections::HashSet::new();
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    for (position, entry) in entries.into_iter().enumerate() {
        if !seen.insert((entry.kind.clone(), entry.id.clone())) {
            return Err(Error::BAD_REQUEST);
        }
        let sql = match entry.kind.as_str() {
            "work" => "UPDATE personal_list_items SET position=? WHERE list_id=? AND work_id=?",
            "music" => "UPDATE playlist_music_items SET position=? WHERE list_id=? AND track_id=?",
            _ => return Err(Error::BAD_REQUEST),
        };
        let changed = query(sql)
            .bind(position as i64)
            .bind(&id)
            .bind(entry.id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
        if changed.rows_affected() != 1 {
            return Err(Error::BAD_REQUEST);
        }
    }
    tx.commit()
        .await
        .map_err(|_| Error::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
