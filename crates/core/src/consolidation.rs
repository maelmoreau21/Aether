//! Global catalogue consolidation, edition/version distinction, probable match detection,
//! and reversible merge/split workflows.
use crate::{
    database::{query, query_as, query_scalar},
    identity::IdentityStore,
};
use media_contracts::media::Probe;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{AnyConnection, Row};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditionSpec {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSpec {
    pub key: String,
    pub label: String,
    pub max_height: i64,
    pub codecs: Vec<String>,
    pub languages: Vec<String>,
}

/// Derive a normalized edition key and human-readable label from a relative file path.
/// Distinguishes Director's Cut, Extended, IMAX, Remastered, Unrated, etc. from standard theatrical releases.
pub fn derive_edition_spec(relative_path: &str) -> EditionSpec {
    let lower = relative_path.to_lowercase();
    if lower.contains("director's cut")
        || lower.contains("directors cut")
        || lower.contains("director.cut")
        || lower.contains("directors.cut")
        || lower.contains("version longue réalisateur")
    {
        EditionSpec {
            key: "directors-cut".into(),
            label: "Director’s Cut".into(),
        }
    } else if lower.contains("extended")
        || lower.contains("version longue")
        || lower.contains("uncut")
        || lower.contains("unrated")
    {
        EditionSpec {
            key: "extended".into(),
            label: "Version longue".into(),
        }
    } else if lower.contains("imax") {
        EditionSpec {
            key: "imax".into(),
            label: "Édition IMAX".into(),
        }
    } else if lower.contains("remastered") || lower.contains("remaster") {
        EditionSpec {
            key: "remastered".into(),
            label: "Édition remastérisée".into(),
        }
    } else if lower.contains("collector") || lower.contains("special edition") {
        EditionSpec {
            key: "special".into(),
            label: "Édition spéciale".into(),
        }
    } else {
        EditionSpec {
            key: "standard".into(),
            label: "Édition locale".into(),
        }
    }
}

/// Derive a normalized version key, human-readable label, and technical summary from a media probe and MIME type.
pub fn derive_version_spec(probe: &Probe, mime: &str) -> VersionSpec {
    let mut max_height: i64 = 0;
    let mut video_codec: Option<String> = None;
    let mut audio_codec: Option<String> = None;
    let mut is_10bit = false;
    let mut codecs = BTreeSet::new();
    let mut languages = BTreeSet::new();

    for stream in &probe.streams {
        let codec_name = stream
            .codec_name
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !codec_name.is_empty() {
            codecs.insert(codec_name.clone());
        }
        let codec_type = stream.codec_type.as_deref().unwrap_or("");
        if codec_type == "video" {
            let h = stream
                .details
                .get("height")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if h > max_height {
                max_height = h;
            }
            if video_codec.is_none() && !codec_name.is_empty() {
                video_codec = Some(codec_name.clone());
            }
            if let Some(pix) = stream.details.get("pix_fmt").and_then(Value::as_str)
                && (pix.contains("10") || pix.contains("p010"))
            {
                is_10bit = true;
            }
        } else if codec_type == "audio" && audio_codec.is_none() && !codec_name.is_empty() {
            audio_codec = Some(codec_name.clone());
        }
        if let Some(lang) = stream.tags.get("language").and_then(Value::as_str) {
            let l = lang.trim().to_ascii_lowercase();
            if !l.is_empty() {
                languages.insert(l);
            }
        }
    }

    let res_tag = if max_height >= 2160 {
        Some("4K")
    } else if max_height >= 1440 {
        Some("1440p")
    } else if max_height >= 1080 {
        Some("1080p")
    } else if max_height >= 720 {
        Some("720p")
    } else if max_height > 0 {
        Some("SD")
    } else {
        None
    };

    let container = match mime {
        "video/mp4" => "MP4",
        "video/webm" => "WebM",
        "video/x-matroska" => "MKV",
        _ => "",
    };

    if res_tag.is_none() && video_codec.is_none() && audio_codec.is_none() {
        return VersionSpec {
            key: "default".into(),
            label: "Version locale".into(),
            max_height,
            codecs: codecs.into_iter().collect(),
            languages: languages.into_iter().collect(),
        };
    }

    let mut parts = Vec::new();
    if let Some(r) = res_tag {
        parts.push(r.to_owned());
    }
    if let Some(vc) = &video_codec {
        parts.push(vc.to_ascii_uppercase());
    }
    if is_10bit {
        parts.push("10-bit".to_owned());
    }
    if let Some(ac) = &audio_codec {
        parts.push(ac.to_ascii_uppercase());
    }
    if !container.is_empty() {
        parts.push(container.to_owned());
    }

    let label = if parts.is_empty() {
        "Version locale".into()
    } else {
        parts.join(" · ")
    };
    let key = format!(
        "{}:{}:{}:{}:{}",
        res_tag.unwrap_or("unk"),
        video_codec.as_deref().unwrap_or("unk"),
        if is_10bit { "10b" } else { "8b" },
        audio_codec.as_deref().unwrap_or("unk"),
        container.to_ascii_lowercase()
    );

    VersionSpec {
        key,
        label,
        max_height,
        codecs: codecs.into_iter().collect(),
        languages: languages.into_iter().collect(),
    }
}

/// Normalize external provider keys and values (`tmdb`, `imdb`, `tvdb`, `eidr`, etc.).
/// Rejects empty, malformed, or placeholder values so false duplicates are never linked.
pub fn normalize_external_ids(raw: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (k, v) in raw {
        let provider = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if provider.is_empty() || val.is_empty() || val == "0" || val.eq_ignore_ascii_case("null") {
            continue;
        }
        if !matches!(
            provider.as_str(),
            "tmdb" | "imdb" | "tvdb" | "eidr" | "anidb" | "musicbrainz"
        ) {
            continue;
        }
        let normalized_val = if provider == "imdb" {
            let lower = val.to_ascii_lowercase();
            let digits = lower.strip_prefix("tt").unwrap_or(&lower);
            if digits.len() < 5 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            format!("tt{digits}")
        } else if matches!(provider.as_str(), "tmdb" | "tvdb" | "anidb") {
            if !val.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            val.trim_start_matches('0').to_owned()
        } else {
            val.to_ascii_lowercase()
        };
        if !normalized_val.is_empty() {
            out.insert(provider, normalized_val);
        }
    }
    out
}

pub fn extract_external_ids_from_json(
    external_ids_json: &str,
    manual_json: Option<&str>,
) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> =
        serde_json::from_str(external_ids_json).unwrap_or_default();
    if let Some(manual) = manual_json
        && let Ok(val) = serde_json::from_str::<Value>(manual)
        && let Some(ext) = val.get("externalIds").and_then(Value::as_object)
    {
        map.clear();
        for (k, v) in ext {
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_owned());
            }
        }
    }
    normalize_external_ids(&map)
}

