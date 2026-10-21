//! Personal lists and explicit editorial controls. No changes to source media or sidecars.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
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
    let rows: Vec<(String, String)> =
        query_as("SELECT id,name FROM personal_lists WHERE user_id=? ORDER BY name,id")
            .bind(actor.user.id)
            .fetch_all(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"lists":rows.into_iter().map(|(id,name)|json!({"id":id,"name":name})).collect::<Vec<_>>()}),
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
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    owner(&auth, &headers, &id, false).await?;
    let works:Vec<String>=query_scalar("SELECT work_id FROM personal_list_items WHERE list_id=? ORDER BY added_at DESC,work_id LIMIT 1000").bind(id).fetch_all(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut items = Vec::new();
    for work in works {
        if crate::feedback::authorized(&auth, &headers, &work, false)
            .await
            .is_ok()
            && parental_allowed(
                &auth.store,
                &auth.authenticate(&headers).await?.user.id,
                &work,
            )
            .await?
            && let Some(detail) = crate::inventory::detail(&auth.store, &work)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            items.push(detail);
        }
    }
    Ok(Json(json!({"items":items})))
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
    query("INSERT INTO personal_list_items(list_id,work_id) VALUES(?,?) ON CONFLICT(list_id,work_id) DO NOTHING").bind(id).bind(work).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_item(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, work)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    owner(&auth, &headers, &id, true).await?;
    query("DELETE FROM personal_list_items WHERE list_id=? AND work_id=?")
        .bind(id)
        .bind(work)
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
    let result = query("UPDATE inventory_works SET manual_json=?,minimum_age=? WHERE id=?")
        .bind(value.to_string())
        .bind(input.minimum_age)
        .bind(work)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn reset_metadata(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("UPDATE inventory_works SET manual_json=NULL,minimum_age=NULL WHERE id=?")
        .bind(work)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    query("INSERT INTO work_posters(work_id,image_data) VALUES(?,?) ON CONFLICT(work_id) DO UPDATE SET image_data=excluded.image_data").bind(work).bind(image).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
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
    Ok(StatusCode::NO_CONTENT)
}
async fn poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<Response, Error> {
    crate::feedback::authorized(&auth, &headers, &work, false).await?;
    let image: Vec<u8> = query_scalar("SELECT image_data FROM work_posters WHERE work_id=?")
        .bind(work)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        image,
    )
        .into_response())
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
    match work {
        Some(work) => parental_allowed(store, user, &work).await,
        None => Ok(false),
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
    let id = canonical_work(&auth.store, &input.work_ids[0]).await?;
    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO media_groups(id,label) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET label=excluded.label")
        .bind(&id)
        .bind(input.label)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for work in input.work_ids {
        let found: i64 =
            query_scalar("SELECT COUNT(*) FROM inventory_works WHERE id=? AND kind='film'")
                .bind(&work)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if found != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
        query("INSERT INTO media_group_members(work_id,group_id) VALUES(?,?) ON CONFLICT(work_id) DO UPDATE SET group_id=excluded.group_id").bind(work).bind(&id).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"id":id})))
}
async fn ungroup(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM media_group_members WHERE work_id=?")
        .bind(work)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn decorate(store: &IdentityStore, value: &mut Value) -> Result<(), sqlx::Error> {
    let Some(id) = value["id"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    let has_poster: i64 = query_scalar("SELECT COUNT(*) FROM work_posters WHERE work_id=?")
        .bind(&id)
        .fetch_one(&store.pool)
        .await?;
    if has_poster == 1 {
        value["posterUrl"] = json!(format!("/api/v1/works/{id}/poster"));
    }
    let group: Option<String> =
        query_scalar("SELECT group_id FROM media_group_members WHERE work_id=?")
            .bind(id)
            .fetch_optional(&store.pool)
            .await?;
    value["globalMediaId"] = json!(group);
    Ok(())
}

pub async fn expand_detail(
    store: &IdentityStore,
    user: &str,
    visible: &[String],
    value: &mut Value,
) -> Result<(), StatusCode> {
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
                *episodes = filtered;
            }
        }
    }
    let Some(group) = value["globalMediaId"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    let original = value["id"].as_str().unwrap_or_default().to_owned();
    let siblings:Vec<(String,String)>=query_as("SELECT w.id,w.library_id FROM media_group_members member JOIN inventory_works w ON w.id=member.work_id WHERE member.group_id=? AND w.id!=? ORDER BY w.id").bind(group).bind(original).fetch_all(&store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    for (id, library) in siblings {
        if visible.contains(&library)
            && parental_allowed(store, user, &id).await?
            && let Some(detail) = crate::inventory::detail(store, &id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            && let Some(editions) = detail["editions"].as_array()
        {
            if detail["available"] == true {
                value["available"] = json!(true);
            }
            if let Some(target) = value["editions"].as_array_mut() {
                target.extend(editions.iter().cloned());
            }
        }
    }
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
