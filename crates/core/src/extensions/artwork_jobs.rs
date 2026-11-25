//! Durable progressive collection on the existing Core worker/runtime and AssetStore quotas.
//! No browser credentials are persisted or manufactured for background execution.
use super::*;
use serde::Serialize;
static WORKER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[cfg(test)]
mod tests;

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    library_id: String,
    #[serde(default)]
    after: Option<String>,
    languages: Vec<String>,
    include_neutral: bool,
    max_images: usize,
    #[serde(default = "default_work_limit")]
    max_works: i64,
    /// Automatic job started by the server itself: `single` (one image of each enabled
    /// kind) or `pool` (multi-image mode). Manual jobs leave it empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auto: Option<String>,
    /// Automatic pass started by an administrator from the task scheduler: every work
    /// not handled since this instant is asked again, instead of once a month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    since: Option<i64>,
}
/// A work handled by an automatic pass is asked again after this delay at most, so
/// images published later by the providers are still picked up.
const AUTO_RETRY_AFTER: i64 = 30 * 86_400;
/// Images already present for a work: provider pool, files beside the media, legacy poster.
async fn present_kinds(
    auth: &AuthState,
    work: &str,
) -> Result<std::collections::HashSet<String>, Error> {
    let kinds: Vec<String> = query_scalar(
        "SELECT kind \
            FROM work_image_entries \
            WHERE work_id=? \
            UNION SELECT i.role \
            FROM inventory_images i \
            LEFT \
            JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id \
            WHERE i.work_id=? AND i.available=1 AND h.image_id IS NULL \
            UNION SELECT 'poster' \
            FROM work_posters \
            WHERE work_id=?",
    )
    .bind(work)
    .bind(work)
    .bind(work)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(kinds.into_iter().collect())
}
use super::image_collection::IMAGE_KINDS;
/// Next identified film or series of an automatic job that still needs the providers.
/// Works with nothing missing are marked done here without any network call.
async fn next_auto_work(
    auth: &AuthState,
    job: &Job,
    mode: &str,
    since: Option<i64>,
) -> Result<(Option<String>, String), Error> {
    let handled_after = since.unwrap_or_else(|| now() - AUTO_RETRY_AFTER);
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_library_options"))?;
    let mut cursor = job.cursor_work.clone();
    loop {
        let next: Option<String> = query_scalar(
            "SELECT w.id \
                FROM inventory_works w \
                WHERE (w.library_id=? OR w.library_id IN (SELECT library_id FROM library_folders \
                WHERE parent_id=? AND active=1)) AND w.id>? AND w.kind IN ('film','series') \
                AND w.external_ids_json NOT IN ('','{}') AND NOT EXISTS(SELECT 1 \
                FROM artwork_auto_attempts a WHERE a.work_id=w.id AND a.mode=? \
                AND a.attempted_at>?) \
                ORDER BY w.id \
                LIMIT 1",
        )
        .bind(&job.library_id)
        .bind(&job.library_id)
        .bind(&cursor)
        .bind(mode)
        .bind(handled_after)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
        let Some(work) = next else {
            return Ok((None, cursor));
        };
        if mode == "single" {
            let present = present_kinds(auth, &work).await?;
            if IMAGE_KINDS
                .iter()
                .all(|kind| !options.image_enabled(kind) || present.contains(*kind))
            {
                mark_auto_attempt(auth, &work, mode).await?;
                cursor = work;
                continue;
            }
        }
        return Ok((Some(work), cursor));
    }
}
async fn mark_auto_attempt(auth: &AuthState, work: &str, mode: &str) -> Result<(), Error> {
    mark_auto_attempt_at(auth, work, mode, now()).await
}
async fn mark_auto_attempt_at(
    auth: &AuthState,
    work: &str,
    mode: &str,
    attempted_at: i64,
) -> Result<(), Error> {
    query("INSERT INTO artwork_auto_attempts(work_id,mode,attempted_at) VALUES(?,?,?) ON CONFLICT(work_id,mode) DO UPDATE SET attempted_at=excluded.attempted_at")
        .bind(work).bind(mode).bind(attempted_at).execute(auth.store.pool()).await.map_err(db)?;
    Ok(())
}

const EPISODE_STILL_MODE: &str = "episode-still-v2";

fn extract_https_still(manual_json: Option<&str>, editorial_json: &str) -> Option<String> {
    for raw in [manual_json, Some(editorial_json)].into_iter().flatten() {
        let Ok(val) = serde_json::from_str::<Value>(raw) else {
            continue;
        };
        for key in ["still_url", "stillUrl", "thumb_url", "thumbUrl"] {
            if let Some(url) = val.get(key).and_then(Value::as_str)
                && url.starts_with("https://")
            {
                return Some(url.to_owned());
            }
        }
    }
    None
}

fn merge_series_external_ids(
    external_ids_json: &str,
    manual_json: Option<&str>,
) -> std::collections::BTreeMap<String, String> {
    let mut ids =
        serde_json::from_str::<std::collections::BTreeMap<String, String>>(external_ids_json)
            .unwrap_or_default();
    if let Some(manual) = manual_json.and_then(|s| serde_json::from_str::<Value>(s).ok())
        && let Some(extra) = manual.get("external_ids").and_then(Value::as_object)
    {
        for (k, v) in extra {
            if let Some(s) = v.as_str()
                && !s.trim().is_empty()
            {
                ids.insert(k.clone(), s.to_owned());
            }
        }
    }
    ids
}