/// Normalize a work title for probable-match comparison (strips punctuation, collapses whitespace, Unicode lowercase).
pub fn normalize_match_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_space = true;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_owned()
}

/// Synchronize normalized summary columns and external IDs for a work inside an active transaction.
pub async fn sync_work_summary_tx(
    tx: &mut AnyConnection,
    work_id: &str,
) -> Result<(), sqlx::Error> {
    let row = query(
        "SELECT kind, title, title_sort, year, external_ids_json, editorial_json, manual_json \
         FROM inventory_works WHERE id = ?",
    )
    .bind(work_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let kind: String = row.get("kind");
    let title: String = row.get("title");
    let title_sort: String = row.get("title_sort");
    let year: Option<i64> = row.get("year");
    let external_ids_json: String = row.get("external_ids_json");
    let editorial_json: String = row.get("editorial_json");
    let manual_json: Option<String> = row.get("manual_json");

    let manual_val: Value = manual_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    let editorial_val: Value = serde_json::from_str(&editorial_json).unwrap_or(Value::Null);

    let display_title = manual_val
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&title);
    let display_title_sort = if manual_val.get("title").is_some() {
        display_title.to_lowercase()
    } else if !title_sort.is_empty() {
        title_sort.clone()
    } else {
        title.to_lowercase()
    };

    let effective_year = if manual_val.get("year").is_some() {
        manual_val.get("year").and_then(Value::as_i64)
    } else {
        year
    };

    let mut genres = BTreeSet::new();
    let genre_source = manual_val
        .get("genres")
        .and_then(Value::as_array)
        .or_else(|| editorial_val.get("genres").and_then(Value::as_array));
    if let Some(arr) = genre_source {
        for item in arr {
            if let Some(g) = item.as_str() {
                let norm = g.trim().to_lowercase();
                if !norm.is_empty() {
                    genres.insert(norm);
                }
            }
        }
    }
    let genres_csv = if genres.is_empty() {
        "|".to_owned()
    } else {
        format!("|{}|", genres.into_iter().collect::<Vec<_>>().join("|"))
    };

    // Compute technical summary across available assets of this work (or its episodes if series)
    let track_jsons: Vec<String> = if kind == "series" {
        query_scalar(
            "SELECT t.metadata_json FROM inventory_tracks t \
             JOIN inventory_assets a ON a.id = t.asset_id \
             JOIN inventory_sources s ON s.id = a.source_id \
             JOIN inventory_versions v ON v.id = s.version_id \
             JOIN inventory_editions e ON e.id = v.edition_id \
             JOIN inventory_episodes ep ON ep.work_id = e.work_id \
             JOIN inventory_seasons sea ON sea.id = ep.season_id \
             WHERE sea.series_id = ? AND a.available = 1",
        )
        .bind(work_id)
        .fetch_all(&mut *tx)
        .await?
    } else {
        query_scalar(
            "SELECT t.metadata_json FROM inventory_tracks t \
             JOIN inventory_assets a ON a.id = t.asset_id \
             JOIN inventory_sources s ON s.id = a.source_id \
             JOIN inventory_versions v ON v.id = s.version_id \
             JOIN inventory_editions e ON e.id = v.edition_id \
             WHERE e.work_id = ? AND a.available = 1",
        )
        .bind(work_id)
        .fetch_all(&mut *tx)
        .await?
    };

    let mut max_height: i64 = 0;
    let mut codecs = BTreeSet::new();
    let mut languages = BTreeSet::new();
    for raw in track_jsons {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(c) = val.get("codec_name").and_then(Value::as_str) {
                let norm = c.trim().to_ascii_lowercase();
                if !norm.is_empty() {
                    codecs.insert(norm);
                }
            }
            if val.get("codec_type").and_then(Value::as_str) == Some("video")
                && let Some(h) = val.get("height").and_then(Value::as_i64)
                && h > max_height
            {
                max_height = h;
            }
            if let Some(l) = val
                .get("tags")
                .and_then(|t| t.get("language"))
                .and_then(Value::as_str)
            {
                let norm = l.trim().to_ascii_lowercase();
                if !norm.is_empty() {
                    languages.insert(norm);
                }
            }
        }
    }

    let codecs_csv = if codecs.is_empty() {
        ",".to_owned()
    } else {
        format!(",{},", codecs.into_iter().collect::<Vec<_>>().join(","))
    };
    let languages_csv = if languages.is_empty() {
        ",".to_owned()
    } else {
        format!(",{},", languages.into_iter().collect::<Vec<_>>().join(","))
    };

    let (has_available_asset, season_count, episode_count): (i64, i64, i64) = if kind == "series" {
        let seasons: i64 = query_scalar(
            "SELECT COUNT(DISTINCT s.season_number) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id = s.id \
             JOIN inventory_editions e ON e.work_id = ep.work_id \
             JOIN inventory_versions v ON v.edition_id = e.id \
             JOIN inventory_sources src ON src.version_id = v.id \
             JOIN inventory_assets a ON a.source_id = src.id \
             WHERE s.series_id = ? AND a.available = 1",
        )
        .bind(work_id)
        .fetch_one(&mut *tx)
        .await?;
        let episodes: i64 = query_scalar(
            "SELECT COUNT(DISTINCT ep.id) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id = s.id \
             JOIN inventory_editions e ON e.work_id = ep.work_id \
             JOIN inventory_versions v ON v.edition_id = e.id \
             JOIN inventory_sources src ON src.version_id = v.id \
             JOIN inventory_assets a ON a.source_id = src.id \
             WHERE s.series_id = ? AND a.available = 1",
        )
        .bind(work_id)
        .fetch_one(&mut *tx)
        .await?;
        (i64::from(episodes > 0), seasons, episodes)
    } else {
        let avail: i64 = query_scalar(
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM inventory_editions e \
             JOIN inventory_versions v ON v.edition_id = e.id \
             JOIN inventory_sources s ON s.version_id = v.id \
             JOIN inventory_assets a ON a.source_id = s.id \
             WHERE e.work_id = ? AND a.available = 1) THEN 1 ELSE 0 END",
        )
        .bind(work_id)
        .fetch_one(&mut *tx)
        .await?;
        (avail, 0, 0)
    };

    query(
        "UPDATE inventory_works \
         SET display_title_sort = ?, effective_year = ?, max_height = ?, \
             codecs_csv = ?, languages_csv = ?, genres_csv = ?, \
             has_available_asset = ?, season_count_cached = ?, episode_count_cached = ? \
         WHERE id = ?",
    )
    .bind(&display_title_sort)
    .bind(effective_year)
    .bind(max_height)
    .bind(&codecs_csv)
    .bind(&languages_csv)
    .bind(&genres_csv)
    .bind(has_available_asset)
    .bind(season_count)
    .bind(episode_count)
    .bind(work_id)
    .execute(&mut *tx)
    .await?;

    // Sync normalized external IDs table
    query("DELETE FROM media_work_external_ids WHERE work_id = ?")
        .bind(work_id)
        .execute(&mut *tx)
        .await?;
    let ext_ids = extract_external_ids_from_json(&external_ids_json, manual_json.as_deref());
    for (provider, ext_id) in ext_ids {
        query(
            "INSERT INTO media_work_external_ids(work_id, kind, provider, external_id) \
             VALUES (?, ?, ?, ?) ON CONFLICT(work_id, provider) DO UPDATE SET external_id = excluded.external_id, kind = excluded.kind",
        )
        .bind(work_id)
        .bind(&kind)
        .bind(&provider)
        .bind(&ext_id)
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}

