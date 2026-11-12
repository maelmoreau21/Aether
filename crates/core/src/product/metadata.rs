use super::*;

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalizedEditorialInput {
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) sort_title: Option<String>,
    #[serde(default)]
    pub(crate) tagline: Option<String>,
    #[serde(default)]
    pub(crate) plot: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Editorial {
    pub(crate) title: String,
    pub(crate) year: Option<i64>,
    #[serde(default)]
    pub(crate) sort_title: Option<String>,
    #[serde(default)]
    pub(crate) original_title: Option<String>,
    #[serde(default)]
    pub(crate) original_language: Option<String>,
    #[serde(default)]
    pub(crate) tagline: Option<String>,
    #[serde(default)]
    pub(crate) plot: String,
    #[serde(default)]
    pub(crate) premiered: Option<String>,
    #[serde(default)]
    pub(crate) added_at: Option<i64>,
    #[serde(default)]
    pub(crate) rating: Option<f64>,
    #[serde(default)]
    pub(crate) critic_rating: Option<f64>,
    #[serde(default)]
    pub(crate) mpaa: Option<String>,
    #[serde(default)]
    pub(crate) genres: Vec<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) studios: Vec<String>,
    #[serde(default)]
    pub(crate) countries: Vec<String>,
    #[serde(default)]
    pub(crate) directors: Vec<String>,
    #[serde(default)]
    pub(crate) writers: Vec<String>,
    #[serde(default)]
    pub(crate) actors: Vec<Value>,
    #[serde(default)]
    pub(crate) crew_credits: Vec<Value>,
    #[serde(default)]
    pub(crate) ratings: Vec<Value>,
    #[serde(default)]
    pub(crate) external_ids: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) collection: Option<CollectionInput>,
    #[serde(default)]
    pub(crate) poster_url: Option<String>,
    #[serde(default)]
    pub(crate) backdrop_url: Option<String>,
    #[serde(default)]
    pub(crate) minimum_age: Option<i64>,
    #[serde(default)]
    pub(crate) locked: Option<bool>,
    #[serde(default)]
    pub(crate) locked_fields: Vec<String>,
    /// Language of `title`, `tagline` and `plot`. Omitted: the correction is
    /// language-neutral and shown to everyone, as before translations existed.
    #[serde(default)]
    pub(crate) locale: Option<String>,
    /// Optional map of per-locale translations (`{ "fr": { title, sortTitle, tagline, plot }, ... }`).
    #[serde(default)]
    pub(crate) translations: std::collections::BTreeMap<String, LocalizedEditorialInput>,
}

pub(crate) async fn get_metadata(
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

pub(crate) async fn metadata(
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

pub(crate) async fn reset_metadata(
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

pub(crate) async fn upload_poster(
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

pub(crate) async fn remove_poster(
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

pub(crate) async fn poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(work): Path<String>,
    axum::extract::Query(options): axum::extract::Query<crate::artwork::PreviewOptions>,
) -> Result<Response, Error> {
    crate::feedback::authorized(&auth, &headers, &work, false).await?;
    crate::artwork::legacy(&auth.store, &work, options.width.unwrap_or(0), &headers).await
}