fn pick_series_details_provider(
    extensions: &[(String, Package)],
    options: &crate::library_options::LibraryOptions,
    ids: &std::collections::BTreeMap<String, String>,
) -> Option<(String, String)> {
    let mut candidates: Vec<(bool, i64, String, String)> = Vec::new();
    for (ext_id, package) in extensions {
        let perms = &package.manifest.permissions;
        if !perms.iter().any(|p| p == "metadata.read")
            || !perms.iter().any(|p| p == "images")
            || !perms.iter().any(|p| p == "metadata.write")
        {
            continue;
        }
        let Some(img_prio) = crate::library_options::LibraryOptions::provider_priority(
            &options.image_providers,
            ext_id,
            100,
        ) else {
            continue;
        };
        let lower = ext_id.to_ascii_lowercase();
        let candidate_id = if lower.contains("tmdb") {
            // TMDb natively resolves IMDb and TVDB IDs via `/find/{id}` when no TMDb ID is stored yet
            // (for example on series identified with TVDB, the default provider for series).
            // Prefix TVDB IDs so TMDb never mistakes a numeric TVDB ID for a TMDb ID.
            ids.get("tmdb")
                .cloned()
                .or_else(|| ids.get("imdb").cloned())
                .or_else(|| ids.get("tvdb").map(|id| format!("tvdb:{id}")))
        } else if lower.contains("tvdb") {
            ids.get("tvdb").cloned()
        } else {
            ids.get(ext_id)
                .or_else(|| ids.get("tmdb"))
                .or_else(|| ids.get("tvdb"))
                .or_else(|| ids.get("imdb"))
                .cloned()
        };
        if let Some(cid) = candidate_id.filter(|s| !s.trim().is_empty()) {
            // Prefer TMDb when available because TMDb details include per-episode stillUrl.
            candidates.push((lower.contains("tmdb"), img_prio, ext_id.clone(), cid));
        }
    }
    candidates.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    candidates
        .into_iter()
        .next()
        .map(|(_, _, ext_id, cid)| (ext_id, cid))
}

async fn load_permitted_extensions(
    auth: &AuthState,
    library_id: &str,
) -> Result<Vec<(String, Package)>, Error> {
    let rows: Vec<(String, String)> = query_as(
        "SELECT e.id, e.package_json \
         FROM extensions e \
         JOIN extension_publishers p ON p.id=e.publisher_id \
         WHERE e.enabled=1 AND p.active=1 \
         AND (e.all_libraries=1 \
              OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=?) \
              OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id \
                        WHERE g.extension_id=e.id AND f.library_id=? AND f.active=1)) \
         AND NOT EXISTS(SELECT 1 FROM automation_policies policy \
                        WHERE policy.extension_id=e.id AND policy.library_id=? AND policy.mode='disabled') \
         ORDER BY e.id",
    )
    .bind(library_id)
    .bind(library_id)
    .bind(library_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, raw)| serde_json::from_str::<Package>(&raw).ok().map(|p| (id, p)))
        .collect())
}

