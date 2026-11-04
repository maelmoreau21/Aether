//! Playback descriptors, source selection and validated track identities.

use super::decode;
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use crate::media::Probe;
use serde_json::{Value, json};
use sqlx::Row;

// Folder scopes inherit the live configuration of their logical library.
pub(super) async fn playback_library_options(
    store: &IdentityStore,
    library_id: &str,
) -> Result<crate::library_options::LibraryOptions, sqlx::Error> {
    let encoded: String = query_scalar("SELECT COALESCE(parent.options_json,l.options_json) FROM libraries l LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 LEFT JOIN libraries parent ON parent.id=f.parent_id WHERE l.id=?")
        .bind(library_id).fetch_one(&store.pool).await?;
    serde_json::from_str(&encoded)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))
}

pub async fn descriptor(
    store: &IdentityStore,
    asset_id: &str,
    session_hash: &str,
) -> Result<Option<Value>, sqlx::Error> {
    descriptor_for_user(store, asset_id, session_hash, None, None).await
}

pub async fn descriptor_for_user(
    store: &IdentityStore,
    asset_id: &str,
    session_hash: &str,
    user_id: Option<&str>,
    visible_libraries: Option<&[String]>,
) -> Result<Option<Value>, sqlx::Error> {
    crate::nodes::ensure_node_registry_warm(store).await?;
    let row = query(
        "SELECT a.id AS asset_id, a.mime, a.probe_json, a.library_id, COALESCE(json_extract(w.manual_json,'$.title'),w.title) AS title, w.id AS work_id, w.kind AS work_kind, l.node_id, t.subtitles_json, au.audios_json \
         FROM inventory_assets a \
         LEFT JOIN inventory_subtitles t ON t.asset_id=a.id \
         LEFT JOIN inventory_audio au ON au.asset_id=a.id \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         JOIN inventory_works w ON w.id=e.work_id \
         JOIN libraries l ON l.id=s.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE a.id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await?;
    let Some(initial_row) = row else {
        return Ok(None);
    };

    let initial_work_id: String = initial_row.get("work_id");
    let initial_work_kind: String = initial_row.get("work_kind");
    let initial_node_id: String = initial_row.get("node_id");
    let initial_mime: String = initial_row.get("mime");

    let now = media_contracts::protocol::unix_now();
    let (initial_node_status, initial_conn_mode, initial_score) =
        match store.node_registry.get(&initial_node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&initial_mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };

    // Resolve all available sibling copies across multi-location folders and consolidated work groups
    let sibling_rows = if initial_work_kind == "episode" {
        query(
            "SELECT a.id AS asset_id, a.mime, a.library_id, l.node_id, w.id AS work_id \
             FROM inventory_episodes ep_orig \
             JOIN inventory_seasons s_orig ON s_orig.id=ep_orig.season_id \
             JOIN inventory_seasons s_cand ON s_cand.season_number=s_orig.season_number \
                  AND (s_cand.series_id=s_orig.series_id OR s_cand.series_id IN ( \
                      SELECT m2.work_id FROM media_group_members m1 \
                      JOIN media_group_members m2 ON m2.group_id=m1.group_id \
                      WHERE m1.work_id=s_orig.series_id \
                  )) \
             JOIN inventory_episodes ep_cand ON ep_cand.season_id=s_cand.id AND ep_cand.episode_number=ep_orig.episode_number \
             JOIN inventory_editions e ON e.work_id=ep_cand.work_id \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources s ON s.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=s.id \
             JOIN inventory_works w ON w.id=e.work_id \
             JOIN libraries l ON l.id=s.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE ep_orig.work_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1",
        )
        .bind(&initial_work_id)
        .fetch_all(&store.pool)
        .await
        .unwrap_or_default()
    } else {
        query(
            "SELECT a.id AS asset_id, a.mime, a.library_id, l.node_id, w.id AS work_id \
             FROM inventory_editions e \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources s ON s.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=s.id \
             JOIN inventory_works w ON w.id=e.work_id \
             JOIN libraries l ON l.id=s.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE (e.work_id=? OR e.work_id IN ( \
                 SELECT m2.work_id FROM media_group_members m1 \
                 JOIN media_group_members m2 ON m2.group_id=m1.group_id \
                 WHERE m1.work_id=? \
             )) AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1",
        )
        .bind(&initial_work_id)
        .bind(&initial_work_id)
        .fetch_all(&store.pool)
        .await
        .unwrap_or_default()
    };

    let mut ranked_siblings: Vec<(String, String, i64)> = Vec::new();
    for srow in sibling_rows {
        let cand_asset_id: String = srow.get("asset_id");
        let cand_lib_id: String = srow.get("library_id");
        let cand_node_id: String = srow.get("node_id");
        let cand_mime: String = srow.get("mime");
        let cand_work_id: String = srow.get("work_id");

        if let Some(vis) = visible_libraries
            && !vis.contains(&cand_lib_id)
        {
            continue;
        }
        if let Some(uid) = user_id
            && cand_work_id != initial_work_id
            && !crate::product::target_allowed(store, uid, "work", &cand_work_id)
                .await
                .unwrap_or(false)
        {
            continue;
        }

        let (cand_status, _cand_mode, cand_score) = match store.node_registry.get(&cand_node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&cand_mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };
        if !ranked_siblings
            .iter()
            .any(|(id, _, _)| id == &cand_asset_id)
        {
            ranked_siblings.push((cand_asset_id, cand_status, cand_score));
        }
    }

    ranked_siblings.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));

    // If the requested asset is on a disconnected Node and a connected sibling copy exists,
    // automatically fail over to the best connected copy!
    let mut active_row = initial_row;
    let mut effective_asset_id = asset_id.to_owned();
    let mut failover_from: Option<String> = None;
    let mut node_status = initial_node_status;
    let mut conn_mode = initial_conn_mode;
    let mut node_score = initial_score;

    if node_status != "connected"
        && let Some((best_alt_id, best_alt_status, _)) = ranked_siblings
            .iter()
            .find(|(id, st, _)| id != asset_id && st == "connected")
        && *best_alt_status == "connected"
        && let Some(alt_row) = query(
            "SELECT a.id AS asset_id, a.mime, a.probe_json, a.library_id, COALESCE(json_extract(w.manual_json,'$.title'),w.title) AS title, w.id AS work_id, w.kind AS work_kind, l.node_id, t.subtitles_json, au.audios_json \
             FROM inventory_assets a \
             LEFT JOIN inventory_subtitles t ON t.asset_id=a.id \
             LEFT JOIN inventory_audio au ON au.asset_id=a.id \
             JOIN inventory_sources s ON s.id=a.source_id \
             JOIN inventory_versions v ON v.id=s.version_id \
             JOIN inventory_editions e ON e.id=v.edition_id \
             JOIN inventory_works w ON w.id=e.work_id \
             JOIN libraries l ON l.id=s.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE a.id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1",
        )
        .bind(best_alt_id)
        .fetch_optional(&store.pool)
        .await?
    {
        let alt_node_id: String = alt_row.get("node_id");
        let alt_mime: String = alt_row.get("mime");
        let (alt_st, alt_cm, alt_sc) = match store.node_registry.get(&alt_node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&alt_mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };
        effective_asset_id = best_alt_id.clone();
        failover_from = Some(asset_id.to_owned());
        node_status = alt_st;
        conn_mode = alt_cm;
        node_score = alt_sc;
        active_row = alt_row;
    }

    let fallback_media_ids: Vec<String> = ranked_siblings
        .into_iter()
        .filter(|(id, st, _)| id != &effective_asset_id && st == "connected")
        .map(|(id, _, _)| id)
        .collect();

    let row = active_row;
    let asset_id = effective_asset_id.as_str();
    let probe = decode(row.get("probe_json"))?;
    let library_id: String = row.get("library_id");
    let node_id: String = row.get("node_id");
    let library_options = playback_library_options(store, &library_id).await?;
    let node_rec = crate::nodes::load_node(store, &node_id).await?;
    let direct_info = if node_status == "connected" {
        node_rec.as_ref().and_then(|n| {
            crate::nodes::build_direct_playback_url(
                n,
                &library_id,
                "asset",
                asset_id,
                "stream",
                session_hash,
            )
        })
    } else {
        None
    };
    let mut original_audios = probe
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("audio"));
    let first_audio = original_audios.next();
    // A browser may select a different default track in a multi-audio container.
    // Only a mono-audio original has an unambiguous language; explicit tracks follow below.
    let language = first_audio
        .filter(|_| original_audios.next().is_none())
        .and_then(|s| s.tags.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("und");
    let language = match language {
        "fra" | "fre" => "fr",
        "eng" => "en",
        other => other,
    };
    let relay_url = format!("/api/v1/assets/{asset_id}/content");
    let mut source_obj = json!({
        "id": asset_id,
        "audioTrackId": "internal-default",
        "label": "original",
        "language": language,
        "url": relay_url,
        "relayUrl": relay_url,
        "nodeId": node_id,
        "nodeStatus": node_status,
        "connectionMode": conn_mode,
        "score": node_score,
        "mime": row.get::<String,_>("mime"),
        "tracks": probe.streams
    });
    if let Some((direct_url, expires_at)) = direct_info {
        source_obj["directUrl"] = json!(direct_url);
        source_obj["tokenExpiresAt"] = json!(expires_at);
    }
    let mut subtitles = subtitle_descriptors(
        asset_id,
        row.get::<Option<String>, _>("subtitles_json").as_deref(),
    )?;
    subtitles.extend(crate::extensions::subtitles::descriptors(store, asset_id).await?);
    let audios: Vec<media_contracts::inventory::ExternalAudio> = serde_json::from_str(
        row.get::<Option<String>, _>("audios_json")
            .as_deref()
            .unwrap_or("[]"),
    )
    .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let mut sources = vec![source_obj.clone()];
    let duration = probe
        .format
        .get("duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);
    let video_source = probe.streams.iter().find(|track| {
        track.codec_type.as_deref() == Some("video") && track.codec_name.as_deref() == Some("av1")
    });
    // Optional preparation is restricted to known codecs; original source stays available.
    if probe
        .streams
        .iter()
        .any(|t| t.codec_type.as_deref() == Some("video") && t.codec_name.as_deref() == Some("av1"))
    {
        for track in probe.streams.iter().filter(|t| {
            t.codec_type.as_deref() == Some("audio") && t.codec_name.as_deref() == Some("opus")
        }) {
            let language = track
                .tags
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("und");
            let language = match language {
                "fra" | "fre" => "fr",
                "eng" => "en",
                other => other,
            };
            sources.push(json!({
                "id": format!("{asset_id}-internal-{}", track.index),
                "audioTrackId": format!("internal-{}", track.index), "language": language,
                "url": format!("/api/v1/assets/{asset_id}/prepared/video"),
                "externalAudioUrl": format!("/api/v1/assets/{asset_id}/prepared/audio-{}", track.index),
                "audioSegmentUrl": format!("/api/v1/assets/{asset_id}/prepared/audio-{}-chunk-", track.index),
                "prepare": true, "label": "interne", "mime": "video/mp4", "tracks": [track]
            }));
            if let Some(source) = sources.last_mut() {
                if let Some(duration) = duration {
                    source["durationSeconds"] = json!(duration);
                }
                if let Some(video) = video_source {
                    if let Some(mime) = av1_video_mime(video) {
                        source["videoMime"] = json!(mime);
                    }
                    let index = video.index;
                    source["videoStreamUrl"] = json!(format!(
                        "/api/v1/assets/{asset_id}/prepared/video-stream-{index}-"
                    ));
                }
                if let Some(id) = native_opus_track_id(&probe, track) {
                    source["audioSegmentUrl"] = json!(format!(
                        "/api/v1/assets/{asset_id}/prepared/opus-{id}-chunk-"
                    ));
                    source["audioSegmentMode"] = json!("absolute");
                }
            }
            if matches!(
                row.get::<String, _>("mime").as_str(),
                "video/mp4" | "video/webm" | "video/x-matroska"
            ) {
                // Try the original timeline with explicit independent audio. MKV clients
                // that reject real decoding retain an atomic prepared-video fallback.
                if let Some(source) = sources.last_mut() {
                    source["url"] = json!(relay_url);
                    source["mime"] = source_obj["mime"].clone();
                    source["prepareVideo"] = json!(false);
                    if source_obj["mime"] == "video/x-matroska" {
                        source["preparedVideoUrl"] =
                            json!(format!("/api/v1/assets/{asset_id}/prepared/video"));
                    }

                    source["directUrl"] = source_obj["directUrl"].clone();
                    source["tokenExpiresAt"] = source_obj["tokenExpiresAt"].clone();
                }
            }
        }
        for track in probe.streams.iter().filter(|t| {
            t.codec_type.as_deref() == Some("subtitle")
                && library_options.embedded_subtitle_allowed(t.codec_name.as_deref())
                // Even allow_all only offers formats that preparation supports.
                && matches!(
                    t.codec_name.as_deref(),
                    Some("subrip" | "webvtt" | "ass" | "ssa" | "mov_text")
                )
        }) {
            let language = track
                .tags
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("und");
            let language = match language {
                "fra" | "fre" => "fr",
                "eng" => "en",
                other => other,
            };
            let forced = track
                .details
                .get("disposition")
                .and_then(|d| d.get("forced"))
                .and_then(Value::as_i64)
                == Some(1);
            subtitles.push(json!({ "id": format!("internal-{}", track.index), "language": language,
                "label": format!("{}{} · intégrés (texte)", language.to_uppercase(), if forced { " forcés" } else { "" }),
                "forced": forced, "sourceFormat": track.codec_name,
                "url": format!("/api/v1/assets/{asset_id}/prepared/subtitle-{}", track.index),
                "assUrl": matches!(track.codec_name.as_deref(), Some("ass" | "ssa")).then(|| format!("/api/v1/assets/{asset_id}/prepared/subtitle-ass-{}",track.index)) }));
        }
    }
    for (index, track) in audios.iter().enumerate() {
        let mut source = source_obj.clone();
        // External audio is authorized through Core; the existing video can remain direct.
        source["id"] = json!(format!("{asset_id}-audio-{index}"));
        source["audioTrackId"] = json!(format!("external-{index}"));
        source["language"] = json!(track.language);
        source["externalAudioUrl"] = json!(format!(
            "/api/v1/assets/{asset_id}/audio/{}",
            audio_operational_id(asset_id, track)
        ));
        source["audioOffsetSeconds"] = json!(track.offset_seconds);
        source["tracks"] = json!(track.probe.streams);
        // External audio shares the original video. Expose the same bounded
        // AV1 packet-copy fallback as an internal audio selection, so a failed
        // browser seek/decode does not strand the external track on the
        // original container. Only advertise it with a real codec config and
        // finite duration; the player still tests browser MSE support.
        if let (Some(video), Some(duration)) = (video_source, duration)
            && let Some(mime) = av1_video_mime(video)
            && matches!(
                row.get::<String, _>("mime").as_str(),
                "video/mp4" | "video/webm" | "video/x-matroska"
            )
        {
            source["durationSeconds"] = json!(duration);
            source["videoMime"] = json!(mime);
            source["videoStreamUrl"] = json!(format!(
                "/api/v1/assets/{asset_id}/prepared/video-stream-{}-",
                video.index
            ));
            if source_obj["mime"] == "video/x-matroska" {
                source["preparedVideoUrl"] =
                    json!(format!("/api/v1/assets/{asset_id}/prepared/video"));
            }
        }
        sources.push(source);
    }
    let total_dur = probe
        .format
        .get("duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    let chapter_list = valid_chapters(&probe.chapters, total_dur);
    let mut intro_segment: Option<serde_json::Value> = None;
    let mut credits_segment: Option<serde_json::Value> = None;
    let mut recap_segment: Option<serde_json::Value> = None;
    let mut preview_segment: Option<serde_json::Value> = None;

    for (i, (start, title)) in chapter_list.iter().enumerate() {
        if initial_work_kind != "episode" {
            break;
        }
        let clean = title
            .to_lowercase()
            .replace(['é', 'è', 'ê', 'ë'], "e")
            .replace(['à', 'â'], "a");
        let next_start = if i + 1 < chapter_list.len() {
            chapter_list[i + 1].0
        } else {
            total_dur.unwrap_or(*start)
        };
        let dur = next_start - start;
        if matches!(
            clean.trim(),
            "intro"
                | "introduction"
                | "opening"
                | "opening credits"
                | "op"
                | "generique debut"
                | "generique d'ouverture"
        ) && (15.0..=150.0).contains(&dur)
            && intro_segment.is_none()
        {
            intro_segment = Some(json!({ "start": start, "end": next_start }));
        } else if matches!(
            clean.trim(),
            "credits"
                | "end credits"
                | "ending"
                | "ending credits"
                | "ed"
                | "generique fin"
                | "generique de fin"
        ) && (10.0..=300.0).contains(&dur)
            && credits_segment.is_none()
        {
            credits_segment = Some(json!({ "start": start, "end": next_start }));
        }
    }

    // A completed marker task is reusable only for this physical asset and
    // the exact inventory revision that produced it. Other copies need their
    // own timeline validation before sharing any marker.
    let stored_markers: Vec<(String,f64,f64,f64)> = query_as(
        "SELECT m.marker_type,m.start_seconds,m.end_seconds,m.confidence FROM automatic_timeline_markers m \
         JOIN automation_observations obs ON obs.asset_id=m.asset_id AND obs.fingerprint=m.source_fingerprint \
         JOIN extensions ext ON ext.id=m.extension_id AND ext.enabled=1 AND ext.generation=m.generation \
         JOIN extension_publishers publisher ON publisher.id=ext.publisher_id AND publisher.active=1 \
         WHERE m.asset_id=? AND \
         (ext.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants grant_row WHERE grant_row.extension_id=ext.id AND (grant_row.library_id=? OR EXISTS(SELECT 1 FROM library_folders folder WHERE folder.library_id=? AND folder.parent_id=grant_row.library_id AND folder.active=1)))) \
         ORDER BY m.confidence DESC LIMIT 8",
    )
    .bind(asset_id)
    .bind(&library_id)
    .bind(&library_id)
    .fetch_all(&store.pool)
    .await?;
    if !stored_markers.is_empty() {
        let mut seen_kinds = std::collections::HashSet::new();
        for (kind, start, end, confidence) in stored_markers {
            if !seen_kinds.insert(kind.clone()) {
                continue;
            }
            match kind.as_str() {
                "intro" => intro_segment = None,
                "credits" => credits_segment = None,
                "recap" => recap_segment = None,
                "preview" => preview_segment = None,
                _ => continue,
            }
            if !start.is_finite()
                || !end.is_finite()
                || !confidence.is_finite()
                || start < 0.0
                || end <= start
                || confidence < 0.9
                || end > total_dur.unwrap_or(0.0)
            {
                continue;
            }
            match kind.as_str() {
                "intro" if intro_segment.is_none() => {
                    intro_segment = Some(json!({"start":start,"end":end}))
                }
                "credits" if credits_segment.is_none() => {
                    credits_segment = Some(json!({"start":start,"end":end}))
                }
                "recap" if recap_segment.is_none() => {
                    recap_segment = Some(json!({"start":start,"end":end}))
                }
                "preview" if preview_segment.is_none() => {
                    preview_segment = Some(json!({"start":start,"end":end}))
                }
                _ => {}
            }
        }
    }
    let manual: Vec<(String, Option<f64>, Option<f64>, i64)> = query_as(
        "SELECT marker_type,start_seconds,end_seconds,enabled FROM manual_timeline_markers WHERE asset_id=?",
    ).bind(asset_id).fetch_all(&store.pool).await?;
    for (kind, start, end, enabled) in manual {
        let value = if enabled == 1 {
            match (start, end) {
                (Some(start), Some(end))
                    if start.is_finite()
                        && end.is_finite()
                        && start >= 0.0
                        && end > start
                        && end <= total_dur.unwrap_or(0.0) =>
                {
                    Some(json!({"start":start,"end":end}))
                }
                _ => None,
            }
        } else {
            None
        };
        match kind.as_str() {
            "intro" => intro_segment = value,
            "credits" => credits_segment = value,
            "recap" => recap_segment = value,
            "preview" => preview_segment = value,
            _ => {}
        }
    }

    let mut desc = json!({
        "id": asset_id,
        "title": row.get::<String,_>("title"),
        "workId": initial_work_id,
        "synthetic": false,
        "nodeId": node_id,
        "nodeStatus": node_status,
        "connectionMode": conn_mode,
        "score": node_score,
        "available": node_status == "connected",
        "fallbackMediaIds": fallback_media_ids,
        "sources": sources,
        "subtitles": subtitles,
        "chapters": chapter_list.iter().map(|(start, title)| json!({"start": start, "title": title})).collect::<Vec<_>>(),
        "intro": intro_segment,
        "credits": credits_segment,
        "recap": recap_segment,
        "preview": preview_segment
    });
    if let Some(orig) = failover_from {
        desc["requestedId"] = json!(orig.clone());
        desc["failoverFrom"] = json!(orig);
    }
    Ok(Some(desc))
}

/// Authorized, inventory-resolved local preparation; unsupported remote packaging is explicit.
// RFC 6381 derives from the actual av1C record, including tier and bit depth;
// never invent a level from resolution or a generic canPlayType result.
fn av1_video_mime(track: &media_node::probe::Track) -> Option<String> {
    if track.codec_type.as_deref() != Some("video") || track.codec_name.as_deref() != Some("av1") {
        return None;
    }
    let dump = track.details.get("extradata")?.as_str()?;
    let line = dump.lines().find(|line| line.starts_with("00000000:"))?;
    let hex = line.split_once(':')?.1.trim_start().split("  ").next()?;
    let bytes = hex
        .split_whitespace()
        .flat_map(|word| word.as_bytes().chunks(2))
        .take(4)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<_>>>()?;
    if bytes.len() != 4 || bytes[0] != 0x81 {
        return None;
    }
    let profile = bytes[1] >> 5;
    let level = bytes[1] & 31;
    if profile > 2 || level > 23 || bytes[2] & 0x20 != 0 && bytes[2] & 0x40 == 0 {
        return None;
    }
    let tier = if bytes[2] & 0x80 != 0 { 'H' } else { 'M' };
    let depth = if bytes[2] & 0x40 == 0 {
        8
    } else if bytes[2] & 0x20 != 0 {
        12
    } else {
        10
    };
    Some(format!(
        "video/mp4; codecs=\"av01.{profile}.{level:02}{tier}.{depth:02}\""
    ))
}

pub(super) fn native_opus_track_id(probe: &Probe, track: &media_node::probe::Track) -> Option<u32> {
    if track.codec_type.as_deref() != Some("audio") || track.codec_name.as_deref() != Some("opus") {
        return None;
    }
    let id = track.details.get("id")?.as_str()?;
    let id = if let Some(hex) = id.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        id.parse::<u32>().ok()?
    };
    probe
        .format
        .get("native_opus_tracks_v1")?
        .as_array()?
        .iter()
        .any(|value| value.as_u64() == Some(u64::from(id)))
        .then_some(id)
}

pub(crate) fn audio_operational_id(
    asset_id: &str,
    track: &media_contracts::inventory::ExternalAudio,
) -> String {
    let identity = json!([track.relative_path, track.size, track.modified_ns]).to_string();
    let hash = media_contracts::protocol::sha256_hex(identity.as_bytes());
    format!("sidecar-{asset_id}-{}", &hash[..32])
}

fn subtitle_descriptors(asset_id: &str, text: Option<&str>) -> Result<Vec<Value>, sqlx::Error> {
    let tracks: Vec<media_contracts::inventory::ExternalSubtitle> =
        serde_json::from_str(text.unwrap_or("[]")).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    Ok(tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            json!({
                "id": format!("external-{index}"), "language": track.language, "label": track.label,
                "forced": track.forced, "sdh": track.sdh, "sourceFormat": track.source_format,
                "url": format!("/api/v1/assets/{asset_id}/subtitles/{index}"),
                "assUrl": track.ass.as_ref().map(|_| format!("/api/v1/assets/{asset_id}/subtitles/{index}?format=ass"))
            })
        })
        .collect())
}

