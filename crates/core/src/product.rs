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
            get(get_metadata).put(metadata).delete(reset_metadata),
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
/// Derived collections share the existing list UI but never grant access or mutate personal lists.
async fn automatic_collections(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let rows: Vec<(String, String, String, Option<String>)> = query_as("SELECT w.id,w.title,w.editorial_json,w.manual_json FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE w.kind='film' AND w.has_available_asset=1 AND l.enabled=1 AND n.status!='revoked' AND CAST(COALESCE(json_extract(l.options_json,'$.automaticCollections'),'0') AS TEXT) IN ('1','true') AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND (json_extract(w.editorial_json,'$.collection') IS NOT NULL OR json_extract(COALESCE(w.manual_json,'{}'),'$.collection') IS NOT NULL) ORDER BY w.effective_year,w.title_sort,w.id LIMIT 10000")
        .bind(actor.user.role.as_str()).bind(&actor.user.id)
        .fetch_all(auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut collections = std::collections::BTreeMap::<String, (String, Vec<Value>)>::new();
    for (work, title, editorial, manual) in rows {
        if !parental_allowed(&auth.store, &actor.user.id, &work).await? {
            continue;
        }
        let mut metadata: Value = serde_json::from_str(&editorial).unwrap_or(Value::Null);
        if let Some(manual) = manual.and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            && let Some(fields) = manual.as_object()
            && let Some(target) = metadata.as_object_mut()
        {
            target.extend(fields.clone());
        }
        let collection = &metadata["collection"];
        let (Some(identity), Some(name)) = (collection["id"].as_str(), collection["name"].as_str())
        else {
            continue;
        };
        if identity.is_empty() || name.is_empty() {
            continue;
        }
        let id = format!(
            "auto-collection-{}",
            crate::automation::hash(&identity).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        );
        collections
            .entry(id)
            .or_insert_with(|| (name.to_owned(), Vec::new()))
            .1
            .push(json!({"id":work,"title":metadata["title"].as_str().unwrap_or(&title),"kind":"film"}));
    }
    collections.retain(|_, (_, works)| works.len() >= 2);
    Ok(collections)
}

/// Whether the account wants playlists built from its favourites: on unless it
/// switched `autoPlaylists` off in its settings.
async fn auto_playlists_enabled(auth: &AuthState, user: &str) -> Result<bool, Error> {
    let raw: Option<String> =
        query_scalar("SELECT preferences_json FROM user_preferences WHERE user_id=?")
            .bind(user)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(raw
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .and_then(|preferences| preferences["autoPlaylists"].as_bool())
        .unwrap_or(true))
}

const FAVORITE_FILMS: &str = "auto-favorites-film";
const FAVORITE_SERIES: &str = "auto-favorites-series";

/// Extra fields of an automatic list: the favourites carry a key the interface
/// translates, collections carry nothing more.
fn automatic_extras(id: &str) -> Value {
    match id {
        FAVORITE_FILMS => json!({"favorites":true,"systemKey":"favorites-film"}),
        FAVORITE_SERIES => json!({"favorites":true,"systemKey":"favorites-series"}),
        _ => json!({}),
    }
}

/// The titles the account marked with a heart, one list per type. Derived on
/// every call, private to the account, never granting access: the library
/// permissions and the parental policy apply exactly as in the catalogue.
async fn automatic_favorites(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let mut lists = std::collections::BTreeMap::<String, (String, Vec<Value>)>::new();
    if !auto_playlists_enabled(auth, &actor.user.id).await? {
        return Ok(lists);
    }
    let rows: Vec<(String, String, String)> = query_as("SELECT w.id,w.title,w.kind FROM work_feedback f JOIN inventory_works w ON w.id=f.work_id JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE f.user_id=? AND f.favorite=1 AND w.kind IN ('film','series') AND w.has_available_asset=1 AND l.enabled=1 AND n.status!='revoked' AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) ORDER BY w.title_sort,w.id LIMIT 10000")
        .bind(&actor.user.id)
        .bind(actor.user.role.as_str())
        .bind(&actor.user.id)
        .fetch_all(auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for (work, title, kind) in rows {
        if !parental_allowed(&auth.store, &actor.user.id, &work).await? {
            continue;
        }
        let (id, name) = if kind == "series" {
            (FAVORITE_SERIES, "Séries favorites")
        } else {
            (FAVORITE_FILMS, "Films favoris")
        };
        lists
            .entry(id.to_owned())
            .or_insert_with(|| (name.to_owned(), Vec::new()))
            .1
            .push(json!({"id":work,"title":title,"kind":kind}));
    }
    Ok(lists)
}

/// Every automatic list of the account: derived collections, then favourites.
async fn automatic_lists(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let mut lists = automatic_collections(auth, actor).await?;
    lists.extend(automatic_favorites(auth, actor).await?);
    Ok(lists)
}

async fn lists(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let rows: Vec<(String, String,String,String)> =
        query_as("SELECT p.id,p.name,p.visibility,p.user_id FROM personal_lists p WHERE p.user_id=? OR (p.visibility='public' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_public=1)) OR (p.visibility='shared' AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_sharing=1) AND EXISTS(SELECT 1 FROM playlist_shares s WHERE s.list_id=p.id AND s.user_id=?)) ORDER BY p.name,p.id LIMIT 500")
            .bind(&actor.user.id).bind(&actor.user.id)
            .fetch_all(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut lists:Vec<Value> = rows.into_iter().map(|(id,name,visibility,user)|json!({"id":id,"name":name,"visibility":visibility,"owned":user==actor.user.id})).collect();
    for (id, (name, works)) in automatic_lists(&auth, &actor).await? {
        let mut list = json!({"id":id,"name":name,"visibility":"collection","owned":false,"automatic":true,"itemCount":works.len()});
        if let (Some(fields), Some(extra)) =
            (list.as_object_mut(), automatic_extras(&id).as_object())
        {
            fields.extend(extra.clone());
        }
        lists.push(list);
    }
    Ok(Json(json!({"lists":lists})))
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
    if id.starts_with("auto-") {
        let actor = auth.authenticate(&headers).await?;
        let Some((name, works)) = automatic_lists(&auth, &actor).await?.remove(&id) else {
            return Err(StatusCode::NOT_FOUND);
        };
        let mut items = Vec::new();
        for work in works {
            if view.compact {
                items.push(work);
                continue;
            }
            if let Some(media) = crate::inventory::detail(
                &auth.store,
                work["id"]
                    .as_str()
                    .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
            )
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            {
                items.push(media);
            }
        }
        let entries:Vec<Value>=items.iter().enumerate().map(|(position,media)|json!({"kind":"work","id":media["id"],"position":position,"media":media})).collect();
        let mut detail = json!({"id":id,"name":name,"visibility":"collection","owned":false,"automatic":true,"items":items,"entries":entries,"shares":[]});
        if let (Some(fields), Some(extra)) =
            (detail.as_object_mut(), automatic_extras(&id).as_object())
        {
            fields.extend(extra.clone());
        }
        return Ok(Json(detail));
    }
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
              AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=dl.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=dl.id AND p.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=i.work_id OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=i.work_id)) \
              AND gl.enabled=1 AND gn.status!='revoked' \
              AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=gl.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=gl.id AND p.user_id=?)) \
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

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionInput {
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalizedEditorialInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    sort_title: Option<String>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    plot: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Editorial {
    title: String,
    year: Option<i64>,
    #[serde(default)]
    sort_title: Option<String>,
    #[serde(default)]
    original_title: Option<String>,
    #[serde(default)]
    original_language: Option<String>,
    #[serde(default)]
    tagline: Option<String>,
    #[serde(default)]
    plot: String,
    #[serde(default)]
    premiered: Option<String>,
    #[serde(default)]
    added_at: Option<i64>,
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    critic_rating: Option<f64>,
    #[serde(default)]
    mpaa: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    studios: Vec<String>,
    #[serde(default)]
    countries: Vec<String>,
    #[serde(default)]
    directors: Vec<String>,
    #[serde(default)]
    writers: Vec<String>,
    #[serde(default)]
    actors: Vec<Value>,
    #[serde(default)]
    crew_credits: Vec<Value>,
    #[serde(default)]
    ratings: Vec<Value>,
    #[serde(default)]
    external_ids: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    collection: Option<CollectionInput>,
    #[serde(default)]
    poster_url: Option<String>,
    #[serde(default)]
    backdrop_url: Option<String>,
    #[serde(default)]
    minimum_age: Option<i64>,
    #[serde(default)]
    locked: Option<bool>,
    #[serde(default)]
    locked_fields: Vec<String>,
    /// Language of `title`, `tagline` and `plot`. Omitted: the correction is
    /// language-neutral and shown to everyone, as before translations existed.
    #[serde(default)]
    locale: Option<String>,
    /// Optional map of per-locale translations (`{ "fr": { title, sortTitle, tagline, plot }, ... }`).
    #[serde(default)]
    translations: std::collections::BTreeMap<String, LocalizedEditorialInput>,
}

async fn get_metadata(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    type WorkMetaRow = (
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
        String,
        String,
        Option<String>,
        String,
    );
    let row: Option<WorkMetaRow> = query_as(
        "SELECT w.library_id, w.kind, w.title, w.year, w.minimum_age, w.added_at, \
                w.display_title_sort, w.external_ids_json, w.editorial_json, w.manual_json, l.options_json \
         FROM inventory_works w \
         JOIN libraries l ON l.id = w.library_id \
         WHERE w.id = ?",
    )
    .bind(&work)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some((
        library_id,
        kind,
        base_title,
        base_year,
        minimum_age,
        base_added_at,
        display_title_sort,
        ext_ids_raw,
        editorial_raw,
        manual_raw,
        lib_options_raw,
    )) = row
    else {
        return Err(StatusCode::NOT_FOUND);
    };

    let lib_options = crate::library_options::LibraryOptions::from_json(&lib_options_raw);
    let mut merged: Value = serde_json::from_str(&editorial_raw).unwrap_or_else(|_| json!({}));
    if let Some(manual_str) = manual_raw
        && let Ok(Value::Object(manual_map)) = serde_json::from_str::<Value>(&manual_str)
        && let Some(obj) = merged.as_object_mut()
    {
        for (k, v) in manual_map {
            if !v.is_null() {
                obj.insert(k, v);
            }
        }
    }

    let mut ext_ids: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&ext_ids_raw).unwrap_or_default();
    if let Some(obj) = merged.get("external_ids").and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
                ext_ids.insert(k.clone(), s.to_owned());
            }
        }
    }

    /// locale, language, source, title, sort title, overview, tagline.
    type TranslationRow = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let tr_rows: Vec<TranslationRow> = query_as(
        "SELECT locale, language, source, title, sort_title, overview, tagline \
         FROM work_translations WHERE work_id = ? \
         ORDER BY priority DESC, CASE WHEN locale = language THEN 0 ELSE 1 END, locale ASC",
    )
    .bind(&work)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut translations_map = serde_json::Map::new();
    for (locale, language, source, t_title, t_sort, t_overview, t_tagline) in tr_rows {
        let loc_key = locale.to_ascii_lowercase();
        let keys = if loc_key != language && loc_key != crate::metadata_locale::NEUTRAL {
            vec![loc_key, language]
        } else {
            vec![language]
        };
        for key in keys {
            let entry = translations_map.entry(key).or_insert_with(|| {
                json!({
                    "locale": locale,
                    "source": source,
                    "title": Value::Null,
                    "sortTitle": Value::Null,
                    "plot": Value::Null,
                    "tagline": Value::Null,
                })
            });
            if let Some(obj) = entry.as_object_mut() {
                if obj.get("title").is_none_or(Value::is_null)
                    && let Some(v) = t_title.as_ref().filter(|s| !s.is_empty())
                {
                    obj.insert("title".into(), json!(v));
                }
                if obj.get("sortTitle").is_none_or(Value::is_null)
                    && let Some(v) = t_sort.as_ref().filter(|s| !s.is_empty())
                {
                    obj.insert("sortTitle".into(), json!(v));
                }
                if obj.get("plot").is_none_or(Value::is_null)
                    && let Some(v) = t_overview.as_ref().filter(|s| !s.is_empty())
                {
                    obj.insert("plot".into(), json!(v));
                }
                if obj.get("tagline").is_none_or(Value::is_null)
                    && let Some(v) = t_tagline.as_ref().filter(|s| !s.is_empty())
                {
                    obj.insert("tagline".into(), json!(v));
                }
            }
        }
    }

    Ok(Json(json!({
        "workId": work,
        "libraryId": library_id,
        "kind": kind,
        "preferredMetadataLanguage": lib_options.preferred_metadata_language,
        "metadataCountryCode": lib_options.metadata_country_code,
        "title": merged["title"].as_str().unwrap_or(&base_title),
        "sortTitle": merged["sort_title"].as_str().unwrap_or(&display_title_sort),
        "originalTitle": merged["original_title"],
        "originalLanguage": merged["original_language"],
        "tagline": merged["tagline"].as_str().or_else(|| merged["outline"].as_str()),
        "plot": merged["plot"].as_str().unwrap_or(""),
        "year": merged["year"].as_i64().or(base_year),
        "premiered": merged["premiered"],
        "addedAt": merged["added_at"].as_i64().or(base_added_at),
        "rating": merged["rating"],
        "criticRating": merged["critic_rating"],
        "mpaa": merged["mpaa"],
        "minimumAge": minimum_age,
        "genres": merged["genres"].as_array().cloned().unwrap_or_default(),
        "tags": merged["tags"].as_array().cloned().unwrap_or_default(),
        "studios": merged["studios"].as_array().cloned().unwrap_or_default(),
        "countries": merged["countries"].as_array().cloned().unwrap_or_default(),
        "directors": merged["directors"].as_array().cloned().unwrap_or_default(),
        "writers": merged["writers"].as_array().cloned().unwrap_or_default(),
        "actors": merged["actors"].as_array().cloned().unwrap_or_default(),
        "crewCredits": merged["crewCredits"].as_array().cloned().unwrap_or_default(),
        "ratings": merged["ratings"].as_array().cloned().unwrap_or_default(),
        "externalIds": ext_ids,
        "collection": merged["collection"],
        "locked": merged["locked"].as_bool().unwrap_or(false),
        "lockedFields": merged["locked_fields"].as_array().cloned().unwrap_or_default(),
        "translations": translations_map,
    })))
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
        || input.sort_title.as_ref().is_some_and(|v| v.len() > 512)
        || input.original_title.as_ref().is_some_and(|v| v.len() > 512)
        || input.original_language.as_ref().is_some_and(|v| {
            v.len() > 35 || !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        || input.premiered.as_ref().is_some_and(|v| v.len() > 32)
        || input.mpaa.as_ref().is_some_and(|v| v.len() > 32)
        || input
            .rating
            .is_some_and(|v| !v.is_finite() || !(0.0..=10.0).contains(&v))
        || input
            .critic_rating
            .is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v))
        || input.plot.len() > 20000
        || input.genres.len() > 32
        || input.genres.iter().any(|v| v.len() > 100)
        || input.studios.len() > 32
        || input.studios.iter().any(|v| v.len() > 100)
        || input.countries.len() > 32
        || input.countries.iter().any(|v| v.len() > 100)
        || input.locked_fields.len() > 32
        || input.locked_fields.iter().any(|v| v.len() > 64)
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
        || input.translations.len() > 32
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let scoped = match input.locale.as_deref() {
        Some(locale) => {
            Some(crate::metadata_locale::normalize(locale).ok_or(StatusCode::BAD_REQUEST)?)
                .filter(|locale| locale != crate::metadata_locale::NEUTRAL)
        }
        None => None,
    };
    let mut normalized_translations = Vec::new();
    for (loc_key, tr_val) in &input.translations {
        let norm = crate::metadata_locale::normalize(loc_key).ok_or(StatusCode::BAD_REQUEST)?;
        if tr_val.title.as_ref().is_some_and(|s| s.len() > 512)
            || tr_val.sort_title.as_ref().is_some_and(|s| s.len() > 512)
            || tr_val.tagline.as_ref().is_some_and(|s| s.len() > 1024)
            || tr_val.plot.as_ref().is_some_and(|s| s.len() > 20000)
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        normalized_translations.push((norm, tr_val.clone()));
    }
    // A correction scoped to one language must not turn the neutral texts into that language.
    let previous: Option<Value> =
        query_scalar::<_, Option<String>>("SELECT manual_json FROM inventory_works WHERE id=?")
            .bind(&work)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok());
    let kept = |key: &str| {
        previous
            .as_ref()
            .and_then(|p| p.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let (title, tagline, plot) = if scoped.is_some() {
        (kept("title"), kept("tagline"), kept("plot"))
    } else {
        (json!(input.title), json!(input.tagline), json!(input.plot))
    };
    let sort_title = input
        .sort_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| json!(s))
        .unwrap_or_else(|| kept("sort_title"));
    let collection_val = match &input.collection {
        Some(col) if !col.id.trim().is_empty() => {
            let name = if col.name.trim().is_empty() {
                format!("Collection {}", col.id.trim())
            } else {
                col.name.trim().to_owned()
            };
            json!({ "id": col.id.trim(), "name": name })
        }
        _ => Value::Null,
    };
    let value = json!({
        "title": title,
        "sort_title": sort_title,
        "original_title": input.original_title,
        "original_language": input.original_language.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        "tagline": tagline,
        "outline": tagline,
        "year": input.year,
        "premiered": input.premiered.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        "added_at": input.added_at,
        "rating": input.rating,
        "critic_rating": input.critic_rating,
        "mpaa": input.mpaa.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        "plot": plot,
        "genres": input.genres.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "tags": input.tags.iter().map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "studios": input.studios.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "countries": input.countries.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "directors": input.directors.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "writers": input.writers.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).collect::<Vec<_>>(),
        "actors": input.actors,
        "crewCredits": input.crew_credits,
        "ratings": input.ratings,
        "external_ids": input.external_ids,
        "collection": collection_val,
        "locked": input.locked.unwrap_or(false),
        "locked_fields": input.locked_fields,
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
    let result = query(
        "UPDATE inventory_works SET manual_json=?, minimum_age=?, added_at=COALESCE(?, added_at) WHERE id=?",
    )
    .bind(value.to_string())
    .bind(input.minimum_age)
    .bind(input.added_at)
    .bind(&work)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    if let Some(locale) = scoped {
        crate::translations::upsert_work(
            &mut tx,
            &work,
            &crate::translations::Translation {
                locale,
                source: crate::metadata_locale::SOURCE_MANUAL.into(),
                title: Some(input.title.clone()),
                sort_title: input.sort_title.clone(),
                overview: Some(input.plot.clone()),
                tagline: input.tagline.clone(),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    for (norm_locale, tr_val) in normalized_translations {
        crate::translations::upsert_work(
            &mut tx,
            &work,
            &crate::translations::Translation {
                locale: norm_locale,
                source: crate::metadata_locale::SOURCE_MANUAL.into(),
                title: tr_val.title.filter(|s| !s.trim().is_empty()),
                sort_title: tr_val.sort_title.filter(|s| !s.trim().is_empty()),
                overview: tr_val.plot.filter(|s| !s.trim().is_empty()),
                tagline: tr_val.tagline.filter(|s| !s.trim().is_empty()),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    query("DELETE FROM work_translations WHERE work_id=? AND source='manual'")
        .bind(&work)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::translations::bump_generation(&mut tx)
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
    let encoded = crate::artwork::prepare(bytes, content, "poster".into()).await?;
    let mut stored = Vec::new();
    let mut leases = Vec::new();
    for (width, mime, data) in encoded.variants {
        let meta = auth
            .store
            .assets
            .put_bytes(&data, mime)
            .await
            .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        leases.push(
            auth.store
                .assets
                .lease(&meta.sha256)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        stored.push((width, meta.sha256));
    }
    let revision = uuid::Uuid::new_v4().to_string();
    let mut tx = auth
        .store
        .pool
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO work_posters(work_id,sha256,revision) VALUES(?,?,?) ON CONFLICT(work_id) DO UPDATE SET sha256=excluded.sha256,revision=excluded.revision")
        .bind(&work).bind(&stored[0].1).bind(&revision).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    for (width, sha) in stored.iter().filter(|v| v.0 != 0) {
        query("INSERT INTO poster_previews(work_id,width,revision,mime,sha256) VALUES(?,?,?,'image/webp',?) ON CONFLICT(work_id,width) DO UPDATE SET revision=excluded.revision,mime=excluded.mime,sha256=excluded.sha256")
            .bind(&work).bind(width).bind(&revision).bind(sha).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    query("INSERT INTO image_migration_attempts(sha256,outcome) VALUES(?,'completed') ON CONFLICT(sha256) DO UPDATE SET outcome='completed'")
        .bind(&stored[0].1).execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

/// Normalizes a provider/NFO classification (TMDb, TVDB, IMDb, OMDb, Kodi NFO)
/// into a comparable minimum age: `FR:-16`, `-16 ans`, `FSK 16`, `GB:15`,
/// `US:PG-13`, `Rated R`, `TV-MA`, `Tous publics`, `U`... Unknown labels stay NULL
/// and follow the policy's `allow_unrated` choice.
macro_rules! rating_age_sql {
    ($expr:literal) => {
        concat!(
            "CASE replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(replace(upper(trim(",
            $expr,
            ")),'FR:',''),'FR-',''),'US:',''),'GB:',''),'UK:',''),'DE:',''),'CA:',''),'BE:',''),'CH:',''),'FSK ',''),'FSK',''),'BBFC ',''),'RATED ',''),' ANS',''),'ANS',''),' ','') ",
            "WHEN 'G' THEN 0 WHEN 'TV-G' THEN 0 WHEN 'TV-Y' THEN 0 WHEN 'U' THEN 0 WHEN 'TOUSPUBLICS' THEN 0 WHEN 'TP' THEN 0 WHEN 'ALL' THEN 0 WHEN 'AL' THEN 0 WHEN '0' THEN 0 ",
            "WHEN '6' THEN 6 WHEN 'TV-Y7' THEN 7 WHEN '7' THEN 7 ",
            "WHEN 'PG' THEN 10 WHEN 'TV-PG' THEN 10 WHEN '10' THEN 10 WHEN '-10' THEN 10 ",
            "WHEN '12' THEN 12 WHEN '-12' THEN 12 WHEN '12A' THEN 12 WHEN 'PG-12' THEN 12 ",
            "WHEN 'PG-13' THEN 13 WHEN 'TV-14' THEN 14 WHEN '14' THEN 14 WHEN '15' THEN 15 WHEN 'MA15+' THEN 15 ",
            "WHEN '16' THEN 16 WHEN '-16' THEN 16 WHEN 'R' THEN 17 ",
            "WHEN '18' THEN 18 WHEN '-18' THEN 18 WHEN 'NC-17' THEN 18 WHEN 'TV-MA' THEN 18 WHEN 'R18' THEN 18 WHEN 'X' THEN 18 ELSE NULL END"
        )
    };
}

// The same expression is used before catalogue pagination and on every derived resource.
// Episodes inherit their series' manual age and provider classification, since
// providers and NFO files classify the series rather than each episode.
pub const AGE_SQL: &str = concat!(
    "COALESCE(w.minimum_age,(SELECT parent.minimum_age FROM inventory_episodes ep JOIN inventory_seasons season ON season.id=ep.season_id JOIN inventory_works parent ON parent.id=season.series_id WHERE ep.work_id=w.id),",
    rating_age_sql!(
        "COALESCE(json_extract(w.manual_json,'$.mpaa'),json_extract(w.editorial_json,'$.mpaa'))"
    ),
    ",(SELECT ",
    rating_age_sql!(
        "COALESCE(json_extract(parent.manual_json,'$.mpaa'),json_extract(parent.editorial_json,'$.mpaa'))"
    ),
    " FROM inventory_episodes ep JOIN inventory_seasons season ON season.id=ep.season_id JOIN inventory_works parent ON parent.id=season.series_id WHERE ep.work_id=w.id))"
);
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
               AND ((? = 'administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.library_id=l.id AND lp.user_id = ?))",
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
        value["imageVariants"] = json!(
            images
                .iter()
                .filter(|e| e.work_id == id)
                .collect::<Vec<_>>()
        );
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

#[cfg(test)]
mod automatic_collection_tests {
    use super::*;
    #[tokio::test]
    async fn collections_follow_opt_in_membership_and_live_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("collections.sqlite"))
            .await
            .unwrap();
        query("INSERT INTO users(id,role) VALUES('reader','user')")
            .execute(store.pool())
            .await
            .unwrap();
        let token = "c".repeat(64);
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).execute(store.pool()).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('films','Films','films','local','/synthetic-read-only',1,'{\"automaticCollections\":true}')").execute(store.pool()).await.unwrap();
        query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
            .execute(store.pool())
            .await
            .unwrap();
        for id in ["one", "two"] {
            query("INSERT INTO inventory_works(id,library_id,identity_key,title,kind,external_ids_json,editorial_json,has_available_asset) VALUES(?,'films',?,?,'film','{}',?,1)")
                .bind(id).bind(id).bind(id).bind(json!({"collection":{"id":"tmdb:42","name":"Legal synthetic saga"}}).to_string()).execute(store.pool()).await.unwrap();
        }
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("aether_session={token}").parse().unwrap(),
        );
        let actor = auth.authenticate(&headers).await.unwrap();
        let collections = automatic_collections(&auth, &actor).await.unwrap();
        assert_eq!(collections.len(), 1);
        let collection_id = collections.keys().next().unwrap().clone();
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let request = |path: &str| {
            axum::http::Request::builder()
                .uri(path)
                .header(header::COOKIE, format!("aether_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let response = router(auth.clone())
            .oneshot(request("/api/v1/lists"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(payload["lists"][0]["automatic"], true);
        assert_eq!(payload["lists"][0]["owned"], false);
        let response = router(auth.clone())
            .oneshot(request(&format!(
                "/api/v1/lists/{collection_id}?compact=true"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let detail: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(detail["entries"].as_array().unwrap().len(), 2);

        assert_eq!(collections.values().next().unwrap().1.len(), 2);
        query("UPDATE inventory_works SET minimum_age=18 WHERE id='two'")
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('reader',12,1)").execute(store.pool()).await.unwrap();
        assert!(
            automatic_collections(&auth, &actor)
                .await
                .unwrap()
                .is_empty(),
            "a hidden film cannot expose a saga"
        );
        query("DELETE FROM parental_policies")
            .execute(store.pool())
            .await
            .unwrap();
        query("DELETE FROM library_permissions")
            .execute(store.pool())
            .await
            .unwrap();
        let response = router(auth.clone())
            .oneshot(request(&format!("/api/v1/lists/{collection_id}")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        assert!(
            automatic_collections(&auth, &actor)
                .await
                .unwrap()
                .is_empty()
        );
        query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
            .execute(store.pool())
            .await
            .unwrap();
        query("UPDATE libraries SET options_json='{}'")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(
            automatic_collections(&auth, &actor)
                .await
                .unwrap()
                .is_empty()
        );
    }
    #[tokio::test]
    async fn favourites_become_one_list_per_type_and_can_be_switched_off() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("favourites.sqlite"))
            .await
            .unwrap();
        query("INSERT INTO users(id,role) VALUES('reader','user')")
            .execute(store.pool())
            .await
            .unwrap();
        let token = "d".repeat(64);
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(crate::auth::hash(&token)).execute(store.pool()).await.unwrap();
        for (library, kind) in [("films", "films"), ("shows", "series")] {
            query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES(?,?,?,'local',?,1,'{}')")
                .bind(library).bind(library).bind(kind).bind(format!("/synthetic-{library}")).execute(store.pool()).await.unwrap();
            query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,'reader')")
                .bind(library)
                .execute(store.pool())
                .await
                .unwrap();
        }
        for (id, library, kind, favorite) in [
            ("loved-film", "films", "film", 1),
            ("plain-film", "films", "film", 0),
            ("loved-show", "shows", "series", 1),
        ] {
            query("INSERT INTO inventory_works(id,library_id,identity_key,title,kind,external_ids_json,editorial_json,has_available_asset) VALUES(?,?,?,?,?,'{}','{}',1)")
                .bind(id).bind(library).bind(id).bind(id).bind(kind).execute(store.pool()).await.unwrap();
            query("INSERT INTO work_feedback(user_id,work_id,favorite) VALUES('reader',?,?)")
                .bind(id)
                .bind(favorite)
                .execute(store.pool())
                .await
                .unwrap();
        }
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("aether_session={token}").parse().unwrap(),
        );
        let actor = auth.authenticate(&headers).await.unwrap();

        // On by default: one list per type, hearts only.
        let lists = automatic_favorites(&auth, &actor).await.unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[FAVORITE_FILMS].1.len(), 1);
        assert_eq!(lists[FAVORITE_FILMS].1[0]["id"], "loved-film");
        assert_eq!(lists[FAVORITE_SERIES].1[0]["id"], "loved-show");

        // Served through the same routes as the other lists, with a translatable key.
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let request = |path: &str| {
            axum::http::Request::builder()
                .uri(path)
                .header(header::COOKIE, format!("aether_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let response = router(auth.clone())
            .oneshot(request("/api/v1/lists"))
            .await
            .unwrap();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let favourite = payload["lists"]
            .as_array()
            .unwrap()
            .iter()
            .find(|list| list["id"] == FAVORITE_FILMS)
            .unwrap();
        assert_eq!(favourite["systemKey"], "favorites-film");
        assert_eq!(favourite["owned"], false);
        let response = router(auth.clone())
            .oneshot(request(&format!(
                "/api/v1/lists/{FAVORITE_FILMS}?compact=true"
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // A library the account lost never leaks through its hearts.
        query("DELETE FROM library_permissions WHERE library_id='shows'")
            .execute(store.pool())
            .await
            .unwrap();
        let lists = automatic_favorites(&auth, &actor).await.unwrap();
        assert!(lists.contains_key(FAVORITE_FILMS) && !lists.contains_key(FAVORITE_SERIES));

        // Switched off in the settings: nothing is derived.
        query("INSERT INTO user_preferences(user_id,preferences_json) VALUES('reader','{\"autoPlaylists\":false}')")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(automatic_favorites(&auth, &actor).await.unwrap().is_empty());
    }
}

#[cfg(test)]
mod tests;