/// Fetch strictly one unique thumbnail per TV episode (never downloading multi-image pools):
/// 1. Imports any pending `still_url` already stored on an episode into `work_image_entries`.
/// 2. Otherwise queries `metadata.details` once for an identified series that still has episodes
///    missing both a local `thumb` and a `still_url`, storing the returned episode `still_url`s
///    for subsequent 1-by-1 local import.
pub(super) async fn episode_stills_tick(auth: &AuthState) -> Result<bool, Error> {
    let owner: Option<String> = query_scalar(
        "SELECT id FROM users WHERE role='administrator' AND disabled_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(auth.store.pool())
    .await
    .map_err(db)?;
    let Some(owner) = owner else {
        return Ok(false);
    };
    let retry_cutoff = now() - AUTO_RETRY_AFTER;

    // Step A: import 1 episode that already has an https:// still_url/thumb_url.
    let pending_stills: Vec<(String, String, String, String, Option<String>)> = query_as(
        "SELECT w.id, w.library_id, COALESCE(parent.options_json, l.options_json), \
                w.editorial_json, w.manual_json \
         FROM inventory_works w \
         JOIN libraries l ON l.id=w.library_id \
         LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 \
         LEFT JOIN libraries parent ON parent.id=f.parent_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE w.kind='episode' AND l.kind='series' AND l.enabled=1 \
         AND (parent.id IS NULL OR parent.enabled=1) AND n.status!='revoked' \
         AND NOT EXISTS(SELECT 1 FROM work_image_entries e WHERE e.work_id=w.id AND e.kind='thumb') \
         AND NOT EXISTS(SELECT 1 FROM inventory_images i \
                        LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id \
                        WHERE i.work_id=w.id AND i.role='thumb' AND i.available=1 AND h.image_id IS NULL) \
         AND NOT EXISTS(SELECT 1 FROM artwork_auto_attempts a \
                        WHERE a.work_id=w.id AND a.mode=? AND a.attempted_at>?) \
         AND COALESCE( \
                json_extract(w.manual_json, '$.still_url'), \
                json_extract(w.manual_json, '$.stillUrl'), \
                json_extract(w.manual_json, '$.thumb_url'), \
                json_extract(w.manual_json, '$.thumbUrl'), \
                json_extract(w.editorial_json, '$.still_url'), \
                json_extract(w.editorial_json, '$.stillUrl'), \
                json_extract(w.editorial_json, '$.thumb_url'), \
                json_extract(w.editorial_json, '$.thumbUrl'), \
                '' \
             ) LIKE 'https://%' \
         ORDER BY w.id \
         LIMIT 8",
    )
    .bind(EPISODE_STILL_MODE)
    .bind(retry_cutoff)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;

    for (ep_id, ep_library_id, options_json, editorial_json, manual_json) in pending_stills {
        let options = crate::library_options::LibraryOptions::from_json(&options_json);
        if !(options.fetch_poster || options.fetch_backdrop || options.fetch_thumb) {
            continue;
        }
        let locked = manual_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| v.get("artwork_locked").and_then(Value::as_bool))
            .unwrap_or(false);
        if locked {
            mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            continue;
        }
        let Some(still_url) = extract_https_still(manual_json.as_deref(), &editorial_json) else {
            mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            continue;
        };
        let Some(host) = reqwest::Url::parse(&still_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
        else {
            mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            continue;
        };
        let extensions = load_permitted_extensions(auth, &ep_library_id).await?;
        let chosen = extensions
            .iter()
            .filter_map(|(ext_id, package)| {
                let perms = &package.manifest.permissions;
                if !perms.iter().any(|p| p == "images")
                    || !perms.iter().any(|p| p == "metadata.write")
                    || !package.manifest.http_hosts.iter().any(|h| h == &host)
                {
                    return None;
                }
                let prio = crate::library_options::LibraryOptions::provider_priority(
                    &options.image_providers,
                    ext_id,
                    100,
                )?;
                Some((prio, ext_id.clone()))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)))
            .map(|(_, ext_id)| ext_id);
        let Some(ext_id) = chosen else {
            mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            continue;
        };
        let Some(_slot) = crate::extensions::runtime::try_acquire_execution(
            &ext_id,
            crate::extensions::runtime::ExecutionClass::Background,
        ) else {
            return Ok(false);
        };
        match adapters::import_episode_still(&auth.store, &ext_id, &ep_id, &still_url, false).await
        {
            Ok(_) => {
                mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            }
            Err((
                StatusCode::BAD_GATEWAY
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::TOO_MANY_REQUESTS,
                _,
            )) => {
                mark_auto_attempt_at(auth, &ep_id, EPISODE_STILL_MODE, retry_cutoff + 3600).await?;
            }
            Err(_) => {
                mark_auto_attempt(auth, &ep_id, EPISODE_STILL_MODE).await?;
            }
        }
        return Ok(true);
    }

    // Step B: find 1 identified series with episodes missing both a thumb and a still_url,
    // and populate episode still_urls via one metadata.details call.
    let series_candidates: Vec<(String, String, String, String, Option<String>)> = query_as(
        "SELECT w.id, w.library_id, COALESCE(parent.options_json, l.options_json), \
                w.external_ids_json, w.manual_json \
         FROM inventory_works w \
         JOIN libraries l ON l.id=w.library_id \
         LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 \
         LEFT JOIN libraries parent ON parent.id=f.parent_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE w.kind='series' AND l.kind='series' AND l.enabled=1 \
         AND (parent.id IS NULL OR parent.enabled=1) AND n.status!='revoked' \
         AND (w.external_ids_json NOT IN ('','{}') \
              OR COALESCE(json_extract(w.manual_json, '$.external_ids'), '{}') NOT IN ('','{}')) \
         AND EXISTS( \
             SELECT 1 FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_works ew ON ew.id=ep.work_id \
             WHERE s.series_id=w.id AND ew.kind='episode' \
             AND COALESCE(CAST(json_extract(ew.manual_json,'$.artwork_locked') AS TEXT),'0') NOT IN ('1','true') \
             AND NOT EXISTS(SELECT 1 FROM work_image_entries e WHERE e.work_id=ew.id AND e.kind='thumb') \
             AND NOT EXISTS(SELECT 1 FROM inventory_images i \
                            LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id \
                            WHERE i.work_id=ew.id AND i.role='thumb' AND i.available=1 AND h.image_id IS NULL) \
             AND NOT EXISTS(SELECT 1 FROM artwork_auto_attempts a \
                            WHERE a.work_id=ew.id AND a.mode=? AND a.attempted_at>?) \
             AND COALESCE( \
                    json_extract(ew.manual_json, '$.still_url'), \
                    json_extract(ew.manual_json, '$.stillUrl'), \
                    json_extract(ew.manual_json, '$.thumb_url'), \
                    json_extract(ew.manual_json, '$.thumbUrl'), \
                    json_extract(ew.editorial_json, '$.still_url'), \
                    json_extract(ew.editorial_json, '$.stillUrl'), \
                    json_extract(ew.editorial_json, '$.thumb_url'), \
                    json_extract(ew.editorial_json, '$.thumbUrl'), \
                    '' \
                 ) NOT LIKE 'https://%' \
         ) \
         ORDER BY w.id \
         LIMIT 4",
    )
    .bind(EPISODE_STILL_MODE)
    .bind(retry_cutoff)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;

    for (series_id, series_library_id, options_json, external_ids_json, manual_json) in
        series_candidates
    {
        let options = crate::library_options::LibraryOptions::from_json(&options_json);
        if !(options.fetch_poster || options.fetch_backdrop || options.fetch_thumb) {
            continue;
        }
        let pending_episodes: Vec<(String, i64, i64, String)> = query_as(
            "SELECT ew.id, s.season_number, ep.episode_number, ew.editorial_json \
             FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_works ew ON ew.id=ep.work_id \
             WHERE s.series_id=? AND ew.kind='episode' \
             AND COALESCE(CAST(json_extract(ew.manual_json,'$.artwork_locked') AS TEXT),'0') NOT IN ('1','true') \
             AND NOT EXISTS(SELECT 1 FROM work_image_entries e WHERE e.work_id=ew.id AND e.kind='thumb') \
             AND NOT EXISTS(SELECT 1 FROM inventory_images i \
                            LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id \
                            WHERE i.work_id=ew.id AND i.role='thumb' AND i.available=1 AND h.image_id IS NULL) \
             AND NOT EXISTS(SELECT 1 FROM artwork_auto_attempts a \
                            WHERE a.work_id=ew.id AND a.mode=? AND a.attempted_at>?) \
             ORDER BY s.season_number, ep.episode_number \
             LIMIT 250",
        )
        .bind(&series_id)
        .bind(EPISODE_STILL_MODE)
        .bind(retry_cutoff)
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
        if pending_episodes.is_empty() {
            continue;
        }
        let ids = merge_series_external_ids(&external_ids_json, manual_json.as_deref());
        let extensions = load_permitted_extensions(auth, &series_library_id).await?;
        let Some((ext_id, candidate_id)) =
            pick_series_details_provider(&extensions, &options, &ids)
        else {
            for (ep_id, _, _, _) in &pending_episodes {
                mark_auto_attempt(auth, ep_id, EPISODE_STILL_MODE).await?;
            }
            continue;
        };

        let response = run::run_with_context(
            auth,
            &HeaderMap::new(),
            &ext_id,
            run::Run {
                work_id: series_id.clone(),
                operation: "metadata.details".into(),
                input: json!({ "id": candidate_id }),
            },
            Some(run::Background::EpisodeStill { owner: &owner }),
        )
        .await;

        let proposal = match response {
            Ok(Json(body)) => body.get("proposal").cloned().unwrap_or(Value::Null),
            Err((StatusCode::CONFLICT, _)) => return Ok(false),
            Err(_) => {
                for (ep_id, _, _, _) in &pending_episodes {
                    mark_auto_attempt_at(auth, ep_id, EPISODE_STILL_MODE, retry_cutoff + 3600)
                        .await?;
                }
                return Ok(false);
            }
        };

        // Persist any newly resolved TMDb ID on the series so future passes skip `/find/`.
        if !ids.contains_key("tmdb") {
            let resolved_tmdb = proposal
                .get("externalIds")
                .and_then(|v| v.get("tmdb"))
                .and_then(Value::as_str)
                .or_else(|| {
                    ext_id
                        .to_ascii_lowercase()
                        .contains("tmdb")
                        .then(|| proposal.get("providerId").and_then(Value::as_str))
                        .flatten()
                })
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            if let Some(tmdb_id) = resolved_tmdb {
                let mut stored_ids = serde_json::from_str::<
                    std::collections::BTreeMap<String, String>,
                >(&external_ids_json)
                .unwrap_or_default();
                if !stored_ids.contains_key("tmdb") {
                    stored_ids.insert("tmdb".into(), tmdb_id.to_owned());
                    if let Ok(encoded_ids) = serde_json::to_string(&stored_ids) {
                        let _ = query("UPDATE inventory_works SET external_ids_json=? WHERE id=?")
                            .bind(&encoded_ids)
                            .bind(&series_id)
                            .execute(auth.store.pool())
                            .await;
                    }
                }
            }
        }

        let mut stills_by_ep: std::collections::HashMap<(i64, i64), Value> =
            std::collections::HashMap::new();
        let mut returned_seasons = std::collections::HashSet::new();
        if let Some(seasons) = proposal.get("seasons").and_then(Value::as_array) {
            for season in seasons {
                let s_num = season
                    .get("seasonNumber")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if let Some(episodes) = season.get("episodes").and_then(Value::as_array)
                    && !episodes.is_empty()
                {
                    returned_seasons.insert(s_num);
                    for ep in episodes {
                        if let Some(ep_num) = ep.get("episodeNumber").and_then(Value::as_i64) {
                            stills_by_ep.insert((s_num, ep_num), ep.clone());
                        }
                    }
                }
            }
        }

        let mut populated = 0usize;
        for (ep_id, s_num, ep_num, editorial_json) in &pending_episodes {
            let still = stills_by_ep
                .get(&(*s_num, *ep_num))
                .and_then(|ep| ep.get("stillUrl").and_then(Value::as_str))
                .filter(|s| s.starts_with("https://"));
            if let Some(st) = still {
                let mut ep_meta =
                    serde_json::from_str::<serde_json::Map<String, Value>>(editorial_json)
                        .unwrap_or_default();
                ep_meta.insert("still_url".into(), json!(st));
                ep_meta.insert("thumb_url".into(), json!(st));
                if let Some(ep_obj) = stills_by_ep.get(&(*s_num, *ep_num)) {
                    if !ep_meta.contains_key("title")
                        && let Some(t) = ep_obj
                            .get("title")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                    {
                        ep_meta.insert("title".into(), json!(t));
                        let _ = query(
                            "UPDATE inventory_works SET title=? WHERE id=? AND (title LIKE 'S%E%' OR title LIKE 'Episode%' OR title='')",
                        )
                        .bind(t)
                        .bind(ep_id)
                        .execute(auth.store.pool())
                        .await;
                    }
                    if !ep_meta.contains_key("plot")
                        && let Some(p) = ep_obj
                            .get("overview")
                            .and_then(Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                    {
                        ep_meta.insert("plot".into(), json!(p));
                        ep_meta.insert("outline".into(), json!(p));
                    }
                    if !ep_meta.contains_key("premiered")
                        && let Some(d) = ep_obj.get("airDate").and_then(Value::as_str)
                    {
                        ep_meta.insert("premiered".into(), json!(d));
                    }
                    if !ep_meta.contains_key("runtime")
                        && let Some(r) = ep_obj.get("runtimeMinutes").and_then(Value::as_u64)
                    {
                        ep_meta.insert("runtime".into(), json!(r));
                    }
                    if !ep_meta.contains_key("rating")
                        && let Some(rt) = ep_obj.get("rating").and_then(Value::as_f64)
                    {
                        ep_meta.insert("rating".into(), json!(rt));
                    }
                }
                let encoded = Value::Object(ep_meta).to_string();
                let _ = query("UPDATE inventory_works SET editorial_json=? WHERE id=?")
                    .bind(&encoded)
                    .bind(ep_id)
                    .execute(auth.store.pool())
                    .await;
                populated += 1;
            } else if returned_seasons.contains(s_num) {
                mark_auto_attempt(auth, ep_id, EPISODE_STILL_MODE).await?;
            } else {
                // The provider did not return episode data for this season (e.g. a provider without
                // episode stills or a truncated response): back off for 6 hours instead of 30 days.
                mark_auto_attempt_at(auth, ep_id, EPISODE_STILL_MODE, retry_cutoff + 6 * 3600)
                    .await?;
            }
        }
        if populated > 0 {
            crate::inventory::catalogue_changed(&auth.store).await;
            return Ok(true);
        }
        return Ok(true);
    }

    Ok(false)
}
static LAST_AUTO_CHECK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// Asks the next worker tick to look for works to fill (new identification, saved settings).
pub(crate) fn request_automatic_check() {
    LAST_AUTO_CHECK.store(0, std::sync::atomic::Ordering::Relaxed);
}
/// Queues an automatic image pass for every film or series library that has identified
/// works not handled yet in the library's current mode. Runs at most every ten minutes
/// unless `force` (library settings just saved).
pub(crate) async fn ensure_automatic(auth: &AuthState, force: bool) -> Result<(), Error> {
    let checked = LAST_AUTO_CHECK.load(std::sync::atomic::Ordering::Relaxed);
    if !force && now() - checked < 600 {
        return Ok(());
    }
    LAST_AUTO_CHECK.store(now(), std::sync::atomic::Ordering::Relaxed);
    let owner: Option<String> = query_scalar("SELECT id FROM users WHERE role='administrator' AND disabled_at IS NULL ORDER BY id LIMIT 1")
        .fetch_optional(auth.store.pool()).await.map_err(db)?;
    let Some(owner) = owner else {
        return Ok(());
    };
    let libraries: Vec<(String, String)> = query_as(
        "SELECT l.id,l.options_json \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE l.enabled=1 AND l.kind IN ('films','series') AND n.status!='revoked' \
            AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id AND f.active=1) \
            AND NOT EXISTS(SELECT 1 FROM artwork_collection_jobs j WHERE j.library_id=l.id \
            AND j.status IN ('queued','running','paused'))",
    )
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    for (library, options) in libraries {
        let Ok(options) = serde_json::from_str::<crate::library_options::LibraryOptions>(&options)
        else {
            continue;
        };
        if !IMAGE_KINDS.iter().any(|kind| options.image_enabled(kind)) {
            continue;
        }
        let mode = library_mode(&options);
        let pending: i64 = query_scalar(
            "SELECT COUNT(*) \
                FROM (SELECT 1 FROM inventory_works w WHERE (w.library_id=? \
                OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? \
                AND active=1)) AND w.kind IN ('film','series') \
                AND w.external_ids_json NOT IN ('','{}') AND NOT EXISTS(SELECT 1 \
                FROM artwork_auto_attempts a WHERE a.work_id=w.id AND a.mode=? \
                AND a.attempted_at>?) LIMIT 1) pending",
        )
        .bind(&library)
        .bind(&library)
        .bind(mode)
        .bind(now() - AUTO_RETRY_AFTER)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
        if pending == 0 {
            continue;
        }
        let languages = if options.artwork_languages.is_empty() {
            vec!["fr".into(), "en".into()]
        } else {
            options.artwork_languages.clone()
        };
        let parameters = Input {
            library_id: library.clone(),
            after: None,
            languages,
            include_neutral: options.artwork_include_neutral,
            max_images: usize::from(options.artwork_max_images).clamp(1, 12),
            max_works: 100_000,
            auto: Some(mode.into()),
            since: None,
        };
        let parameters = serde_json::to_string(&parameters)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_parameters"))?;
        // The partial unique index keeps a single active job per library even if two
        // passes race; the loser's insert is simply ignored.
        let _ = query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,'',?,?)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&library).bind(&owner).bind(parameters).bind(now()).bind(now())
            .execute(auth.store.pool()).await;
    }
    query("DELETE FROM artwork_collection_jobs WHERE status IN ('completed','cancelled') AND id NOT IN (SELECT id FROM artwork_collection_jobs ORDER BY created_at DESC,id DESC LIMIT 1000)").execute(auth.store.pool()).await.map_err(db)?;
    Ok(())
}
/// Image mode of a library: `pool` (several images per kind) or `single`.
fn library_mode(options: &crate::library_options::LibraryOptions) -> &'static str {
    if options.artwork_pool_enabled {
        "pool"
    } else {
        "single"
    }
}
/// Queue, on an administrator's request, an automatic image pass over every identified
/// work of a library: the library's mode (pool or one image per kind), languages and
/// limits apply, and works handled earlier are asked again. `false` when a pass of
/// this library is already queued or running.
pub(crate) async fn queue_library_pass(
    auth: &AuthState,
    library: &str,
    owner: &str,
) -> Result<bool, Error> {
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options = crate::library_options::LibraryOptions::from_json(&options);
    let parameters = Input {
        library_id: library.to_owned(),
        after: None,
        languages: if options.artwork_languages.is_empty() {
            vec!["fr".into(), "en".into()]
        } else {
            options.artwork_languages.clone()
        },
        include_neutral: options.artwork_include_neutral,
        max_images: usize::from(options.artwork_max_images).clamp(1, 12),
        max_works: 100_000,
        auto: Some(library_mode(&options).into()),
        since: Some(now()),
    };
    let parameters = serde_json::to_string(&parameters)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_parameters"))?;
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    let active: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs WHERE library_id=? AND status IN ('queued','running','paused')")
        .bind(library).fetch_one(&mut *tx).await.map_err(db)?;
    if active > 0 {
        return Ok(false);
    }
    query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,'',?,?)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(library).bind(owner).bind(parameters).bind(now()).bind(now())
        .execute(&mut *tx).await.map_err(db)?;
    // Also retry missing episode thumbnails for this library on an explicit artwork pass.
    query(
        "DELETE FROM artwork_auto_attempts \
         WHERE mode=? AND work_id IN ( \
             SELECT w.id FROM inventory_works w \
             WHERE w.kind='episode' AND (w.library_id=? \
                 OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) \
         )",
    )
    .bind(EPISODE_STILL_MODE)
    .bind(library)
    .bind(library)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}