// Probe output is external data: chapter ordering and timestamps cannot be
// assumed valid, especially when their boundaries drive skip controls.
pub(crate) fn valid_chapters(chapters: &[Value], duration: Option<f64>) -> Vec<(f64, String)> {
    let mut valid: Vec<_> = chapters
        .iter()
        .take(1000)
        .filter_map(|chapter| {
            let start = chapter["start_time"].as_str()?.parse::<f64>().ok()?;
            if !start.is_finite()
                || start < 0.0
                || duration.is_some_and(|duration| start >= duration)
            {
                return None;
            }
            Some((
                start,
                chapter["tags"]["title"]
                    .as_str()
                    .unwrap_or("Chapitre")
                    .to_string(),
            ))
        })
        .collect();
    valid.sort_by(|left, right| left.0.total_cmp(&right.0));
    valid
}

#[cfg(test)]
mod tests {
    #[test]
    fn chapter_boundaries_are_sorted_and_confined_to_the_media() {
        let chapters = serde_json::json!([
            {"start_time":"60", "tags":{"title":"Episode"}},
            {"start_time":"10", "tags":{"title":"Intro"}},
            {"start_time":"1200", "tags":{"title":"Invalid end"}},
            {"start_time":"NaN"},
            {"start_time":"-1"}
        ]);
        assert_eq!(
            super::valid_chapters(chapters.as_array().unwrap(), Some(1200.0)),
            vec![(10.0, "Intro".into()), (60.0, "Episode".into())]
        );
    }

