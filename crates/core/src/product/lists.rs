use super::*;

/// Derived collections share the existing list UI but never grant access or mutate personal lists.
pub(crate) async fn automatic_collections(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let rows: Vec<(String, String, String, Option<String>)> = query_as("SELECT w.id,w.title,w.editorial_json,w.manual_json \
            FROM inventory_works w \
            JOIN libraries l ON l.id=w.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE w.kind='film' AND w.has_available_asset=1 AND l.enabled=1 AND n.status!='revoked' \
            AND CAST(COALESCE(json_extract(l.options_json,'$.automaticCollections'),'0') AS TEXT) \
            IN ('1','true') AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
            JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
            JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
            AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
            OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
            AND p.user_id=?)) AND (json_extract(w.editorial_json,'$.collection') IS NOT NULL \
            OR json_extract(COALESCE(w.manual_json,'{}'),'$.collection') IS NOT NULL) \
            ORDER BY w.effective_year,w.title_sort,w.id \
            LIMIT 10000")
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
pub(crate) async fn auto_playlists_enabled(auth: &AuthState, user: &str) -> Result<bool, Error> {
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

pub(crate) const FAVORITE_FILMS: &str = "auto-favorites-film";

pub(crate) const FAVORITE_SERIES: &str = "auto-favorites-series";

/// Extra fields of an automatic list: the favourites carry a key the interface
/// translates, collections carry nothing more.
pub(crate) fn automatic_extras(id: &str) -> Value {
    match id {
        FAVORITE_FILMS => json!({"favorites":true,"systemKey":"favorites-film"}),
        FAVORITE_SERIES => json!({"favorites":true,"systemKey":"favorites-series"}),
        _ => json!({}),
    }
}

/// The titles the account marked with a heart, one list per type. Derived on
/// every call, private to the account, never granting access: the library
/// permissions and the parental policy apply exactly as in the catalogue.
pub(crate) async fn automatic_favorites(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let mut lists = std::collections::BTreeMap::<String, (String, Vec<Value>)>::new();
    if !auto_playlists_enabled(auth, &actor.user.id).await? {
        return Ok(lists);
    }
    let rows: Vec<(String, String, String)> = query_as(
        "SELECT w.id,w.title,w.kind \
            FROM work_feedback f \
            JOIN inventory_works w ON w.id=f.work_id \
            JOIN libraries l ON l.id=w.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE f.user_id=? AND f.favorite=1 AND w.kind IN ('film','series') \
            AND w.has_available_asset=1 AND l.enabled=1 AND n.status!='revoked' \
            AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
            JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
            JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
            AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
            OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
            AND p.user_id=?)) \
            ORDER BY w.title_sort,w.id \
            LIMIT 10000",
    )
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
pub(crate) async fn automatic_lists(
    auth: &AuthState,
    actor: &Authenticated,
) -> Result<std::collections::BTreeMap<String, (String, Vec<Value>)>, Error> {
    let mut lists = automatic_collections(auth, actor).await?;
    lists.extend(automatic_favorites(auth, actor).await?);
    Ok(lists)
}

pub(crate) async fn lists(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let rows: Vec<(String, String,String,String)> =
        query_as("SELECT p.id,p.name,p.visibility,p.user_id \
                FROM personal_lists p \
                WHERE p.user_id=? OR (p.visibility='public' AND EXISTS(SELECT 1 \
                FROM playlist_policy WHERE allow_public=1)) OR (p.visibility='shared' \
                AND EXISTS(SELECT 1 FROM playlist_policy WHERE allow_sharing=1) AND EXISTS(SELECT 1 \
                FROM playlist_shares s WHERE s.list_id=p.id AND s.user_id=?)) \
                ORDER BY p.name,p.id \
                LIMIT 500")
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
pub(crate) struct ListName {
    pub(crate) name: String,
}

pub(crate) async fn create_list(
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
pub(crate) struct ListView {
    #[serde(default)]
    pub(crate) compact: bool,
}

pub(crate) async fn list(
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
              AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
              JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
              JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=dl.id \
              AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
              OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=dl.id \
              AND p.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=i.work_id OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=i.work_id)) \
              AND gl.enabled=1 AND gn.status!='revoked' \
              AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
              JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
              JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=gl.id \
              AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
              OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=gl.id \
              AND p.user_id=?)) \
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
        query_as(
            "SELECT s.user_id,COALESCE(u.display_name,c.login,u.id) \
                FROM playlist_shares s \
                JOIN users u ON u.id=s.user_id \
                LEFT \
                JOIN local_credentials c ON c.user_id=u.id \
                WHERE s.list_id=? \
                ORDER BY s.user_id",
        )
        .bind(&id)
        .fetch_all(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        vec![]
    };
    Ok(Json(
        json!({"id":id,"name":name,"visibility":visibility,"owned":user==actor.user.id,"items":items,"entries":entries,"shares":shares.into_iter().map(|(id,name)|json!({"id":id,"name":name})).collect::<Vec<_>>()}),
    ))
}

pub(crate) async fn delete_list(
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

pub(crate) async fn add_item(
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

pub(crate) async fn remove_item(
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
pub(crate) struct CollectionInput {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
}
