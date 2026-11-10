//! Playback descriptors, source selection and validated track identities.

use super::decode;
use super::equivalence::{SourceEquivalence, SourceFacts, classify};
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

/// An automatic timeline marker reaches the player when its bounds fit the file
/// and its confidence clears the library floor (`marker_min_confidence`).
pub(crate) fn usable_marker(
    start: f64,
    end: f64,
    confidence: f64,
    min_confidence: f64,
    duration: Option<f64>,
) -> bool {
    start.is_finite()
        && end.is_finite()
        && confidence.is_finite()
        && start >= 0.0
        && end > start
        && confidence >= min_confidence
        && end <= duration.unwrap_or(0.0)
}

pub async fn descriptor(
    store: &IdentityStore,
    asset_id: &str,
    session_hash: &str,
) -> Result<Option<Value>, sqlx::Error> {
    descriptor_for_user(store, asset_id, session_hash, None, None).await
}

/// Descriptor of a file, or of the first playable file of a work: a "watch together"
/// session points at a film, an episode or a series, never at one of its files.
pub async fn descriptor_for_user(
    store: &IdentityStore,
    id: &str,
    session_hash: &str,
    user_id: Option<&str>,
    visible_libraries: Option<&[String]>,
) -> Result<Option<Value>, sqlx::Error> {
    if let Some(desc) =
        descriptor_for_asset(store, id, session_hash, user_id, visible_libraries).await?
    {
        return Ok(Some(desc));
    }
    let Some(asset_id) = playable_asset_for_work(store, id).await? else {
        return Ok(None);
    };
    let mut desc =
        descriptor_for_asset(store, &asset_id, session_hash, user_id, visible_libraries).await?;
    if let Some(desc) = desc.as_mut()
        && desc.get("requestedId").is_none()
    {
        desc["requestedId"] = json!(id);
    }
    Ok(desc)
}

