//! Bounded explicit harvesting; providers remain optional sandboxed extensions.
use super::*;
use media_extension_sdk::{ImageType, MediaImage};
use std::collections::HashSet;

pub(super) fn valid_language(value: &str) -> bool {
    let mut parts = value.split('-');
    value.len() <= 35
        && parts.next().is_some_and(|p| {
            (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic())
        })
        && parts
            .all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn languages(input: &Value) -> Result<Vec<String>, Error> {
    let Some(raw) = input.get("languages") else {
        return Ok(vec!["fr".into(), "en".into()]);
    };
    let values = raw
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 5)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_languages"))?;
    let mut result = Vec::new();
    for value in values {
        let language = value
            .as_str()
            .filter(|v| valid_language(v))
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_languages"))?
            .to_ascii_lowercase();
        if !result.contains(&language) {
            result.push(language);
        }
    }
    Ok(result)
}

pub(super) fn apply_language_settings(input: &Value, settings: &mut Value) -> Result<(), Error> {
    if input.get("languages").is_none() {
        return Ok(());
    }
    let ordered = languages(input)?;
    let target = settings
        .as_object_mut()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_settings"))?;
    target.insert("language".into(), json!(ordered[0]));
    target.insert("fallbackLanguages".into(), json!(&ordered[1..]));
    target.insert(
        "includeNeutral".into(),
        json!(input["includeNeutral"].as_bool().unwrap_or(true)),
    );
    Ok(())
}

fn rank(language: Option<&str>, ordered: &[String], neutral: bool) -> Option<usize> {
    let Some(language) = language.filter(|v| !v.is_empty() && *v != "00" && *v != "und") else {
        return neutral.then_some(ordered.len() * 2);
    };
    let language = language.to_ascii_lowercase();
    ordered.iter().enumerate().find_map(|(i, preferred)| {
        if &language == preferred {
            Some(i * 2)
        } else if language.split('-').next() == preferred.split('-').next() {
            Some(i * 2 + 1)
        } else {
            None
        }
    })
}

fn kind(value: ImageType) -> &'static str {
    match value {
        ImageType::Poster => "poster",
        ImageType::Backdrop => "backdrop",
        ImageType::Banner => "banner",
        ImageType::Logo => "logo",
        ImageType::ClearArt => "illustration",
        ImageType::Thumb => "thumb",
        ImageType::DiscArt => "disc",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchCandidates {
    pub(super) work_id: String,
    #[serde(default)]
    pub(super) provider: Option<String>,
    #[serde(default)]
    pub(super) image_type: Option<String>,
    #[serde(default = "default_languages")]
    pub(super) languages: Vec<String>,
    #[serde(default = "yes")]
    pub(super) include_neutral: bool,
    #[serde(default)]
    pub(super) all_languages: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Collect {
    work_id: String,
    #[serde(default = "default_languages")]
    languages: Vec<String>,
    #[serde(default = "yes")]
    include_neutral: bool,
    #[serde(default = "default_max")]
    max_images: usize,
}
fn default_languages() -> Vec<String> {
    vec!["fr".into(), "en".into()]
}
fn default_max() -> usize {
    12
}
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CollectBatch {
    library_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "one")]
    limit: usize,
    #[serde(default = "default_languages")]
    languages: Vec<String>,
    #[serde(default = "yes")]
    include_neutral: bool,
    #[serde(default = "default_max")]
    max_images: usize,
}

fn one() -> usize {
    1
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ListWorks {
    library_id: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default = "page_size")]
    limit: usize,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    posters: Option<String>,
}
fn page_size() -> usize {
    25
}

fn effective_title(title: String, editorial: &str, manual: Option<&str>) -> String {
    let mut value: Value = serde_json::from_str(editorial).unwrap_or_else(|_| json!({}));
    if let (Some(target), Some(fields)) = (
        value.as_object_mut(),
        manual
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|v| v.as_object().cloned()),
    ) {
        target.extend(fields);
    }
    value["title"].as_str().map(str::to_owned).unwrap_or(title)
}

