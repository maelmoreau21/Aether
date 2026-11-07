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

/// Durable error code for a search that found no single trustworthy candidate.
pub(crate) const UNIDENTIFIED_ERROR: &str = "metadata_unidentified";

/// Episodes sent together to one `intro.detect`, the target included. Measured with real Chromaprint
/// output run in this Core's wasmi runtime: three episodes with a full head and tail use about
/// 105 M of the guest's 200 M fuel and 63 KB of its 64 KiB request; a fourth exceeds both. The
/// Core used to send up to eight (170 KB), which the runtime refuses as `input_too_large`.
const MARKER_BATCH_MAX: usize = 3;
/// JSON added by the keys, ids and numbers of one episode besides its two base64 fingerprints.
const MARKER_EPISODE_OVERHEAD: usize = 320;
/// Left free in the request for the extension settings' growth and the envelope.
const MARKER_REQUEST_MARGIN: usize = 1024;

/// Order the candidates of one season for a detection: the target first, then the episodes whose
/// number is closest to it (neighbours most often share the same opening), keeping the incoming
/// order between equals. Returns indices into `episodes`: `(is_target, number)`.
pub(super) fn marker_peer_order(episodes: &[(bool, Option<i64>)]) -> Vec<usize> {
    let target = episodes.iter().find(|e| e.0).and_then(|e| e.1);
    let mut order: Vec<usize> = (0..episodes.len()).collect();
    order.sort_by_key(|&i| {
        let (is_target, number) = episodes[i];
        let distance = match (target, number) {
            (Some(t), Some(n)) => (t - n).abs(),
            _ => i64::MAX,
        };
        (!is_target, distance)
    });
    order
}

/// Whether one more episode of `size` bytes still fits the request.
pub(super) fn marker_request_fits(used: usize, size: usize, budget: usize) -> bool {
    used.saturating_add(size) <= budget
}

fn unidentified_search(non_music: bool, operation: &str, value: &Value) -> bool {
    non_music && operation == "metadata.search" && value.is_array()
}

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
        Some(super::run::Background::Task(job)),
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
        let order = marker_peer_order(
            &candidates
                .iter()
                .map(|c| (c.0 == job.target_id, c.6.parse::<i64>().ok()))
                .collect::<Vec<_>>(),
        );
        let mut candidates: Vec<Option<_>> = candidates.into_iter().map(Some).collect();
        let candidates: Vec<_> = order
            .into_iter()
            .filter_map(|i| candidates[i].take())
            .collect();
        let request_budget = media_extension_sdk::MAX_JSON.saturating_sub(
            serde_json::to_vec(&settings).map_or(0, |bytes| bytes.len()) + MARKER_REQUEST_MARGIN,
        );
        let mut request_bytes = 0usize;
        let mut episodes = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (asset, raw, key, source, probe, episode_work, number) in candidates {
            if episodes.len() >= MARKER_BATCH_MAX {
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
            let size = bytes.len().div_ceil(3) * 4
                + tail_compact.as_ref().map_or(0, String::len)
                + MARKER_EPISODE_OVERHEAD;
            if !episodes.is_empty() && !marker_request_fits(request_bytes, size, request_budget) {
                continue;
            }
            request_bytes += size;
            seen.insert(episode_work);
            marker_sources.push((asset.clone(), source));
            let mut ep_val = json!({
                "id": asset,
                "seriesId": scope.1,
                "seasonNumber": scope.2,
                "episodeNumber": number.parse::<u32>().ok(),
                "duration": duration,
                "fingerprintsCompact": STANDARD.encode(bytes),
                // The words are Chromaprint's; results stored before November 2026 carry a mislabelled
                // rate (8.6133), so the rate is the algorithm's, never the stored one.
                "fingerprintRate": media_node::tasks::FINGERPRINT_RATE
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
    let invocation = crate::extensions::runtime::Invocation {
        wasm: crate::extensions::storage::wasm(&auth.store, &package)
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
        // Older builds cached unresolved candidate lists; those must be retried
        // against the provider instead of replaying "not identified" for a day.
        && !unidentified_search(music_track.is_none(), operation, &cached)
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
        Some(super::run::Background::Task(job)),
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
            Some(super::run::Background::Task(job)),
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
            Some(super::run::Background::Task(job)),
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
    // No single trustworthy candidate: leave the work untouched and fail the
    // task so the engine retries it later (ids found meanwhile by other
    // providers or an NFO are picked up), then surfaces it for manual identify.
    if unidentified_search(music_track.is_none(), operation, &result) {
        drop(tx);
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, UNIDENTIFIED_ERROR));
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

#[cfg(test)]
mod marker_batch_tests {
    use super::*;

    #[test]
    fn target_first_then_neighbours_in_incoming_order() {
        // (is_target, episode number): the target is episode 6, listed after two newer results.
        let episodes = [
            (false, Some(1)),
            (false, Some(12)),
            (true, Some(6)),
            (false, Some(7)),
            (false, Some(5)),
            (false, Some(7)),
            (false, None),
        ];
        // Distances 5, 6, -, 1, 1, 1, unknown: ties keep the order the Core listed them in.
        assert_eq!(marker_peer_order(&episodes), vec![2, 3, 4, 5, 0, 1, 6]);
        // Without a numbered target nothing is reordered except that the target leads.
        assert_eq!(
            marker_peer_order(&[(false, Some(2)), (true, None), (false, Some(1))]),
            vec![1, 0, 2]
        );
    }

    #[test]
    fn three_full_episodes_fit_the_guest_request_and_four_do_not() {
        // Real Chromaprint output: a 300 s head is 2402 items, a 186 s tail 1481 items.
        let head: usize = 2402 * 4;
        let tail: usize = 1481 * 4;
        let size = head.div_ceil(3) * 4 + tail.div_ceil(3) * 4 + MARKER_EPISODE_OVERHEAD;
        let budget = media_extension_sdk::MAX_JSON - 400 - MARKER_REQUEST_MARGIN;
        let mut used = 0;
        let mut fitted = 0;
        while marker_request_fits(used, size, budget) {
            used += size;
            fitted += 1;
        }
        assert_eq!(fitted, 3, "{size} bytes each, budget {budget}");
        assert_eq!(fitted, MARKER_BATCH_MAX);
        // Episodes with no tail leave room, but the batch size stays capped by the fuel measurement.
        assert!(marker_request_fits(0, size, budget));
        assert!(!marker_request_fits(usize::MAX, 1, budget));
    }
}