/// First playable file of a film or an episode (by work id or episode id), or of a
/// series: its first episode with a file, specials last.
pub(crate) async fn playable_asset_for_work(
    store: &IdentityStore,
    id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let own: Option<String> = query_scalar(
        "SELECT a.id FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         JOIN libraries l ON l.id=s.library_id \
         WHERE a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 \
           AND e.work_id=COALESCE((SELECT work_id FROM inventory_episodes WHERE id=?), ?) \
         ORDER BY a.id LIMIT 1",
    )
    .bind(id)
    .bind(id)
    .fetch_optional(&store.pool)
    .await?;
    if own.is_some() {
        return Ok(own);
    }
    query_scalar(
        "SELECT a.id FROM inventory_seasons se \
         JOIN inventory_episodes ep ON ep.season_id=se.id \
         JOIN inventory_editions e ON e.work_id=ep.work_id \
         JOIN inventory_versions v ON v.edition_id=e.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=s.library_id \
         WHERE se.series_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 \
         ORDER BY CASE WHEN se.season_number=0 THEN 1 ELSE 0 END, se.season_number, ep.episode_number, a.id \
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&store.pool)
    .await
}

async fn descriptor_for_asset(
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
            "SELECT a.id AS asset_id, a.mime, a.library_id, a.size AS size, a.probe_json AS probe_json, fp.fingerprint AS fingerprint, l.node_id, w.id AS work_id \
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
             LEFT JOIN inventory_asset_fingerprints fp ON fp.asset_id=a.id \
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
            "SELECT a.id AS asset_id, a.mime, a.library_id, a.size AS size, a.probe_json AS probe_json, fp.fingerprint AS fingerprint, l.node_id, w.id AS work_id \
             FROM inventory_editions e \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources s ON s.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=s.id \
             LEFT JOIN inventory_asset_fingerprints fp ON fp.asset_id=a.id \
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

    // Facts about the copy the viewer asked for, used to decide which other copies may stand in
    // for it. A copy is a candidate only when it is provably the same content (see `equivalence`).
    let requested_library_id: String = initial_row.get("library_id");
    let requested_facts = {
        let stored: Option<(i64, Option<String>)> = query_as(
            "SELECT a.size, fp.fingerprint FROM inventory_assets a \
             LEFT JOIN inventory_asset_fingerprints fp ON fp.asset_id=a.id WHERE a.id=?",
        )
        .bind(asset_id)
        .fetch_optional(&store.pool)
        .await?;
        let (size, fingerprint) = stored.unwrap_or((0, None));
        SourceFacts {
            size,
            fingerprint,
            duration_secs: SourceFacts::duration_from_probe_json(
                &initial_row.get::<String, _>("probe_json"),
            ),
        }
    };

    struct Candidate {
        asset_id: String,
        node_id: String,
        status: String,
        mode: String,
        score: i64,
        equivalence: SourceEquivalence,
        mime: String,
    }
    let mut ranked_siblings: Vec<Candidate> = Vec::new();
    for srow in sibling_rows {
        let cand_asset_id: String = srow.get("asset_id");
        let cand_lib_id: String = srow.get("library_id");
        let cand_node_id: String = srow.get("node_id");
        let cand_mime: String = srow.get("mime");
        let cand_work_id: String = srow.get("work_id");

        // Permissions are never merged across Nodes. A caller that did not resolve the viewer's
        // libraries only ever sees copies from the requested asset's own library.
        let library_allowed = match visible_libraries {
            Some(visible) => visible.contains(&cand_lib_id),
            None => cand_lib_id == requested_library_id,
        };
        if !library_allowed {
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
        if ranked_siblings
            .iter()
            .any(|candidate| candidate.asset_id == cand_asset_id)
        {
            continue;
        }

        let equivalence = if cand_asset_id == asset_id {
            SourceEquivalence::Identical
        } else {
            let mut candidate_facts = SourceFacts {
                size: srow.get("size"),
                fingerprint: srow.get("fingerprint"),
                duration_secs: None,
            };
            // The stored fingerprint settles identity without touching the probe document;
            // only otherwise is the (larger) probe read, and then only for its duration.
            match classify(&requested_facts, &candidate_facts) {
                Some(equivalence) => equivalence,
                None => {
                    candidate_facts.duration_secs =
                        SourceFacts::duration_from_probe_json(&srow.get::<String, _>("probe_json"));
                    match classify(&requested_facts, &candidate_facts) {
                        Some(equivalence) => equivalence,
                        // Same work, but not provably the same content: never a substitute.
                        None => continue,
                    }
                }
            }
        };

        let (cand_status, cand_mode, cand_score) = match store.node_registry.get(&cand_node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&cand_mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };
        ranked_siblings.push(Candidate {
            asset_id: cand_asset_id,
            node_id: cand_node_id,
            status: cand_status,
            mode: cand_mode,
            score: cand_score,
            equivalence,
            mime: cand_mime,
        });
    }

    // Best score first; an identical file wins a tie against a merely same-runtime encode.
    ranked_siblings.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.equivalence.cmp(&a.equivalence))
            .then_with(|| a.asset_id.cmp(&b.asset_id))
    });

    // Failover is decided here, *before* playback starts, from the Core's view of the Nodes
    // (heartbeats, staleness, recent failures). It is not a mid-stream switch: once a browser is
    // reading a file, a Node that dies interrupts that response and the player must request
    // again, which lands on this selection with the new state.
    let mut active_row = initial_row;
    let mut effective_asset_id = asset_id.to_owned();
    let mut failover_from: Option<String> = None;
    let mut node_status = initial_node_status;
    let mut conn_mode = initial_conn_mode;
    let mut node_score = initial_score;

    if node_status != "connected"
        && let Some(best_alt) = ranked_siblings
            .iter()
            .find(|candidate| candidate.asset_id != asset_id && candidate.status == "connected")
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
        .bind(&best_alt.asset_id)
        .fetch_optional(&store.pool)
        .await?
    {
        effective_asset_id = best_alt.asset_id.clone();
        failover_from = Some(asset_id.to_owned());
        node_status = best_alt.status.clone();
        conn_mode = best_alt.mode.clone();
        node_score = best_alt.score;
        active_row = alt_row;
    }

    let fallback_media_ids: Vec<String> = ranked_siblings
        .iter()
        .filter(|candidate| {
            candidate.asset_id != effective_asset_id && candidate.status == "connected"
        })
        .map(|candidate| candidate.asset_id.clone())
        .collect();
    // Equivalent copies the viewer may choose explicitly, whatever their Node's state, so a
    // player can offer "play from another Node". Selecting one simply requests its descriptor,
    // which every route authorizes again for that asset's own library.
    let alternatives: Vec<Value> = ranked_siblings
        .iter()
        .filter(|candidate| candidate.asset_id != effective_asset_id)
        .map(|candidate| {
            let node_name = store
                .node_registry
                .get(&candidate.node_id)
                .map(|entry| entry.record.name)
                .unwrap_or_default();
            json!({
                "id": candidate.asset_id,
                "nodeId": candidate.node_id,
                "nodeName": node_name,
                "nodeStatus": candidate.status,
                "connectionMode": candidate.mode,
                "score": candidate.score,
                "mime": candidate.mime,
                "equivalence": candidate.equivalence.as_str(),
            })
        })
        .collect();

    let row = active_row;
    let asset_id = effective_asset_id.as_str();
    let report_tracks: Vec<(String, i64)> =
        query_as("SELECT id,stream_index FROM inventory_tracks WHERE asset_id=?")
            .bind(asset_id)
            .fetch_all(store.pool())
            .await?;
    let probe = decode(row.get("probe_json"))?;
    let library_id: String = row.get("library_id");
    let node_id: String = row.get("node_id");
    let library_options = playback_library_options(store, &library_id).await?;
    // Fallback transcoding is a server choice, audio and video apart; an unreadable
    // setting offers none.
    let (audio_transcoding, video_transcoding) =
        crate::server_settings::load(store)
            .await
            .map_or((false, false), |settings| {
                (
                    settings.audio_transcoding_enabled,
                    settings.video_transcoding_enabled,
                )
            });
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
    let original_audio: Vec<_> = probe
        .streams
        .iter()
        .filter(|track| track.codec_type.as_deref() == Some("audio"))
        .collect();
    if original_audio.len() == 1 {
        source_obj["reportTrackId"] = json!(
            report_tracks
                .iter()
                .find(|(_, index)| *index == i64::from(original_audio[0].index))
                .map(|(id, _)| id)
        );
    }
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
                "reportTrackId": report_tracks.iter().find(|(_,index)|*index==i64::from(track.index)).map(|(id,_)|id),
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
    }
    for track in probe.streams.iter().filter(|t| {
            t.codec_type.as_deref() == Some("subtitle")
                && library_options.embedded_subtitle_allowed(t.codec_name.as_deref())
                // Even allow_all only offers formats that preparation supports.
                && (matches!(
                    t.codec_name.as_deref(),
                    Some("subrip" | "webvtt" | "ass" | "ssa" | "mov_text" | "hdmv_pgs_subtitle")
                ) || video_transcoding && matches!(t.codec_name.as_deref(), Some("dvd_subtitle" | "dvb_subtitle")))
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
            let sdh = track
                .details
                .get("disposition")
                .and_then(|d| d.get("hearing_impaired"))
                .and_then(Value::as_i64)
                == Some(1)
                || track
                    .tags
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| {
                        let title = title.to_lowercase();
                        ["sdh", "malentendant", "sourds", "hearing", "cc"]
                            .iter()
                            .any(|word| title.split(|c: char| !c.is_alphanumeric()).any(|part| part == *word || (word.len() > 3 && part.starts_with(word))))
                    });
            let image = match track.codec_name.as_deref() {
                Some("hdmv_pgs_subtitle") => Some("pgs"),
                Some("dvd_subtitle" | "dvb_subtitle") => Some("burn"),
                _ => None,
            };
            let url = match image {
                Some("pgs") => format!("/api/v1/assets/{asset_id}/prepared/subtitle-pgs-{}", track.index),
                Some(_) => String::new(),
                None => format!("/api/v1/assets/{asset_id}/prepared/subtitle-{}", track.index),
            };
            subtitles.push(json!({ "id": format!("internal-{}", track.index), "language": language,
                "reportTrackId":report_tracks.iter().find(|(_,index)|*index==i64::from(track.index)).map(|(id,_)|id),
                "label": format!("{}{}{} · intégrés ({})", language.to_uppercase(), if forced { " forcés" } else { "" }, if sdh { " SDH" } else { "" }, if image.is_some() { "image" } else { "texte" }),
                "forced": forced, "sdh": sdh, "sourceFormat": track.codec_name,
                "url": url,
                "image": image,
                "burnIndex": (image.is_some() && video_transcoding).then_some(track.index),
                "assUrl": matches!(track.codec_name.as_deref(), Some("ass" | "ssa")).then(|| format!("/api/v1/assets/{asset_id}/prepared/subtitle-ass-{}",track.index)) }));
    }
    // Every other embedded audio track becomes selectable through fallback transcoding
    // (video copied when the browser decodes it): direct play only reaches the default one.
    if audio_transcoding && original_audio.len() > 1 {
        // The track a browser plays from the original (flagged default, else the first)
        // stays direct play: its source is the original itself, with a known language.
        let browser_default = original_audio
            .iter()
            .find(|track| {
                track
                    .details
                    .get("disposition")
                    .and_then(|d| d.get("default"))
                    .and_then(Value::as_i64)
                    == Some(1)
            })
            .or(original_audio.first())
            .map(|track| track.index);
        for track in &original_audio {
            let id = format!("internal-{}", track.index);
            if sources
                .iter()
                .any(|source| source["audioTrackId"] == id.as_str())
            {
                continue;
            }
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
            if browser_default == Some(track.index) {
                let mut alias = source_obj.clone();
                alias["id"] = json!(format!("{asset_id}-default-{}", track.index));
                alias["audioTrackId"] = json!(id);
                alias["language"] = json!(language);
                alias["reportTrackId"] = json!(
                    report_tracks
                        .iter()
                        .find(|(_, index)| *index == i64::from(track.index))
                        .map(|(id, _)| id)
                );
                alias["tracks"] = json!([track]);
                sources.push(alias);
                continue;
            }
            sources.push(json!({
                "id": format!("{asset_id}-transcode-{}", track.index),
                "audioTrackId": id, "language": language,
                "reportTrackId": report_tracks.iter().find(|(_,index)|*index==i64::from(track.index)).map(|(id,_)|id),
                "url": relay_url, "relayUrl": relay_url, "mime": source_obj["mime"].clone(),
                "nodeId": node_id, "nodeStatus": node_status,
                "label": "interne", "tracks": [track], "transcodeOnly": true,
                "durationSeconds": duration
            }));
        }
    }
    for (index, track) in audios.iter().enumerate() {
        let mut source = source_obj.clone();
        // External audio is authorized through Core; the existing video can remain direct.
        source["id"] = json!(format!("{asset_id}-audio-{index}"));
        source["audioTrackId"] = json!(format!("external-{index}"));
        source["reportTrackId"] = json!(audio_operational_id(asset_id, track));
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
    // The most confident detection of each kind replaces the container-chapter
    // fallback, but only when it clears the library's confidence floor: a weak
    // detection must not erase a usable chapter marker.
    let min_confidence = f64::from(library_options.marker_min_confidence) / 100.0;
    let mut seen_kinds = std::collections::HashSet::new();
    for (kind, start, end, confidence) in stored_markers {
        if !usable_marker(start, end, confidence, min_confidence, total_dur)
            || !seen_kinds.insert(kind.clone())
        {
            continue;
        }
        let value = Some(json!({"start":start,"end":end}));
        match kind.as_str() {
            "intro" => intro_segment = value,
            "credits" => credits_segment = value,
            "recap" => recap_segment = value,
            "preview" => preview_segment = value,
            _ => {}
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
        "alternatives": alternatives,
        "sources": sources,
        "subtitles": subtitles,
        "chapters": chapter_list.iter().map(|(start, title)| json!({"start": start, "title": title})).collect::<Vec<_>>(),
        "intro": intro_segment,
        "credits": credits_segment,
        "recap": recap_segment,
        "preview": preview_segment
    });
    let picture = probe.streams.iter().find(|track| {
        track.codec_type.as_deref() == Some("video")
            && track
                .details
                .get("disposition")
                .and_then(|d| d.get("attached_pic"))
                .and_then(Value::as_i64)
                != Some(1)
    });
    if (audio_transcoding || video_transcoding)
        && let (Some(video), Some(duration)) = (picture, total_dur)
    {
        let bitrate = probe
            .format
            .get("bit_rate")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<u64>().ok())
            .map(|bits| bits / 1000);
        desc["transcode"] = json!({
            "url": format!("/api/v1/assets/{asset_id}/prepared/transcode-"),
            "videoIndex": video.index,
            "videoCodec": video.codec_name,
            "width": video.details.get("width"),
            "height": video.details.get("height"),
            "bitrateKbps": bitrate,
            "durationSeconds": duration,
            "dynamicRange": super::resources::video_dynamic_range(video),
            "copyVideoMime": av1_video_mime(video).or_else(|| h264_video_mime(video)),
            "audio": audio_transcoding,
            "video": video_transcoding,
        });
    }
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