pub(super) async fn list_works(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(input): Query<ListWorks>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    if input.limit == 0 || input.limit > 100 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_limit"));
    }
    if input
        .after
        .as_ref()
        .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_cursor"));
    }
    let search = input.q.as_deref().unwrap_or("").trim();
    if search.len() > 100 || search.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_query"));
    }
    let filter = input.posters.as_deref().unwrap_or("all");
    if !matches!(filter, "all" | "with" | "without") {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_filter"));
    }
    // Treat LIKE metacharacters in the user's text literally.
    let pattern = format!(
        "%{}%",
        search
            .to_lowercase()
            .replace('!', "!!")
            .replace('%', "!%")
            .replace('_', "!_")
    );
    let exists: i64 =
        query_scalar("SELECT COUNT(*) FROM libraries WHERE id=? AND kind IN ('films','series')")
            .bind(&input.library_id)
            .fetch_one(auth.store.pool())
            .await
            .map_err(db)?;
    if exists == 0 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_library"));
    }
    let rows: Vec<(String,String,String,Option<String>,i64)> = query_as("SELECT w.id,w.title,w.editorial_json,w.manual_json,(SELECT COUNT(*) FROM work_image_entries i WHERE i.work_id=w.id AND i.kind='poster') FROM inventory_works w WHERE (w.library_id=? OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND w.id>? AND (CASE WHEN w.display_title_sort != '' THEN w.display_title_sort WHEN w.title_sort != '' THEN w.title_sort ELSE LOWER(w.title) END) LIKE ? ESCAPE '!' AND (?='all' OR (?='with' AND EXISTS(SELECT 1 FROM work_image_entries i WHERE i.work_id=w.id AND i.kind='poster')) OR (?='without' AND NOT EXISTS(SELECT 1 FROM work_image_entries i WHERE i.work_id=w.id AND i.kind='poster'))) ORDER BY w.id LIMIT ?")
        .bind(&input.library_id).bind(&input.library_id).bind(input.after.as_deref().unwrap_or(""))
        .bind(pattern).bind(filter).bind(filter).bind(filter)
        .bind((input.limit + 1) as i64).fetch_all(auth.store.pool()).await.map_err(db)?;
    let more = rows.len() > input.limit;
    let mut last = None;
    let items: Vec<Value> = rows
        .into_iter()
        .take(input.limit)
        .map(|(id, title, editorial, manual, count)| {
            last = Some(id.clone());
            json!({"workId":id,"title":effective_title(title, &editorial, manual.as_deref()),"posterCount":count})
        })
        .collect();
    Ok(Json(
        json!({"items":items,"nextCursor":if more {last} else {None}}),
    ))
}

/// Explicit, resumable batches: never enqueue an unbounded library scan.
pub(super) async fn collect_batch(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<CollectBatch>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if input.limit != 1 || input.max_images == 0 || input.max_images > 12 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_limit"));
    }
    if input
        .after
        .as_ref()
        .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_cursor"));
    }
    languages(&json!({"languages":input.languages}))?;
    let exists: i64 =
        query_scalar("SELECT COUNT(*) FROM libraries WHERE id=? AND kind IN ('films','series')")
            .bind(&input.library_id)
            .fetch_one(auth.store.pool())
            .await
            .map_err(db)?;
    if exists == 0 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_library"));
    }
    let works: Vec<(String, String, String, Option<String>)> = query_as(
        "SELECT id,title,editorial_json,manual_json FROM inventory_works WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND id>? ORDER BY id LIMIT ?",
    )
    .bind(&input.library_id)
    .bind(&input.library_id)
    .bind(input.after.as_deref().unwrap_or(""))
    .bind((input.limit + 1) as i64)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    let more = works.len() > input.limit;
    let mut items = Vec::new();
    let mut last = None;
    for (work_id, title, editorial, manual) in works.into_iter().take(input.limit) {
        let title = effective_title(title, &editorial, manual.as_deref());
        let request = Collect {
            work_id: work_id.clone(),
            languages: input.languages.clone(),
            include_neutral: input.include_neutral,
            max_images: input.max_images,
        };
        let item = match collect(State(auth.clone()), headers.clone(), Json(request)).await {
            Ok(Json(result)) => json!({"workId":work_id,"title":title,"result":result}),
            Err((status, Json(body))) => {
                json!({"workId":work_id,"title":title,"error":{"status":status.as_u16(),"code":body["error"]}})
            }
        };
        last = Some(work_id);
        items.push(item);
    }
    Ok(Json(
        json!({"items":items,"nextCursor":if more { last } else { None }}),
    ))
}

