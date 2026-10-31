//! Generic host adapters: no provider names, credentials or host inference.
use super::*;
use media_extension_sdk::{Lyrics, MediaImage, SearchResultItem, StructuredMetadata};

#[derive(sqlx::FromRow)]
pub(super) struct WorkContext {
    pub library_id: String,
    title: String,
    year: Option<i64>,
    pub(super) kind: String,
    external_ids_json: String,
    editorial_json: String,
    manual_json: Option<String>,
}
pub(super) fn authorize_operation(manifest: &Manifest, operation: &str) -> Result<(), Error> {
    let permission = match operation {
        "metadata.propose" | "metadata.search" | "metadata.details" | "lyrics.get" => {
            "metadata.read"
        }
        "metadata.images" => "images",
        "avatars.list" | "avatars.generate" => "avatars",
        "intro.detect"
        | "markers.detect"
        | "intro.match_fingerprints"
        | "intro.parse_chapters"
        | "markers.get" => "playback.markers",
        _ => return Err(err(StatusCode::BAD_REQUEST, "unsupported_operation")),
    };
    if !manifest.permissions.iter().any(|p| p == permission) {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    Ok(())
}
pub(super) fn metadata_input(
    context: &WorkContext,
    work: &str,
    operation: &str,
    input: &Value,
) -> Result<Value, Error> {
    if !operation.starts_with("metadata.") {
        return Err(err(StatusCode::BAD_REQUEST, "unsupported_operation"));
    }
    let mut editorial: Value =
        serde_json::from_str(&context.editorial_json).unwrap_or_else(|_| json!({}));
    if let Some(manual) = context
        .manual_json
        .as_deref()
        .and_then(|v| serde_json::from_str::<Value>(v).ok())
        && let (Some(target), Some(fields)) = (editorial.as_object_mut(), manual.as_object())
    {
        target.extend(fields.clone());
    }
    let mut ids: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&context.external_ids_json).unwrap_or_default();
    if let Some(extra) = editorial.get("external_ids").and_then(Value::as_object) {
        for (key, value) in extra {
            if let Some(value) = value.as_str() {
                ids.insert(key.clone(), value.into());
            }
        }
    }
    let media_type = match context.kind.as_str() {
        "series" => "tv_show",
        "episode" => "episode",
        _ => "movie",
    };
    if operation == "metadata.propose" {
        return Ok(
            json!({"apiVersion":1,"workId":work,"libraryId":context.library_id,
            "title":editorial["title"].as_str().unwrap_or(&context.title)}),
        );
    }
    let mut value = json!({"apiVersion":1,"workId":work,"libraryId":context.library_id,
        "title":editorial["title"].as_str().unwrap_or(&context.title),
        "year":editorial["year"].as_i64().or(context.year),"mediaType":media_type,"externalIds":ids});
    if let Some(language) = input.get("language").and_then(Value::as_str) {
        if !super::image_collection::valid_language(language) {
            return Err(err(StatusCode::BAD_REQUEST, "invalid_language"));
        }
        value["language"] = json!(language);
    }
    if operation == "metadata.details" || operation == "metadata.images" {
        let id = input
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| {
                s.len() <= 256
                    && (!s.is_empty() || (operation == "metadata.images" && !ids.is_empty()))
            })
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "candidate_required"))?;
        value["id"] = json!(id);
    }
    // Context belongs to the selected work; callers cannot substitute another library or settings.
    Ok(value)
}
pub(super) fn validate_result(operation: &str, result: Value) -> Result<Value, Error> {
    if let Some(code) = result.get("error").and_then(Value::as_str) {
        // Only a bounded machine code is public. Provider messages may contain credential-bearing URLs.
        let code: String = code
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(80)
            .collect();
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":"extension_error","pluginError":code})),
        ));
    }
    let invalid = || err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output");
    match operation {
        "metadata.propose" => {
            let proposal: MetadataProposal =
                serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            if proposal.title.is_empty()
                || proposal.title.len() > 512
                || proposal.summary.len() > 16384
                || proposal.title.chars().any(char::is_control)
            {
                return Err(invalid());
            }
        }
        "metadata.search" => {
            let items: Vec<SearchResultItem> =
                serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            if items.len() > 100
                || items.iter().any(|v| {
                    v.id.is_empty()
                        || v.id.len() > 256
                        || v.title.len() > 512
                        || v.artist.as_ref().is_some_and(|artist| {
                            artist.len() > 512 || artist.chars().any(char::is_control)
                        })
                })
            {
                return Err(invalid());
            }
        }
        "metadata.details" => {
            let _: StructuredMetadata =
                serde_json::from_value(result.clone()).map_err(|_| invalid())?;
        }
        "lyrics.get" => {
            let lyrics: Lyrics = serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            if lyrics.track_name.trim().is_empty()
                || lyrics.track_name.len() > 512
                || lyrics.artist_name.trim().is_empty()
                || lyrics.artist_name.len() > 512
                || lyrics
                    .album_name
                    .as_ref()
                    .is_some_and(|value| value.len() > 512)
                || lyrics
                    .plain
                    .as_ref()
                    .is_some_and(|value| value.len() > 32768)
                || lyrics
                    .synced
                    .as_ref()
                    .is_some_and(|value| value.len() > 32768)
                || lyrics.synced_lines.len() > 4096
                || lyrics
                    .synced_lines
                    .iter()
                    .any(|line| line.text.len() > 32768)
                || lyrics
                    .duration_secs
                    .is_some_and(|d| !d.is_finite() || d <= 0.0)
            {
                return Err(invalid());
            }
        }
        "metadata.images" => {
            let images: Vec<MediaImage> =
                serde_json::from_value(result.clone()).map_err(|_| invalid())?;
            if images.len() > 200 || images.iter().any(|image| !public_image_url(&image.url)) {
                return Err(invalid());
            }
        }
        "avatars.list" => {
            if !result.is_array() || result.as_array().is_some_and(|v| v.len() > 50) {
                return Err(invalid());
            }
        }
        "avatars.generate" => {
            let svg = result["svg"].as_str().ok_or_else(invalid)?;
            crate::avatars::validate_svg(svg).map_err(|_| invalid())?;
        }
        "intro.detect" | "markers.detect" => {
            let episodes = result.as_array().ok_or_else(invalid)?;
            if episodes.len() > 32
                || episodes.iter().any(|episode| {
                    episode["episodeId"]
                        .as_str()
                        .is_none_or(|id| id.is_empty() || id.len() > 128)
                        || episode["markers"].as_array().is_none_or(|markers| {
                            markers.len() > 8 || markers.iter().any(|m| !valid_timeline_marker(m))
                        })
                })
            {
                return Err(invalid());
            }
        }
        "intro.parse_chapters" | "markers.get" => {
            let markers = result.as_array().ok_or_else(invalid)?;
            if markers.len() > 8 || markers.iter().any(|m| !valid_timeline_marker(m)) {
                return Err(invalid());
            }
        }
        "intro.match_fingerprints" => {
            if !result.is_null()
                && (result["offsetA"]
                    .as_f64()
                    .is_none_or(|n| !n.is_finite() || n < 0.0)
                    || result["offsetB"]
                        .as_f64()
                        .is_none_or(|n| !n.is_finite() || n < 0.0)
                    || result["duration"]
                        .as_f64()
                        .is_none_or(|n| !n.is_finite() || n <= 0.0)
                    || result["score"]
                        .as_f64()
                        .is_none_or(|n| !n.is_finite() || !(0.0..=1.0).contains(&n)))
            {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(result)
}
// The guest may only report on the exact copies it received. Bind boundaries
// to the input duration too, including responses from the provider cache.
pub(super) fn marker_result_matches_input(input: &Value, result: &Value) -> bool {
    let Some(inputs) = input["episodes"].as_array() else {
        return false;
    };
    let Some(outputs) = result.as_array() else {
        return false;
    };
    if inputs.len() < 2 || inputs.len() > 32 || outputs.len() > inputs.len() {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    outputs.iter().all(|output| {
        let Some(id) = output["episodeId"].as_str() else {
            return false;
        };
        let Some(source) = inputs.iter().find(|entry| entry["id"].as_str() == Some(id)) else {
            return false;
        };
        let Some(duration) = source["duration"]
            .as_f64()
            .filter(|d| d.is_finite() && *d > 0.0)
        else {
            return false;
        };
        seen.insert(id)
            && output["markers"].as_array().is_some_and(|markers| {
                markers.len() <= 8
                    && markers.iter().all(|marker| {
                        valid_timeline_marker(marker)
                            && marker["end"].as_f64().is_some_and(|end| end <= duration)
                    })
            })
    })
}

fn valid_timeline_marker(marker: &Value) -> bool {
    let (Some(start), Some(end), Some(confidence)) = (
        marker["start"].as_f64(),
        marker["end"].as_f64(),
        marker["confidence"].as_f64(),
    ) else {
        return false;
    };
    start.is_finite()
        && end.is_finite()
        && confidence.is_finite()
        && start >= 0.0
        && end > start
        && end - start <= 600.0
        && (0.0..=1.0).contains(&confidence)
        && marker["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 128)
        && matches!(
            marker["markerType"].as_str(),
            Some("intro" | "credits" | "recap" | "preview")
        )
        && matches!(
            marker["method"].as_str(),
            Some("fingerprint" | "chapter" | "silence" | "manual")
        )
}
#[cfg(test)]
mod marker_tests {
    use super::*;

    #[test]
    fn detection_results_are_bound_to_submitted_copies_and_durations() {
        let input = json!({"episodes":[{"id":"a","duration":120.0},{"id":"b","duration":90.0}]});
        let marker = json!({"id":"m","start":10.0,"end":40.0,"confidence":0.95,"markerType":"intro","method":"fingerprint"});
        let valid = json!([{"episodeId":"a","markers":[marker.clone()]},{"episodeId":"b","markers":[marker.clone()]}]);
        assert!(marker_result_matches_input(&input, &valid));
        let mut unknown = valid.clone();
        unknown[1]["episodeId"] = json!("other");
        assert!(!marker_result_matches_input(&input, &unknown));
        let mut duplicate = valid.clone();
        duplicate[1]["episodeId"] = json!("a");
        assert!(!marker_result_matches_input(&input, &duplicate));
        let mut past_end = valid.clone();
        past_end[1]["markers"][0]["end"] = json!(91.0);
        assert!(!marker_result_matches_input(&input, &past_end));
        let mut bad_confidence = valid;
        bad_confidence[0]["markers"][0]["confidence"] = json!(1.1);
        assert!(!marker_result_matches_input(&input, &bad_confidence));
    }

    #[test]
    fn lyrics_result_is_typed_and_bounded() {
        let result = json!({"id":42,"trackName":"Song","artistName":"Artist","instrumental":false,"plain":"Words"});
        assert!(validate_result("lyrics.get", result.clone()).is_ok());
        assert!(validate_result("lyrics.get", json!({"lyrics":"untyped"})).is_err());
        let mut bad = result.clone();
        bad["synced"] = json!("x".repeat(32769));
        assert!(validate_result("lyrics.get", bad).is_err());
        let mut bad = result;
        bad["trackName"] = json!("");
        assert!(validate_result("lyrics.get", bad).is_err());
    }

    #[test]
    fn lyrics_operation_requires_metadata_permission() {
        let mut manifest = Manifest::default();
        assert_eq!(
            authorize_operation(&manifest, "lyrics.get").unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        manifest.permissions.push("metadata.read".into());
        assert!(authorize_operation(&manifest, "lyrics.get").is_ok());
    }

    #[test]
    fn marker_operations_require_the_dedicated_permission() {
        let mut manifest = Manifest {
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        };
        for operation in [
            "intro.detect",
            "markers.detect",
            "intro.match_fingerprints",
            "intro.parse_chapters",
            "markers.get",
        ] {
            assert_eq!(
                authorize_operation(&manifest, operation).unwrap_err().0,
                StatusCode::FORBIDDEN
            );
        }
        manifest.permissions.push("playback.markers".into());
        for operation in [
            "intro.detect",
            "markers.detect",
            "intro.match_fingerprints",
            "intro.parse_chapters",
            "markers.get",
        ] {
            assert!(authorize_operation(&manifest, operation).is_ok());
        }
        manifest.permissions.clear();
        assert!(authorize_operation(&manifest, "intro.detect").is_err());
    }

    #[test]
    fn rejects_unbounded_or_invalid_wasm_markers() {
        let valid = json!({"id":"m1","start":10.0,"end":40.0,"confidence":0.98,"markerType":"intro","method":"chapter"});
        assert!(validate_result("intro.parse_chapters", json!([valid.clone()])).is_ok());
        let mut invalid = valid;
        invalid["end"] = json!(1000.0);
        assert!(validate_result("intro.parse_chapters", json!([invalid])).is_err());
        assert!(
            validate_result("intro.detect", json!([{"episodeId":"ep","markers":"bad"}])).is_err()
        );
        assert!(
            validate_result(
                "intro.match_fingerprints",
                json!({"offsetA":0,"offsetB":0,"duration":-1,"score":1})
            )
            .is_err()
        );
    }
}
fn public_image_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}
pub(super) fn catalogue_metadata(metadata: Value) -> Result<serde_json::Map<String, Value>, Error> {
    let mut fields = metadata
        .as_object()
        .cloned()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_metadata"))?;
    for (source, target) in [
        ("summary", "plot"),
        ("synopsis", "plot"),
        ("originalTitle", "original_title"),
        ("externalIds", "external_ids"),
        ("releaseDate", "premiered"),
    ] {
        if let Some(value) = fields.remove(source) {
            fields.insert(target.into(), value);
        }
    }
    // Artwork is explicitly imported and normalized, never installed by remote URL in metadata.
    for key in [
        "posterUrl",
        "poster_url",
        "backdropUrl",
        "backdrop_url",
        "logoUrl",
        "logo_url",
        "posters",
        "backdrops",
        "images",
    ] {
        fields.remove(key);
    }
    fields.retain(|_, v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty));
    Ok(fields)
}
pub(super) fn merge_metadata(
    previous: Option<String>,
    mut fields: serde_json::Map<String, Value>,
) -> Value {
    let mut merged = previous
        .and_then(|v| serde_json::from_str::<Value>(&v).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(ids) = fields
        .get_mut("external_ids")
        .and_then(Value::as_object_mut)
        && let Some(existing) = merged["external_ids"].as_object()
    {
        for (key, value) in existing {
            ids.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    if let Some(target) = merged.as_object_mut() {
        target.extend(fields);
    }
    merged
}

pub(super) async fn avatar_extensions(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let rows: Vec<String> = query_scalar("SELECT e.package_json FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.enabled=1 AND p.active=1 ORDER BY e.id")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    let extensions: Vec<Value> = rows
        .into_iter()
        .filter_map(|s| serde_json::from_str::<Package>(&s).ok())
        .filter(|p| p.manifest.permissions.iter().any(|v| v == "avatars"))
        .map(|p| json!({"id":p.manifest.id,"name":p.manifest.name}))
        .collect();
    Ok(Json(json!({"extensions":extensions})))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AvatarOperation {
    operation: String,
    #[serde(default)]
    pack: Option<String>,
    #[serde(default)]
    seed: Option<String>,
    #[serde(default)]
    apply: bool,
}
pub(super) async fn avatar_operation(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AvatarOperation>,
) -> Result<Json<Value>, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    auth.require_csrf(&headers, &actor)
        .map_err(|s| err(s, "csrf_required"))?;
    if !matches!(
        input.operation.as_str(),
        "avatars.list" | "avatars.generate"
    ) || (input.apply && input.operation != "avatars.generate")
        || input.pack.as_ref().is_some_and(|v| v.len() > 80)
        || input.seed.as_ref().is_some_and(|v| v.len() > 128)
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_input"));
    }
    let _slot = crate::extension_runtime::try_acquire_execution(
        &id,
        crate::extension_runtime::ExecutionClass::Interactive,
    )
    .ok_or_else(|| err(StatusCode::CONFLICT, "extension_busy"))?;
    let row: Option<(String, String)> =
        query_as("SELECT package_json,generation FROM extensions WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (stored, generation) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;
    if !permitted(&auth, &headers, &id, "", &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    authorize_operation(&package.manifest, &input.operation)?;
    // Avatar operations receive no library data and no network capability.
    let invocation = crate::extension_runtime::Invocation::new(
        crate::extension_storage::wasm(&auth.store, &package)
            .await
            .map_err(super::storage_error)?,
        ExtensionRequest {
            api_version: 1,
            operation: input.operation,
            input: json!({"pack":input.pack,"seed":input.seed}),
        },
    );
    let result = invoke_worker(&auth, &headers, &id, "", &generation, invocation).await?;
    if input.apply {
        let svg = result["svg"]
            .as_str()
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
        let _lock = MUTATIONS.lock().await;
        if !permitted(&auth, &headers, &id, "", &generation).await? {
            return Err(err(StatusCode::FORBIDDEN, "extension_revoked"));
        }
        let meta = auth
            .store
            .assets
            .put_bytes(svg.as_bytes(), "image/svg+xml")
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))?;
        query("INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET sha256=excluded.sha256")
            .bind(&actor.user.id)
            .bind(&meta.sha256)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    }
    Ok(Json(json!({"result":result,"applied":input.apply})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ImportImage {
    pub(super) work_id: String,
    pub(super) url: String,
    pub(super) kind: String,
    #[serde(default)]
    pub(super) language: Option<String>,
    #[serde(default)]
    pub(super) append: bool,
    #[serde(skip)]
    pub(super) apply_collection_policy: bool,
}
pub(super) async fn import_image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ImportImage>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if !matches!(
        input.kind.as_str(),
        "poster" | "backdrop" | "logo" | "banner" | "thumb" | "illustration" | "disc" | "landscape"
    ) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_image_kind"));
    }
    if input
        .language
        .as_deref()
        .is_some_and(|v| !super::image_collection::valid_language(v))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_language"));
    }
    let library: Option<String> = query_scalar("SELECT library_id FROM inventory_works WHERE id=?")
        .bind(&input.work_id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    let library = library.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    let row: Option<(String, String)> =
        query_as("SELECT package_json,generation FROM extensions WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (stored, generation) =
        row.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;
    if !permitted(&auth, &headers, &id, &library, &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    for permission in ["images", "metadata.write"] {
        if !package.manifest.permissions.iter().any(|p| p == permission) {
            return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
        }
    }
    let _slot = crate::extension_runtime::try_acquire_execution(
        &id,
        crate::extension_runtime::ExecutionClass::Interactive,
    )
    .ok_or_else(|| err(StatusCode::CONFLICT, "extension_busy"))?;
    let url = reqwest::Url::parse(&input.url)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_image_url"))?;
    let host = url
        .host_str()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_image_url"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !package.manifest.http_hosts.iter().any(|v| v == host)
    {
        return Err(err(StatusCode::FORBIDDEN, "image_host_denied"));
    }
    let host = host.to_owned();
    let bytes = tokio::time::timeout(Duration::from_secs(12), async {
        let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "image_dns_failed"))?
            .collect();
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|a| crate::extension_runtime::is_forbidden_ip(a.ip()))
        {
            return Err(err(StatusCode::FORBIDDEN, "image_host_denied"));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addresses)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "image_download_failed"))?;
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "image_download_failed"))?;
        if !response.status().is_success() {
            return Err(err(StatusCode::BAD_GATEWAY, "image_download_failed"));
        }
        let content = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "image_download_failed"))?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((bytes, content))
    })
    .await
    .map_err(|_| err(StatusCode::REQUEST_TIMEOUT, "image_timeout"))??;
    let _lock = MUTATIONS.lock().await;
    if !permitted(&auth, &headers, &id, &library, &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_revoked"));
    }
    let mut headers = headers;
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&bytes.1)
            .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_image_type"))?,
    );
    let (_, result) = crate::artwork::upload(
        State(auth),
        Path((input.work_id, input.kind)),
        Query(crate::artwork::ImageQuery {
            append: input.append,
            language: input.language,
            provider: Some(id),
            source_url_hash: Some(format!("{:x}", Sha256::digest(input.url.as_bytes()))),
            apply_collection_policy: input.apply_collection_policy,
            ..Default::default()
        }),
        headers,
        bytes.0.into(),
    )
    .await
    .map_err(|s| err(s, "image_import_failed"))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_context_drives_series_search_and_candidate_details() {
        let context = WorkContext {
            library_id: "shows".into(),
            title: "Old".into(),
            year: Some(1999),
            kind: "series".into(),
            external_ids_json: r#"{"external":"old"}"#.into(),
            editorial_json: "{}".into(),
            manual_json: Some(
                r#"{"title":"Correct title","year":2020,"external_ids":{"external":"correct"}}"#
                    .into(),
            ),
        };
        let search = metadata_input(
            &context,
            "work",
            "metadata.search",
            &json!({"mediaType":"movie","libraryId":"other","config":{"secret":"foreign"}}),
        )
        .unwrap();
        assert_eq!(search["title"], "Correct title");
        assert_eq!(search["year"], 2020);
        assert_eq!(search["mediaType"], "tv_show");
        assert_eq!(search["externalIds"]["external"], "correct");
        assert_eq!(search["libraryId"], "shows");
        assert!(search.get("config").is_none());
        assert!(metadata_input(&context, "work", "metadata.details", &json!({})).is_err());
        assert_eq!(
            metadata_input(
                &context,
                "work",
                "metadata.details",
                &json!({"id":"chosen"})
            )
            .unwrap()["id"],
            "chosen"
        );
    }
    #[test]
    fn operation_permissions_errors_and_catalogue_mapping_are_explicit() {
        let manifest = Manifest {
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        };
        assert!(authorize_operation(&manifest, "metadata.search").is_ok());
        assert!(authorize_operation(&manifest, "metadata.images").is_err());
        assert!(authorize_operation(&manifest, "avatars.generate").is_err());
        let failure=validate_result("metadata.search",json!({"error":"missing_api_key","message":"secret https://example.invalid?api_key=secret"})).unwrap_err();
        assert_eq!(
            failure.1.0,
            json!({"error":"extension_error","pluginError":"missing_api_key"})
        );
        assert!(validate_result("metadata.search", json!({"title":"wrong shape"})).is_err());
        let value=catalogue_metadata(json!({"title":"Film","synopsis":"Plot","originalTitle":"Original","externalIds":{"external":"id"},"posters":[{"url":"https://example.invalid"}],"posterUrl":"https://example.invalid","genres":[]})).unwrap();
        assert_eq!(value["plot"], "Plot");
        assert_eq!(value["original_title"], "Original");
        assert_eq!(value["external_ids"]["external"], "id");
        assert!(!value.contains_key("posters"));
        assert!(!value.contains_key("genres"));
        let merged = merge_metadata(
            Some(json!({"plot":"Keep plot","external_ids":{"first":"preserved"}}).to_string()),
            value,
        );
        assert_eq!(merged["external_ids"]["first"], "preserved");
        assert_eq!(merged["external_ids"]["external"], "id");
    }
}