/// RFC 6381 `avc1` string from the probed profile and level, for copied H.264.
fn h264_video_mime(track: &media_node::probe::Track) -> Option<String> {
    if track.codec_name.as_deref() != Some("h264") {
        return None;
    }
    let profile = match track.details.get("profile")?.as_str()? {
        "Constrained Baseline" => "42E0",
        "Baseline" => "4200",
        "Main" => "4D40",
        "High" => "6400",
        _ => return None,
    };
    let level = track
        .details
        .get("level")?
        .as_u64()
        .filter(|level| (10..=62).contains(level))?;
    Some(format!("video/mp4; codecs=\"avc1.{profile}{level:02X}\""))
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
                "reportTrackId":subtitle_report_id(asset_id,track),
                "forced": track.forced, "sdh": track.sdh, "sourceFormat": track.source_format,
                "url": format!("/api/v1/assets/{asset_id}/subtitles/{index}"),
                "assUrl": track.ass.as_ref().map(|_| format!("/api/v1/assets/{asset_id}/subtitles/{index}?format=ass"))
            })
        })
        .collect())
}

fn subtitle_report_id(asset: &str, track: &media_contracts::inventory::ExternalSubtitle) -> String {
    // Content and metadata identify the exact subtitle without exposing its text or a path.
    let identity = serde_json::to_vec(track).unwrap_or_default();
    let hash = media_contracts::protocol::sha256_hex(&identity);
    format!("subtitle-{asset}-{}", &hash[..32])
}

