use super::*;
use crate::{
    auth::AuthState,
    database::{query_as, query_scalar},
};
use axum::http::{HeaderMap, StatusCode};
use base64::engine::general_purpose::STANDARD;
use media_extension_sdk::{ExtensionRequest, Package};
use serde_json::{Value, json};

/// Durable invocation uses the same validation, language policy and isolated worker
/// as interactive calls. No disk path or SQL capability is sent to the guest.
pub(super) type TargetResolution = (
    Option<String>,
    Option<adapters::WorkContext>,
    Option<(String, String, String, Option<f64>)>,
);

pub(crate) struct AutomatedResult {
    pub value: Value,
    pub marker_sources: Vec<(String, String)>,
}

pub(crate) async fn run_automated(
    auth: &AuthState,
    job: &crate::task_engine::Job,
    extension_id: &str,
    generation: &str,
    operation: &str,
) -> Result<AutomatedResult, Error> {
    let headers = HeaderMap::new();
    if !worker_permitted(
        auth,
        &headers,
        extension_id,
        &job.library_id,
        generation,
        Some(job),
    )
    .await?
    {
        return Err(err(StatusCode::FORBIDDEN, "extension_revoked"));
    }
    let (work_id, context, music_track): TargetResolution = {
        let work_id: Option<String> = query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root")
            .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
        if let Some(work_id) = work_id {
            let context = query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
                .bind(&work_id).fetch_one(auth.store.pool()).await.map_err(db)?;
            (Some(work_id), Some(context), None)
        } else {
            let track: Option<(String, String, String, Option<f64>)> = query_as("SELECT title, artist, album, duration FROM music_tracks WHERE id=? AND library_id=? AND available=1")
                .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
            let track =
                track.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_asset_or_track"))?;
            (None, None, Some(track))
        }
    };
    let raw: String =
        query_scalar("SELECT package_json FROM extensions WHERE id=? AND generation=?")
            .bind(extension_id)
            .bind(generation)
            .fetch_one(auth.store.pool())
            .await
            .map_err(db)?;
    let package: Package =
        serde_json::from_str(&raw).map_err(|_| err(StatusCode::CONFLICT, "invalid_package"))?;
    adapters::authorize_operation(&package.manifest, operation)?;
    let raw: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(extension_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = raw
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_settings"))?
        .unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        return Err(err(StatusCode::CONFLICT, "invalid_settings"));
    }
    let mut settings = resolve_effective_extension_settings(extension_id, settings);
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_library_options"))?;
    let providers = if operation.starts_with("intro.") {
        &options.segment_providers
    } else if operation == "metadata.images" {
        &options.image_providers
    } else {
        &options.metadata_providers
    };
    if crate::library_options::LibraryOptions::provider_priority(providers, extension_id, 100)
        .is_none()
    {
        return Err(err(StatusCode::FORBIDDEN, "library_provider_disabled"));
    }
    let legacy = operation == "metadata.propose"
        && package
            .manifest
            .config_schema
            .as_ref()
            .is_none_or(Vec::is_empty)
        && settings.as_object().is_some_and(|v| v.is_empty());
    let s_settings = crate::server_settings::load(&auth.store)
        .await
        .unwrap_or_default();
    let preferred_lang = if !options.preferred_metadata_language.trim().is_empty() {
        options.preferred_metadata_language.as_str()
    } else {
        s_settings.preferred_metadata_language.as_str()
    };
    let country_code = if !options.metadata_country_code.trim().is_empty() {
        options.metadata_country_code.as_str()
    } else {
        s_settings.metadata_country_code.as_str()
    };
    if !legacy {
        settings["language"] = json!(preferred_lang);
        settings["country"] = json!(country_code);
        settings["countryCode"] = json!(country_code);
    }
    let mut marker_sources = Vec::new();
    let mut payload = if let Some((title, artist, album, duration)) = &music_track {
        if operation == "lyrics.get" {
            lyrics_request_input(title, artist, album, *duration)
        } else if operation == "metadata.search" {
            music_album_search_input(title, artist, album).ok_or_else(|| {
                err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "music_identity_incomplete",
                )
            })?
        } else {
            json!({
                "title": title,
                "artist": artist,
                "album": album,
                "duration": duration,
                "mediaType": "music",
            })
        }
    } else if operation == "intro.detect" {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"))?;
        let work_id = work_id
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_required"))?;
        if context.kind != "episode" {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"));
        }
        let scope: (String, String, i64) = query_as("SELECT ep.season_id,se.series_id,se.season_number FROM inventory_episodes ep JOIN inventory_seasons se ON se.id=ep.season_id WHERE ep.work_id=?")
            .bind(work_id).fetch_one(auth.store.pool()).await.map_err(db)?;
        let candidates: Vec<(String,String,String,String,String,String,String)> = query_as("SELECT a.id,t.result_json,t.idempotency_key,obs.fingerprint,a.probe_json,ep.work_id,CAST(ep.episode_number AS TEXT) FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=a.library_id JOIN automation_observations obs ON obs.asset_id=a.id JOIN durable_tasks t ON t.target_id=a.id AND t.category='analyze' AND t.component='fingerprint' AND t.status='completed' WHERE ep.season_id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root ORDER BY CASE WHEN a.id=? THEN 0 ELSE 1 END,t.finished_at DESC,a.id LIMIT 12")
            .bind(&scope.0).bind(&job.library_id).bind(&job.target_id).fetch_all(auth.store.pool()).await.map_err(db)?;
        let mut episodes = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (asset, raw, key, source, probe, episode_work, number) in candidates {
            if episodes.len() >= 8 {
                break;
            }
            if seen.contains(&episode_work) {
                continue;
            }
            let expected = format!(
                "fp:{}",
                crate::automation::hash(&(&asset, &source, "chromaprint-v1")).map_err(db)?
            );
            if key != expected {
                continue;
            }
            let Ok(result) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(words) = result["fingerprints"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 6000)
            else {
                continue;
            };
            let mut bytes = Vec::with_capacity(words.len() * 4);
            let mut valid = true;
            for word in words {
                if let Some(n) = word.as_u64().filter(|n| *n <= u32::MAX as u64) {
                    bytes.extend_from_slice(&(n as u32).to_le_bytes());
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }

            let mut tail_compact = None;
            if let Some(tail_words) = result["tailFingerprints"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 4000)
            {
                let mut tail_bytes = Vec::with_capacity(tail_words.len() * 4);
                let mut tail_valid = true;
                for word in tail_words {
                    if let Some(n) = word.as_u64().filter(|n| *n <= u32::MAX as u64) {
                        tail_bytes.extend_from_slice(&(n as u32).to_le_bytes());
                    } else {
                        tail_valid = false;
                        break;
                    }
                }
                if tail_valid && !tail_bytes.is_empty() {
                    tail_compact = Some(STANDARD.encode(tail_bytes));
                }
            }

            let Ok(probe) = serde_json::from_str::<Value>(&probe) else {
                continue;
            };
            let Some(duration) = probe["format"]["duration"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n > 0.0)
            else {
                continue;
            };
            seen.insert(episode_work);
            marker_sources.push((asset.clone(), source));
            let mut ep_val = json!({
                "id": asset,
                "seriesId": scope.1,
                "seasonNumber": scope.2,
                "episodeNumber": number.parse::<u32>().ok(),
                "duration": duration,
                "fingerprintsCompact": STANDARD.encode(bytes),
                "fingerprintRate": result["fingerprintRate"]
            });
            if let Some(compact) = tail_compact {
                ep_val["tailFingerprintsCompact"] = json!(compact);
            }
            if let Some(ts) = result["tailStart"].as_f64() {
                ep_val["tailStart"] = json!(ts);
            }
            episodes.push(ep_val);
        }
        if episodes.len() < 2 || episodes[0]["id"] != job.target_id {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "compatible_fingerprint_peer_required",
            ));
        }
        json!({"episodes":episodes})
    } else if operation == "intro.parse_chapters" {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"))?;
        if context.kind != "episode" {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"));
        }
        let raw: String = query_scalar(
            "SELECT probe_json FROM inventory_assets WHERE id=? AND library_id=? AND available=1",
        )
        .bind(&job.target_id)
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
        let probe: Value = serde_json::from_str(&raw)
            .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_probe"))?;
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "duration_required"))?;
        let chapters = probe["chapters"].as_array().cloned().unwrap_or_default();
        if chapters.len() > 1000 {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "too_many_chapters"));
        }
        let chapters: Vec<Value> = chapters
            .iter()
            .filter_map(|chapter| {
                let start = chapter["start_time"].as_str()?.parse::<f64>().ok()?;
                let title = chapter["tags"]["title"].as_str()?;
                (start.is_finite() && start >= 0.0 && title.len() <= 128)
                    .then(|| json!({"start":start,"title":title}))
            })
            .collect();
        json!({"episodeId":work_id,"duration":duration,"chapters":chapters})
    } else {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_context_required"))?;
        let work_id = work_id
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_id_required"))?;
        let mut value = adapters::metadata_input(context, work_id, operation, &json!({}))?;
        if operation != "metadata.propose" {
            value["language"] = settings["language"].clone();
            value["country"] = settings["country"].clone();
            value["countryCode"] = settings["countryCode"].clone();
        }
        value
    };
    // An authoritative NFO/manual provider ID resolves details directly. Local
    // titles may be translated or a release filename and must not veto the ID.
    let known_id = (music_track.is_none() && operation == "metadata.search")
        .then(|| known_metadata_id(&payload, extension_id).map(str::to_owned))
        .flatten();
    if let Some(id) = &known_id {
        payload["id"] = json!(id);
    }
    let resolved_operation = if known_id.is_some() {
        "metadata.details"
    } else {
        operation
    };
    let invocation = crate::extension_runtime::Invocation {
        wasm: crate::extension_storage::wasm(&auth.store, &package)
            .await
            .map_err(storage_error)?,
        input: ExtensionRequest {
            api_version: 1,
            operation: resolved_operation.into(),
            input: metadata_input_with_settings(payload.clone(), settings),
        },
        allowed_hosts: package.manifest.http_hosts.clone(),
        auth_headers: Default::default(),
    };
    let cache_payload = provider_cache_context(
        &payload,
        &invocation.input.input["config"],
        generation,
        &package.sha256,
        music_track.is_some() && operation == "metadata.search",
    )
    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "cache_context_invalid"))?;
    let now = media_contracts::protocol::unix_now();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    if let Some(cached) = crate::automation::get_cached_provider_response(
        &mut tx,
        extension_id,
        operation,
        &cache_payload,
        now,
    )
    .await
    .map_err(db)?
    {
        if operation == "lyrics.get" && !lyrics_result_matches_input(&payload, &cached) {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_lyrics_origin",
            ));
        }
        if operation == "intro.detect" && !adapters::marker_result_matches_input(&payload, &cached)
        {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_marker_origin",
            ));
        }
        return Ok(AutomatedResult {
            value: cached,
            marker_sources,
        });
    }
    let result = invoke_worker_context(
        auth,
        &headers,
        extension_id,
        &job.library_id,
        generation,
        invocation.clone(),
        Some(job),
        None,
    )
    .await?;
    if let Some(id) = &known_id
        && result["providerId"].as_str() != Some(id.as_str())
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_metadata_origin",
        ));
    }
    if operation == "lyrics.get" && !lyrics_result_matches_input(&payload, &result) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_lyrics_origin",
        ));
    }
    if operation == "intro.detect" && !adapters::marker_result_matches_input(&payload, &result) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_marker_origin",
        ));
    }
    if music_track.is_some()
        && operation == "metadata.search"
        && let Some(items) = result.as_array()
        && let Some(candidate) = trusted_music_candidate(items, &invocation.input.input)
    {
        let mut details = invocation.clone();
        details.input.operation = "metadata.details".into();
        details.input.input["id"] = candidate["id"].clone();
        let enriched = invoke_worker_context(
            auth,
            &headers,
            extension_id,
            &job.library_id,
            generation,
            details,
            Some(job),
            None,
        )
        .await?;
        if music_details_match(&enriched, candidate) {
            let _ = crate::automation::put_cached_provider_response(
                &mut tx,
                extension_id,
                operation,
                &cache_payload,
                &enriched,
                86400,
                now,
            )
            .await;
            let _ = crate::database::commit(tx).await;
            return Ok(AutomatedResult {
                value: enriched,
                marker_sources,
            });
        }
    }
    // A cross-provider ID lookup may return a translated title or correct a
    // local year. Resolve its sole candidate, then verify the source ID in details.
    if music_track.is_none()
        && operation == "metadata.search"
        && let Some(items) = result.as_array()
        && let Some(candidate) = trusted_metadata_candidate(items, &invocation.input.input)
            .or_else(|| external_id_candidate(items, &invocation.input.input))
    {
        let source_input = invocation.input.input.clone();
        let identified_by_title = trusted_metadata_candidate(items, &source_input).is_some();
        let mut details = invocation;
        details.input.operation = "metadata.details".into();
        details.input.input["id"] = candidate["id"].clone();
        let enriched = invoke_worker_context(
            auth,
            &headers,
            extension_id,
            &job.library_id,
            generation,
            details,
            Some(job),
            None,
        )
        .await?;
        let id_matches = enriched["providerId"]
            .as_str()
            .is_some_and(|id| id == candidate["id"].as_str().unwrap_or(""));
        let title_matches = enriched["title"].as_str().is_none_or(|title| {
            matchable_title(title) == matchable_title(candidate["title"].as_str().unwrap_or(""))
        });
        let year_matches = invocation_year_matches(&enriched, candidate);
        let identity_matches = metadata_external_ids_match(&enriched, &source_input);
        if id_matches
            && !metadata_external_ids_conflict(&enriched, &source_input)
            && (identity_matches || (identified_by_title && title_matches && year_matches))
        {
            let _ = crate::automation::put_cached_provider_response(
                &mut tx,
                extension_id,
                operation,
                &cache_payload,
                &enriched,
                86400,
                now,
            )
            .await;
            let _ = crate::database::commit(tx).await;
            return Ok(AutomatedResult {
                value: enriched,
                marker_sources,
            });
        }
    }
    let _ = crate::automation::put_cached_provider_response(
        &mut tx,
        extension_id,
        operation,
        &cache_payload,
        &result,
        86400,
        now,
    )
    .await;
    let _ = crate::database::commit(tx).await;
    Ok(AutomatedResult {
        value: result,
        marker_sources,
    })
}