/// Pause, resume or stop (`cancel`) the image pass of a library. `false` when nothing changed.
/// Clearing the lease token fences a work in flight: its result is not written back.
pub(crate) async fn control_library_pass(
    auth: &AuthState,
    library: &str,
    action: &str,
) -> Result<bool, Error> {
    let (status, from) = match action {
        "pause" => ("paused", "('queued','running')"),
        "resume" => ("queued", "('paused')"),
        "cancel" => ("cancelled", "('queued','running','paused')"),
        _ => return Err(err(StatusCode::BAD_REQUEST, "invalid_action")),
    };
    Ok(query(sqlx::AssertSqlSafe(format!("UPDATE artwork_collection_jobs SET status=?,lease_token='',lease_until=0,next_run_at=0,updated_at=? WHERE library_id=? AND status IN {from}")))
        .bind(status).bind(now()).bind(library).execute(auth.store.pool()).await.map_err(db)?.rows_affected() > 0)
}
fn default_work_limit() -> i64 {
    1000
}
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(super) struct Job {
    pub id: String,
    pub library_id: String,
    pub owner: String,
    pub parameters: String,
    pub cursor_work: String,
    pub status: String,
    #[serde(skip)]
    pub lease_token: String,
    pub processed: i64,
    pub downloaded: i64,
    pub duplicates: i64,
    pub skipped: i64,
    pub errors: i64,
    pub work_attempts: i64,
    pub last_error: String,
}
const SELECT: &str = "SELECT id,library_id,owner,parameters,cursor_work,status,lease_token,processed,downloaded,duplicates,skipped,errors,work_attempts,last_error FROM artwork_collection_jobs";