    #[test]
    fn packet_stream_descriptors_use_validated_track_ids_and_real_av1_configuration() {
        let probe: super::Probe = serde_json::from_value(serde_json::json!({
            "streams": [{"index": 1, "codec_type":"audio", "codec_name":"opus", "id":"0x2"}],
            "format": {"native_opus_tracks_v1":[2]}
        }))
        .unwrap();
        assert_eq!(
            super::native_opus_track_id(&probe, &probe.streams[0]),
            Some(2)
        );
        let mut unsupported = probe.clone();
        unsupported.format.clear();
        assert_eq!(
            super::native_opus_track_id(&unsupported, &unsupported.streams[0]),
            None
        );
        let video = serde_json::from_value(
            serde_json::json!({"index":0,"codec_type":"video", "codec_name":"av1",
            "extradata":"\n00000000: 8108 4c00                                ..L.\n"}),
        )
        .unwrap();
        assert_eq!(
            super::av1_video_mime(&video).as_deref(),
            Some("video/mp4; codecs=\"av01.0.08M.10\"")
        );
        let guessed = serde_json::from_value(serde_json::json!({"index":0,"codec_type":"video", "codec_name":"av1", "level":8,"pix_fmt":"yuv420p10le"})).unwrap();
        assert_eq!(super::av1_video_mime(&guessed), None);
    }
}
