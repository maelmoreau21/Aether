//! Atomic publication of staged films/series and their automation snapshot.

use super::editorial::refresh_editorial_for_works;
use super::{decode, id};
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashMap, path::Path};

pub async fn publish_staged_scan(
    store: &IdentityStore,
    scan_id: &str,
    library_id: &str,
    root: &Path,
    complete: bool,
) -> Result<(), sqlx::Error> {
    let mut conn = store.pool.acquire().await?;
    let mut tx = crate::database::begin_write(&mut conn).await?;

    let configured: Option<(String, String)> = query_as(
        "SELECT l.root, l.kind FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(library_id)
    .fetch_optional(&mut *tx)
    .await?;

    let configured_matches = configured.as_ref().is_some_and(|(c, _)| {
        let root_str = root.to_str().unwrap_or("");
        let c_clean = c.strip_prefix(r"\\?\").unwrap_or(c);
        let root_clean = root_str.strip_prefix(r"\\?\").unwrap_or(root_str);
        c == root_str
            || Path::new(c) == root
            || c_clean == root_clean
            || Path::new(c_clean) == Path::new(root_clean)
    });

    if !configured_matches {
        return Err(sqlx::Error::Protocol(
            "library configuration changed during scan".into(),
        ));
    }
    let (root_snapshot, lib_kind) = configured.unwrap();

    // The mode is fixed by the Core when the unique library job is queued; the Node cannot choose it.
    let refresh_mode: String =
        query_scalar("SELECT refresh_mode FROM scan_jobs WHERE library_id=?")
            .bind(library_id)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or_else(|| "incremental".into());
    let mut affected_work_ids = std::collections::BTreeSet::new();

    // A film renamed or moved inside this library keeps its asset (and so its work, corrections and
    // prepared caches). Needs a complete scan: an unreadable folder is not a vanished one.
    if complete && lib_kind == "films" {
        let (_relocated, works) =
            super::relocation::apply(&mut tx, scan_id, library_id, &root_snapshot).await?;
        affected_work_ids.extend(works);
    }

    if complete {
        // Collect works whose available assets are about to be marked unavailable
        let removed_works: Vec<String> = query_scalar(
            "SELECT DISTINCT e.work_id 
             FROM inventory_assets a 
             JOIN inventory_sources s ON s.id=a.source_id 
             JOIN inventory_versions v ON v.id=s.version_id 
             JOIN inventory_editions e ON e.id=v.edition_id 
             WHERE a.library_id=? AND a.available=1 AND a.relative_path NOT IN (
                 SELECT relative_path FROM scan_staging_files WHERE scan_id=?
             )",
        )
        .bind(library_id)
        .bind(scan_id)
        .fetch_all(&mut *tx)
        .await?;

        for w in removed_works {
            affected_work_ids.insert(w);
        }

        query(
            "UPDATE inventory_assets 
             SET available=0 
             WHERE library_id=? AND available=1 AND relative_path NOT IN (
                 SELECT relative_path FROM scan_staging_files WHERE scan_id=?
             )",
        )
        .bind(library_id)
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

        query(
            "UPDATE inventory_images 
             SET available=0 
             WHERE library_id=? AND available=1 AND relative_path NOT IN (
                 SELECT poster_path FROM scan_staging_files WHERE scan_id=? AND poster_path IS NOT NULL
                 UNION
                 SELECT series_poster_path FROM scan_staging_files WHERE scan_id=? AND series_poster_path IS NOT NULL
             )",
        )
        .bind(library_id)
        .bind(scan_id)
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;
    }

    // Collect works whose assets had errors in this scan
    let error_works: Vec<String> = query_scalar(
        "SELECT DISTINCT e.work_id 
         FROM inventory_assets a 
         JOIN inventory_sources s ON s.id=a.source_id 
         JOIN inventory_versions v ON v.id=s.version_id 
         JOIN inventory_editions e ON e.id=v.edition_id 
         WHERE a.library_id=? AND a.available=1 AND a.relative_path IN (
             SELECT relative_path FROM scan_staging_issues WHERE scan_id=? AND kind='error' AND relative_path != ''
         )",
    )
    .bind(library_id)
    .bind(scan_id)
    .fetch_all(&mut *tx)
    .await?;

    for w in error_works {
        affected_work_ids.insert(w);
    }

    query(
        "UPDATE inventory_assets 
         SET available=0 
         WHERE library_id=? AND relative_path IN (
             SELECT relative_path FROM scan_staging_issues WHERE scan_id=? AND kind='error' AND relative_path != ''
         )",
    )
    .bind(library_id)
    .bind(scan_id)
    .execute(&mut *tx)
    .await?;

    query(
        "UPDATE inventory_assets 
         SET available=1 
         WHERE library_id=? AND available=0 AND relative_path IN (
             SELECT relative_path FROM scan_staging_files WHERE scan_id=? AND reused=1
         )",
    )
    .bind(library_id)
    .bind(scan_id)
    .execute(&mut *tx)
    .await?;

    if lib_kind == "series" {
        // Deterministic series metadata and poster selection per series_identity_key:
        // 1. Metadata tuple (series_title, series_year, series_external_ids_json, series_editorial_json)
        //    is selected atomically from a single representative row (ranked_meta.rn = 1), ordered by:
        //    - Valid NFO presence (is_valid_nfo = 1 first)
        //    - Non-empty external IDs
        //    - Year presence
        //    - Non-empty title
        //    - Lexical relative_path ASC (unique per scan_id, independent of SQLite insertion order)
        // 2. Poster tuple (series_poster_path, series_poster_size, series_poster_modified_ns,
        //    series_poster_mime, series_poster_priority) is selected atomically from a single
        //    representative row (ranked_poster.rn = 1), ordered by:
        //    - series_poster_priority ASC (lower number = higher priority)
        //    - series_poster_path ASC
        //    - relative_path ASC
        let staged_series = query(
            "WITH ranked_meta AS (
                SELECT
                    series_identity_key,
                    series_title,
                    series_year,
                    series_external_ids_json,
                    series_editorial_json,
                    CASE
                        WHEN series_editorial_json IS NOT NULL
                             AND json_valid(series_editorial_json) = 1
                             AND json_extract(series_editorial_json, '$._outcome') IS NULL
                             AND series_editorial_json != '{}'
                        THEN 1
                        ELSE 0
                    END AS is_valid_nfo,
                    MAX(
                        CASE
                            WHEN (series_editorial_json IS NOT NULL
                                  AND (json_valid(series_editorial_json) = 0
                                       OR json_extract(series_editorial_json, '$._outcome') = 'invalid'))
                                 OR (series_editorial_json IS NULL AND editorial_outcome = 'invalid')
                            THEN 1
                            ELSE 0
                        END
                    ) OVER (PARTITION BY series_identity_key) AS has_invalid_nfo,
                    ROW_NUMBER() OVER (
                        PARTITION BY series_identity_key
                        ORDER BY
                            CASE
                                WHEN series_editorial_json IS NOT NULL
                                     AND json_valid(series_editorial_json) = 1
                                     AND json_extract(series_editorial_json, '$._outcome') IS NULL
                                     AND series_editorial_json != '{}'
                                THEN 0
                                ELSE 1
                            END ASC,
                            CASE
                                WHEN series_external_ids_json IS NOT NULL
                                     AND series_external_ids_json != ''
                                     AND series_external_ids_json != '{}'
                                THEN 0
                                ELSE 1
                            END ASC,
                            CASE WHEN series_year IS NOT NULL THEN 0 ELSE 1 END ASC,
                            CASE WHEN series_title IS NOT NULL AND series_title != '' THEN 0 ELSE 1 END ASC,
                            relative_path ASC
                    ) AS rn
                FROM scan_staging_files
                WHERE scan_id = ? AND series_identity_key IS NOT NULL
            ),
            ranked_poster AS (
                SELECT
                    series_identity_key,
                    series_poster_path,
                    series_poster_size,
                    series_poster_modified_ns,
                    series_poster_mime,
                    series_poster_priority,
                    ROW_NUMBER() OVER (
                        PARTITION BY series_identity_key
                        ORDER BY
                            series_poster_priority ASC,
                            series_poster_path ASC,
                            relative_path ASC
                    ) AS rn
                FROM scan_staging_files
                WHERE scan_id = ?
                  AND series_identity_key IS NOT NULL
                  AND series_poster_path IS NOT NULL
                  AND series_poster_size IS NOT NULL
                  AND series_poster_modified_ns IS NOT NULL
                  AND series_poster_mime IS NOT NULL
                  AND series_poster_priority IS NOT NULL
            )
            SELECT
                m.series_identity_key,
                m.series_title,
                m.series_year,
                m.series_external_ids_json,
                m.series_editorial_json,
                m.is_valid_nfo,
                m.has_invalid_nfo,
                p.series_poster_path,
                p.series_poster_size,
                p.series_poster_modified_ns,
                p.series_poster_mime,
                p.series_poster_priority
            FROM ranked_meta m
            LEFT JOIN ranked_poster p
                ON p.series_identity_key = m.series_identity_key AND p.rn = 1
            WHERE m.rn = 1
            ORDER BY m.series_identity_key ASC",
        )
        .bind(scan_id)
        .bind(scan_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut series_work_map: HashMap<String, String> = HashMap::new();

        for s_row in staged_series {
            let series_key: String = s_row.get("series_identity_key");
            let series_scoped_key = format!("{}\n{}", root_snapshot, series_key);
            let is_valid_nfo: i64 = s_row.get("is_valid_nfo");
            let has_invalid_nfo: i64 = s_row.get("has_invalid_nfo");
            let row_title: Option<String> = s_row.get("series_title");
            let row_year: Option<i64> = s_row.get("series_year");
            let row_ext_ids: String = s_row
                .get::<Option<String>, _>("series_external_ids_json")
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "{}".into());
            let row_editorial: Option<String> = s_row.get("series_editorial_json");

            let (s_title, s_year, s_ext_ids, s_editorial) = if is_valid_nfo == 1 {
                let mut nfo_val: Value = row_editorial
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({}));
                let title = nfo_val
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| row_title.filter(|s| !s.trim().is_empty()))
                    .unwrap_or_else(|| series_key.clone());
                let year = nfo_val.get("year").and_then(Value::as_i64).or(row_year);
                let ext_ids = if let Some(ext_obj) = nfo_val
                    .get("external_ids")
                    .and_then(Value::as_object)
                    .filter(|o| !o.is_empty())
                {
                    json!(ext_obj).to_string()
                } else {
                    row_ext_ids
                };
                if let Some(obj) = nfo_val.as_object_mut() {
                    obj.entry("provenance").or_insert_with(|| json!("nfo"));
                    if (!obj.contains_key("external_ids")
                        || obj
                            .get("external_ids")
                            .and_then(Value::as_object)
                            .is_some_and(|o| o.is_empty()))
                        && let Ok(parsed_ids) = serde_json::from_str::<Value>(&ext_ids)
                    {
                        obj.insert("external_ids".to_string(), parsed_ids);
                    }
                }
                (title, year, ext_ids, nfo_val.to_string())
            } else {
                let title = row_title
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| series_key.clone());
                (title, row_year, row_ext_ids, "{}".to_string())
            };
            let s_title_sort = s_title.to_lowercase();

            let existing_series = query(
                "SELECT id, editorial_json FROM inventory_works WHERE library_id=? AND identity_key=?",
            )
            .bind(library_id)
            .bind(&series_scoped_key)
            .fetch_optional(&mut *tx)
            .await?;

            let series_work_id = if let Some(row) = existing_series {
                let swid: String = row.get("id");
                let prev_editorial: String = row.get("editorial_json");
                // Preserve already-validated NFO metadata if the replacement NFO in this scan is invalid.
                if refresh_mode == "new_only"
                    || (refresh_mode == "missing_metadata"
                        && serde_json::from_str::<Value>(&prev_editorial)
                            .ok()
                            .is_some_and(|v| {
                                v.get("provenance").and_then(Value::as_str) == Some("nfo")
                            }))
                    || (is_valid_nfo == 0 && has_invalid_nfo == 1 && prev_editorial != "{}")
                {
                    query("UPDATE inventory_works SET kind='series' WHERE id=?")
                        .bind(&swid)
                        .execute(&mut *tx)
                        .await?;
                } else {
                    query(
                        "UPDATE inventory_works SET title=?, year=?, external_ids_json=?, title_sort=?, editorial_json=?, kind='series' WHERE id=?",
                    )
                    .bind(&s_title)
                    .bind(s_year)
                    .bind(&s_ext_ids)
                    .bind(&s_title_sort)
                    .bind(&s_editorial)
                    .bind(&swid)
                    .execute(&mut *tx)
                    .await?;
                }
                swid
            } else {
                let swid = id();
                query(
                    "INSERT INTO inventory_works (id, library_id, identity_key, title, year, external_ids_json, title_sort, editorial_json, kind) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'series')",
                )
                .bind(&swid)
                .bind(library_id)
                .bind(&series_scoped_key)
                .bind(&s_title)
                .bind(s_year)
                .bind(&s_ext_ids)
                .bind(&s_title_sort)
                .bind(&s_editorial)
                .execute(&mut *tx)
                .await?;
                swid
            };

            affected_work_ids.insert(series_work_id.clone());
            series_work_map.insert(series_key.clone(), series_work_id.clone());

            let poster_path: Option<String> = s_row.get("series_poster_path");
            if let Some(poster_path) = poster_path {
                let poster_size: i64 = s_row.get("series_poster_size");
                let poster_modified_ns: String = s_row.get("series_poster_modified_ns");
                let poster_mime: String = s_row.get("series_poster_mime");
                let poster_priority: i64 = s_row.get("series_poster_priority");

                if complete {
                    query(
                        "UPDATE inventory_images SET available=0 WHERE work_id=? AND root_snapshot=? AND relative_path != ?",
                    )
                    .bind(&series_work_id)
                    .bind(&root_snapshot)
                    .bind(&poster_path)
                    .execute(&mut *tx)
                    .await?;
                }

                query(
                    "INSERT INTO inventory_images(id, work_id, library_id, root_snapshot, relative_path, size, modified_ns, mime, priority, available)
                     VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
                     ON CONFLICT(work_id, root_snapshot, relative_path) DO UPDATE SET
                        size=excluded.size, modified_ns=excluded.modified_ns, mime=excluded.mime, priority=excluded.priority, available=1",
                )
                .bind(id())
                .bind(&series_work_id)
                .bind(library_id)
                .bind(&root_snapshot)
                .bind(&poster_path)
                .bind(poster_size)
                .bind(&poster_modified_ns)
                .bind(&poster_mime)
                .bind(poster_priority)
                .execute(&mut *tx)
                .await?;
            }
        }

        // Seasons
        let staged_seasons = query(
            "SELECT series_identity_key, season_number
             FROM scan_staging_files
             WHERE scan_id=? AND series_identity_key IS NOT NULL AND season_number IS NOT NULL
             GROUP BY series_identity_key, season_number
             ORDER BY series_identity_key, season_number",
        )
        .bind(scan_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut season_map: HashMap<(String, i64), String> = HashMap::new();

        for s_row in staged_seasons {
            let series_key: String = s_row.get("series_identity_key");
            let season_num: i64 = s_row.get("season_number");
            let Some(series_work_id) = series_work_map.get(&series_key) else {
                continue;
            };

            let existing_season =
                query("SELECT id FROM inventory_seasons WHERE series_id=? AND season_number=?")
                    .bind(series_work_id)
                    .bind(season_num)
                    .fetch_optional(&mut *tx)
                    .await?;

            let season_id = if let Some(row) = existing_season {
                row.get::<String, _>("id")
            } else {
                let sid = id();
                let season_title = if season_num == 0 {
                    "Hors-série".to_string()
                } else {
                    format!("Saison {}", season_num)
                };
                query(
                    "INSERT INTO inventory_seasons(id, series_id, season_number, title) VALUES(?, ?, ?, ?)",
                )
                .bind(&sid)
                .bind(series_work_id)
                .bind(season_num)
                .bind(season_title)
                .execute(&mut *tx)
                .await?;
                sid
            };

            season_map.insert((series_key, season_num), season_id);
        }

        // Episodes: Keyset pagination for new / modified files (reused=0)
        let mut last_new_path = String::new();
        loop {
            let chunk = query(
                "SELECT relative_path, identity_key, title, year, external_ids_json, probe_json, size, modified_ns, mime,
                        series_identity_key, season_number, episode_number
                 FROM scan_staging_files 
                 WHERE scan_id=? AND reused=0 AND relative_path > ? 
                 ORDER BY relative_path ASC 
                 LIMIT 250",
            )
            .bind(scan_id)
            .bind(&last_new_path)
            .fetch_all(&mut *tx)
            .await?;

            if chunk.is_empty() {
                break;
            }

            for row in &chunk {
                let relative_path: String = row.get("relative_path");
                let file_identity_key: String = row.get("identity_key");
                let series_identity_key: Option<String> = row.get("series_identity_key");
                let season_number: Option<i64> = row.get("season_number");
                let episode_number: Option<i64> = row.get("episode_number");

                let identity_key = if let (Some(s_key), Some(s_num), Some(e_num)) =
                    (&series_identity_key, season_number, episode_number)
                {
                    format!("{}\n{}/s{:02}e{:02}", root_snapshot, s_key, s_num, e_num)
                } else {
                    format!("{}\n{}", root_snapshot, file_identity_key)
                };

                let title: String = row.get("title");
                let year: Option<i64> = row.get("year");
                let external_ids_json: String = row.get("external_ids_json");
                let probe_json: Option<String> = row.get("probe_json");
                let probe_json = probe_json.unwrap_or_else(|| "{}".into());
                let size: i64 = row.get("size");
                let modified_ns: String = row.get("modified_ns");
                let mime: String = row.get("mime");

                let existing = query(
                    "SELECT a.id,w.id as work_id,w.identity_key,s.root_snapshot 
                     FROM inventory_assets a 
                     JOIN inventory_sources s ON s.id=a.source_id 
                     JOIN inventory_versions v ON v.id=s.version_id 
                     JOIN inventory_editions e ON e.id=v.edition_id 
                     JOIN inventory_works w ON w.id=e.work_id 
                     WHERE a.library_id=? AND a.relative_path=?",
                )
                .bind(library_id)
                .bind(&relative_path)
                .fetch_optional(&mut *tx)
                .await?;

                let previous_asset_id = existing.as_ref().map(|r| r.get::<String, _>("id"));
                let is_existing_matching = existing.as_ref().is_some_and(|r| {
                    r.get::<String, _>("identity_key") == identity_key
                        && r.get::<String, _>("root_snapshot") == root_snapshot
                });

                let (asset_id, ep_work_id) = if is_existing_matching {
                    let r = existing.unwrap();
                    let asset_id: String = r.get("id");
                    let ep_work_id: String = r.get("work_id");
                    query(
                        "UPDATE inventory_assets SET size=?,modified_ns=?,mime=?,probe_json=?,available=1 WHERE id=?",
                    )
                    .bind(size)
                    .bind(&modified_ns)
                    .bind(&mime)
                    .bind(&probe_json)
                    .bind(&asset_id)
                    .execute(&mut *tx)
                    .await?;

                    (asset_id, ep_work_id)
                } else {
                    let work = query(
                        "SELECT id FROM inventory_works WHERE library_id=? AND identity_key=?",
                    )
                    .bind(library_id)
                    .bind(&identity_key)
                    .fetch_optional(&mut *tx)
                    .await?;

                    let ep_work_id = if let Some(row) = work {
                        row.get::<String, _>("id")
                    } else {
                        let wid = id();
                        let title_sort = title.to_lowercase();
                        query(
                            "INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES(?,?,?,?,?,?,?,'episode')",
                        )
                        .bind(&wid)
                        .bind(library_id)
                        .bind(&identity_key)
                        .bind(&title)
                        .bind(year)
                        .bind(&external_ids_json)
                        .bind(title_sort)
                        .execute(&mut *tx)
                        .await?;
                        wid
                    };

                    if let (Some(s_key), Some(s_num), Some(e_num)) =
                        (&series_identity_key, season_number, episode_number)
                        && let Some(season_id) = season_map.get(&(s_key.clone(), s_num))
                    {
                        query(
                            "INSERT INTO inventory_episodes(id, season_id, episode_number, work_id)
                             VALUES(?, ?, ?, ?)
                             ON CONFLICT(season_id, episode_number) DO UPDATE SET work_id=excluded.work_id",
                        )
                        .bind(id())
                        .bind(season_id)
                        .bind(e_num)
                        .bind(&ep_work_id)
                        .execute(&mut *tx)
                        .await?;
                    }

                    let edition_spec = crate::consolidation::derive_edition_spec(&relative_path);
                    let parsed_probe = decode(&probe_json).unwrap_or_default();
                    let version_spec =
                        crate::consolidation::derive_version_spec(&parsed_probe, &mime);
                    let edition_id = query_scalar::<_, String>(
                        "SELECT e.id FROM inventory_editions e \
                         LEFT JOIN inventory_edition_metadata em ON em.edition_id=e.id \
                         WHERE e.work_id=? AND COALESCE(em.edition_key,'standard')=? LIMIT 1",
                    )
                    .bind(&ep_work_id)
                    .bind(&edition_spec.key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    let edition_id = if let Some(edition_id) = edition_id {
                        query(
                            "INSERT INTO inventory_edition_metadata(edition_id,edition_key,label) VALUES(?,?,?) \
                             ON CONFLICT(edition_id) DO UPDATE SET edition_key=excluded.edition_key,label=excluded.label",
                        )
                        .bind(&edition_id)
                        .bind(&edition_spec.key)
                        .bind(&edition_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        edition_id
                    } else {
                        let edition_id = id();
                        query("INSERT INTO inventory_editions VALUES(?,?)")
                            .bind(&edition_id)
                            .bind(&ep_work_id)
                            .execute(&mut *tx)
                            .await?;
                        query(
                            "INSERT INTO inventory_edition_metadata(edition_id,edition_key,label) VALUES(?,?,?) \
                             ON CONFLICT(edition_id) DO UPDATE SET edition_key=excluded.edition_key,label=excluded.label",
                        )
                        .bind(&edition_id)
                        .bind(&edition_spec.key)
                        .bind(&edition_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        edition_id
                    };

                    let existing_version_id = query_scalar::<_, String>(
                        "SELECT v.id FROM inventory_versions v \
                         LEFT JOIN inventory_version_metadata vm ON vm.version_id=v.id \
                         WHERE v.edition_id=? AND COALESCE(vm.version_key,'default')=? LIMIT 1",
                    )
                    .bind(&edition_id)
                    .bind(&version_spec.key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    let version_id = if let Some(version_id) = existing_version_id {
                        query(
                            "INSERT INTO inventory_version_metadata(version_id,version_key,label) VALUES(?,?,?) \
                             ON CONFLICT(version_id) DO UPDATE SET version_key=excluded.version_key,label=excluded.label",
                        )
                        .bind(&version_id)
                        .bind(&version_spec.key)
                        .bind(&version_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        version_id
                    } else {
                        let version_id = id();
                        query("INSERT INTO inventory_versions VALUES(?,?)")
                            .bind(&version_id)
                            .bind(&edition_id)
                            .execute(&mut *tx)
                            .await?;
                        query(
                            "INSERT INTO inventory_version_metadata(version_id,version_key,label) VALUES(?,?,?) \
                             ON CONFLICT(version_id) DO UPDATE SET version_key=excluded.version_key,label=excluded.label",
                        )
                        .bind(&version_id)
                        .bind(&version_spec.key)
                        .bind(&version_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        version_id
                    };
                    let source_id = id();
                    let is_new_asset = previous_asset_id.is_none();
                    let asset_id = previous_asset_id.unwrap_or_else(id);
                    if !is_new_asset {
                        query("DELETE FROM inventory_editorial WHERE asset_id=?")
                            .bind(&asset_id)
                            .execute(&mut *tx)
                            .await?;
                    }
                    query(
                        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,?)",
                    )
                    .bind(&source_id)
                    .bind(&version_id)
                    .bind(library_id)
                    .bind(&root_snapshot)
                    .execute(&mut *tx)
                    .await?;
                    query(
                        "INSERT INTO inventory_assets VALUES(?,?,?,?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET source_id=excluded.source_id,size=excluded.size,modified_ns=excluded.modified_ns,mime=excluded.mime,probe_json=excluded.probe_json,available=1",
                    )
                    .bind(&asset_id)
                    .bind(source_id)
                    .bind(library_id)
                    .bind(&relative_path)
                    .bind(size)
                    .bind(&modified_ns)
                    .bind(&mime)
                    .bind(&probe_json)
                    .execute(&mut *tx)
                    .await?;

                    (asset_id, ep_work_id)
                };

                affected_work_ids.insert(ep_work_id.clone());

                if let Ok(probe) = decode(&probe_json) {
                    let indexes: Vec<i64> =
                        probe.streams.iter().map(|t| i64::from(t.index)).collect();
                    let previous =
                        query("SELECT id,stream_index FROM inventory_tracks WHERE asset_id=?")
                            .bind(&asset_id)
                            .fetch_all(&mut *tx)
                            .await?;
                    for r in previous {
                        if !indexes.contains(&r.get("stream_index")) {
                            query("DELETE FROM inventory_tracks WHERE id=?")
                                .bind(r.get::<String, _>("id"))
                                .execute(&mut *tx)
                                .await?;
                        }
                    }
                    for track in &probe.streams {
                        query(
                            "INSERT INTO inventory_tracks VALUES(?,?,?,?) 
                             ON CONFLICT(asset_id,stream_index) DO UPDATE SET metadata_json=excluded.metadata_json",
                        )
                        .bind(id())
                        .bind(&asset_id)
                        .bind(i64::from(track.index))
                        .bind(json!(track).to_string())
                        .execute(&mut *tx)
                        .await?;
                    }
                }
            }

            last_new_path = chunk.last().unwrap().get("relative_path");
        }
    } else {
        // Keyset pagination in bounded chunks (250 items at a time) for new / modified files.
        // Avoids loading all probe_json blobs into RAM simultaneously, strictly bounding RSS.
        let mut last_new_path = String::new();
        loop {
            let chunk = query(
                "SELECT relative_path, identity_key, title, year, external_ids_json, probe_json, size, modified_ns, mime 
                 FROM scan_staging_files 
                 WHERE scan_id=? AND reused=0 AND relative_path > ? 
                 ORDER BY relative_path ASC 
                 LIMIT 250",
            )
            .bind(scan_id)
            .bind(&last_new_path)
            .fetch_all(&mut *tx)
            .await?;

            if chunk.is_empty() {
                break;
            }

            for row in &chunk {
                let relative_path: String = row.get("relative_path");
                let file_identity_key: String = row.get("identity_key");
                let identity_key = format!("{}\n{}", root_snapshot, file_identity_key);
                let title: String = row.get("title");
                let year: Option<i64> = row.get("year");
                let external_ids_json: String = row.get("external_ids_json");
                let probe_json: Option<String> = row.get("probe_json");
                let probe_json = probe_json.unwrap_or_else(|| "{}".into());
                let size: i64 = row.get("size");
                let modified_ns: String = row.get("modified_ns");
                let mime: String = row.get("mime");

                let existing = query(
                    "SELECT a.id,w.id as work_id,w.identity_key,s.root_snapshot 
                     FROM inventory_assets a 
                     JOIN inventory_sources s ON s.id=a.source_id 
                     JOIN inventory_versions v ON v.id=s.version_id 
                     JOIN inventory_editions e ON e.id=v.edition_id 
                     JOIN inventory_works w ON w.id=e.work_id 
                     WHERE a.library_id=? AND a.relative_path=?",
                )
                .bind(library_id)
                .bind(&relative_path)
                .fetch_optional(&mut *tx)
                .await?;

                let previous_asset_id = existing.as_ref().map(|r| r.get::<String, _>("id"));
                let is_existing_matching = existing.as_ref().is_some_and(|r| {
                    r.get::<String, _>("identity_key") == identity_key
                        && r.get::<String, _>("root_snapshot") == root_snapshot
                });

                let (asset_id, work_id) = if is_existing_matching {
                    let r = existing.unwrap();
                    let asset_id: String = r.get("id");
                    let work_id: String = r.get("work_id");
                    query(
                        "UPDATE inventory_assets SET size=?,modified_ns=?,mime=?,probe_json=?,available=1 WHERE id=?",
                    )
                    .bind(size)
                    .bind(&modified_ns)
                    .bind(&mime)
                    .bind(&probe_json)
                    .bind(&asset_id)
                    .execute(&mut *tx)
                    .await?;

                    (asset_id, work_id)
                } else {
                    let work = query(
                        "SELECT id FROM inventory_works WHERE library_id=? AND identity_key=?",
                    )
                    .bind(library_id)
                    .bind(&identity_key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    let work_id = if let Some(row) = work {
                        row.get::<String, _>("id")
                    } else {
                        let work_id = id();
                        let title_sort = title.to_lowercase();
                        query(
                            "INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES(?,?,?,?,?,?,?,'film')",
                        )
                        .bind(&work_id)
                        .bind(library_id)
                        .bind(&identity_key)
                        .bind(&title)
                        .bind(year)
                        .bind(&external_ids_json)
                        .bind(title_sort)
                        .execute(&mut *tx)
                        .await?;
                        work_id
                    };

                    let edition_spec = crate::consolidation::derive_edition_spec(&relative_path);
                    let parsed_probe = decode(&probe_json).unwrap_or_default();
                    let version_spec =
                        crate::consolidation::derive_version_spec(&parsed_probe, &mime);
                    let edition_id = query_scalar::<_, String>(
                        "SELECT e.id FROM inventory_editions e \
                         LEFT JOIN inventory_edition_metadata em ON em.edition_id=e.id \
                         WHERE e.work_id=? AND COALESCE(em.edition_key,'standard')=? LIMIT 1",
                    )
                    .bind(&work_id)
                    .bind(&edition_spec.key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    let edition_id = if let Some(edition_id) = edition_id {
                        query(
                            "INSERT INTO inventory_edition_metadata(edition_id,edition_key,label) VALUES(?,?,?) \
                             ON CONFLICT(edition_id) DO UPDATE SET edition_key=excluded.edition_key,label=excluded.label",
                        )
                        .bind(&edition_id)
                        .bind(&edition_spec.key)
                        .bind(&edition_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        edition_id
                    } else {
                        let edition_id = id();
                        query("INSERT INTO inventory_editions VALUES(?,?)")
                            .bind(&edition_id)
                            .bind(&work_id)
                            .execute(&mut *tx)
                            .await?;
                        query(
                            "INSERT INTO inventory_edition_metadata(edition_id,edition_key,label) VALUES(?,?,?) \
                             ON CONFLICT(edition_id) DO UPDATE SET edition_key=excluded.edition_key,label=excluded.label",
                        )
                        .bind(&edition_id)
                        .bind(&edition_spec.key)
                        .bind(&edition_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        edition_id
                    };

                    let existing_version_id = query_scalar::<_, String>(
                        "SELECT v.id FROM inventory_versions v \
                         LEFT JOIN inventory_version_metadata vm ON vm.version_id=v.id \
                         WHERE v.edition_id=? AND COALESCE(vm.version_key,'default')=? LIMIT 1",
                    )
                    .bind(&edition_id)
                    .bind(&version_spec.key)
                    .fetch_optional(&mut *tx)
                    .await?;
                    let version_id = if let Some(version_id) = existing_version_id {
                        query(
                            "INSERT INTO inventory_version_metadata(version_id,version_key,label) VALUES(?,?,?) \
                             ON CONFLICT(version_id) DO UPDATE SET version_key=excluded.version_key,label=excluded.label",
                        )
                        .bind(&version_id)
                        .bind(&version_spec.key)
                        .bind(&version_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        version_id
                    } else {
                        let version_id = id();
                        query("INSERT INTO inventory_versions VALUES(?,?)")
                            .bind(&version_id)
                            .bind(&edition_id)
                            .execute(&mut *tx)
                            .await?;
                        query(
                            "INSERT INTO inventory_version_metadata(version_id,version_key,label) VALUES(?,?,?) \
                             ON CONFLICT(version_id) DO UPDATE SET version_key=excluded.version_key,label=excluded.label",
                        )
                        .bind(&version_id)
                        .bind(&version_spec.key)
                        .bind(&version_spec.label)
                        .execute(&mut *tx)
                        .await?;
                        version_id
                    };
                    let source_id = id();
                    let is_new_asset = previous_asset_id.is_none();
                    let asset_id = previous_asset_id.unwrap_or_else(id);
                    if !is_new_asset {
                        query("DELETE FROM inventory_editorial WHERE asset_id=?")
                            .bind(&asset_id)
                            .execute(&mut *tx)
                            .await?;
                    }
                    query(
                        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,?)",
                    )
                    .bind(&source_id)
                    .bind(&version_id)
                    .bind(library_id)
                    .bind(&root_snapshot)
                    .execute(&mut *tx)
                    .await?;
                    query(
                        "INSERT INTO inventory_assets VALUES(?,?,?,?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET source_id=excluded.source_id,size=excluded.size,modified_ns=excluded.modified_ns,mime=excluded.mime,probe_json=excluded.probe_json,available=1",
                    )
                    .bind(&asset_id)
                    .bind(source_id)
                    .bind(library_id)
                    .bind(&relative_path)
                    .bind(size)
                    .bind(&modified_ns)
                    .bind(&mime)
                    .bind(&probe_json)
                    .execute(&mut *tx)
                    .await?;

                    (asset_id, work_id)
                };

                affected_work_ids.insert(work_id.clone());

                if let Ok(probe) = decode(&probe_json) {
                    let indexes: Vec<i64> =
                        probe.streams.iter().map(|t| i64::from(t.index)).collect();
                    let previous =
                        query("SELECT id,stream_index FROM inventory_tracks WHERE asset_id=?")
                            .bind(&asset_id)
                            .fetch_all(&mut *tx)
                            .await?;
                    for r in previous {
                        if !indexes.contains(&r.get("stream_index")) {
                            query("DELETE FROM inventory_tracks WHERE id=?")
                                .bind(r.get::<String, _>("id"))
                                .execute(&mut *tx)
                                .await?;
                        }
                    }
                    for track in &probe.streams {
                        query(
                            "INSERT INTO inventory_tracks VALUES(?,?,?,?) 
                             ON CONFLICT(asset_id,stream_index) DO UPDATE SET metadata_json=excluded.metadata_json",
                        )
                        .bind(id())
                        .bind(&asset_id)
                        .bind(i64::from(track.index))
                        .bind(json!(track).to_string())
                        .execute(&mut *tx)
                        .await?;
                    }
                }
            }

            last_new_path = chunk.last().unwrap().get("relative_path");
        }
    }

    // Unified single-pass keyset pagination for artwork and editorial across all staged files.
    // LEFT JOIN with inventory_editorial and inventory_images eliminates thousands of redundant individual queries
    // on unchanged media, reducing rescan publication time by over 90%.
    let mut last_staged_path = String::new();
    loop {
        let chunk = query(
            "SELECT s.artwork_json, s.relative_path, s.poster_path, s.poster_size, s.poster_modified_ns, s.poster_mime, s.poster_priority,
                    s.title, s.year, s.external_ids_json, s.editorial_outcome, s.editorial_json, s.editorial_path, s.editorial_priority,
                    a.id as asset_id, e.work_id,
                    ed.fallback_json as curr_fallback, ed.nfo_json as curr_nfo, ed.nfo_path as curr_nfo_path, ed.nfo_priority as curr_nfo_priority,
                    img.size as curr_img_size, img.modified_ns as curr_img_mod, img.priority as curr_img_prio, img.available as curr_img_avail
             FROM scan_staging_files s
             JOIN inventory_assets a ON a.library_id=s.library_id AND a.relative_path=s.relative_path
             JOIN inventory_sources src ON src.id=a.source_id
             JOIN inventory_versions v ON v.id=src.version_id
             JOIN inventory_editions e ON e.id=v.edition_id
             LEFT JOIN inventory_editorial ed ON ed.asset_id=a.id
             LEFT JOIN inventory_images img ON img.work_id=e.work_id AND img.root_snapshot=src.root_snapshot AND img.relative_path=s.poster_path
             WHERE s.scan_id=? AND s.relative_path > ?
             ORDER BY s.relative_path ASC
             LIMIT 250",
        )
        .bind(scan_id)
        .bind(&last_staged_path)
        .fetch_all(&mut *tx)
        .await?;

        if chunk.is_empty() {
            break;
        }

        for row in &chunk {
            let work_id: String = row.get("work_id");
            let artwork: std::collections::BTreeMap<String, media_contracts::media::LocalImage> =
                serde_json::from_str(row.get("artwork_json")).unwrap_or_default();
            for (role, image) in artwork {
                let (owner, role) = if let Some(role) = role.strip_prefix("series/") {
                    let owner: Option<String> = query_scalar("SELECT s.series_id FROM inventory_seasons s JOIN inventory_episodes ep ON ep.season_id=s.id WHERE ep.work_id=?").bind(&work_id).fetch_optional(&mut *tx).await?;
                    let Some(owner) = owner else {
                        continue;
                    };
                    (owner, role.to_owned())
                } else {
                    (work_id.clone(), role)
                };
                query("INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available,role) VALUES(?,?,?,?,?,?,?,?,100,1,?) ON CONFLICT(work_id,root_snapshot,relative_path) DO UPDATE SET size=excluded.size,modified_ns=excluded.modified_ns,mime=excluded.mime,available=1,role=excluded.role")
                    .bind(id()).bind(owner).bind(library_id).bind(&root_snapshot).bind(&image.path).bind(image.size_bytes as i64).bind(&image.modified_ns).bind(&image.mime).bind(role).execute(&mut *tx).await?;
            }
            let poster_path: Option<String> = row.get("poster_path");
            if let Some(poster_path) = poster_path {
                let poster_size: i64 = row.get("poster_size");
                let poster_modified_ns: String = row.get("poster_modified_ns");
                let poster_mime: String = row.get("poster_mime");
                let poster_priority: i64 = row.get("poster_priority");

                let curr_img_size: Option<i64> = row.get("curr_img_size");
                let curr_img_mod: Option<String> = row.get("curr_img_mod");
                let curr_img_prio: Option<i64> = row.get("curr_img_prio");
                let curr_img_avail: Option<i64> = row.get("curr_img_avail");

                let poster_needs_update = curr_img_avail != Some(1)
                    || curr_img_size != Some(poster_size)
                    || curr_img_mod.as_deref() != Some(&poster_modified_ns)
                    || curr_img_prio != Some(poster_priority);

                if poster_needs_update {
                    query(
                        "INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available) 
                         VALUES(?,?,?,?,?,?,?,?,?,1) 
                         ON CONFLICT(work_id,root_snapshot,relative_path) DO UPDATE SET 
                            size=excluded.size, modified_ns=excluded.modified_ns, mime=excluded.mime, priority=excluded.priority, available=1",
                    )
                    .bind(id())
                    .bind(&work_id)
                    .bind(library_id)
                    .bind(&root_snapshot)
                    .bind(&poster_path)
                    .bind(poster_size)
                    .bind(&poster_modified_ns)
                    .bind(&poster_mime)
                    .bind(poster_priority)
                    .execute(&mut *tx)
                    .await?;
                }
            }

            let asset_id: String = row.get("asset_id");
            let title: String = row.get("title");
            let year: Option<i64> = row.get("year");
            let ext_ids: String = row.get("external_ids_json");
            let outcome: String = row.get("editorial_outcome");
            let nfo_json: Option<String> = row.get("editorial_json");
            let nfo_path: Option<String> = row.get("editorial_path");
            let nfo_priority: Option<i64> = row.get("editorial_priority");

            let fallback = json!({
                "title": title,
                "year": year,
                "external_ids": serde_json::from_str::<Value>(&ext_ids).unwrap_or_else(|_| json!({}))
            });
            let fallback_str = fallback.to_string();

            let curr_fallback: Option<String> = row.get("curr_fallback");
            let curr_nfo: Option<String> = row.get("curr_nfo");
            let curr_path: Option<String> = row.get("curr_nfo_path");
            let curr_priority: Option<i64> = row.get("curr_nfo_priority");

            // Technical inventory and sidecar validation always refresh. Existing editorial data
            // survives only while its asset/source identity still matches (replacement clears it above).
            if (refresh_mode == "new_only" && curr_fallback.is_some())
                || (refresh_mode == "missing_metadata" && curr_nfo.is_some())
            {
                continue;
            }
            let changed = curr_fallback.as_deref() != Some(&fallback_str)
                || curr_nfo != nfo_json
                || curr_path != nfo_path
                || curr_priority != nfo_priority;

            if changed {
                affected_work_ids.insert(work_id);

                query(
                    "INSERT INTO inventory_editorial(asset_id,fallback_json) VALUES(?,?) 
                     ON CONFLICT(asset_id) DO UPDATE SET fallback_json=excluded.fallback_json",
                )
                .bind(&asset_id)
                .bind(&fallback_str)
                .execute(&mut *tx)
                .await?;

                match outcome.as_str() {
                    "missing" => {
                        query(
                            "UPDATE inventory_editorial SET nfo_json=NULL,nfo_path=NULL,nfo_priority=NULL WHERE asset_id=?",
                        )
                        .bind(&asset_id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    "valid" => {
                        query(
                            "UPDATE inventory_editorial SET nfo_json=?,nfo_path=?,nfo_priority=? WHERE asset_id=?",
                        )
                        .bind(&nfo_json)
                        .bind(&nfo_path)
                        .bind(nfo_priority)
                        .bind(&asset_id)
                        .execute(&mut *tx)
                        .await?;
                    }
                    _ => {}
                }
            }
        }

        last_staged_path = chunk.last().unwrap().get("relative_path");
    }

    refresh_editorial_for_works(&mut tx, &affected_work_ids).await?;
    let encoded: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_one(&mut *tx)
        .await?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&encoded)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
    for role in ["poster", "backdrop", "logo", "thumb", "banner", "disc"] {
        if !options.image_enabled(role) {
            query("UPDATE inventory_images SET available=0 WHERE library_id=? AND role=?")
                .bind(library_id)
                .bind(role)
                .execute(&mut *tx)
                .await?;
        }
    }

    // Publish text snapshots atomically with assets, including unchanged video files.
    query("INSERT INTO inventory_subtitles(asset_id,subtitles_json) SELECT a.id,f.subtitles_json FROM scan_staging_files f JOIN inventory_assets a ON a.library_id=f.library_id AND a.relative_path=f.relative_path WHERE f.scan_id=? AND a.available=1 ON CONFLICT(asset_id) DO UPDATE SET subtitles_json=excluded.subtitles_json")
        .bind(scan_id).execute(&mut *tx).await?;
    query("INSERT INTO inventory_audio(asset_id,audios_json) SELECT a.id,f.audios_json FROM scan_staging_files f JOIN inventory_assets a ON a.library_id=f.library_id AND a.relative_path=f.relative_path WHERE f.scan_id=? AND a.available=1 ON CONFLICT(asset_id) DO UPDATE SET audios_json=excluded.audios_json")
        .bind(scan_id).execute(&mut *tx).await?;

    // Remember the partial fingerprints of the files now published, for the next scan's rename
    // detection and so a restarted Node never rereads unchanged files. Hints only.
    query("INSERT INTO inventory_asset_fingerprints(asset_id,library_id,size,modified_ns,fingerprint) SELECT a.id,a.library_id,a.size,a.modified_ns,f.fingerprint FROM scan_staging_fingerprints f JOIN inventory_assets a ON a.library_id=f.library_id AND a.relative_path=f.relative_path WHERE f.scan_id=? AND a.available=1 ON CONFLICT(asset_id) DO UPDATE SET library_id=excluded.library_id,size=excluded.size,modified_ns=excluded.modified_ns,fingerprint=excluded.fingerprint")
        .bind(scan_id).execute(&mut *tx).await?;

    crate::automation::emit_snapshot(
        &mut tx,
        library_id,
        scan_id,
        media_contracts::protocol::unix_now(),
    )
    .await?;

    if matches!(refresh_mode.as_str(), "missing_metadata" | "full") {
        let _ = crate::automation::schedule_library(
            &mut tx,
            library_id,
            "metadata.missing",
            &uuid::Uuid::new_v4().to_string(),
            None,
            media_contracts::protocol::unix_now(),
        )
        .await;
    }

    crate::trickplay::queue_scanned_assets(&mut tx, library_id, scan_id, &options).await?;

    query("DELETE FROM scan_staging_files WHERE scan_id=?")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

    query("DELETE FROM scan_staging_fingerprints WHERE scan_id=?")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

    query("DELETE FROM scan_staging_issues WHERE scan_id=?")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    crate::task_engine::WAKE.notify_one();
    crate::system_memory::trim_memory();
    Ok(())
}