/// Consolidate a work with existing works in the catalogue based on verified external IDs,
/// or record probable match candidates (ambiguities) without ever merging on title alone.
pub async fn consolidate_work_tx(tx: &mut AnyConnection, work_id: &str) -> Result<(), sqlx::Error> {
    sync_work_summary_tx(tx, work_id).await?;

    let row = query(
        "SELECT kind, title, effective_year, external_ids_json, manual_json \
         FROM inventory_works WHERE id = ?",
    )
    .bind(work_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let kind: String = row.get("kind");
    if !matches!(kind.as_str(), "film" | "series") {
        return Ok(());
    }
    let title: String = row.get("title");
    let effective_year: Option<i64> = row.get("effective_year");
    let external_ids_json: String = row.get("external_ids_json");
    let manual_json: Option<String> = row.get("manual_json");
    let ext_ids = extract_external_ids_from_json(&external_ids_json, manual_json.as_deref());

    // 1. Check for exact external ID matches of the same kind (`film` or `series`)
    if !ext_ids.is_empty() {
        let mut matched_works = BTreeSet::new();
        for (provider, ext_id) in &ext_ids {
            let peers: Vec<String> = query_scalar(
                "SELECT work_id FROM media_work_external_ids \
                 WHERE kind = ? AND provider = ? AND external_id = ? AND work_id != ?",
            )
            .bind(&kind)
            .bind(provider)
            .bind(ext_id)
            .bind(work_id)
            .fetch_all(&mut *tx)
            .await?;
            for peer in peers {
                matched_works.insert(peer);
            }
        }

        // Filter out any peer that has a CONFLICTING external ID for the same provider
        let mut verified_peers = Vec::new();
        for peer_id in matched_works {
            let peer_row = query(
                "SELECT external_ids_json, manual_json, title, effective_year FROM inventory_works WHERE id = ?",
            )
            .bind(&peer_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(peer_row) = peer_row else {
                continue;
            };
            let peer_ext_json: String = peer_row.get("external_ids_json");
            let peer_manual: Option<String> = peer_row.get("manual_json");
            let peer_ext = extract_external_ids_from_json(&peer_ext_json, peer_manual.as_deref());

            let has_conflict = ext_ids
                .iter()
                .any(|(prov, val)| peer_ext.get(prov).is_some_and(|peer_val| peer_val != val));
            // Also check if an admin previously split or rejected merging these two works
            let was_rejected: i64 = query_scalar(
                "SELECT COUNT(*) FROM media_match_candidates \
                 WHERE ((left_work_id = ? AND right_work_id = ?) OR (left_work_id = ? AND right_work_id = ?)) \
                   AND status IN ('rejected', 'undone')",
            )
            .bind(work_id)
            .bind(&peer_id)
            .bind(&peer_id)
            .bind(work_id)
            .fetch_one(&mut *tx)
            .await?;

            if has_conflict {
                record_candidate_tx(
                    tx,
                    work_id,
                    &peer_id,
                    &kind,
                    65,
                    "Identifiant externe partagé mais conflit sur un autre fournisseur",
                )
                .await?;
            } else if was_rejected == 0 {
                verified_peers.push(peer_id);
            }
        }

        if !verified_peers.is_empty() {
            let mut all_ids = vec![work_id.to_owned()];
            all_ids.extend(verified_peers);
            merge_works_tx(tx, &all_ids, &title, "auto", None).await?;
            return Ok(());
        }
    }

    // 2. Probable match / ambiguity detection (NEVER merges automatically on title!)
    let norm_title = normalize_match_title(&title);
    if norm_title.len() >= 2 {
        type PeerMatchRow = (String, String, Option<i64>, String, Option<String>);
        let same_title_peers: Vec<PeerMatchRow> = query_as(
            "SELECT id, title, effective_year, external_ids_json, manual_json \
             FROM inventory_works \
             WHERE kind = ? AND id != ? AND display_title_sort = ? \
             LIMIT 10",
        )
        .bind(&kind)
        .bind(work_id)
        .bind(title.to_lowercase())
        .fetch_all(&mut *tx)
        .await?;

        for (peer_id, peer_title, peer_year, peer_ext_json, peer_manual) in same_title_peers {
            if normalize_match_title(&peer_title) != norm_title {
                continue;
            }
            // Check if already in the same media_group
            let same_group: i64 = query_scalar(
                "SELECT COUNT(*) FROM media_group_members a \
                 JOIN media_group_members b ON a.group_id = b.group_id \
                 WHERE a.work_id = ? AND b.work_id = ?",
            )
            .bind(work_id)
            .bind(&peer_id)
            .fetch_one(&mut *tx)
            .await?;
            if same_group > 0 {
                continue;
            }

            let peer_ext = extract_external_ids_from_json(&peer_ext_json, peer_manual.as_deref());
            // If both have external IDs for the same provider and they differ, they are distinct works (e.g. remakes) -> do NOT propose merge
            let distinct_by_id = !ext_ids.is_empty()
                && !peer_ext.is_empty()
                && ext_ids
                    .iter()
                    .any(|(k, v)| peer_ext.get(k).is_some_and(|pv| pv != v));
            if distinct_by_id {
                continue;
            }

            let (score, reason) = match (effective_year, peer_year) {
                (Some(y1), Some(y2)) if y1 == y2 => (
                    85,
                    "Même titre normalisé et même année sans identifiant externe commun",
                ),
                (Some(y1), Some(y2)) if (y1 - y2).abs() <= 1 => (
                    75,
                    "Même titre normalisé et années proches (±1 an) sans identifiant externe commun",
                ),
                (None, Some(_)) | (Some(_), None) => (
                    70,
                    "Même titre normalisé (année manquante sur l'une des copies)",
                ),
                (None, None) => (65, "Même titre normalisé sans année ni identifiant externe"),
                _ => continue, // Different years (e.g. 1995 vs 2020 remake) -> distinct works, do not flag
            };

            record_candidate_tx(tx, work_id, &peer_id, &kind, score, reason).await?;
        }
    }

    Ok(())
}

async fn record_candidate_tx(
    tx: &mut AnyConnection,
    work_a: &str,
    work_b: &str,
    kind: &str,
    score: i64,
    reason: &str,
) -> Result<(), sqlx::Error> {
    let (left, right) = if work_a <= work_b {
        (work_a, work_b)
    } else {
        (work_b, work_a)
    };
    let id = uuid::Uuid::new_v4().to_string();
    query(
        "INSERT INTO media_match_candidates(id, left_work_id, right_work_id, kind, score, reason, status) \
         VALUES (?, ?, ?, ?, ?, ?, 'pending') \
         ON CONFLICT(left_work_id, right_work_id) DO UPDATE \
         SET score = excluded.score, reason = excluded.reason \
         WHERE media_match_candidates.status = 'pending'",
    )
    .bind(id)
    .bind(left)
    .bind(right)
    .bind(kind)
    .bind(score)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Merge multiple works of the same kind (`film` or `series`) into a single `media_groups` identity,
/// recording a full reversible snapshot in `media_merge_history`.
/// If `kind == "series"`, also links matching episodes (`SxxExx`) into episode `media_groups`.
pub async fn merge_works_tx(
    tx: &mut AnyConnection,
    work_ids: &[String],
    label: &str,
    origin: &str,
    candidate_id: Option<&str>,
) -> Result<(String, String), sqlx::Error> {
    let mut ids = Vec::new();
    for id in work_ids {
        let trimmed = id.trim();
        if !trimmed.is_empty() && !ids.iter().any(|existing| existing == trimmed) {
            ids.push(trimmed.to_owned());
        }
    }
    // Check existing group memberships to capture snapshot for undo
    let mut previous_memberships: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut existing_group_id: Option<String> = None;
    for wid in &ids {
        let prev: Option<String> =
            query_scalar("SELECT group_id FROM media_group_members WHERE work_id = ?")
                .bind(wid)
                .fetch_optional(&mut *tx)
                .await?;
        if existing_group_id.is_none() && prev.is_some() {
            existing_group_id = prev.clone();
        }
        previous_memberships.insert(wid.clone(), prev);
    }

    // If any of the works already belonged to a group, include all existing members of those groups
    let mut full_member_set: BTreeSet<String> = ids.iter().cloned().collect();
    let prev_group_ids: BTreeSet<String> =
        previous_memberships.values().flatten().cloned().collect();
    for prev_gid in prev_group_ids {
        let members: Vec<String> =
            query_scalar("SELECT work_id FROM media_group_members WHERE group_id = ?")
                .bind(&prev_gid)
                .fetch_all(&mut *tx)
                .await?;
        for m in members {
            previous_memberships
                .entry(m.clone())
                .or_insert_with(|| Some(prev_gid.clone()));
            full_member_set.insert(m);
        }
    }

    let group_id = existing_group_id.unwrap_or_else(|| ids[0].clone());
    let kind: String = query_scalar("SELECT kind FROM inventory_works WHERE id = ?")
        .bind(&ids[0])
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or_else(|| "film".into());

    query(
        "INSERT INTO media_groups(id, label) VALUES (?, ?) \
         ON CONFLICT(id) DO UPDATE SET label = excluded.label",
    )
    .bind(&group_id)
    .bind(label.trim())
    .execute(&mut *tx)
    .await?;

    for wid in &full_member_set {
        query(
            "INSERT INTO media_group_members(work_id, group_id) VALUES (?, ?) \
             ON CONFLICT(work_id) DO UPDATE SET group_id = excluded.group_id",
        )
        .bind(wid)
        .bind(&group_id)
        .execute(&mut *tx)
        .await?;
    }

    // Clean up orphaned groups
    query(
        "DELETE FROM media_groups WHERE NOT EXISTS(SELECT 1 FROM media_group_members m WHERE m.group_id = media_groups.id)",
    )
    .execute(&mut *tx)
    .await?;

    // If this is a series group, automatically group matching episodes (same season_number and episode_number)
    let mut episode_groups_created = Vec::new();
    if kind == "series" {
        let member_vec: Vec<String> = full_member_set.iter().cloned().collect();
        let mut by_se: BTreeMap<(i64, i64), Vec<(String, String)>> = BTreeMap::new();
        for series_wid in &member_vec {
            let eps: Vec<(i64, i64, String, String)> = query_as(
                "SELECT s.season_number, ep.episode_number, ep.work_id, w.title \
                 FROM inventory_episodes ep \
                 JOIN inventory_seasons s ON s.id = ep.season_id \
                 JOIN inventory_works w ON w.id = ep.work_id \
                 WHERE s.series_id = ? \
                 ORDER BY s.season_number, ep.episode_number, ep.work_id",
            )
            .bind(series_wid)
            .fetch_all(&mut *tx)
            .await?;
            for (s_num, e_num, ep_wid, ep_title) in eps {
                by_se
                    .entry((s_num, e_num))
                    .or_default()
                    .push((ep_wid, ep_title));
            }
        }
        for ((_s_num, _e_num), ep_list) in by_se {
            if ep_list.len() > 1 {
                let ep_group_id = ep_list[0].0.clone();
                let ep_label = ep_list[0].1.clone();
                query(
                    "INSERT INTO media_groups(id, label) VALUES (?, ?) \
                     ON CONFLICT(id) DO UPDATE SET label = excluded.label",
                )
                .bind(&ep_group_id)
                .bind(&ep_label)
                .execute(&mut *tx)
                .await?;
                for (ep_wid, _) in &ep_list {
                    query(
                        "INSERT INTO media_group_members(work_id, group_id) VALUES (?, ?) \
                         ON CONFLICT(work_id) DO UPDATE SET group_id = excluded.group_id",
                    )
                    .bind(ep_wid)
                    .bind(&ep_group_id)
                    .execute(&mut *tx)
                    .await?;
                }
                episode_groups_created.push((
                    ep_group_id,
                    ep_list.into_iter().map(|(w, _)| w).collect::<Vec<_>>(),
                ));
            }
        }
    }

    let merge_id = uuid::Uuid::new_v4().to_string();
    let snapshot = json!({
        "groupId": group_id,
        "label": label.trim(),
        "workIds": ids,
        "previousMemberships": previous_memberships,
        "episodeGroups": episode_groups_created,
    });

    query(
        "INSERT INTO media_merge_history(id, group_id, kind, origin, candidate_id, snapshot_json) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&merge_id)
    .bind(&group_id)
    .bind(&kind)
    .bind(origin)
    .bind(candidate_id)
    .bind(snapshot.to_string())
    .execute(&mut *tx)
    .await?;

    // Mark any pending candidate between these works as confirmed
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let (left, right) = if ids[i] <= ids[j] {
                (&ids[i], &ids[j])
            } else {
                (&ids[j], &ids[i])
            };
            query(
                "UPDATE media_match_candidates SET status = 'confirmed', merge_id = ? \
                 WHERE left_work_id = ? AND right_work_id = ?",
            )
            .bind(&merge_id)
            .bind(left)
            .bind(right)
            .execute(&mut *tx)
            .await?;
        }
    }

    Ok((group_id, merge_id))
}

/// Undo a previously recorded merge operation, restoring each work's prior group membership
/// and preserving all manual metadata, custom artwork, and library permissions.
pub async fn undo_merge_tx(tx: &mut AnyConnection, merge_id: &str) -> Result<bool, sqlx::Error> {
    let row = query(
        "SELECT group_id, kind, candidate_id, snapshot_json, undone_at \
         FROM media_merge_history WHERE id = ?",
    )
    .bind(merge_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let undone_at: Option<i64> = row.get("undone_at");
    if undone_at.is_some() {
        return Ok(false);
    }
    let snapshot_json: String = row.get("snapshot_json");
    let snapshot: Value = serde_json::from_str(&snapshot_json).unwrap_or(Value::Null);

    if let Some(prev_map) = snapshot
        .get("previousMemberships")
        .and_then(Value::as_object)
    {
        for (work_id, prev_gid_val) in prev_map {
            if let Some(prev_gid) = prev_gid_val.as_str() {
                query(
                    "INSERT INTO media_groups(id, label) VALUES (?, ?) ON CONFLICT(id) DO NOTHING",
                )
                .bind(prev_gid)
                .bind(prev_gid)
                .execute(&mut *tx)
                .await?;
                query(
                    "INSERT INTO media_group_members(work_id, group_id) VALUES (?, ?) \
                     ON CONFLICT(work_id) DO UPDATE SET group_id = excluded.group_id",
                )
                .bind(work_id)
                .bind(prev_gid)
                .execute(&mut *tx)
                .await?;
            } else {
                query("DELETE FROM media_group_members WHERE work_id = ?")
                    .bind(work_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }

    if let Some(ep_groups) = snapshot.get("episodeGroups").and_then(Value::as_array) {
        for item in ep_groups {
            if let Some(arr) = item.as_array()
                && let Some(ep_wids) = arr.get(1).and_then(Value::as_array)
            {
                for ep_wid in ep_wids.iter().filter_map(Value::as_str) {
                    query("DELETE FROM media_group_members WHERE work_id = ?")
                        .bind(ep_wid)
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }
    }

    // Mark candidate status as 'undone' so automatic consolidation will not re-merge them without explicit admin action
    if let Some(work_ids) = snapshot.get("workIds").and_then(Value::as_array) {
        let wids: Vec<&str> = work_ids.iter().filter_map(Value::as_str).collect();
        for i in 0..wids.len() {
            for j in (i + 1)..wids.len() {
                let (left, right) = if wids[i] <= wids[j] {
                    (wids[i], wids[j])
                } else {
                    (wids[j], wids[i])
                };
                let id = uuid::Uuid::new_v4().to_string();
                query(
                    "INSERT INTO media_match_candidates(id, left_work_id, right_work_id, kind, score, reason, status, merge_id) \
                     VALUES (?, ?, ?, 'film', 100, 'Fusion annulée par un administrateur', 'undone', ?) \
                     ON CONFLICT(left_work_id, right_work_id) DO UPDATE SET status = 'undone', merge_id = excluded.merge_id",
                )
                .bind(id)
                .bind(left)
                .bind(right)
                .bind(merge_id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    query(
        "DELETE FROM media_groups WHERE NOT EXISTS(SELECT 1 FROM media_group_members m WHERE m.group_id = media_groups.id)",
    )
    .execute(&mut *tx)
    .await?;

    query("UPDATE media_merge_history SET undone_at = 1 WHERE id = ?")
        .bind(merge_id)
        .execute(&mut *tx)
        .await?;

    Ok(true)
}

/// Separate a single work from its group (`ungroup` / split), also ungrouping its episodes if it is a series,
/// and marking any match candidates involving this work as `'undone'` so it is not auto-merged again on rescan.
pub async fn ungroup_work_tx(tx: &mut AnyConnection, work_id: &str) -> Result<(), sqlx::Error> {
    let group_id: Option<String> =
        query_scalar("SELECT group_id FROM media_group_members WHERE work_id = ?")
            .bind(work_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(group_id) = group_id else {
        return Ok(());
    };

    let peers: Vec<String> =
        query_scalar("SELECT work_id FROM media_group_members WHERE group_id = ? AND work_id != ?")
            .bind(&group_id)
            .bind(work_id)
            .fetch_all(&mut *tx)
            .await?;

    query("DELETE FROM media_group_members WHERE work_id = ?")
        .bind(work_id)
        .execute(&mut *tx)
        .await?;

    // Also remove episode group memberships for episodes belonging to this series
    let ep_wids: Vec<String> = query_scalar(
        "SELECT ep.work_id FROM inventory_episodes ep \
         JOIN inventory_seasons s ON s.id = ep.season_id \
         WHERE s.series_id = ?",
    )
    .bind(work_id)
    .fetch_all(&mut *tx)
    .await?;
    for ep_wid in ep_wids {
        query("DELETE FROM media_group_members WHERE work_id = ?")
            .bind(&ep_wid)
            .execute(&mut *tx)
            .await?;
    }

    query(
        "DELETE FROM media_groups WHERE NOT EXISTS(SELECT 1 FROM media_group_members m WHERE m.group_id = media_groups.id)",
    )
    .execute(&mut *tx)
    .await?;

    let kind: String = query_scalar("SELECT kind FROM inventory_works WHERE id = ?")
        .bind(work_id)
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or_else(|| "film".into());

    for peer in peers {
        let (left, right) = if work_id <= peer.as_str() {
            (work_id, peer.as_str())
        } else {
            (peer.as_str(), work_id)
        };
        let id = uuid::Uuid::new_v4().to_string();
        query(
            "INSERT INTO media_match_candidates(id, left_work_id, right_work_id, kind, score, reason, status) \
             VALUES (?, ?, ?, ?, 100, 'Copie dissociée manuellement', 'undone') \
             ON CONFLICT(left_work_id, right_work_id) DO UPDATE SET status = 'undone'",
        )
        .bind(id)
        .bind(left)
        .bind(right)
        .bind(&kind)
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}

/// Backfill summary columns for any work where `has_available_asset = 0` or `display_title_sort = ''`
/// (e.g. after migrations or synthetic test inserts).
pub async fn backfill_summaries(conn: &mut AnyConnection) -> Result<(), sqlx::Error> {
    query(
        "UPDATE inventory_works \
         SET display_title_sort = CASE WHEN display_title_sort = '' THEN lower(COALESCE(json_extract(manual_json, '$.title'), title_sort)) ELSE display_title_sort END, \
             effective_year = COALESCE(effective_year, CAST(json_extract(manual_json, '$.year') AS INTEGER), year) \
         WHERE display_title_sort = '' OR (effective_year IS NULL AND year IS NOT NULL)",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// List probable match candidates (optionally filtered by status: 'pending', 'confirmed', 'rejected', 'undone').
pub async fn list_match_candidates(
    store: &IdentityStore,
    status: Option<&str>,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = if let Some(status) = status {
        query(
            "SELECT c.id, c.left_work_id, lw.title AS left_title, lw.year AS left_year, lw.library_id AS left_lib, \
                    c.right_work_id, rw.title AS right_title, rw.year AS right_year, rw.library_id AS right_lib, \
                    c.kind, c.score, c.reason, c.status, c.merge_id \
             FROM media_match_candidates c \
             JOIN inventory_works lw ON lw.id = c.left_work_id \
             JOIN inventory_works rw ON rw.id = c.right_work_id \
             WHERE c.status = ? \
             ORDER BY c.score DESC, c.id ASC LIMIT 200",
        )
        .bind(status)
        .fetch_all(&store.pool)
        .await?
    } else {
        query(
            "SELECT c.id, c.left_work_id, lw.title AS left_title, lw.year AS left_year, lw.library_id AS left_lib, \
                    c.right_work_id, rw.title AS right_title, rw.year AS right_year, rw.library_id AS right_lib, \
                    c.kind, c.score, c.reason, c.status, c.merge_id \
             FROM media_match_candidates c \
             JOIN inventory_works lw ON lw.id = c.left_work_id \
             JOIN inventory_works rw ON rw.id = c.right_work_id \
             ORDER BY c.score DESC, c.id ASC LIMIT 200",
        )
        .fetch_all(&store.pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.get::<String, _>("id"),
                "leftWorkId": r.get::<String, _>("left_work_id"),
                "leftTitle": r.get::<String, _>("left_title"),
                "leftYear": r.get::<Option<i64>, _>("left_year"),
                "leftLibraryId": r.get::<String, _>("left_lib"),
                "rightWorkId": r.get::<String, _>("right_work_id"),
                "rightTitle": r.get::<String, _>("right_title"),
                "rightYear": r.get::<Option<i64>, _>("right_year"),
                "rightLibraryId": r.get::<String, _>("right_lib"),
                "kind": r.get::<String, _>("kind"),
                "score": r.get::<i64, _>("score"),
                "reason": r.get::<String, _>("reason"),
                "status": r.get::<String, _>("status"),
                "mergeId": r.get::<Option<String>, _>("merge_id"),
            })
        })
        .collect())
}

/// List active (not undone) merge history entries.
pub async fn list_merge_history(store: &IdentityStore) -> Result<Vec<Value>, sqlx::Error> {
    let rows = query(
        "SELECT id, group_id, kind, origin, candidate_id, snapshot_json, created_at \
         FROM media_merge_history \
         WHERE undone_at IS NULL \
         ORDER BY created_at DESC, id DESC LIMIT 200",
    )
    .fetch_all(&store.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let snapshot_str: String = r.get("snapshot_json");
            let snapshot: Value = serde_json::from_str(&snapshot_str).unwrap_or(Value::Null);
            json!({
                "id": r.get::<String, _>("id"),
                "groupId": r.get::<String, _>("group_id"),
                "kind": r.get::<String, _>("kind"),
                "origin": r.get::<String, _>("origin"),
                "candidateId": r.get::<Option<String>, _>("candidate_id"),
                "snapshot": snapshot,
                "createdAt": r.get::<i64, _>("created_at"),
            })
        })
        .collect())
}

/// Confirm a pending match candidate, merging the two works and recording a reversible merge history entry.
pub async fn confirm_candidate_tx(
    tx: &mut AnyConnection,
    candidate_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row = query(
        "SELECT c.left_work_id, c.right_work_id, lw.title AS left_title \
         FROM media_match_candidates c \
         JOIN inventory_works lw ON lw.id = c.left_work_id \
         WHERE c.id = ?",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let left_id: String = row.get("left_work_id");
    let right_id: String = row.get("right_work_id");
    let label: String = row.get("left_title");
    let (group_id, merge_id) = merge_works_tx(
        tx,
        &[left_id, right_id],
        &label,
        "candidate",
        Some(candidate_id),
    )
    .await?;
    Ok(Some((group_id, merge_id)))
}

/// Reject a match candidate so it is not proposed or auto-merged again.
pub async fn reject_candidate_tx(
    tx: &mut AnyConnection,
    candidate_id: &str,
) -> Result<bool, sqlx::Error> {
    let res = query("UPDATE media_match_candidates SET status = 'rejected' WHERE id = ?")
        .bind(candidate_id)
        .execute(&mut *tx)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Resolve all member work IDs in the same `media_group` as `work_id` that the given user (`user_id`, `role`)
/// is permitted to read (library enabled, node not revoked, library permission granted, parental policy satisfied).
pub async fn authorized_group_members(
    store: &IdentityStore,
    work_id: &str,
    user_id: Option<&str>,
    role: Option<&str>,
) -> Result<Vec<String>, sqlx::Error> {
    let group_id: Option<String> = query_scalar(
        "SELECT COALESCE( \
            (SELECT group_id FROM media_group_members WHERE work_id = ?), \
            (SELECT id FROM media_groups WHERE id = ?) \
         )",
    )
    .bind(work_id)
    .bind(work_id)
    .fetch_optional(&store.pool)
    .await?
    .flatten();
    let candidates: Vec<String> = if let Some(gid) = group_id {
        query_scalar("SELECT work_id FROM media_group_members WHERE group_id = ? ORDER BY work_id")
            .bind(gid)
            .fetch_all(&store.pool)
            .await?
    } else {
        vec![work_id.to_owned()]
    };

    let Some(uid) = user_id else {
        return Ok(candidates);
    };
    let is_admin = role == Some("administrator")
        || query_scalar::<_, Option<String>>("SELECT role FROM users WHERE id = ?")
            .bind(uid)
            .fetch_optional(&store.pool)
            .await?
            .flatten()
            .as_deref()
            == Some("administrator");

    let mut allowed = Vec::new();
    for cid in candidates {
        let lib_ok: i64 = query_scalar(
            "SELECT CASE WHEN EXISTS( \
                SELECT 1 FROM inventory_works w \
                JOIN libraries l ON l.id = w.library_id \
                JOIN media_nodes n ON n.id = l.node_id \
                WHERE w.id = ? AND l.enabled = 1 AND n.status != 'revoked' \
                  AND (? = 1 OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id = l.id AND p.user_id = ?)) \
            ) THEN 1 ELSE 0 END",
        )
        .bind(&cid)
        .bind(i64::from(is_admin))
        .bind(uid)
        .fetch_one(&store.pool)
        .await?;
        if lib_ok == 0 {
            continue;
        }
        if crate::product::parental_allowed(store, uid, &cid)
            .await
            .unwrap_or(false)
        {
            allowed.push(cid);
        }
    }
    Ok(allowed)
}