pub(super) async fn search_candidates(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<SearchCandidates>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    let ordered = languages(&json!({"languages":input.languages}))?;
    let context: Option<adapters::WorkContext> = query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
        .bind(&input.work_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
    let context = context.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    let payload =
        adapters::metadata_input(&context, &input.work_id, "metadata.search", &json!({}))?;
    let ids = payload["externalIds"]
        .as_object()
        .filter(|ids| !ids.is_empty())
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "external_ids_required"))?;
    crate::feedback::authorized(&auth, &headers, &input.work_id, false)
        .await
        .map_err(|s| err(s, "work_denied"))?;
    let rows: Vec<(String,String,String)> = query_as("SELECT e.id,e.package_json,e.generation FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.enabled=1 AND p.active=1 ORDER BY e.id LIMIT 65")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    let mut images_out = Vec::new();
    let mut errors = Vec::new();
    let mut available_providers = Vec::new();

    for (id, stored, generation) in rows {
        let package: Package = serde_json::from_str(&stored)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
        if !["images", "metadata.write"]
            .iter()
            .all(|p| package.manifest.permissions.iter().any(|v| v == p))
        {
            continue;
        }
        if !permitted(&auth, &headers, &id, &context.library_id, &generation).await? {
            continue;
        }
        available_providers.push(json!({
            "id": id,
            "name": package.manifest.name,
        }));
        if let Some(target) = input
            .provider
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "all")
            && id != target
        {
            continue;
        }
        let provider_id = ids.get(&id).and_then(Value::as_str).unwrap_or("");
        let media_type = match context.kind.as_str() {
            "series" => "tv_show",
            "episode" => "episode",
            _ => "movie",
        };
        let request = Run {
            work_id: input.work_id.clone(),
            operation: "metadata.images".into(),
            input: json!({
                "id": provider_id,
                "mediaType": media_type,
                "externalIds": ids,
                "languages": ordered,
                "includeNeutral": input.include_neutral
            }),
        };
        let provider_deadline = deadline.min(tokio::time::Instant::now() + Duration::from_secs(15));
        let result = tokio::time::timeout_at(
            provider_deadline,
            super::run(
                State(auth.clone()),
                headers.clone(),
                Path(id.clone()),
                Json(request),
            ),
        )
        .await;
        match result {
            Ok(Ok(result)) => {
                let images: Vec<MediaImage> = serde_json::from_value(result.0["proposal"].clone())
                    .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
                for image in images {
                    let image_kind_str = kind(image.image_type);
                    if let Some(target_type) = input
                        .image_type
                        .as_deref()
                        .filter(|s| !s.is_empty() && *s != "all")
                        && image_kind_str != target_type
                    {
                        continue;
                    }
                    if !input.all_languages {
                        let matches_lang =
                            rank(image.language.as_deref(), &ordered, input.include_neutral)
                                .is_some();
                        if !matches_lang {
                            continue;
                        }
                    }
                    images_out.push(json!({
                        "url": image.url,
                        "provider": id,
                        "providerName": package.manifest.name,
                        "imageType": image_kind_str,
                        "width": image.width,
                        "height": image.height,
                        "language": image.language,
                        "rating": image.rating,
                        "voteCount": image.vote_count,
                        "isPrimary": image.is_primary,
                    }));
                }
            }
            Ok(Err((status, _))) => errors.push(json!({"provider":id,"status":status.as_u16()})),
            Err(_) => errors.push(json!({"provider":id,"status":408})),
        }
    }
    // Sort candidate images: higher vote count/rating first, then resolution
    images_out.sort_by(|a, b| {
        let vote_b = b.get("voteCount").and_then(Value::as_u64).unwrap_or(0);
        let vote_a = a.get("voteCount").and_then(Value::as_u64).unwrap_or(0);
        let rating_b = b.get("rating").and_then(Value::as_f64).unwrap_or(0.0);
        let rating_a = a.get("rating").and_then(Value::as_f64).unwrap_or(0.0);
        vote_b.cmp(&vote_a).then_with(|| {
            rating_b
                .partial_cmp(&rating_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    Ok(Json(json!({
        "images": images_out,
        "providers": available_providers,
        "errors": errors
    })))
}

pub(super) async fn collect(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Collect>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if input.max_images == 0 || input.max_images > 12 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_limit"));
    }
    let ordered = languages(&json!({"languages":input.languages}))?;
    let context: Option<adapters::WorkContext> = query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
        .bind(&input.work_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
    let context = context.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    // Use corrected identifiers from the same canonical context as manual metadata operations.
    let payload =
        adapters::metadata_input(&context, &input.work_id, "metadata.search", &json!({}))?;
    let ids = payload["externalIds"]
        .as_object()
        .filter(|ids| !ids.is_empty())
        .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "external_ids_required"))?;
    crate::feedback::authorized(&auth, &headers, &input.work_id, false)
        .await
        .map_err(|s| err(s, "work_denied"))?;
    let rows: Vec<(String,String,String)> = query_as("SELECT e.id,e.package_json,e.generation FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.enabled=1 AND p.active=1 ORDER BY e.id LIMIT 65")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    if rows.len() > 64 {
        return Err(err(StatusCode::CONFLICT, "too_many_providers"));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(55);
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    let mut providers = 0;
    let mut partial = false;
    for (id, stored, generation) in rows {
        let package: Package = serde_json::from_str(&stored)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
        if !["images", "metadata.write"]
            .iter()
            .all(|p| package.manifest.permissions.iter().any(|v| v == p))
        {
            continue;
        }
        if !permitted(&auth, &headers, &id, &context.library_id, &generation).await? {
            continue;
        }
        let provider_id = ids.get(&id).and_then(Value::as_str).unwrap_or("");
        let media_type = match context.kind.as_str() {
            "series" => "tv_show",
            "episode" => "episode",
            _ => "movie",
        };
        let request = Run {
            work_id: input.work_id.clone(),
            operation: "metadata.images".into(),
            input: json!({
                "id": provider_id,
                "mediaType": media_type,
                "externalIds": ids,
                "languages": ordered,
                "includeNeutral": input.include_neutral
            }),
        };
        providers += 1;
        let provider_deadline = deadline.min(tokio::time::Instant::now() + Duration::from_secs(15));
        let result = tokio::time::timeout_at(
            provider_deadline,
            super::run(
                State(auth.clone()),
                headers.clone(),
                Path(id.clone()),
                Json(request),
            ),
        )
        .await;
        match result {
            Ok(Ok(result)) => {
                let images: Vec<MediaImage> = serde_json::from_value(result.0["proposal"].clone())
                    .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
                for image in images {
                    if let Some(priority) =
                        rank(image.language.as_deref(), &ordered, input.include_neutral)
                    {
                        candidates.push((priority, id.clone(), image));
                    }
                }
            }
            Ok(Err((status, _))) => errors.push(json!({"provider":id,"status":status.as_u16()})),
            Err(_) => {
                partial = true;
                errors.push(json!({"provider":id,"status":408}));
                if tokio::time::Instant::now() >= deadline {
                    break;
                }
            }
        }
    }
    // Interleave roles/providers at equal language rank so one poster list cannot fill the whole pool.
    candidates
        .sort_by(|a, b| (a.0, kind(a.2.image_type), &a.1).cmp(&(b.0, kind(b.2.image_type), &b.1)));
    let mut buckets: std::collections::BTreeMap<
        (usize, String, String),
        std::collections::VecDeque<MediaImage>,
    > = std::collections::BTreeMap::new();
    for (priority, provider, image) in candidates {
        buckets
            .entry((priority, kind(image.image_type).into(), provider))
            .or_default()
            .push_back(image);
    }
    let mut queue = Vec::new();
    while !buckets.is_empty() {
        buckets.retain(|(_, _, provider), images| {
            if let Some(image) = images.pop_front() {
                queue.push((provider.clone(), image));
            }
            !images.is_empty()
        });
    }
    let mut downloaded = 0;
    let mut duplicates = 0;
    let mut skipped = 0;
    let mut attempts = 0;
    let mut seen = HashSet::new();
    let counts: Vec<(String, i64)> =
        query_as("SELECT kind,COUNT(*) FROM work_image_entries WHERE work_id=? GROUP BY kind")
            .bind(&input.work_id)
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let mut counts: std::collections::HashMap<String, i64> = counts.into_iter().collect();
    let mut total: i64 = counts.values().sum();
    for (provider, image) in queue {
        let image_kind = kind(image.image_type);
        if total >= 64 || counts.get(image_kind).copied().unwrap_or(0) >= 12 {
            skipped += 1;
            continue;
        }
        if downloaded >= input.max_images
            || attempts >= 24
            || tokio::time::Instant::now() >= deadline
        {
            partial = true;
            break;
        }
        let hash = format!("{:x}", Sha256::digest(image.url.as_bytes()));
        if !seen.insert(hash.clone()) {
            skipped += 1;
            continue;
        }
        let known: i64 = query_scalar(
            "SELECT COUNT(*) FROM artwork_image_sources WHERE work_id=? AND source_url_hash=?",
        )
        .bind(&input.work_id)
        .bind(&hash)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
        if known > 0 {
            skipped += 1;
            continue;
        }
        attempts += 1;
        let request = adapters::ImportImage {
            work_id: input.work_id.clone(),
            url: image.url,
            kind: kind(image.image_type).into(),
            language: image
                .language
                .filter(|v| !v.is_empty() && v != "00" && v != "und"),
            append: true,
            apply_collection_policy: true,
        };
        match tokio::time::timeout_at(
            deadline,
            adapters::import_image(
                State(auth.clone()),
                headers.clone(),
                Path(provider.clone()),
                Json(request),
            ),
        )
        .await
        {
            Ok(Ok(result)) => {
                if result.0["skipped"].as_bool() == Some(true) {
                    skipped += 1;
                } else if result.0["duplicate"].as_bool() == Some(true) {
                    duplicates += 1;
                } else {
                    downloaded += 1;
                    total += 1;
                    *counts.entry(image_kind.into()).or_default() += 1;
                }
            }
            Ok(Err((status, _))) => {
                errors.push(json!({"provider":provider,"status":status.as_u16()}))
            }
            Err(_) => {
                partial = true;
                break;
            }
        }
    }
    Ok(Json(
        json!({"providers":providers,"downloaded":downloaded,"duplicates":duplicates,"skipped":skipped,"attempts":attempts,"errors":errors,"partial":partial}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_order_keeps_regions_and_explicit_neutral_policy() {
        let ordered = vec!["fr-ca".into(), "en".into()];
        assert_eq!(rank(Some("fr-CA"), &ordered, true), Some(0));
        assert_eq!(rank(Some("fr"), &ordered, true), Some(1));
        assert_eq!(rank(Some("en"), &ordered, true), Some(2));
        assert_eq!(rank(None, &ordered, true), Some(4));
        assert_eq!(rank(None, &ordered, false), None);
        assert_eq!(rank(Some("de"), &ordered, true), None);
        assert!(valid_language("zh-Hant-TW"));
        assert!(!valid_language("fr&api_key=secret"));
        assert!(!valid_language("-fr"));
    }
    #[test]
    fn language_overrides_cannot_replace_private_credentials() {
        let mut settings = json!({"apiKey":"private","language":"de"});
        apply_language_settings(
            &json!({"languages":["fr-CA","en"],"includeNeutral":false}),
            &mut settings,
        )
        .unwrap();
        assert_eq!(settings["apiKey"], "private");
        assert_eq!(settings["language"], "fr-ca");
        assert_eq!(settings["fallbackLanguages"], json!(["en"]));
        assert_eq!(settings["includeNeutral"], false);
        assert!(languages(&json!({"languages":[]})).is_err());
    }
}