pub(crate) async fn report_track_allowed(
    store: &IdentityStore,
    asset: &str,
    kind: &str,
    reference: &str,
) -> Result<bool, sqlx::Error> {
    let internal: Option<String> =
        query_scalar("SELECT metadata_json FROM inventory_tracks WHERE id=? AND asset_id=?")
            .bind(reference)
            .bind(asset)
            .fetch_optional(store.pool())
            .await?;
    if let Some(raw) = internal {
        let track: crate::media::Track =
            serde_json::from_str(&raw).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        return Ok(
            track.codec_type.as_deref() == Some(if kind == "audio" { "audio" } else { "subtitle" })
        );
    }
    if kind == "audio" {
        let raw: Option<String> =
            query_scalar("SELECT audios_json FROM inventory_audio WHERE asset_id=?")
                .bind(asset)
                .fetch_optional(store.pool())
                .await?;
        let tracks: Vec<media_contracts::inventory::ExternalAudio> =
            serde_json::from_str(raw.as_deref().unwrap_or("[]"))
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        return Ok(tracks
            .iter()
            .any(|track| audio_operational_id(asset, track) == reference));
    }
    if let Some(id) = reference.strip_prefix("downloaded-") {
        let count:i64=query_scalar("SELECT COUNT(*) FROM subtitle_downloads d JOIN inventory_assets a ON a.id=d.asset_id WHERE d.id=? AND d.asset_id=? AND d.status='completed' AND d.fingerprint=CAST(a.size AS TEXT)||':'||a.modified_ns").bind(id).bind(asset).fetch_one(store.pool()).await?;
        return Ok(count == 1);
    }
    let raw: Option<String> =
        query_scalar("SELECT subtitles_json FROM inventory_subtitles WHERE asset_id=?")
            .bind(asset)
            .fetch_optional(store.pool())
            .await?;
    let tracks: Vec<media_contracts::inventory::ExternalSubtitle> =
        serde_json::from_str(raw.as_deref().unwrap_or("[]"))
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    Ok(tracks
        .iter()
        .any(|track| subtitle_report_id(asset, track) == reference))
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