pub(super) async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = admin(&auth, &headers, true).await?;
    if input
        .after
        .as_ref()
        .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
        || input.max_images == 0
        || input.max_images > 12
        || !(1..=100_000).contains(&input.max_works)
        || input.languages.is_empty()
        || input.languages.len() > 5
        || input
            .languages
            .iter()
            .any(|v| !image_collection::valid_language(v))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_collection_limits"));
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films','series') AND n.status!='revoked'")
        .bind(&input.library_id).fetch_one(auth.store.pool()).await.map_err(db)?;
    if count != 1 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_library"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    let active: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs WHERE status IN ('queued','running','paused')").fetch_one(&mut *tx).await.map_err(db)?;
    if active >= 32 {
        return Err(err(StatusCode::CONFLICT, "collection_queue_full"));
    }
    let existing: Option<String> = query_scalar("SELECT id FROM artwork_collection_jobs WHERE library_id=? AND status IN ('queued','running','paused')")
        .bind(&input.library_id).fetch_optional(&mut *tx).await.map_err(db)?;
    if let Some(id) = existing {
        return Ok((StatusCode::ACCEPTED, Json(json!({"id":id,"existing":true}))));
    }
    query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,?,?,?)")
        .bind(&id).bind(&input.library_id).bind(&actor.user.id).bind(serde_json::to_string(&input).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_parameters"))?).bind(input.after.as_deref().unwrap_or("")).bind(now()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    query("DELETE FROM artwork_collection_jobs WHERE status IN ('completed','cancelled') AND id NOT IN (SELECT id FROM artwork_collection_jobs ORDER BY created_at DESC,id DESC LIMIT 1000)").execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::ACCEPTED, Json(json!({"id":id}))))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Filter {
    library_id: Option<String>,
}
pub(super) async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<Job>>, Error> {
    admin(&auth, &headers, false).await?;
    query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE (CAST(? AS TEXT) IS NULL OR library_id=?) ORDER BY created_at DESC,id DESC LIMIT 32")))
        .bind(&filter.library_id).bind(&filter.library_id).fetch_all(auth.store.pool()).await.map(Json).map_err(db)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Control {
    action: String,
}
pub(super) async fn control(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Control>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let status = match input.action.as_str() {
        "pause" => "paused",
        "cancel" => "cancelled",
        "resume" => "queued",
        _ => return Err(err(StatusCode::BAD_REQUEST, "invalid_action")),
    };
    let changed = query("UPDATE artwork_collection_jobs SET status=?,lease_token='',lease_until=0,next_run_at=0,updated_at=? WHERE id=? AND status IN ('queued','running','paused')")
        .bind(status).bind(now()).bind(id).execute(auth.store.pool()).await.map_err(db)?.rows_affected();
    if changed == 0 {
        return Err(err(StatusCode::CONFLICT, "collection_not_active"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn authorize_work(auth: &AuthState, job: &Job, work: &str) -> Result<(), Error> {
    let valid: i64 = query_scalar(
        "SELECT COUNT(*) \
            FROM artwork_collection_jobs j \
            JOIN users u ON u.id=j.owner \
            JOIN inventory_works w ON w.id=? \
            JOIN libraries l ON l.id=w.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE j.id=? AND j.lease_token=? AND j.status='running' AND j.lease_until>? \
            AND u.role='administrator' AND u.disabled_at IS NULL AND l.enabled=1 \
            AND n.status!='revoked' AND (l.id=j.library_id OR EXISTS(SELECT 1 \
            FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id \
            WHERE f.library_id=l.id AND f.parent_id=j.library_id AND f.active=1 \
            AND parent.enabled=1))",
    )
    .bind(work)
    .bind(&job.id)
    .bind(&job.lease_token)
    .bind(now())
    .fetch_one(auth.store.pool())
    .await
    .map_err(db)?;
    if valid != 1
        || !crate::product::target_allowed(&auth.store, &job.owner, "work", work)
            .await
            .map_err(|s| err(s, "work_denied"))?
    {
        return Err(err(StatusCode::FORBIDDEN, "collection_access_revoked"));
    }
    Ok(())
}
pub(super) async fn provider_permitted(
    auth: &AuthState,
    job: &Job,
    id: &str,
    library: &str,
    generation: &str,
) -> Result<bool, Error> {
    let valid: i64 = query_scalar("SELECT COUNT(*) \
            FROM artwork_collection_jobs j \
            JOIN users u ON u.id=j.owner \
            JOIN extensions e ON e.id=? \
            JOIN extension_publishers p ON p.id=e.publisher_id \
            JOIN libraries l ON l.id=? \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE j.id=? AND j.lease_token=? AND j.status='running' AND j.lease_until>? \
            AND u.role='administrator' AND u.disabled_at IS NULL AND e.generation=? AND e.enabled=1 \
            AND p.active=1 AND l.enabled=1 AND n.status!='revoked' AND (l.id=j.library_id \
            OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent \
            ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.parent_id=j.library_id \
            AND f.active=1 AND parent.enabled=1)) AND (e.all_libraries=1 OR EXISTS(SELECT 1 \
            FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) \
            OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f \
            ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id \
            AND f.active=1)) AND NOT EXISTS(SELECT 1 FROM automation_policies policy \
            WHERE policy.extension_id=e.id AND policy.library_id=l.id AND policy.mode='disabled')")
        .bind(id).bind(library).bind(&job.id).bind(&job.lease_token).bind(now()).bind(generation).fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(valid == 1)
}

/// Process one work of the next ready pass. `true` when a work (or the end of a pass) was
/// handled, so the worker can go on without waiting.
pub(crate) async fn tick(auth: &AuthState) -> Result<bool, Error> {
    let Ok(_worker) = WORKER.try_lock() else {
        return Ok(false);
    };
    ensure_automatic(auth, false).await?;
    let token = uuid::Uuid::new_v4().to_string();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    // Images follow identification: a pass waits while its library still has works being
    // identified (paused identification, or a later retry, does not hold it).
    let own = crate::task_engine::pipeline::identify_pending("j.library_id", "?");
    let folder = crate::task_engine::pipeline::identify_pending("f.library_id", "?");
    let next: Option<String> = query_scalar(sqlx::AssertSqlSafe(format!("SELECT j.id FROM artwork_collection_jobs j WHERE (j.status='queued' OR (j.status='running' AND j.lease_until<=?)) AND j.next_run_at<=? AND NOT {own} AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.parent_id=j.library_id AND f.active=1 AND {folder}) ORDER BY j.updated_at,j.id LIMIT 1")))
        .bind(now()).bind(now()).bind(now()).bind(now()).fetch_optional(&mut *tx).await.map_err(db)?;
    let Some(id) = next else {
        drop(tx);
        drop(conn);
        return episode_stills_tick(auth).await;
    };
    query("UPDATE artwork_collection_jobs SET status='running',lease_token=?,lease_until=?,updated_at=? WHERE id=?")
        .bind(&token).bind(now()+120).bind(now()).bind(&id).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    drop(conn);
    let job: Job = query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id=?")))
        .bind(&id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let parameters: Input = serde_json::from_str(&job.parameters)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_collection_parameters"))?;
    let auto = parameters.auto.clone();
    let (work, cursor) = if let Some(mode) = auto.as_deref() {
        next_auto_work(auth, &job, mode, parameters.since).await?
    } else {
        let work: Option<String> = query_scalar("SELECT id FROM inventory_works WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND id>? ORDER BY id LIMIT 1")
            .bind(&job.library_id).bind(&job.library_id).bind(&job.cursor_work).fetch_optional(auth.store.pool()).await.map_err(db)?;
        (work, job.cursor_work.clone())
    };
    let Some(work) = work.filter(|_| job.processed < parameters.max_works) else {
        query("UPDATE artwork_collection_jobs SET status='completed',cursor_work=?,lease_until=0,updated_at=? WHERE id=? AND lease_token=? AND status='running'").bind(&cursor).bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
        let _ =
            crate::libraries::ensure_generated_library_artwork(&auth.store, &job.library_id).await;
        return Ok(true);
    };
    let result = image_collection::collect_with_context(
        auth,
        &HeaderMap::new(),
        image_collection::Collect {
            work_id: work.clone(),
            languages: Some(parameters.languages),
            include_neutral: Some(parameters.include_neutral),
            max_images: Some(parameters.max_images),
            one_per_kind: auto.as_deref() == Some("single"),
        },
        Some(super::run::Background::Artwork(&job)),
    )
    .await;
    let (value, error_code, denied) = match result {
        Ok(value) => (value.0, String::new(), false),
        Err((status, body)) => (
            json!({}),
            body.0["error"]
                .as_str()
                .unwrap_or("collection_failed")
                .to_owned(),
            status == StatusCode::FORBIDDEN,
        ),
    };
    // An automatic pass skips a work the providers cannot identify yet: it is taken
    // again by a later pass once its identifiers are known.
    let unidentified = error_code == "external_ids_required";
    let retry = !denied
        && !unidentified
        && job.work_attempts < 2
        && (value["partial"] == true
            || !error_code.is_empty()
            || value["errors"].as_array().is_some_and(|v| !v.is_empty()));
    query("UPDATE artwork_collection_jobs SET status=?,cursor_work=?,processed=processed+?,downloaded=downloaded+?,duplicates=duplicates+?,skipped=skipped+?,errors=errors+?,work_attempts=?,last_error=?,lease_until=0,next_run_at=?,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
        .bind(if denied {"paused"} else {"queued"}).bind(if retry||denied {&cursor} else {&work}).bind(i64::from(!retry&&!denied))
        .bind(value["downloaded"].as_i64().unwrap_or(0)).bind(value["duplicates"].as_i64().unwrap_or(0)).bind(value["skipped"].as_i64().unwrap_or(0))
        .bind(i64::from(!error_code.is_empty())+value["errors"].as_array().map_or(0,|v|v.len() as i64)).bind(if retry {job.work_attempts+1} else {0}).bind(error_code)
        .bind(if retry { now() + 15 } else { 0 }).bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
    if let Some(mode) = auto.as_deref()
        && !retry
        && !denied
        && !unidentified
    {
        mark_auto_attempt(auth, &work, mode).await?;
    }
    Ok(true)
}
