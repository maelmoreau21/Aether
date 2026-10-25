//! Personal lists and explicit editorial controls. No changes to source media or sidecars.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
type Error = StatusCode;
static POSTER_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/lists", get(lists).post(create_list))
        .route("/api/v1/lists/{id}", get(list).delete(delete_list))
        .route(
            "/api/v1/lists/{id}/items/{work}",
            axum::routing::put(add_item).delete(remove_item),
        )
        .route(
            "/api/v1/admin/works/{work}/metadata",
            axum::routing::put(metadata).delete(reset_metadata),
        )
        .route(
            "/api/v1/works/{work}/poster",
            get(poster).put(upload_poster).delete(remove_poster),
        )
        .route(
            "/api/v1/admin/users/{user}/parental",
            get(parental).put(set_parental).delete(reset_parental),
        )
        .route("/api/v1/admin/media-groups", axum::routing::post(group))
        .route(
            "/api/v1/admin/media-groups/candidates",
            get(list_candidates),
        )
        .route(
            "/api/v1/admin/media-groups/candidates/{id}/confirm",
            axum::routing::post(confirm_candidate),
        )
        .route(
            "/api/v1/admin/media-groups/candidates/{id}/reject",
            axum::routing::post(reject_candidate),
        )
        .route(
            "/api/v1/admin/media-groups/merges/{id}/undo",
            axum::routing::post(undo_merge),
        )
        .route(
            "/api/v1/admin/media-groups/{work}",
            axum::routing::delete(ungroup),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(auth)
}
async fn admin(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(actor)
}
async fn owner(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    mutation: bool,
) -> Result<String, Error> {
    let actor = auth.authenticate(headers).await?;
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM personal_lists WHERE id=? AND user_id=?")
        .bind(id)
        .bind(&actor.user.id)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count != 1 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(actor.user.id)
}
async fn lists(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let rows: Vec<(String, String,String,String)> =
        query_as("SELECT p.id,p.name,p.visibility,p.user_id FROM personal_lists p WHERE p.user_id=? OR (p.visibility='public' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_public=1)) OR (p.visibility='shared' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_sharing=1) AND EXISTS(SELECT 1 FROM playlist_shares s WHERE s.list_id=p.id AND s.user_id=?)) ORDER BY p.name,p.id LIMIT 500")
            .bind(&actor.user.id).bind(&actor.user.id)
            .fetch_all(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"lists":rows.into_iter().map(|(id,name,visibility,user)|json!({"id":id,"name":name,"visibility":visibility,"owned":user==actor.user.id})).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListName {
    name: String,
}
async fn create_list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<ListName>,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM personal_lists WHERE user_id=?")
        .bind(&actor.user.id)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count >= 100 {
        return Err(StatusCode::CONFLICT);
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO personal_lists(id,user_id,name) VALUES(?,?,?)")
        .bind(&id)
        .bind(actor.user.id)
        .bind(name)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"id":id,"name":name})))
}
#[derive(Default, Deserialize)]
struct ListView {
    #[serde(default)]
    compact: bool,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(view): Query<ListView>,
) -> Result<Json<Value>, Error> {
    let actor = crate::playlists::access(&auth, &headers, &id, false).await?;
    let (name, visibility, user): (String, String, String) =
        query_as("SELECT name,visibility,user_id FROM personal_lists WHERE id=?")
            .bind(&id)
            .fetch_one(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // List presentation needs summaries, not recursive series details and all their assets.
    // Apply library authorization in this batch (including via group member works); parental policy still applies per work.
    let works: Vec<(String, i64, String, String, String, Option<String>)> = query_as(
        "SELECT w.id,i.position,w.title,w.kind,w.editorial_json,w.manual_json FROM personal_list_items i \
         JOIN inventory_works w ON w.id = COALESCE( \
           (SELECT dw.id FROM inventory_works dw JOIN libraries dl ON dl.id=dw.library_id JOIN media_nodes dn ON dn.id=dl.node_id \
            WHERE dw.id=i.work_id AND dl.enabled=1 AND dn.status!='revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=dl.id AND p.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=i.work_id OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=i.work_id)) \
              AND gl.enabled=1 AND gn.status!='revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=gl.id AND p.user_id=?)) \
            ORDER BY gw.id LIMIT 1) \
         ) \
         WHERE i.list_id=? AND w.kind IN ('film','series','episode') \
         ORDER BY i.position,w.id LIMIT 1000",
    )
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .bind(&id)
    .fetch_all(&auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut items = Vec::new();
    let mut entries = Vec::new();
    for (work, position, mut title, kind, editorial, manual) in works {
        if !parental_allowed(&auth.store, &actor.user.id, &work).await? {
            continue;
        }
        let media = if view.compact {
            for source in [Some(editorial), manual].into_iter().flatten() {
                if let Ok(metadata) = serde_json::from_str::<Value>(&source)
                    && let Some(override_title) = metadata["title"].as_str()
                {
                    title = override_title.to_owned();
                }
            }
            Some(json!({"id":work,"title":title,"kind":kind}))
        } else {
            crate::inventory::detail(&auth.store, &work)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        };
        if let Some(media) = media {
            entries.push(json!({"kind":"work","id":work,"position":position,"media":media}));
            items.push(media);
        }
    }
    let tracks:Vec<(String,i64)>=query_as("SELECT track_id,position FROM playlist_music_items WHERE list_id=? ORDER BY position,track_id LIMIT 1000").bind(&id).fetch_all(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    for (track, position) in tracks {
        match crate::music::detail(&auth, &headers, &track).await {
            Ok(media) => {
                entries.push(json!({"kind":"music","id":track,"position":position,"media":media}))
            }
            Err(StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) => {}
            Err(error) => return Err(error),
        }
    }
    entries.sort_by_key(|entry| entry["position"].as_i64().unwrap_or(0));
    let shares: Vec<(String, String)> = if user == actor.user.id {
        query_as("SELECT s.user_id,COALESCE(u.display_name,c.login,u.id) FROM playlist_shares s JOIN users u ON u.id=s.user_id LEFT JOIN local_credentials c ON c.user_id=u.id WHERE s.list_id=? ORDER BY s.user_id").bind(&id).fetch_all(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        vec![]
    };
    Ok(Json(
        json!({"id":id,"name":name,"visibility":visibility,"owned":user==actor.user.id,"items":items,"entries":entries,"shares":shares.into_iter().map(|(id,name)|json!({"id":id,"name":name})).collect::<Vec<_>>()}),
    ))
}
async fn delete_list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let user = owner(&auth, &headers, &id, true).await?;
    query("DELETE FROM personal_lists WHERE id=? AND user_id=?")
        .bind(id)
        .bind(user)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn add_item(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, work)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    owner(&auth, &headers, &id, true).await?;
    crate::feedback::authorized(&auth, &headers, &work, true).await?;
    if !target_allowed(
        &auth.store,
        &auth.authenticate(&headers).await?.user.id,
        "work",
        &work,
    )
    .await?
    {
        return Err(StatusCode::FORBIDDEN);
    }
    crate::playlists::insert_item(&auth, &id, &work, false).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_item(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, work)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    owner(&auth, &headers, &id, true).await?;
    let canonical = canonical_work(&auth.store, &work).await?;
    query("DELETE FROM personal_list_items WHERE list_id=? AND (work_id=? OR work_id=?)")
        .bind(id)
        .bind(&work)
        .bind(&canonical)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Editorial {
    title: String,
    year: Option<i64>,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    plot: String,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    directors: Vec<String>,
    #[serde(default)]
    writers: Vec<String>,
    #[serde(default)]
    actors: Vec<Value>,
    #[serde(default)]
    ratings: Vec<Value>,
    #[serde(default)]
    external_ids: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    poster_url: Option<String>,
    #[serde(default)]
    backdrop_url: Option<String>,
    #[serde(default)]
    minimum_age: Option<i64>,
}
async fn metadata(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
    Json(input): Json<Editorial>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    if input.title.trim().is_empty()
        || input.title.len() > 512
        || input.plot.len() > 20000
        || input.genres.len() > 32
        || input.genres.iter().any(|v| v.len() > 100)
        || input.tags.len() > 64
        || input
            .tags
            .iter()
            .any(|v| v.len() > 100 || v.chars().any(char::is_control))
        || input.external_ids.len() > 16
        || input
            .external_ids
            .iter()
            .any(|(k, v)| k.len() > 64 || v.len() > 256)
        || input.year.is_some_and(|v| !(1800..=2200).contains(&v))
        || input.minimum_age.is_some_and(|v| !(0..=21).contains(&v))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let value = json!({
        "title": input.title,
        "original_title": input.original_title,
        "tagline": input.tagline,
        "year": input.year,
        "plot": input.plot,
        "genres": input.genres,
        "tags": input.tags.iter().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "directors": input.directors,
        "writers": input.writers,
        "actors": input.actors,
        "ratings": input.ratings,
        "external_ids": input.external_ids,
        "poster_url": input.poster_url,
        "backdrop_url": input.backdrop_url,
        "provenance": "extension"
    });
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = query("UPDATE inventory_works SET manual_json=?,minimum_age=? WHERE id=?")
        .bind(value.to_string())
        .bind(input.minimum_age)
        .bind(&work)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    crate::consolidation::consolidate_work_tx(&mut tx, &work)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn reset_metadata(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("UPDATE inventory_works SET manual_json=NULL,minimum_age=NULL WHERE id=?")
        .bind(&work)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = crate::consolidation::consolidate_work_tx(&mut tx, &work).await;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn upload_poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
    bytes: Bytes,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    crate::feedback::authorized(&auth, &headers, &work, false).await?;
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let permit = POSTER_WORKERS
        .try_acquire()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let image = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::avatars::normalize_image(bytes, &content, true)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    query("INSERT INTO work_posters(work_id,image_data,revision) VALUES(?,?,?) ON CONFLICT(work_id) DO UPDATE SET image_data=excluded.image_data,revision=excluded.revision").bind(work).bind(image).bind(uuid::Uuid::new_v4().to_string()).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM work_posters WHERE work_id=?")
        .bind(work)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
    axum::extract::Query(options): axum::extract::Query<crate::artwork::PreviewOptions>,
) -> Result<Response, Error> {
    crate::feedback::authorized(&auth, &headers, &work, false).await?;
    crate::artwork::legacy(&auth.store, &work, options.width.unwrap_or(0), &headers).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    maximum_age: i64,
    allow_unrated: bool,
    #[serde(default)]
    blocked_tags: Vec<String>,
    #[serde(default)]
    access_schedule: Vec<AccessWindow>,
}
#[derive(serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccessWindow {
    /// Monday=0, Sunday=6. Minutes are UTC, end exclusive; split overnight windows.
    day: i64,
    start_minute: i64,
    end_minute: i64,
}
pub(crate) fn schedule_allows(windows: &[AccessWindow], epoch: i64) -> bool {
    let day = (epoch.div_euclid(86400) + 3).rem_euclid(7);
    let minute = epoch.rem_euclid(86400) / 60;
    windows.is_empty()
        || windows
            .iter()
            .any(|w| w.day == day && w.start_minute <= minute && minute < w.end_minute)
}
pub(crate) async fn access_now(
    store: &IdentityStore,
    user: &str,
    epoch: i64,
) -> Result<bool, Error> {
    let encoded: Option<String> =
        query_scalar("SELECT access_schedule_json FROM parental_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match encoded {
        None => Ok(true),
        Some(encoded) => {
            let windows: Vec<AccessWindow> =
                serde_json::from_str(&encoded).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(schedule_allows(&windows, epoch))
        }
    }
}
async fn parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let row: Option<(i64, i64, String, String)> =
        query_as("SELECT maximum_age,allow_unrated,blocked_tags_json,access_schedule_json FROM parental_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        row.map(|(age, unrated, tags, schedule)| json!({"maximumAge":age,"allowUnrated":unrated!=0,"blockedTags":serde_json::from_str::<Value>(&tags).unwrap_or(Value::Null),"accessSchedule":serde_json::from_str::<Value>(&schedule).unwrap_or(Value::Null)}))
            .unwrap_or(Value::Null),
    ))
}
async fn set_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
    Json(input): Json<Policy>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    if !(0..=21).contains(&input.maximum_age)
        || input.blocked_tags.len() > 64
        || input
            .blocked_tags
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 100 || v.chars().any(char::is_control))
        || input.access_schedule.len() > 56
        || input.access_schedule.iter().any(|w| {
            !(0..=6).contains(&w.day)
                || !(0..1440).contains(&w.start_minute)
                || !(1..=1440).contains(&w.end_minute)
                || w.start_minute >= w.end_minute
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let exists: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=?")
        .bind(&user)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let tags: Vec<String> = input
        .blocked_tags
        .iter()
        .map(|v| v.trim().to_lowercase())
        .collect();
    let mut tx = auth
        .store
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated,blocked_tags_json,access_schedule_json) VALUES(?,?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET maximum_age=excluded.maximum_age,allow_unrated=excluded.allow_unrated,blocked_tags_json=excluded.blocked_tags_json,access_schedule_json=excluded.access_schedule_json").bind(&user).bind(input.maximum_age).bind(i64::from(input.allow_unrated)).bind(json!(tags).to_string()).bind(json!(input.access_schedule).to_string()).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    query("DELETE FROM parental_blocked_tags WHERE user_id=?")
        .bind(&user)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for tag in tags {
        query("INSERT INTO parental_blocked_tags(user_id,tag_json) VALUES(?,?) ON CONFLICT DO NOTHING").bind(&user).bind(json!(tag).to_string()).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn reset_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(user): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM parental_policies WHERE user_id=?")
        .bind(user)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}

// The same expression is used before catalogue pagination and on every derived resource.
pub const AGE_SQL: &str = "COALESCE(w.minimum_age,(SELECT parent.minimum_age FROM inventory_episodes ep JOIN inventory_seasons season ON season.id=ep.season_id JOIN inventory_works parent ON parent.id=season.series_id WHERE ep.work_id=w.id),CASE upper(COALESCE(json_extract(w.manual_json,'$.mpaa'),json_extract(w.editorial_json,'$.mpaa'))) WHEN 'G' THEN 0 WHEN 'TV-G' THEN 0 WHEN 'PG' THEN 10 WHEN 'PG-13' THEN 13 WHEN 'TV-14' THEN 14 WHEN 'R' THEN 17 WHEN 'NC-17' THEN 18 WHEN 'TV-MA' THEN 18 WHEN '-10' THEN 10 WHEN 'FR-10' THEN 10 WHEN '-12' THEN 12 WHEN 'FR-12' THEN 12 WHEN '-16' THEN 16 WHEN 'FR-16' THEN 16 WHEN '-18' THEN 18 WHEN 'FR-18' THEN 18 WHEN 'TOUS PUBLICS' THEN 0 ELSE NULL END)";
pub fn policy_sql() -> String {
    format!(
        "NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=? AND (({AGE_SQL})>policy.maximum_age OR (({AGE_SQL}) IS NULL AND policy.allow_unrated=0) OR {BLOCKED_TAGS_SQL}))"
    )
}
pub const BLOCKED_TAGS_SQL: &str = "EXISTS(SELECT 1 FROM parental_blocked_tags bt WHERE bt.user_id=policy.user_id AND (instr(lower(COALESCE(json_extract(w.manual_json,'$.tags'),json_extract(w.editorial_json,'$.tags'),'[]')),bt.tag_json)>0 OR EXISTS(SELECT 1 FROM inventory_episodes te JOIN inventory_seasons ts ON ts.id=te.season_id JOIN inventory_works tw ON tw.id=ts.series_id WHERE te.work_id=w.id AND instr(lower(COALESCE(json_extract(tw.manual_json,'$.tags'),json_extract(tw.editorial_json,'$.tags'),'[]')),bt.tag_json)>0)))";
pub async fn parental_allowed(
    store: &IdentityStore,
    user: &str,
    work: &str,
) -> Result<bool, Error> {
    let sql = format!(
        "SELECT COUNT(*) FROM inventory_works w WHERE w.id=? AND {}",
        policy_sql()
    );
    let sql = crate::database::normalize_sql(&sql);
    let allowed: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(work)
        .bind(user)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(allowed == 1)
}
pub async fn target_allowed(
    store: &IdentityStore,
    user: &str,
    kind: &str,
    target: &str,
) -> Result<bool, Error> {
    let policies: i64 = query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=?")
        .bind(user)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if policies == 0 {
        return Ok(true);
    }
    let work:Option<String>=match kind{
        "work"=>Some(target.to_owned()),
        "image"=>query_scalar("SELECT work_id FROM inventory_images WHERE id=?").bind(target).fetch_optional(&store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?,
        _=>query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE a.id=?").bind(target).fetch_optional(&store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    let Some(work) = work else {
        return Ok(false);
    };
    if kind == "work" {
        if parental_allowed(store, user, &work).await? {
            return Ok(true);
        }
        // If `work` is part of a group, allow opening the unified work card as long as at least
        // one member work in an authorized library is parental-allowed (expand_detail will swap
        // out the restricted base work metadata and exclude restricted editions/sources).
        let role_str: Option<String> = query_scalar("SELECT role FROM users WHERE id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let role = role_str.as_deref().unwrap_or("user");
        let members: Vec<String> = query_scalar(
            "SELECT gm.work_id FROM media_group_members gm \
             JOIN inventory_works w ON w.id = gm.work_id \
             JOIN libraries l ON l.id = w.library_id \
             JOIN media_nodes n ON n.id = l.node_id \
             WHERE (gm.group_id = ? OR gm.group_id = (SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id = ?)) \
               AND l.enabled = 1 AND n.status != 'revoked' \
               AND (? = 'administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.library_id = l.id AND lp.user_id = ?))",
        )
        .bind(&work)
        .bind(&work)
        .bind(role)
        .bind(user)
        .fetch_all(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for member in members {
            if parental_allowed(store, user, &member).await? {
                return Ok(true);
            }
        }
        Ok(false)
    } else {
        parental_allowed(store, user, &work).await
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Group {
    work_ids: Vec<String>,
    label: String,
}
async fn group(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Group>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if !(2..=32).contains(&input.work_ids.len())
        || input.label.is_empty()
        || input.label.len() > 512
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input
        .work_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != input.work_ids.len()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut kinds = std::collections::BTreeSet::new();
    for work in &input.work_ids {
        let kind: Option<String> = query_scalar(
            "SELECT kind FROM inventory_works WHERE id=? AND kind IN ('film','series','episode')",
        )
        .bind(work)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let Some(kind) = kind else {
            return Err(StatusCode::BAD_REQUEST);
        };
        kinds.insert(kind);
    }
    if kinds.len() != 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (id, merge_id) = crate::consolidation::merge_works_tx(
        &mut tx,
        &input.work_ids,
        &input.label,
        "manual",
        None,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(Json(json!({"id": id, "mergeId": merge_id})))
}
async fn ungroup(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::consolidation::ungroup_work_tx(&mut tx, &work)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Default, Deserialize)]
struct CandidateQuery {
    status: Option<String>,
}

async fn list_candidates(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(params): Query<CandidateQuery>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let candidates =
        crate::consolidation::list_match_candidates(&auth.store, params.status.as_deref())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let merges = crate::consolidation::list_merge_history(&auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "candidates": candidates,
        "merges": merges
    })))
}

async fn confirm_candidate(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = crate::consolidation::confirm_candidate_tx(&mut tx, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some((group_id, merge_id)) = result else {
        return Err(StatusCode::NOT_FOUND);
    };
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(Json(json!({
        "id": group_id,
        "mergeId": merge_id
    })))
}

async fn reject_candidate(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rejected = crate::consolidation::reject_candidate_tx(&mut tx, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !rejected {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn undo_merge(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let undone = crate::consolidation::undo_merge_tx(&mut tx, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !undone {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}

pub async fn decorate(store: &IdentityStore, value: &mut Value) -> Result<(), sqlx::Error> {
    decorate_many(store, std::slice::from_mut(value)).await
}
pub async fn decorate_many(store: &IdentityStore, values: &mut [Value]) -> Result<(), sqlx::Error> {
    use sqlx::Execute;
    let ids: Vec<String> = values
        .iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let images = crate::artwork::entries(store, &ids).await?;
    let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT work_id,group_id FROM media_group_members WHERE work_id IN (",
    );
    let mut bind = builder.separated(",");
    for id in &ids {
        bind.push_bind(id);
    }
    bind.push_unseparated(")");
    let sql = crate::database::normalize_sql(builder.sql().as_str());
    let args = builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let groups: Vec<(String, String)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql), args)
        .fetch_all(&store.pool)
        .await?;
    for value in values {
        let id = value["id"].as_str().unwrap_or_default().to_owned();
        value["globalMediaId"] = json!(groups.iter().find(|g| g.0 == id).map(|g| &g.1));
        let mut artwork = value["artwork"].as_object().cloned().unwrap_or_default();
        for role in crate::artwork::ROLES {
            artwork.remove(*role);
        }
        for image in images.iter().filter(|e| e.work_id == id) {
            artwork
                .entry(image.kind.clone())
                .or_insert(json!(image.url));
        }
        value["posterUrl"] = artwork.get("poster").cloned().unwrap_or(Value::Null);
        value["thumbUrl"] = artwork.get("thumb").cloned().unwrap_or(Value::Null);
        value["artwork"] = Value::Object(artwork);
    }
    Ok(())
}

fn merge_edition_arrays(target: &mut Vec<Value>, incoming: &[Value]) {
    for inc_ed in incoming {
        let inc_label = inc_ed["label"].as_str().unwrap_or("Édition locale");
        if let Some(existing_ed) = target
            .iter_mut()
            .find(|ed| ed["label"].as_str().unwrap_or("Édition locale") == inc_label)
        {
            if let (Some(target_versions), Some(inc_versions)) = (
                existing_ed["versions"].as_array_mut(),
                inc_ed["versions"].as_array(),
            ) {
                for inc_ver in inc_versions {
                    let inc_ver_label = inc_ver["label"].as_str().unwrap_or("Version locale");
                    if let Some(existing_ver) = target_versions
                        .iter_mut()
                        .find(|v| v["label"].as_str().unwrap_or("Version locale") == inc_ver_label)
                    {
                        if let (Some(target_sources), Some(inc_sources)) = (
                            existing_ver["sources"].as_array_mut(),
                            inc_ver["sources"].as_array(),
                        ) {
                            for src in inc_sources {
                                let src_id = src["id"].as_str().unwrap_or_default();
                                if !target_sources
                                    .iter()
                                    .any(|s| s["id"].as_str() == Some(src_id))
                                {
                                    target_sources.push(src.clone());
                                }
                            }
                        }
                    } else {
                        target_versions.push(inc_ver.clone());
                    }
                }
            }
        } else {
            target.push(inc_ed.clone());
        }
    }
    crate::inventory::sort_edition_sources_by_score(target);
}

pub async fn expand_detail(
    store: &IdentityStore,
    user: &str,
    visible: &[String],
    value: &mut Value,
) -> Result<(), StatusCode> {
    let current_id = value["id"].as_str().unwrap_or_default().to_owned();
    let current_lib = value["libraryId"].as_str().unwrap_or_default().to_owned();
    let current_authorized = !current_id.is_empty()
        && visible.contains(&current_lib)
        && parental_allowed(store, user, &current_id).await?;

    let group = if let Some(g) = value["globalMediaId"].as_str() {
        Some(g.to_owned())
    } else {
        let g: Option<String> =
            query_scalar("SELECT group_id FROM media_group_members WHERE work_id=? LIMIT 1")
                .bind(&current_id)
                .fetch_optional(&store.pool)
                .await
                .ok()
                .flatten();
        if let Some(ref gid) = g {
            value["globalMediaId"] = json!(gid);
        }
        g
    };

    // If the requested work itself is not authorized for this user (e.g. accessed via group ID
    // where the canonical work is in a library the user cannot see or is parental-restricted),
    // swap the base work payload with the first authorized member of the group so no restricted
    // metadata, libraryId, or editions leak.
    if !current_authorized {
        let group_opt = group.clone().or_else(|| Some(current_id.clone()));
        let mut replacement = None;
        if let Some(group) = group_opt {
            let siblings: Vec<(String, String)> = query_as(
                "SELECT w.id,w.library_id FROM media_group_members member JOIN inventory_works w ON w.id=member.work_id WHERE member.group_id=? ORDER BY w.id",
            )
            .bind(&group)
            .fetch_all(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            for (sib_id, sib_lib) in siblings {
                if visible.contains(&sib_lib)
                    && parental_allowed(store, user, &sib_id).await?
                    && let Some(detail) = crate::inventory::detail(store, &sib_id)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                {
                    replacement = Some(detail);
                    break;
                }
            }
        }
        if let Some(detail) = replacement {
            *value = detail;
        } else {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    if let Some(seasons) = value["seasons"].as_array_mut() {
        for season in seasons {
            if let Some(episodes) = season["episodes"].as_array_mut() {
                let mut filtered = Vec::new();
                for episode in episodes.drain(..) {
                    if let Some(id) = episode["id"].as_str()
                        && parental_allowed(store, user, id).await?
                    {
                        filtered.push(episode);
                    }
                }
                crate::product::decorate_many(store, &mut filtered)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                *episodes = filtered;
            }
        }
    }
    let Some(group) = group else {
        return Ok(());
    };
    let original = value["id"].as_str().unwrap_or_default().to_owned();
    let siblings: Vec<(String, String)> = query_as(
        "SELECT w.id,w.library_id FROM media_group_members member JOIN inventory_works w ON w.id=member.work_id WHERE member.group_id=? AND w.id!=? ORDER BY w.id",
    )
    .bind(group)
    .bind(original)
    .fetch_all(&store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for (id, library) in siblings {
        if visible.contains(&library)
            && parental_allowed(store, user, &id).await?
            && let Some(detail) = crate::inventory::detail(store, &id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            if detail["available"] == true {
                value["available"] = json!(true);
            }
            if let (Some(target), Some(editions)) = (
                value["editions"].as_array_mut(),
                detail["editions"].as_array(),
            ) {
                merge_edition_arrays(target, editions);
            }
            // Merge seasons and episodes across grouped series works
            if let (Some(target_seasons), Some(inc_seasons)) = (
                value["seasons"].as_array_mut(),
                detail["seasons"].as_array(),
            ) {
                for inc_season in inc_seasons {
                    let s_num = inc_season["number"].as_i64().unwrap_or(0);
                    let mut clean_season = inc_season.clone();
                    if let Some(eps) = clean_season["episodes"].as_array_mut() {
                        let mut allowed_eps = Vec::new();
                        for ep in eps.drain(..) {
                            if let Some(ep_id) = ep["id"].as_str()
                                && parental_allowed(store, user, ep_id).await?
                            {
                                allowed_eps.push(ep);
                            }
                        }
                        crate::product::decorate_many(store, &mut allowed_eps)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                        *eps = allowed_eps;
                    }
                    if let Some(existing_season) = target_seasons
                        .iter_mut()
                        .find(|s| s["number"].as_i64().unwrap_or(-1) == s_num)
                    {
                        if let (Some(target_eps), Some(inc_eps)) = (
                            existing_season["episodes"].as_array_mut(),
                            clean_season["episodes"].as_array(),
                        ) {
                            for inc_ep in inc_eps {
                                let ep_num = inc_ep["number"]
                                    .as_i64()
                                    .or_else(|| inc_ep["episodeNumber"].as_i64())
                                    .unwrap_or(0);
                                if let Some(existing_ep) = target_eps.iter_mut().find(|e| {
                                    e["number"]
                                        .as_i64()
                                        .or_else(|| e["episodeNumber"].as_i64())
                                        .unwrap_or(-1)
                                        == ep_num
                                }) {
                                    if let (Some(t_eds), Some(i_eds)) = (
                                        existing_ep["editions"].as_array_mut(),
                                        inc_ep["editions"].as_array(),
                                    ) {
                                        merge_edition_arrays(t_eds, i_eds);
                                    }
                                } else {
                                    target_eps.push(inc_ep.clone());
                                }
                            }
                            target_eps.sort_by_key(|e| {
                                e["number"]
                                    .as_i64()
                                    .or_else(|| e["episodeNumber"].as_i64())
                                    .unwrap_or(0)
                            });
                        }
                    } else {
                        target_seasons.push(clean_season);
                    }
                }
                target_seasons.sort_by_key(|s| s["number"].as_i64().unwrap_or(0));
                let season_count = target_seasons.len();
                let episode_count: usize = target_seasons
                    .iter()
                    .map(|s| s["episodes"].as_array().map_or(0, Vec::len))
                    .sum();
                value["seasonCount"] = json!(season_count);
                value["episodeCount"] = json!(episode_count);
            }
        }
    }
    crate::inventory::refresh_work_availability_from_editions(value);
    Ok(())
}

pub async fn canonical_work(store: &IdentityStore, work: &str) -> Result<String, StatusCode> {
    let group: Option<String> =
        query_scalar("SELECT group_id FROM media_group_members WHERE work_id=?")
            .bind(work)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(group.unwrap_or_else(|| work.to_owned()))
}
