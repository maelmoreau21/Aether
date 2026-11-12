use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Group {
    pub(crate) work_ids: Vec<String>,
    pub(crate) label: String,
}

pub(crate) async fn group(
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

pub(crate) async fn ungroup(
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
pub(crate) struct CandidateQuery {
    pub(crate) status: Option<String>,
}

pub(crate) async fn list_candidates(
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

pub(crate) async fn confirm_candidate(
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

pub(crate) async fn reject_candidate(
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

pub(crate) async fn undo_merge(
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

pub(crate) fn merge_edition_arrays(target: &mut Vec<Value>, incoming: &[Value]) {
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
