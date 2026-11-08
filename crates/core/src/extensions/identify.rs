//! Administrator-driven manual identification of media against external providers (Jellyfin-style Identify).
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IdentifySearchInput {
    pub(super) work_id: String,
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) year: Option<u16>,
    #[serde(default)]
    pub(super) provider: Option<String>,
    #[serde(default)]
    pub(super) external_ids: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct IdentifyApplyInput {
    pub(super) work_id: String,
    pub(super) provider: String,
    pub(super) candidate_id: String,
    #[serde(default = "default_true")]
    pub(super) replace_images: bool,
}
fn default_true() -> bool {
    true
}

pub(super) async fn search(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<IdentifySearchInput>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;

    let context: Option<adapters::WorkContext> =
        query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
            .bind(&input.work_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let context = context.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    let library_id = context.library_id.clone();
    let mut requested = json!({"title":input.title,"year":input.year});
    if let Some(ids) = &input.external_ids {
        requested["externalIds"] = json!(ids);
    }
    let search_input =
        adapters::metadata_input(&context, &input.work_id, "metadata.search", &requested)?;

    let mut rows: Vec<(String, String, String)> = query_as(
        "SELECT e.id, e.package_json, e.generation FROM extensions e \
         JOIN extension_publishers p ON p.id = e.publisher_id \
         WHERE e.enabled = 1 AND p.active = 1 ORDER BY e.id LIMIT 32",
    )
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;

    let is_series = context.kind == "series" || context.kind == "episode";
    rows.sort_by(|(a, _, _), (b, _, _)| {
        let rank = |id: &str| -> u8 {
            if is_series {
                match id {
                    "tvdb" => 0,
                    "tmdb" => 1,
                    "imdb" => 2,
                    "omdb" => 3,
                    _ => 4,
                }
            } else {
                match id {
                    "tmdb" => 0,
                    "imdb" => 1,
                    "omdb" => 2,
                    "tvdb" => 3,
                    _ => 4,
                }
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
    });

    let mut permitted_rows = Vec::new();
    let mut available_providers = Vec::new();
    for (id, stored, generation) in rows {
        if matches!(
            id.as_str(),
            "discogs" | "musicbrainz" | "lrclib" | "fanart-tv" | "poster-rotator"
        ) {
            continue;
        }
        let package: Package = serde_json::from_str(&stored)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;

        if !package
            .manifest
            .permissions
            .iter()
            .any(|p| p == "metadata.read")
        {
            continue;
        }
        if matches!(
            package.manifest.category.as_deref(),
            Some("Lyrics" | "Artwork" | "Subtitles" | "Playback" | "Applications")
        ) {
            continue;
        }

        if !permitted(&auth, &headers, &id, &library_id, &generation).await? {
            continue;
        }

        available_providers.push(json!({
            "id": id,
            "name": package.manifest.name,
        }));
        permitted_rows.push((id, package));
    }

    let explicit_provider = input.provider.as_deref().map(str::trim).unwrap_or_default();
    let effective_target: Option<String> = if explicit_provider == "all" {
        None
    } else if !explicit_provider.is_empty() {
        Some(explicit_provider.to_owned())
    } else {
        let preferred =
            crate::library_options::LibraryOptions::default_metadata_provider_for_kind(
                &context.kind,
            );
        preferred
            .filter(|pref| permitted_rows.iter().any(|(id, _)| id == pref))
            .map(str::to_owned)
            .or_else(|| permitted_rows.first().map(|(id, _)| id.clone()))
    };

    let mut results = Vec::new();
    let mut errors: Vec<Value> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);

    for (id, package) in permitted_rows {
        if let Some(target) = effective_target.as_deref()
            && id != target
        {
            continue;
        }

        let known_id = known_metadata_id(&search_input, &id).map(str::to_owned);
        let mut provider_input = search_input.clone();
        // The common run path applies the owning library's language and country.
        if let Some(candidate_id) = &known_id {
            provider_input["id"] = json!(candidate_id);
        }
        let search_request = Run {
            work_id: input.work_id.clone(),
            operation: if known_id.is_some() {
                "metadata.details"
            } else {
                "metadata.search"
            }
            .into(),
            input: provider_input,
        };

        let provider_deadline = deadline.min(tokio::time::Instant::now() + Duration::from_secs(12));
        let outcome = tokio::time::timeout_at(
            provider_deadline,
            super::run(
                State(auth.clone()),
                headers.clone(),
                Path(id.clone()),
                Json(search_request),
            ),
        )
        .await;

        match &outcome {
            Err(_) => errors.push(json!({
                "provider": id,
                "providerName": package.manifest.name,
                "error": "provider_timeout",
            })),
            Ok(Err((_, body))) => {
                let code = body
                    .0
                    .get("pluginError")
                    .or_else(|| body.0.get("error"))
                    .and_then(Value::as_str)
                    .unwrap_or("provider_error");
                errors.push(json!({
                    "provider": id,
                    "providerName": package.manifest.name,
                    "error": code,
                }));
            }
            Ok(Ok(_)) => {}
        }

        if let Ok(Ok(res)) = outcome {
            let mut proposal = res.0["proposal"].clone();
            if let Some(candidate_id) = &known_id {
                if proposal["providerId"].as_str() != Some(candidate_id.as_str()) {
                    errors.push(json!({"provider":id,"providerName":package.manifest.name,"error":"invalid_metadata_origin"}));
                    continue;
                }
                proposal = json!([{
                    "id":candidate_id,"title":proposal["title"],
                    "originalTitle":proposal["originalTitle"],"year":proposal["year"],
                    "overview":proposal["synopsis"],
                    "posterUrl":proposal["posters"][0]["url"],
                }]);
            }
            if let Some(items) = proposal.as_array() {
                for item in items.iter().take(30) {
                    if let (Some(item_id), Some(item_title)) = (
                        item.get("id").and_then(Value::as_str),
                        item.get("title").and_then(Value::as_str),
                    ) {
                        results.push(json!({
                            "provider": id,
                            "providerName": package.manifest.name,
                            "id": item_id,
                            "title": item_title,
                            "originalTitle": item.get("originalTitle").and_then(Value::as_str),
                            "year": item.get("year").and_then(Value::as_u64),
                            "overview": item.get("overview").and_then(Value::as_str),
                            "posterUrl": item.get("posterUrl").and_then(Value::as_str),
                        }));
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "results": results,
        "providers": available_providers,
        "errors": errors,
    })))
}

pub(super) async fn apply(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<IdentifyApplyInput>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;

    let context: Option<(String, String, Option<i64>, String)> =
        query_as("SELECT library_id, title, year, kind FROM inventory_works WHERE id=?")
            .bind(&input.work_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (library_id, _, _, kind) =
        context.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;

    let row: Option<(String, String)> =
        query_as("SELECT package_json, generation FROM extensions WHERE id=?")
            .bind(&input.provider)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (stored, generation) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;

    if !permitted(&auth, &headers, &input.provider, &library_id, &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }

    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;

    if !package
        .manifest
        .permissions
        .iter()
        .any(|p| p == "metadata.read")
    {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }

    let media_type = match kind.as_str() {
        "series" => "tv_show",
        _ => "movie",
    };

    let details_request = Run {
        work_id: input.work_id.clone(),
        operation: "metadata.details".into(),
        input: json!({
            "id": input.candidate_id,
            "mediaType": media_type,
        }),
    };

    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        super::run(
            State(auth.clone()),
            headers.clone(),
            Path(input.provider.clone()),
            Json(details_request),
        ),
    )
    .await
    .map_err(|_| err(StatusCode::GATEWAY_TIMEOUT, "provider_timeout"))?
    .map_err(|_| err(StatusCode::BAD_GATEWAY, "provider_error"))?;

    let proposal = outcome.0["proposal"].clone();
    if !proposal.is_object() {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_details"));
    }

    // Prepare editorial metadata map
    let mut metadata_map = serde_json::Map::new();

    if let Some(v) = proposal.get("title").and_then(Value::as_str) {
        metadata_map.insert("title".into(), json!(v));
    }
    if let Some(v) = proposal.get("originalTitle").and_then(Value::as_str) {
        metadata_map.insert("original_title".into(), json!(v));
    }
    if let Some(v) = proposal.get("originalLanguage").and_then(Value::as_str)
        && (2..=35).contains(&v.len())
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        metadata_map.insert("original_language".into(), json!(v));
    }
    if let Some(v) = proposal.get("tagline").and_then(Value::as_str) {
        metadata_map.insert("tagline".into(), json!(v));
        metadata_map.insert("outline".into(), json!(v));
    }
    if let Some(v) = proposal.get("synopsis").and_then(Value::as_str) {
        metadata_map.insert("plot".into(), json!(v));
    }
    if let Some(v) = proposal.get("year").and_then(Value::as_u64) {
        metadata_map.insert("year".into(), json!(v));
    }
    if let Some(v) = proposal.get("releaseDate").and_then(Value::as_str) {
        metadata_map.insert("premiered".into(), json!(v));
    }
    if let Some(v) = proposal.get("runtimeMinutes").and_then(Value::as_u64) {
        metadata_map.insert("runtime".into(), json!(v));
    }
    if let Some(v) = proposal.get("genres").and_then(Value::as_array) {
        metadata_map.insert("genres".into(), json!(v));
    }
    if let Some(v) = proposal.get("directors").and_then(Value::as_array) {
        metadata_map.insert("directors".into(), json!(v));
    }
    if let Some(v) = proposal.get("writers").and_then(Value::as_array) {
        metadata_map.insert("writers".into(), json!(v));
    }
    if let Some(v) = proposal.get("actors").and_then(Value::as_array) {
        metadata_map.insert("actors".into(), json!(v));
    }
    if let Some(v) = proposal.get("crewCredits").and_then(Value::as_array) {
        metadata_map.insert(
            "crewCredits".into(),
            json!(v.iter().take(100).collect::<Vec<_>>()),
        );
    }
    if let Some(v) = proposal.get("ratings").and_then(Value::as_array) {
        metadata_map.insert("ratings".into(), json!(v));
    }
    if let Some(v) = proposal.get("mpaa").and_then(Value::as_str) {
        metadata_map.insert("mpaa".into(), json!(v));
    }

    // Replace external IDs with the newly identified candidate's IDs so stale IDs from a
    // previous mistaken provider identification do not linger on the work.
    let mut external_ids = std::collections::BTreeMap::<String, String>::new();
    external_ids.insert(input.provider.clone(), input.candidate_id.clone());

    if let Some(ext) = proposal.get("externalIds").and_then(Value::as_object) {
        for (k, v) in ext {
            if let Some(s) = v.as_str().filter(|s| !s.is_empty()) {
                external_ids.insert(k.clone(), s.to_string());
            }
        }
    }

    metadata_map.insert("external_ids".into(), json!(external_ids));
    metadata_map.insert(
        "provenance".into(),
        json!(format!("identify:{}", input.provider)),
    );

    let mut tx = auth.store.pool().begin().await.map_err(db)?;

    query(
        "UPDATE inventory_works SET external_ids_json=?, editorial_json=?, manual_json=? WHERE id=?",
    )
    .bind(serde_json::to_string(&external_ids).unwrap_or_default())
    .bind(serde_json::to_string(&metadata_map).unwrap_or_default())
    .bind(serde_json::to_string(&metadata_map).unwrap_or_default())
    .bind(&input.work_id)
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    // Clear provider translations from other providers before storing the newly identified provider's translations.
    query("DELETE FROM work_translations WHERE work_id=? AND source LIKE 'provider:%'")
        .bind(&input.work_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    crate::translations::store_proposal_translations(
        &mut tx,
        &input.provider,
        &input.work_id,
        &proposal,
    )
    .await
    .map_err(db)?;

    // Enrich seasons and episodes if this is a series
    if kind == "series"
        && let Some(seasons) = proposal.get("seasons").and_then(Value::as_array)
    {
        for season in seasons {
            let s_num = season
                .get("seasonNumber")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if let Some(s_title) = season
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                let _ = query(
                    "UPDATE inventory_seasons SET title=? WHERE series_id=? AND season_number=?",
                )
                .bind(s_title)
                .bind(&input.work_id)
                .bind(s_num as i64)
                .execute(&mut *tx)
                .await;
            }
            if let Some(episodes) = season.get("episodes").and_then(Value::as_array) {
                for ep in episodes {
                    let ep_num = ep.get("episodeNumber").and_then(Value::as_u64).unwrap_or(0);
                    let ep_work: Option<String> = query_scalar(
                        "SELECT ep.work_id FROM inventory_episodes ep JOIN inventory_seasons s ON s.id=ep.season_id WHERE s.series_id=? AND s.season_number=? AND ep.episode_number=?",
                    )
                    .bind(&input.work_id)
                    .bind(s_num as i64)
                    .bind(ep_num as i64)
                    .fetch_optional(&mut *tx)
                    .await
                    .unwrap_or(None);

                    if let Some(ep_wid) = ep_work {
                        let mut ep_meta = serde_json::Map::new();
                        if let Some(t) = ep
                            .get("title")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                        {
                            ep_meta.insert("title".into(), json!(t));
                            let _ = query("UPDATE inventory_works SET title=? WHERE id=?")
                                .bind(t)
                                .bind(&ep_wid)
                                .execute(&mut *tx)
                                .await;
                        }
                        if let Some(p) = ep
                            .get("overview")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                        {
                            ep_meta.insert("plot".into(), json!(p));
                            ep_meta.insert("outline".into(), json!(p));
                        }
                        if let Some(d) = ep.get("airDate").and_then(Value::as_str) {
                            ep_meta.insert("premiered".into(), json!(d));
                        }
                        if let Some(r) = ep.get("runtimeMinutes").and_then(Value::as_u64) {
                            ep_meta.insert("runtime".into(), json!(r));
                        }
                        if let Some(st) = ep.get("stillUrl").and_then(Value::as_str) {
                            ep_meta.insert("still_url".into(), json!(st));
                            ep_meta.insert("thumb_url".into(), json!(st));
                        }
                        if let Some(rt) = ep.get("rating").and_then(Value::as_f64) {
                            ep_meta.insert("rating".into(), json!(rt));
                        }
                        ep_meta.insert(
                            "provenance".into(),
                            json!(format!("identify:{}", input.provider)),
                        );

                        let ep_json = serde_json::to_string(&ep_meta).unwrap_or_default();
                        let _ = query(
                            "UPDATE inventory_works SET editorial_json=?, manual_json=? WHERE id=?",
                        )
                        .bind(&ep_json)
                        .bind(&ep_json)
                        .bind(&ep_wid)
                        .execute(&mut *tx)
                        .await;
                    }
                }
            }
        }
    }

    audit(
        &mut tx,
        &actor.user.id,
        &input.provider,
        "metadata.identified",
    )
    .await?;
    tx.commit().await.map_err(db)?;

    // Optional image import (Poster and Backdrop)
    if input.replace_images {
        if let Some(posters) = proposal.get("posters").and_then(Value::as_array)
            && let Some(first_poster) = posters.first()
            && let Some(url) = first_poster.get("url").and_then(Value::as_str)
        {
            let lang = first_poster.get("language").and_then(Value::as_str);
            let _ = adapters::import_image(
                State(auth.clone()),
                headers.clone(),
                Path(input.provider.clone()),
                Json(adapters::ImportImage {
                    work_id: input.work_id.clone(),
                    url: url.to_string(),
                    kind: "poster".into(),
                    language: lang.map(str::to_owned),
                    append: false,
                    apply_collection_policy: false,
                }),
            )
            .await;
        }

        if let Some(backdrops) = proposal.get("backdrops").and_then(Value::as_array)
            && let Some(first_backdrop) = backdrops.first()
            && let Some(url) = first_backdrop.get("url").and_then(Value::as_str)
        {
            let lang = first_backdrop.get("language").and_then(Value::as_str);
            let _ = adapters::import_image(
                State(auth.clone()),
                headers.clone(),
                Path(input.provider.clone()),
                Json(adapters::ImportImage {
                    work_id: input.work_id.clone(),
                    url: url.to_string(),
                    kind: "backdrop".into(),
                    language: lang.map(str::to_owned),
                    append: false,
                    apply_collection_policy: false,
                }),
            )
            .await;
        }
    }

    Ok(Json(json!({
        "success": true,
        "workId": input.work_id,
        "externalIds": external_ids,
        "metadata": metadata_map,
    })))
}
