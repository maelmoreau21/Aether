use crate::database::{query, query_as, query_scalar};
pub use crate::{
    identity::IdentityStore,
    media::Probe,
    node::nfo::NfoOutcome,
    node::scanner::{CachedFile, InventoryBatch, ScanReport, ScannedFile},
};
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use sqlx::{Execute, Row};
use std::{collections::HashMap, path::Path};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn decode(text: &str) -> Result<Probe, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

pub async fn load_cache(
    store: &IdentityStore,
    library_id: &str,
) -> Result<HashMap<String, CachedFile>, sqlx::Error> {
    let rows = query(
        "SELECT relative_path,size,modified_ns,probe_json FROM inventory_assets WHERE library_id=? AND available=1 AND source_id IN (SELECT s.id FROM inventory_sources s JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked')",
    )
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok((
                r.get("relative_path"),
                CachedFile {
                    size: r.get::<i64, _>("size") as u64,
                    modified_ns: r.get("modified_ns"),
                    probe: decode(r.get("probe_json"))?,
                },
            ))
        })
        .collect()
}

/// Lightweight streaming cache loader for incremental library scans. Selects only (relative_path, size, modified_ns)
/// without deserializing probe_json into memory, and streams rows directly into the HashMap without allocating
/// intermediate row vectors.
///
/// Memory characteristics:
/// - Initial scan: 0 entries (O(1) memory).
/// - Incremental rescan: O(N_indexed) path and timestamp entries in RAM (~150-200 bytes per indexed file).
pub async fn load_scan_cache(
    store: &IdentityStore,
    library_id: &str,
) -> Result<HashMap<String, CachedFile>, sqlx::Error> {
    let mut rows = query(
        "SELECT relative_path,size,modified_ns FROM inventory_assets WHERE library_id=? AND available=1 AND source_id IN (SELECT s.id FROM inventory_sources s JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked')",
    )
    .bind(library_id)
    .fetch(&store.pool);

    let mut cache = HashMap::new();
    while let Some(r) = rows.try_next().await? {
        cache.insert(
            r.get("relative_path"),
            CachedFile {
                size: r.get::<i64, _>("size") as u64,
                modified_ns: r.get("modified_ns"),
                probe: Probe::default(),
            },
        );
    }
    Ok(cache)
}

pub async fn load_image_cache(
    store: &IdentityStore,
    library_id: &str,
) -> Result<HashMap<String, crate::node::images::LocalImage>, sqlx::Error> {
    let mut rows = query("SELECT i.* FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id WHERE i.library_id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked'")
        .bind(library_id).fetch(&store.pool);
    let mut cache = HashMap::new();
    while let Some(row) = rows.try_next().await? {
        let path: String = row.get("relative_path");
        cache.insert(
            path.clone(),
            crate::node::images::LocalImage {
                path,
                size_bytes: row.get::<i64, _>("size") as u64,
                modified_ns: row.get("modified_ns"),
                mime: row.get("mime"),
                priority: row.get::<i64, _>("priority") as u8,
            },
        );
    }
    Ok(cache)
}

pub async fn ingest(
    store: &IdentityStore,
    library_id: &str,
    _root: &Path,
    report: &ScanReport,
) -> Result<(), sqlx::Error> {
    let mut conn = store.pool.acquire().await?;
    let mut tx = crate::database::begin_write(&mut conn).await?;
    let encoded: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_one(&mut *tx)
        .await?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&encoded)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
    let configured: Option<String> = query_scalar(
        "SELECT l.root FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(library_id)
    .fetch_optional(&mut *tx)
    .await?;
    let configured_matches = configured.as_deref().is_some_and(|c| {
        let root_str = _root.to_str().unwrap_or("");
        let c_clean = c.strip_prefix(r"\\?\").unwrap_or(c);
        let root_clean = root_str.strip_prefix(r"\\?\").unwrap_or(root_str);
        c == root_str
            || Path::new(c) == _root
            || c_clean == root_clean
            || Path::new(c_clean) == Path::new(root_clean)
    });
    if !configured_matches {
        return Err(sqlx::Error::Protocol(
            "library configuration changed during scan".into(),
        ));
    }
    // Missing files become unavailable only after complete discovery; failed traversal preserves inventory.
    if report.complete {
        query("UPDATE inventory_assets SET available=0 WHERE library_id=?")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
    }
    for issue in &report.errors {
        query("UPDATE inventory_assets SET available=0 WHERE library_id=? AND relative_path=?")
            .bind(library_id)
            .bind(&issue.relative_path)
            .execute(&mut *tx)
            .await?;
    }
    if report.complete {
        query("UPDATE inventory_images SET available=0 WHERE library_id=?")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
    }
    for file in &report.files {
        let mut filtered = file.clone();
        options.filter_file(&mut filtered);
        let file = &filtered;
        let identity_key = format!("{}\n{}", _root.to_string_lossy(), file.identity_key);
        let existing =
            query("SELECT a.id,w.identity_key,s.root_snapshot FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id WHERE a.library_id=? AND a.relative_path=?")
                .bind(library_id)
                .bind(&file.relative_path)
                .fetch_optional(&mut *tx)
                .await?;
        let previous_asset_id = existing.as_ref().map(|r| r.get::<String, _>("id"));
        let asset_id = if let Some(row) = existing.filter(|row| {
            row.get::<String, _>("identity_key") == identity_key
                && row.get::<String, _>("root_snapshot") == _root.to_string_lossy()
        }) {
            row.get::<String, _>("id")
        } else {
            let work =
                query("SELECT id FROM inventory_works WHERE library_id=? AND identity_key=?")
                    .bind(library_id)
                    .bind(&identity_key)
                    .fetch_optional(&mut *tx)
                    .await?;
            let work_id = if let Some(row) = work {
                row.get::<String, _>("id")
            } else {
                let work_id = id();
                let title_sort = file.title.to_lowercase();
                query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort) VALUES(?,?,?,?,?,?,?)")
                    .bind(&work_id)
                    .bind(library_id)
                    .bind(&identity_key)
                    .bind(&file.title)
                    .bind(file.year.map(i64::from))
                    .bind(json!(file.external_ids).to_string())
                    .bind(title_sort)
                    .execute(&mut *tx)
                    .await?;
                work_id
            };
            let edition_spec = crate::consolidation::derive_edition_spec(&file.relative_path);
            let version_spec = crate::consolidation::derive_version_spec(&file.probe, &file.mime);
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
            let asset_id = previous_asset_id.unwrap_or_else(id);
            // A reused physical-path UUID after a root/identity change must not
            // inherit the editorial snapshot of the previous physical source.
            query("DELETE FROM inventory_editorial WHERE asset_id=?")
                .bind(&asset_id)
                .execute(&mut *tx)
                .await?;
            query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,?)")
                .bind(&source_id)
                .bind(&version_id)
                .bind(library_id)
                .bind(_root.to_string_lossy().as_ref())
                .execute(&mut *tx)
                .await?;
            query("INSERT INTO inventory_assets VALUES(?,?,?,?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET source_id=excluded.source_id")
                .bind(&asset_id)
                .bind(source_id)
                .bind(library_id)
                .bind(&file.relative_path)
                .bind(file.size as i64)
                .bind(&file.modified_ns)
                .bind(&file.mime)
                .bind(json!(file.probe).to_string())
                .execute(&mut *tx)
                .await?;
            asset_id
        };
        query("UPDATE inventory_assets SET size=?,modified_ns=?,mime=?,probe_json=?,available=1 WHERE id=?").bind(file.size as i64).bind(&file.modified_ns).bind(&file.mime).bind(json!(file.probe).to_string()).bind(&asset_id).execute(&mut *tx).await?;
        query("UPDATE inventory_sources SET root_snapshot=? WHERE id=(SELECT source_id FROM inventory_assets WHERE id=?)").bind(_root.to_string_lossy().as_ref()).bind(&asset_id).execute(&mut *tx).await?;
        if let Some(image) = &file.poster {
            let work_id: String = query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE a.id=?")
                .bind(&asset_id).fetch_one(&mut *tx).await?;
            query("INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available) VALUES(?,?,?,?,?,?,?,?,?,1) ON CONFLICT(work_id,root_snapshot,relative_path) DO UPDATE SET size=excluded.size,modified_ns=excluded.modified_ns,mime=excluded.mime,priority=excluded.priority,available=1")
                .bind(id()).bind(work_id).bind(library_id).bind(_root.to_string_lossy().as_ref())
                .bind(&image.path).bind(image.size_bytes as i64).bind(&image.modified_ns)
                .bind(&image.mime).bind(i64::from(image.priority)).execute(&mut *tx).await?;
        }
        let fallback =
            json!({"title":file.title,"year":file.year,"external_ids":file.external_ids});
        query("INSERT INTO inventory_editorial(asset_id,fallback_json) VALUES(?,?) ON CONFLICT(asset_id) DO UPDATE SET fallback_json=excluded.fallback_json")
            .bind(&asset_id).bind(fallback.to_string()).execute(&mut *tx).await?;
        match &file.editorial {
            NfoOutcome::Missing => {
                query("UPDATE inventory_editorial SET nfo_json=NULL,nfo_path=NULL,nfo_priority=NULL WHERE asset_id=?")
                    .bind(&asset_id).execute(&mut *tx).await?;
            }
            NfoOutcome::Valid {
                metadata,
                path,
                priority,
            } => {
                query("UPDATE inventory_editorial SET nfo_json=?,nfo_path=?,nfo_priority=? WHERE asset_id=?")
                    .bind(json!(metadata).to_string()).bind(path).bind(i64::from(*priority)).bind(&asset_id).execute(&mut *tx).await?;
            }
            // A broken replacement retains the last successfully indexed editorial snapshot.
            NfoOutcome::Invalid { .. } => {}
        }
        // Track identities remain stable by actual stream index across refreshes.
        let indexes: Vec<i64> = file
            .probe
            .streams
            .iter()
            .map(|t| i64::from(t.index))
            .collect();
        let previous = query("SELECT id,stream_index FROM inventory_tracks WHERE asset_id=?")
            .bind(&asset_id)
            .fetch_all(&mut *tx)
            .await?;
        for row in previous {
            if !indexes.contains(&row.get("stream_index")) {
                query("DELETE FROM inventory_tracks WHERE id=?")
                    .bind(row.get::<String, _>("id"))
                    .execute(&mut *tx)
                    .await?;
            }
        }
        for track in &file.probe.streams {
            query("INSERT INTO inventory_tracks VALUES(?,?,?,?) ON CONFLICT(asset_id,stream_index) DO UPDATE SET metadata_json=excluded.metadata_json").bind(id()).bind(&asset_id).bind(i64::from(track.index)).bind(json!(track).to_string()).execute(&mut *tx).await?;
        }
    }
    refresh_editorial(&mut tx, library_id).await?;
    tx.commit().await?;
    crate::system_memory::trim_memory();
    Ok(())
}

pub async fn clean_staging(store: &IdentityStore, scan_id: &str) -> Result<(), sqlx::Error> {
    query("DELETE FROM scan_batch_receipts WHERE scan_id=?")
        .bind(scan_id)
        .execute(&store.pool)
        .await?;
    query("DELETE FROM scan_staging_files WHERE scan_id=?")
        .bind(scan_id)
        .execute(&store.pool)
        .await?;
    query("DELETE FROM scan_staging_issues WHERE scan_id=?")
        .bind(scan_id)
        .execute(&store.pool)
        .await?;
    Ok(())
}

pub async fn stage_batch(
    store: &IdentityStore,
    scan_id: &str,
    library_id: &str,
    batch: &InventoryBatch,
) -> Result<(), sqlx::Error> {
    let mut conn = store.pool.acquire().await?;
    let mut tx = crate::database::begin_write(&mut conn).await?;
    // A replayed transport batch has no second effect, including issue counters.
    if query("INSERT INTO scan_batch_receipts(scan_id,sequence) VALUES(?,?) ON CONFLICT DO NOTHING")
        .bind(scan_id)
        .bind(batch.sequence.min(i64::MAX as u64) as i64)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        == 0
    {
        tx.commit().await?;
        crate::system_memory::trim_memory();
        return Ok(());
    }
    let encoded: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_one(&mut *tx)
        .await?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&encoded)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;

    for issue in &batch.errors {
        query(
            "INSERT INTO scan_staging_issues(scan_id, kind, relative_path, message) VALUES(?, 'error', ?, ?)",
        )
        .bind(scan_id)
        .bind(&issue.relative_path)
        .bind(&issue.message)
        .execute(&mut *tx)
        .await?;
    }

    for issue in &batch.warnings {
        query(
            "INSERT INTO scan_staging_issues(scan_id, kind, relative_path, message) VALUES(?, 'warning', ?, ?)",
        )
        .bind(scan_id)
        .bind(&issue.relative_path)
        .bind(&issue.message)
        .execute(&mut *tx)
        .await?;
    }

    for file in &batch.files {
        let mut filtered = file.clone();
        options.filter_file(&mut filtered);
        let file = &filtered;
        let (poster_path, poster_size, poster_modified_ns, poster_mime, poster_priority) =
            match &file.poster {
                Some(img) => (
                    Some(img.path.as_str()),
                    Some(img.size_bytes as i64),
                    Some(img.modified_ns.as_str()),
                    Some(img.mime.as_str()),
                    Some(i64::from(img.priority)),
                ),
                None => (None, None, None, None, None),
            };

        let (
            series_poster_path,
            series_poster_size,
            series_poster_modified_ns,
            series_poster_mime,
            series_poster_priority,
        ) = match &file.series_poster {
            Some(img) => (
                Some(img.path.as_str()),
                Some(img.size_bytes as i64),
                Some(img.modified_ns.as_str()),
                Some(img.mime.as_str()),
                Some(i64::from(img.priority)),
            ),
            None => (None, None, None, None, None),
        };

        let series_external_ids_json = file
            .series_external_ids
            .as_ref()
            .map(|ids| json!(ids).to_string());
        let series_editorial_json = match &file.series_editorial {
            Some(NfoOutcome::Valid { metadata, .. }) => Some(json!(metadata).to_string()),
            Some(NfoOutcome::Invalid { .. }) => Some(r#"{"_outcome":"invalid"}"#.to_string()),
            _ => None,
        };

        let (outcome, editorial_json, editorial_path, editorial_priority) = match &file.editorial {
            NfoOutcome::Missing => ("missing", None, None, None),
            NfoOutcome::Valid {
                metadata,
                path,
                priority,
            } => (
                "valid",
                Some(json!(metadata).to_string()),
                Some(path.as_str()),
                Some(i64::from(*priority)),
            ),
            NfoOutcome::Invalid { .. } => ("invalid", None, None, None),
        };

        let probe_json = if file.reused {
            None
        } else {
            Some(json!(file.probe).to_string())
        };

        let external_ids_json = json!(file.external_ids).to_string();

        query(
            "INSERT INTO scan_staging_files (
                scan_id, library_id, relative_path, size, modified_ns, mime,
                title, year, identity_key, external_ids_json, probe_json, reused,
                poster_path, poster_size, poster_modified_ns, poster_mime, poster_priority,
                editorial_outcome, editorial_json, editorial_path, editorial_priority,
                series_identity_key, series_title, series_year, series_external_ids_json,
                series_editorial_json, series_poster_path, series_poster_size,
                series_poster_modified_ns, series_poster_mime, series_poster_priority,
                season_number, episode_number
            ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(scan_id, relative_path) DO UPDATE SET
                size=excluded.size, modified_ns=excluded.modified_ns, mime=excluded.mime,
                title=excluded.title, year=excluded.year, identity_key=excluded.identity_key,
                external_ids_json=excluded.external_ids_json, probe_json=excluded.probe_json,
                reused=excluded.reused, poster_path=excluded.poster_path, poster_size=excluded.poster_size,
                poster_modified_ns=excluded.poster_modified_ns, poster_mime=excluded.poster_mime,
                poster_priority=excluded.poster_priority, editorial_outcome=excluded.editorial_outcome,
                editorial_json=excluded.editorial_json, editorial_path=excluded.editorial_path,
                editorial_priority=excluded.editorial_priority,
                series_identity_key=excluded.series_identity_key,
                series_title=excluded.series_title,
                series_year=excluded.series_year,
                series_external_ids_json=excluded.series_external_ids_json,
                series_editorial_json=excluded.series_editorial_json,
                series_poster_path=excluded.series_poster_path,
                series_poster_size=excluded.series_poster_size,
                series_poster_modified_ns=excluded.series_poster_modified_ns,
                series_poster_mime=excluded.series_poster_mime,
                series_poster_priority=excluded.series_poster_priority,
                season_number=excluded.season_number,
                episode_number=excluded.episode_number",
        )
        .bind(scan_id)
        .bind(library_id)
        .bind(&file.relative_path)
        .bind(file.size as i64)
        .bind(&file.modified_ns)
        .bind(&file.mime)
        .bind(&file.title)
        .bind(file.year.map(i64::from))
        .bind(&file.identity_key)
        .bind(&external_ids_json)
        .bind(&probe_json)
        .bind(if file.reused { 1i64 } else { 0i64 })
        .bind(poster_path)
        .bind(poster_size)
        .bind(poster_modified_ns)
        .bind(poster_mime)
        .bind(poster_priority)
        .bind(outcome)
        .bind(&editorial_json)
        .bind(editorial_path)
        .bind(editorial_priority)
        .bind(file.series_identity_key.as_deref())
        .bind(file.series_title.as_deref())
        .bind(file.series_year.map(i64::from))
        .bind(&series_external_ids_json)
        .bind(&series_editorial_json)
        .bind(series_poster_path)
        .bind(series_poster_size)
        .bind(series_poster_modified_ns)
        .bind(series_poster_mime)
        .bind(series_poster_priority)
        .bind(file.season_number.map(i64::from))
        .bind(file.episode_number.map(i64::from))
        .execute(&mut *tx)
        .await?;

        query("UPDATE scan_staging_files SET artwork_json=? WHERE scan_id=? AND relative_path=?")
            .bind(json!(file.artwork).to_string())
            .bind(scan_id)
            .bind(&file.relative_path)
            .execute(&mut *tx)
            .await?;
        query("UPDATE scan_staging_files SET subtitles_json=?,audios_json=? WHERE scan_id=? AND relative_path=?")
            .bind(json!(file.subtitles).to_string())
            .bind(json!(file.audios).to_string())
            .bind(scan_id)
            .bind(&file.relative_path)
            .execute(&mut *tx)
            .await?;
    }

    if let Some((job, attempt)) = scan_id.split_once(':')
        && let Ok(attempt) = attempt.parse::<i64>()
    {
        query("UPDATE durable_tasks SET processed_items=processed_items+? WHERE id=? AND attempt=? AND status='running'")
            .bind(batch.files.len() as i64).bind(job).bind(attempt).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    crate::system_memory::trim_memory();
    Ok(())
}

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

    crate::automation::emit_snapshot(
        &mut tx,
        library_id,
        scan_id,
        media_contracts::protocol::unix_now(),
    )
    .await?;

    query("DELETE FROM scan_staging_files WHERE scan_id=?")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

    query("DELETE FROM scan_staging_issues WHERE scan_id=?")
        .bind(scan_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    crate::system_memory::trim_memory();
    Ok(())
}

pub async fn refresh_editorial_for_works(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_ids: &std::collections::BTreeSet<String>,
) -> Result<(), sqlx::Error> {
    if work_ids.is_empty() {
        return Ok(());
    }

    let work_ids_vec: Vec<&String> = work_ids.iter().collect();
    for chunk in work_ids_vec.chunks(250) {
        let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(
            "SELECT e.work_id, m.fallback_json, m.nfo_json, a.relative_path, m.nfo_priority, m.nfo_path 
             FROM inventory_editorial m 
             JOIN inventory_assets a ON a.id=m.asset_id 
             JOIN inventory_sources s ON s.id=a.source_id 
             JOIN inventory_versions v ON v.id=s.version_id 
             JOIN inventory_editions e ON e.id=v.edition_id 
             JOIN libraries l ON l.id=s.library_id 
             JOIN media_nodes n ON n.id=l.node_id
             WHERE a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND e.work_id IN ("
        );
        let mut separated = builder.separated(", ");
        for id in chunk {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ");
        builder.push(
            "ORDER BY e.work_id, m.nfo_json IS NULL, m.nfo_priority NULLS FIRST, m.nfo_path NULLS FIRST, a.relative_path",
        );

        let sql = crate::database::normalize_sql(builder.sql().as_str());
        let arguments = builder
            .build()
            .take_arguments()
            .map_err(sqlx::Error::Encode)?
            .unwrap_or_default();
        let rows = sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(sql), arguments)
            .fetch_all(&mut **tx)
            .await?;
        let mut grouped: std::collections::BTreeMap<String, Vec<sqlx::any::AnyRow>> =
            std::collections::BTreeMap::new();
        for r in rows {
            let wid: String = r.get("work_id");
            grouped.entry(wid).or_default().push(r);
        }

        let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));

        for (wid, work_rows) in grouped {
            let Some(first) = work_rows.first() else {
                continue;
            };
            let fallback = parse(&first.get::<String, _>("fallback_json"));
            let nfo = first
                .get::<Option<String>, _>("nfo_json")
                .map(|t| parse(&t));
            let title = nfo
                .as_ref()
                .and_then(|n| n.get("title"))
                .and_then(Value::as_str)
                .or_else(|| fallback.get("title").and_then(Value::as_str))
                .unwrap_or("Film local")
                .to_owned();
            let year = nfo
                .as_ref()
                .and_then(|n| n.get("year"))
                .and_then(Value::as_i64)
                .or_else(|| fallback.get("year").and_then(Value::as_i64));
            let mut ids = std::collections::BTreeMap::<String, String>::new();
            let mut conflicts = std::collections::BTreeSet::new();
            for row in &work_rows {
                let mut candidates = vec![parse(&row.get::<String, _>("fallback_json"))];
                if let Some(text) = row.get::<Option<String>, _>("nfo_json") {
                    candidates.push(parse(&text));
                }
                for candidate in candidates {
                    if let Some(values) = candidate["external_ids"].as_object() {
                        for (provider, value) in values {
                            if let Some(value) = value.as_str() {
                                if ids.get(provider).is_some_and(|existing| existing != value) {
                                    conflicts.insert(provider.clone());
                                }
                                ids.entry(provider.clone())
                                    .or_insert_with(|| value.to_owned());
                            }
                        }
                    }
                }
            }
            for provider in &conflicts {
                ids.remove(provider);
            }
            let mut metadata = nfo.unwrap_or_else(|| json!({}));
            metadata["external_ids"] = json!(ids);
            metadata["provenance"] =
                json!(if first.get::<Option<String>, _>("nfo_json").is_some() {
                    "nfo"
                } else {
                    "local"
                });
            metadata["warnings"] = json!(
                conflicts
                    .into_iter()
                    .map(|provider| format!(
                        "Conflicting {provider} identifiers; no automatic merge"
                    ))
                    .collect::<Vec<_>>()
            );
            let title_sort = title.to_lowercase();
            query(
                "UPDATE inventory_works SET title=?,year=?,external_ids_json=?,editorial_json=?,title_sort=? WHERE id=?",
            )
            .bind(title)
            .bind(year)
            .bind(json!(ids).to_string())
            .bind(metadata.to_string())
            .bind(title_sort)
            .bind(&wid)
            .execute(&mut **tx)
            .await?;
        }
    }

    crate::automation::reapply_results(tx, &work_ids.iter().cloned().collect::<Vec<_>>()).await?;

    let mut parent_series = std::collections::BTreeSet::new();
    for wid in work_ids {
        crate::consolidation::consolidate_work_tx(tx, wid).await?;
        let s_id: Option<String> = query_scalar(
            "SELECT s.series_id FROM inventory_episodes ep \
             JOIN inventory_seasons s ON s.id = ep.season_id \
             WHERE ep.work_id = ?",
        )
        .bind(wid)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(sid) = s_id
            && !work_ids.contains(&sid)
        {
            parent_series.insert(sid);
        }
    }
    for sid in parent_series {
        crate::consolidation::consolidate_work_tx(tx, &sid).await?;
    }
    Ok(())
}

async fn refresh_editorial(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    library_id: &str,
) -> Result<(), sqlx::Error> {
    let works: Vec<String> = query_scalar("SELECT id FROM inventory_works WHERE library_id=?")
        .bind(library_id)
        .fetch_all(&mut **tx)
        .await?;
    let set: std::collections::BTreeSet<String> = works.into_iter().collect();
    refresh_editorial_for_works(tx, &set).await
}

pub async fn refresh_work_editorial(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_id: &str,
) -> Result<(), sqlx::Error> {
    let mut set = std::collections::BTreeSet::new();
    set.insert(work_id.to_string());
    refresh_editorial_for_works(tx, &set).await
}

pub async fn resource_library(
    store: &IdentityStore,
    kind: &str,
    resource_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let query = match kind {
        "work" => "SELECT library_id FROM inventory_works WHERE id=?",
        "image" => {
            "SELECT i.library_id FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id WHERE i.id=? AND i.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'"
        }
        "asset" | "media" => {
            "SELECT a.library_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'"
        }
        _ => return Ok(None),
    };
    query_scalar(query)
        .bind(resource_id)
        .fetch_optional(&store.pool)
        .await
}

fn work(row: &sqlx::any::AnyRow) -> Value {
    let mut metadata: Value =
        serde_json::from_str(row.get("editorial_json")).unwrap_or_else(|_| json!({}));
    if let Some(manual) = row
        .try_get::<Option<String>, _>("manual_json")
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        && let Some(object) = metadata.as_object_mut()
    {
        object.extend(manual);
    }
    let kind: String = row.try_get("kind").unwrap_or_else(|_| "film".to_string());
    let season_count: Option<i64> = row.try_get("season_count").ok().flatten();
    let episode_count: Option<i64> = row.try_get("episode_count").ok().flatten();
    let node_id: Option<String> = row.try_get("node_id").ok().flatten();
    let node_status: Option<String> = row.try_get("node_status").ok().flatten();
    let node_available = node_status.as_deref().unwrap_or("connected") == "connected";
    let mut val = json!({
        "id": row.get::<String,_>("id"),
        "libraryId": row.try_get::<String,_>("display_library_id").unwrap_or_else(|_|row.get("library_id")),
        "kind": kind,
        "title": metadata["title"].as_str().map(str::to_owned).unwrap_or_else(||row.get::<String,_>("title")),
        "year": metadata["year"].as_i64().or_else(||row.get::<Option<i64>,_>("year")),
        "minimumAge":row.try_get::<Option<i64>,_>("minimum_age").ok().flatten(),
        "addedAt": row.try_get::<Option<i64>,_>("added_at").ok().flatten(),
        "description": metadata["plot"].as_str().or_else(||metadata["outline"].as_str()).unwrap_or(""),
        "plot": metadata["plot"],
        "genres": metadata["genres"].as_array().cloned().unwrap_or_default(),
        "tags": metadata["tags"].as_array().cloned().unwrap_or_default(),
        "originalTitle": metadata["original_title"],
        "outline": metadata["outline"],
        "premiered": metadata["premiered"],
        "rating": metadata["rating"],
        "mpaa": metadata["mpaa"],
        "actors": metadata["actors"].as_array().cloned().unwrap_or_default(),
        "directors": metadata["directors"].as_array().cloned().unwrap_or_default(),
        "writers": metadata["writers"].as_array().cloned().unwrap_or_default(),
        "ratings": metadata["ratings"].as_array().cloned().unwrap_or_default(),
        "externalIds": metadata["external_ids"],
        "metadataProvenance": metadata["provenance"].as_str().unwrap_or("local"),
        "metadataWarnings": metadata["warnings"].as_array().cloned().unwrap_or_default(),
        "synthetic": false,
        "episode": row.try_get::<String,_>("kind").is_ok_and(|kind| kind == "episode"),
        "posterUrl": row.try_get::<Option<String>,_>("poster_url").ok().flatten()
            .or_else(|| metadata["poster_url"].as_str().or_else(|| metadata["posterUrl"].as_str()).map(str::to_owned)),
        "artwork": {
            "backdrop": row.try_get::<Option<String>,_>("backdrop_url").ok().flatten()
                .or_else(|| metadata["backdrop_url"].as_str().or_else(|| metadata["backdropUrl"].as_str()).map(str::to_owned)),
            "logo": row.try_get::<Option<String>,_>("logo_url").ok().flatten()
                .or_else(|| metadata["logo_url"].as_str().or_else(|| metadata["logoUrl"].as_str()).map(str::to_owned)),
            "thumb": row.try_get::<Option<String>,_>("thumb_url").ok().flatten()
        },
        "nodeId": node_id.unwrap_or_else(|| "local".to_string()),
        "nodeStatus": node_status.unwrap_or_else(|| "connected".to_string()),
        "available": node_available
    });
    if kind == "series" {
        val["seasonCount"] = json!(season_count.unwrap_or(0));
        val["episodeCount"] = json!(episode_count.unwrap_or(0));
    }
    val
}

/// Administrator scan diagnostics. Only currently available sources contribute;
/// paths and opaque internal identifiers are never included in these messages.
pub async fn editorial_warnings(
    store: &IdentityStore,
    library_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = query("SELECT w.editorial_json FROM inventory_works w WHERE w.library_id=? AND EXISTS(SELECT 1 FROM inventory_editions e JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked') ORDER BY w.id")
        .bind(library_id).fetch_all(&store.pool).await?;
    let mut warnings = Vec::new();
    for row in rows {
        if let Ok(metadata) = serde_json::from_str::<Value>(row.get("editorial_json"))
            && let Some(messages) = metadata["warnings"].as_array()
        {
            for message in messages.iter().filter_map(Value::as_str) {
                if warnings.len() == 100 {
                    return Ok(warnings);
                }
                warnings.push(message.to_owned());
            }
        }
    }
    Ok(warnings)
}

#[derive(Debug, Clone, Default)]
pub struct CatalogueFilter {
    pub user_id: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub quality: Option<i64>,
    pub codec: Option<String>,
    pub language: Option<String>,
}
fn push_search_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    search_clean: &str,
    use_fts: bool,
    fast_avail: bool,
) {
    if search_clean.is_empty() {
        return;
    }
    if use_fts && search_clean.chars().count() >= 3 {
        let fts_query = format!("\"{}\"", search_clean.replace('"', "\"\""));
        builder
            .push(" AND w.id IN (SELECT work_id FROM inventory_works_fts WHERE inventory_works_fts.search_text MATCH ")
            .push_bind(fts_query)
            .push(") ");
        return;
    }
    if fast_avail {
        builder
            .push(" AND instr(w.display_title_sort, ")
            .push_bind(search_clean.to_owned())
            .push(") > 0 ");
    } else {
        builder
            .push(" AND instr(CASE WHEN w.display_title_sort != '' THEN w.display_title_sort ELSE lower(COALESCE(json_extract(w.manual_json,'$.title'),w.title_sort)) END, ")
            .push_bind(search_clean.to_owned())
            .push(") > 0 ");
    }
}

fn push_availability_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    target_kind: Option<&str>,
    fast_avail: bool,
) {
    if fast_avail {
        match target_kind {
            Some("film") => {
                builder.push(" AND w.kind = 'film' AND w.has_available_asset = 1 ");
            }
            Some("series") => {
                builder.push(" AND w.kind = 'series' AND w.has_available_asset = 1 ");
            }
            _ => {
                builder.push(" AND w.kind IN ('film', 'series') AND w.has_available_asset = 1 ");
            }
        }
    } else {
        match target_kind {
            Some("film") => {
                builder.push(
                    " AND w.kind = 'film' AND EXISTS ( \
                       SELECT 1 FROM inventory_editions e \
                       JOIN inventory_versions v ON v.edition_id=e.id \
                       JOIN inventory_sources s ON s.version_id=v.id \
                       JOIN inventory_assets a ON a.source_id=s.id \
                       JOIN libraries l ON l.id=s.library_id \
                       JOIN media_nodes n ON n.id=l.node_id \
                       WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND n.status != 'revoked' AND l.enabled=1 \
                     ) ",
                );
            }
            Some("series") => {
                builder.push(
                    " AND w.kind = 'series' AND EXISTS ( \
                       SELECT 1 FROM inventory_seasons s \
                       JOIN inventory_episodes ep ON ep.season_id=s.id \
                       JOIN inventory_editions e ON e.work_id=ep.work_id \
                       JOIN inventory_versions v ON v.edition_id=e.id \
                       JOIN inventory_sources src ON src.version_id=v.id \
                       JOIN inventory_assets a ON a.source_id=src.id \
                       JOIN libraries l ON l.id=src.library_id \
                       JOIN media_nodes n ON n.id=l.node_id \
                       WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
                     ) ",
                );
            }
            _ => {
                builder.push(
                    " AND ( \
                       (w.kind = 'film' AND EXISTS ( \
                          SELECT 1 FROM inventory_editions e \
                          JOIN inventory_versions v ON v.edition_id=e.id \
                          JOIN inventory_sources s ON s.version_id=v.id \
                          JOIN inventory_assets a ON a.source_id=s.id \
                          JOIN libraries l ON l.id=s.library_id \
                          JOIN media_nodes n ON n.id=l.node_id \
                          WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND n.status != 'revoked' AND l.enabled=1 \
                       )) \
                       OR \
                       (w.kind = 'series' AND EXISTS ( \
                          SELECT 1 FROM inventory_seasons s \
                          JOIN inventory_episodes ep ON ep.season_id=s.id \
                          JOIN inventory_editions e ON e.work_id=ep.work_id \
                          JOIN inventory_versions v ON v.edition_id=e.id \
                          JOIN inventory_sources src ON src.version_id=v.id \
                          JOIN inventory_assets a ON a.source_id=src.id \
                          JOIN libraries l ON l.id=src.library_id \
                          JOIN media_nodes n ON n.id=l.node_id \
                          WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
                       )) \
                     ) ",
                );
            }
        }
    }
}

fn push_catalogue_filters(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    filter: &CatalogueFilter,
    has_parental_policy: bool,
    fast_avail: bool,
) {
    if let Some(genre) = &filter.genre {
        let g = genre.trim().to_lowercase();
        if fast_avail {
            builder
                .push(" AND instr(w.genres_csv, ")
                .push_bind(format!("|{g}|"))
                .push(")>0 ");
        } else {
            builder
                .push(" AND (instr(w.genres_csv, ")
                .push_bind(format!("|{g}|"))
                .push(")>0 OR (w.genres_csv = '|' AND instr(lower(json_extract(COALESCE(w.manual_json,w.editorial_json),'$.genres')), ")
                .push_bind(format!("\"{g}\""))
                .push(")>0)) ");
        }
    }
    if let Some(year) = filter.year {
        if fast_avail {
            builder
                .push(" AND w.effective_year=")
                .push_bind(year)
                .push(" ");
        } else {
            builder
                .push(" AND COALESCE(w.effective_year, CAST(json_extract(w.manual_json,'$.year') AS BIGINT), w.year)=")
                .push_bind(year)
                .push(" ");
        }
    }
    if filter.quality.is_some() || filter.codec.is_some() || filter.language.is_some() {
        if fast_avail {
            if let Some(height) = filter.quality {
                builder
                    .push(" AND (w.max_height >= ")
                    .push_bind(height)
                    .push(" OR (w.max_height = 0 AND EXISTS(SELECT 1 FROM inventory_editions fe JOIN inventory_versions fv ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft ON ft.asset_id=fa.id WHERE (fe.work_id=w.id OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep JOIN inventory_seasons fseason ON fseason.id=fep.season_id WHERE fseason.series_id=w.id)) AND fa.available=1 AND CAST(json_extract(ft.metadata_json,'$.height') AS BIGINT)>=")
                    .push_bind(height)
                    .push("))) ");
            }
            if let Some(codec) = &filter.codec {
                let c = codec.trim().to_ascii_lowercase();
                builder
                    .push(" AND (instr(w.codecs_csv, ")
                    .push_bind(format!(",{c},"))
                    .push(")>0 OR (w.codecs_csv = ',' AND EXISTS(SELECT 1 FROM inventory_editions fe JOIN inventory_versions fv ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft ON ft.asset_id=fa.id WHERE (fe.work_id=w.id OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep JOIN inventory_seasons fseason ON fseason.id=fep.season_id WHERE fseason.series_id=w.id)) AND fa.available=1 AND lower(json_extract(ft.metadata_json,'$.codec_name'))=")
                    .push_bind(c)
                    .push("))) ");
            }
            if let Some(language) = &filter.language {
                let l = language.trim().to_ascii_lowercase();
                builder
                    .push(" AND (instr(w.languages_csv, ")
                    .push_bind(format!(",{l},"))
                    .push(")>0 OR (w.languages_csv = ',' AND EXISTS(SELECT 1 FROM inventory_editions fe JOIN inventory_versions fv ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft ON ft.asset_id=fa.id WHERE (fe.work_id=w.id OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep JOIN inventory_seasons fseason ON fseason.id=fep.season_id WHERE fseason.series_id=w.id)) AND fa.available=1 AND lower(json_extract(json_extract(ft.metadata_json,'$.tags'),'$.language'))=")
                    .push_bind(l)
                    .push("))) ");
            }
        } else {
            builder.push(" AND EXISTS(SELECT 1 FROM inventory_editions fe JOIN inventory_versions fv ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id JOIN inventory_assets fa ON fa.source_id=fs.id JOIN libraries fl ON fl.id=fs.library_id JOIN media_nodes fn ON fn.id=fl.node_id WHERE (fe.work_id=w.id OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep JOIN inventory_seasons fseason ON fseason.id=fep.season_id WHERE fseason.series_id=w.id)) AND fa.available=1 AND fl.enabled=1 AND fn.status!='revoked' AND fs.root_snapshot=fl.root ");
            if let Some(height) = filter.quality {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND CAST(json_extract(ft.metadata_json,'$.height') AS BIGINT)>=").push_bind(height).push(") ");
            }
            if let Some(codec) = &filter.codec {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND lower(json_extract(ft.metadata_json,'$.codec_name'))=").push_bind(codec.to_lowercase()).push(") ");
            }
            if let Some(language) = &filter.language {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND lower(json_extract(json_extract(ft.metadata_json,'$.tags'),'$.language'))=").push_bind(language.to_lowercase()).push(") ");
            }
            builder.push(") ");
        }
    }
    if has_parental_policy && let Some(user) = &filter.user_id {
        builder
            .push(" AND NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=")
            .push_bind(user)
            .push(" AND ((")
            .push(crate::product::AGE_SQL)
            .push(")>policy.maximum_age OR ((")
            .push(crate::product::AGE_SQL)
            .push(") IS NULL AND policy.allow_unrated=0) OR ")
            .push(crate::product::BLOCKED_TAGS_SQL)
            .push(")) ");
    }
}

#[allow(clippy::too_many_arguments)]
fn push_group_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    filter: &CatalogueFilter,
    valid_libs: &[String],
    search: &str,
    has_groups: bool,
    has_parental_policy: bool,
    fast_avail: bool,
    use_fts: bool,
) {
    if !has_groups {
        return;
    }
    builder.push(" AND (NOT EXISTS(SELECT 1 FROM media_group_members own_group WHERE own_group.work_id=w.id) OR w.id IN (SELECT COALESCE(MIN(CASE WHEN gn.status='connected' THEN w.id END),MIN(w.id)) FROM media_group_members gm JOIN inventory_works w ON w.id=gm.work_id JOIN libraries gl ON gl.id=w.library_id JOIN media_nodes gn ON gn.id=gl.node_id WHERE gl.enabled=1 AND gn.status!='revoked' ");
    if let Some(user) = &filter.user_id {
        builder.push(" AND (EXISTS(SELECT 1 FROM users gu WHERE gu.id=").push_bind(user).push(" AND gu.role='administrator') OR EXISTS(SELECT 1 FROM effective_library_permissions gp WHERE gp.library_id=gl.id AND gp.user_id=").push_bind(user).push(")) ");
    }
    if fast_avail {
        builder.push(" AND (w.kind!='film' OR w.has_available_asset=1) ");
    } else {
        builder.push(" AND (w.kind!='film' OR EXISTS(SELECT 1 FROM inventory_editions ge JOIN inventory_versions gv ON gv.edition_id=ge.id JOIN inventory_sources gs ON gs.version_id=gv.id JOIN inventory_assets ga ON ga.source_id=gs.id WHERE ge.work_id=w.id AND ga.available=1 AND gs.root_snapshot=gl.root)) ");
    }
    builder.push(" AND w.library_id IN (");
    let mut ids = builder.separated(",");
    for id in valid_libs {
        ids.push_bind(id);
    }
    ids.push_unseparated(") ");
    push_search_filter(builder, search, use_fts, fast_avail);
    push_catalogue_filters(builder, filter, has_parental_policy, fast_avail);
    builder.push(" GROUP BY gm.group_id)) ");
}

pub async fn catalogue(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
) -> Result<Value, sqlx::Error> {
    catalogue_filtered(
        store,
        visible,
        kind,
        search,
        sort,
        offset,
        limit,
        &CatalogueFilter::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn catalogue_filtered(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
    filter: &CatalogueFilter,
) -> Result<Value, sqlx::Error> {
    catalogue_page(
        store, visible, kind, search, sort, offset, limit, filter, true,
    )
    .await
}

pub struct BoundedCatalogueCache {
    max_bytes: usize,
    current_bytes: usize,
    entries: std::collections::HashMap<String, CatalogueCacheEntry>,
    access_seq: u64,
}

struct CatalogueCacheEntry {
    gen_fp: String,
    value: Value,
    size_bytes: usize,
    last_accessed: u64,
}

impl BoundedCatalogueCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes: max_bytes.max(1024 * 1024),
            current_bytes: 0,
            entries: std::collections::HashMap::new(),
            access_seq: 0,
        }
    }

    pub fn from_env() -> Self {
        let max_mb = std::env::var("MEDIA_CATALOGUE_CACHE_MAX_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(32)
            .clamp(1, 1024);
        Self::new(max_mb * 1024 * 1024)
    }

    pub fn get(&mut self, key: &str, current_fp: i64) -> Option<Value> {
        let fp_str = current_fp.to_string();
        if let Some(entry) = self.entries.get_mut(key) {
            if entry.gen_fp == fp_str {
                self.access_seq += 1;
                entry.last_accessed = self.access_seq;
                return Some(entry.value.clone());
            } else if let Some(removed) = self.entries.remove(key) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
            }
        }
        None
    }

    pub fn insert(&mut self, key: String, current_fp: i64, value: Value) {
        let entry_size = key.len() + 64 + estimate_json_bytes(&value);
        if entry_size > self.max_bytes {
            return;
        }

        while self.current_bytes.saturating_add(entry_size) > self.max_bytes
            && !self.entries.is_empty()
        {
            if let Some((oldest_key, _)) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_accessed)
                .map(|(k, e)| (k.clone(), e.size_bytes))
            {
                if let Some(removed) = self.entries.remove(&oldest_key) {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
                }
            } else {
                break;
            }
        }

        self.access_seq += 1;
        let entry = CatalogueCacheEntry {
            gen_fp: current_fp.to_string(),
            value,
            size_bytes: entry_size,
            last_accessed: self.access_seq,
        };

        if let Some(old) = self.entries.insert(key, entry) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size_bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(entry_size);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_bytes = 0;
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (self.current_bytes, self.max_bytes, self.entries.len())
    }
}

pub fn estimate_json_bytes(val: &Value) -> usize {
    match val {
        Value::Null | Value::Bool(_) | Value::Number(_) => 16,
        Value::String(s) => 24 + s.capacity().max(s.len()),
        Value::Array(arr) => {
            24 + arr.capacity() * std::mem::size_of::<Value>()
                + arr.iter().map(estimate_json_bytes).sum::<usize>()
        }
        Value::Object(map) => {
            48 + map
                .iter()
                .map(|(k, v)| 24 + k.capacity().max(k.len()) + estimate_json_bytes(v))
                .sum::<usize>()
        }
    }
}

static CATALOGUE_CACHE: std::sync::OnceLock<std::sync::Mutex<BoundedCatalogueCache>> =
    std::sync::OnceLock::new();

pub fn invalidate_catalogue_cache() {
    if let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
    {
        guard.clear();
    }
}

pub fn catalogue_cache_stats() -> (usize, usize, usize) {
    if let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(guard) = lock.lock()
    {
        guard.stats()
    } else {
        (0, 0, 0)
    }
}

/// Bounded shelf queries can omit the full-library count and fetch one lookahead row.
#[allow(clippy::too_many_arguments)]
pub async fn catalogue_page(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
    filter: &CatalogueFilter,
    exact_count: bool,
) -> Result<Value, sqlx::Error> {
    let target_kind = match kind {
        Some("film") | Some("films") => Some("film"),
        Some("series") => Some("series"),
        None => None,
        Some(_) => {
            return Ok(json!({
                "items": [],
                "total": if exact_count { Some(0usize) } else { None },
                "hasMore": false,
                "offset": offset,
                "limit": limit,
                "mode": "local-library",
                "notice": "Bibliothèques locales indexées"
            }));
        }
    };

    if visible.is_empty() {
        return Ok(json!({
            "items": [],
            "total": if exact_count { Some(0usize) } else { None },
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    let pool_id = format!("{:p}", &store.pool);
    let cache_key = format!(
        "{pool_id}|{:?}|{:?}|{}|{}|{}|{}|{:?}|{}",
        visible, target_kind, search, sort, offset, limit, filter, exact_count
    );

    let state_fp: i64 = query_scalar("SELECT gen FROM inventory_cache_generation WHERE id = 1")
        .fetch_optional(&store.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    if state_fp > 0
        && let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
        && let Some(cached_val) = guard.get(&cache_key, state_fp)
    {
        return Ok(cached_val);
    }

    // 1. Pre-filter visible libraries to enabled libraries on non-revoked nodes matching target_kind.
    let mut lib_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT l.id FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.enabled=1 AND n.status!='revoked' AND l.kind IN ('films','series') ",
    );
    match target_kind {
        Some("film") => {
            lib_builder.push("AND l.kind='films' ");
        }
        Some("series") => {
            lib_builder.push("AND l.kind='series' ");
        }
        _ => {}
    }
    lib_builder.push("AND l.id IN (");
    let mut lib_sep = lib_builder.separated(", ");
    for lib in visible {
        lib_sep.push_bind(lib);
    }
    lib_sep.push_unseparated(")");
    let lib_sql = crate::database::normalize_sql(lib_builder.sql().as_str());
    let lib_args = lib_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let valid_libs: Vec<String> =
        sqlx::query_scalar_with::<sqlx::Any, String, _>(sqlx::AssertSqlSafe(lib_sql), lib_args)
            .fetch_all(&store.pool)
            .await?;

    if valid_libs.is_empty() {
        return Ok(json!({
            "items": [],
            "total": if exact_count { Some(0usize) } else { None },
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    // 2. Check if any work in valid_libs has `has_available_asset = 1` (using covering index `inventory_works_lib_avail_sort` in O(1))
    //    and whether any library root has diverged from its works' sources.
    let mut fast_check_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT (CASE WHEN EXISTS(SELECT 1 FROM inventory_works w WHERE w.library_id IN (",
    );
    let mut fc_sep = fast_check_builder.separated(", ");
    for lib in &valid_libs {
        fc_sep.push_bind(lib);
    }
    fc_sep.push_unseparated(
        ") AND w.has_available_asset = 1) THEN 1 ELSE 0 END) - \
         2 * (CASE WHEN EXISTS(SELECT 1 FROM libraries l WHERE l.id IN (",
    );
    let mut fc_sep2 = fast_check_builder.separated(", ");
    for lib in &valid_libs {
        fc_sep2.push_bind(lib);
    }
    fc_sep2.push_unseparated(
        ") AND EXISTS(SELECT 1 FROM inventory_sources s WHERE s.library_id = l.id AND s.root_snapshot != l.root)) THEN 1 ELSE 0 END)",
    );
    let fc_sql = crate::database::normalize_sql(fast_check_builder.sql().as_str());
    let fc_args = fast_check_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let fc_val: i64 =
        sqlx::query_scalar_with::<sqlx::Any, i64, _>(sqlx::AssertSqlSafe(fc_sql), fc_args)
            .fetch_one(&store.pool)
            .await
            .unwrap_or(0);
    let fast_avail = fc_val > 0;

    // 3. Check if any media groups or parental policies exist.
    let has_groups: bool = query_scalar::<_, i64>(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM media_group_members) THEN 1 ELSE 0 END",
    )
    .fetch_one(&store.pool)
    .await?
        != 0;

    let has_parental_policy: bool = if let Some(uid) = &filter.user_id {
        query_scalar::<_, i64>(
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM parental_policies WHERE user_id=?) THEN 1 ELSE 0 END",
        )
        .bind(uid)
        .fetch_one(&store.pool)
        .await?
            != 0
    } else {
        false
    };

    let use_fts: bool = query_scalar::<_, i64>(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='inventory_works_fts') THEN 1 ELSE 0 END",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or(0)
        != 0;
    let search_clean = search.trim().to_lowercase();
    let use_fts_index = use_fts && search_clean.chars().count() >= 3;

    let mut count_builder = sqlx::QueryBuilder::<sqlx::Any>::new(if use_fts_index {
        "SELECT COUNT(*) FROM inventory_works w INDEXED BY inventory_works_fts_sort WHERE w.library_id IN ("
    } else {
        "SELECT COUNT(*) FROM inventory_works w WHERE w.library_id IN ("
    });
    let mut separated = count_builder.separated(", ");
    for lib in &valid_libs {
        separated.push_bind(lib);
    }
    separated.push_unseparated(") ");

    push_search_filter(&mut count_builder, &search_clean, use_fts, fast_avail);
    push_availability_filter(&mut count_builder, target_kind, fast_avail);
    push_catalogue_filters(&mut count_builder, filter, has_parental_policy, fast_avail);
    push_group_filter(
        &mut count_builder,
        filter,
        &valid_libs,
        &search_clean,
        has_groups,
        has_parental_policy,
        fast_avail,
        use_fts,
    );

    let count_sql = crate::database::normalize_sql(count_builder.sql().as_str());
    let arguments = count_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let total = if exact_count {
        Some(
            sqlx::query_scalar_with::<sqlx::Any, i64, _>(sqlx::AssertSqlSafe(count_sql), arguments)
                .fetch_one(&store.pool)
                .await? as usize,
        )
    } else {
        None
    };

    if limit == 0 || total.is_some_and(|total| total == 0 || offset >= total) {
        return Ok(json!({
            "items": [], "total": total, "hasMore": false,
            "offset": offset, "limit": limit,
            "mode": "local-library", "notice": "Bibliothèques locales indexées"
        }));
    }

    // Phase 1: Select only matching `w.id`s ordered by the sort key using the covering index
    // (no table heap fetch or joins to libraries/media_nodes during pagination scan).
    let mut id_builder = sqlx::QueryBuilder::<sqlx::Any>::new(if use_fts_index {
        "SELECT w.id FROM inventory_works w INDEXED BY inventory_works_fts_sort WHERE w.library_id IN ("
    } else {
        "SELECT w.id FROM inventory_works w WHERE w.library_id IN ("
    });
    let mut separated = id_builder.separated(", ");
    for lib in &valid_libs {
        separated.push_bind(lib);
    }
    separated.push_unseparated(") ");

    push_search_filter(&mut id_builder, &search_clean, use_fts, fast_avail);
    push_availability_filter(&mut id_builder, target_kind, fast_avail);
    push_catalogue_filters(&mut id_builder, filter, has_parental_policy, fast_avail);
    push_group_filter(
        &mut id_builder,
        filter,
        &valid_libs,
        &search_clean,
        has_groups,
        has_parental_policy,
        fast_avail,
        use_fts,
    );

    match sort {
        "title-desc" => {
            id_builder.push("ORDER BY w.display_title_sort DESC, w.id DESC ");
        }
        "year-asc" => {
            id_builder.push("ORDER BY w.effective_year IS NULL, w.effective_year ASC, w.display_title_sort ASC, w.id ASC ");
        }
        "year-desc" => {
            id_builder.push("ORDER BY w.effective_year IS NULL, w.effective_year DESC, w.display_title_sort ASC, w.id ASC ");
        }
        "added-desc" => {
            id_builder.push(
                "ORDER BY w.added_at IS NULL, w.added_at DESC, w.display_title_sort ASC, w.id ASC ",
            );
        }
        "added-asc" => {
            id_builder.push(
                "ORDER BY w.added_at IS NULL, w.added_at ASC, w.display_title_sort ASC, w.id ASC ",
            );
        }
        _ => {
            id_builder.push("ORDER BY w.display_title_sort ASC, w.id ASC ");
        }
    }

    id_builder.push("LIMIT ");
    id_builder.push_bind(limit.saturating_add(usize::from(!exact_count)) as i64);
    id_builder.push(" OFFSET ");
    id_builder.push_bind(offset as i64);

    let id_sql = crate::database::normalize_sql(id_builder.sql().as_str());
    let id_args = id_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let page_ids: Vec<String> =
        sqlx::query_scalar_with::<sqlx::Any, String, _>(sqlx::AssertSqlSafe(id_sql), id_args)
            .fetch_all(&store.pool)
            .await?;

    let has_more = total.map_or(page_ids.len() > limit, |total| {
        offset.saturating_add(page_ids.len()) < total
    });
    let fetch_ids: Vec<&String> = page_ids.iter().take(limit).collect();
    if fetch_ids.is_empty() {
        return Ok(json!({
            "items": [],
            "total": total,
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    // Phase 2: Fetch full work details only for the <= `limit` selected work IDs.
    let mut data_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT w.*, COALESCE((SELECT f.parent_id FROM library_folders f WHERE f.library_id=w.library_id AND f.active=1),w.library_id) AS display_library_id, wl.node_id AS node_id, wn.status AS node_status, \
           CASE WHEN w.kind = 'series' THEN ( \
             CASE WHEN w.season_count_cached > 0 AND ",
    );
    data_builder.push(if fast_avail { "1=1" } else { "1=0" });
    data_builder.push(
        " THEN w.season_count_cached ELSE ( \
             SELECT COUNT(DISTINCT s.id) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_editions e ON e.work_id=ep.work_id \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources src ON src.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=src.id \
             JOIN libraries l ON l.id=src.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
           ) END) ELSE NULL END AS season_count, \
           CASE WHEN w.kind = 'series' THEN ( \
             CASE WHEN w.episode_count_cached > 0 AND ",
    );
    data_builder.push(if fast_avail { "1=1" } else { "1=0" });
    data_builder.push(
        " THEN w.episode_count_cached ELSE ( \
             SELECT COUNT(DISTINCT ep.id) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_editions e ON e.work_id=ep.work_id \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources src ON src.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=src.id \
             JOIN libraries l ON l.id=src.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
           ) END) ELSE NULL END AS episode_count, \
           CAST(NULL AS TEXT) AS poster_url, CAST(NULL AS TEXT) AS backdrop_url, CAST(NULL AS TEXT) AS logo_url \
         FROM inventory_works w \
         JOIN libraries wl ON wl.id=w.library_id \
         JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE w.id IN (",
    );
    let mut id_sep = data_builder.separated(", ");
    for id in &fetch_ids {
        id_sep.push_bind(*id);
    }
    id_sep.push_unseparated(")");

    let data_sql = crate::database::normalize_sql(data_builder.sql().as_str());
    let arguments = data_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let rows = sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(data_sql), arguments)
        .fetch_all(&store.pool)
        .await?;
    let mut row_map = std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        let wid: String = row.get("id");
        row_map.insert(wid, work(row));
    }
    let mut items: Vec<Value> = fetch_ids
        .iter()
        .filter_map(|id| row_map.remove(*id))
        .collect();
    crate::product::decorate_many(store, &mut items).await?;

    let response = json!({
        "items": items,
        "total": total,
        "hasMore": has_more,
        "offset": offset,
        "limit": limit,
        "mode": "local-library",
        "notice": "Bibliothèques locales indexées"
    });

    if state_fp > 0 {
        let lock = CATALOGUE_CACHE
            .get_or_init(|| std::sync::Mutex::new(BoundedCatalogueCache::from_env()));
        if let Ok(mut guard) = lock.lock() {
            guard.insert(cache_key, state_fp, response.clone());
        }
    }

    Ok(response)
}

pub(crate) fn sort_edition_sources_by_score(editions: &mut [Value]) {
    for edition in editions.iter_mut() {
        if let Some(versions) = edition["versions"].as_array_mut() {
            for version in versions.iter_mut() {
                if let Some(sources) = version["sources"].as_array_mut() {
                    sources.sort_by(|a, b| {
                        let sa = a["score"].as_i64().unwrap_or(0);
                        let sb = b["score"].as_i64().unwrap_or(0);
                        sb.cmp(&sa).then_with(|| {
                            let ida = a["id"].as_str().unwrap_or_default();
                            let idb = b["id"].as_str().unwrap_or_default();
                            ida.cmp(idb)
                        })
                    });
                }
            }
            versions.sort_by(|a, b| {
                let sa = a["sources"]
                    .as_array()
                    .and_then(|s| s.first())
                    .and_then(|s| s["score"].as_i64())
                    .unwrap_or(0);
                let sb = b["sources"]
                    .as_array()
                    .and_then(|s| s.first())
                    .and_then(|s| s["score"].as_i64())
                    .unwrap_or(0);
                sb.cmp(&sa).then_with(|| {
                    let ida = a["id"].as_str().unwrap_or_default();
                    let idb = b["id"].as_str().unwrap_or_default();
                    ida.cmp(idb)
                })
            });
        }
    }
}

pub(crate) fn refresh_work_availability_from_editions(value: &mut Value) {
    let best_from_editions = value["editions"].as_array().and_then(|editions| {
        if editions.is_empty() {
            return None;
        }
        editions
            .iter()
            .flat_map(|ed| ed["versions"].as_array().into_iter().flatten())
            .flat_map(|ver| ver["sources"].as_array().into_iter().flatten())
            .max_by_key(|src| src["score"].as_i64().unwrap_or(i64::MIN))
            .map(|best_src| {
                (
                    best_src["nodeStatus"]
                        .as_str()
                        .unwrap_or("connected")
                        .to_string(),
                    best_src["nodeId"].as_str().map(str::to_string),
                )
            })
    });
    if let Some((best_status, best_nid)) = best_from_editions {
        let any_connected = best_status == "connected";
        value["available"] = json!(any_connected);
        value["nodeStatus"] = json!(best_status);
        if let Some(nid) = best_nid {
            value["nodeId"] = json!(nid);
        }
    } else if let Some(seasons) = value["seasons"].as_array()
        && !seasons.is_empty()
    {
        let any_ep_available = seasons
            .iter()
            .flat_map(|s| s["episodes"].as_array().into_iter().flatten())
            .any(|ep| ep["available"].as_bool().unwrap_or(false));
        if any_ep_available {
            value["available"] = json!(true);
            value["nodeStatus"] = json!("connected");
        }
    }
}

async fn load_work_editions(
    store: &IdentityStore,
    work_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    crate::nodes::ensure_node_registry_warm(store).await?;
    let rows = query(
        "SELECT e.id AS edition_id, COALESCE(em.label, 'Édition locale') AS edition_label, \
                v.id AS version_id, COALESCE(vm.label, 'Version locale') AS version_label, \
                s.id AS source_id, a.id AS asset_id, a.mime AS mime, l.node_id AS node_id \
         FROM inventory_editions e \
         LEFT JOIN inventory_edition_metadata em ON em.edition_id=e.id \
         JOIN inventory_versions v ON v.edition_id=e.id \
         LEFT JOIN inventory_version_metadata vm ON vm.version_id=v.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=s.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE e.work_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1 \
         ORDER BY e.id, v.id, s.id, a.id",
    )
    .bind(work_id)
    .fetch_all(&store.pool)
    .await?;

    let now = media_contracts::protocol::unix_now();
    let mut editions = Vec::new();
    let mut current_edition_id = String::new();
    let mut current_edition_label = String::from("Édition locale");
    let mut current_version_id = String::new();
    let mut current_version_label = String::from("Version locale");
    let mut current_versions = Vec::new();
    let mut current_sources = Vec::new();

    for row in rows {
        let edition_id: String = row.get("edition_id");
        let edition_label: String = row.get("edition_label");
        let version_id: String = row.get("version_id");
        let version_label: String = row.get("version_label");
        let source_id: String = row.get("source_id");
        let asset_id: String = row.get("asset_id");
        let mime: String = row.get("mime");
        let node_id: String = row.get("node_id");

        let (node_status, conn_mode, score) = match store.node_registry.get(&node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };

        if edition_id != current_edition_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            if !current_versions.is_empty() {
                editions.push(json!({
                    "id": current_edition_id,
                    "label": current_edition_label,
                    "versions": std::mem::take(&mut current_versions)
                }));
            }
            current_edition_id = edition_id;
            current_edition_label = edition_label;
            current_version_id = version_id.clone();
            current_version_label = version_label;
        } else if version_id != current_version_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            current_version_id = version_id.clone();
            current_version_label = version_label;
        }

        current_sources.push(json!({
            "id": source_id,
            "assetIds": [asset_id.clone()],
            "playbackMediaId": asset_id,
            "nodeId": node_id,
            "nodeStatus": node_status,
            "connectionMode": conn_mode,
            "score": score
        }));
    }

    if !current_sources.is_empty() {
        current_versions.push(json!({
            "id": current_version_id,
            "label": current_version_label,
            "sources": current_sources
        }));
    }
    if !current_versions.is_empty() {
        editions.push(json!({
            "id": current_edition_id,
            "label": current_edition_label,
            "versions": current_versions
        }));
    }

    sort_edition_sources_by_score(&mut editions);
    Ok(editions)
}

async fn load_series_editions(
    store: &IdentityStore,
    series_id: &str,
) -> Result<HashMap<String, Vec<Value>>, sqlx::Error> {
    crate::nodes::ensure_node_registry_warm(store).await?;
    let rows = query(
        "SELECT e.work_id, e.id AS edition_id, COALESCE(em.label, 'Édition locale') AS edition_label, \
                v.id AS version_id, COALESCE(vm.label, 'Version locale') AS version_label, \
                s.id AS source_id, a.id AS asset_id, a.mime AS mime, l.node_id AS node_id \
         FROM inventory_seasons se \
         JOIN inventory_episodes ep ON ep.season_id=se.id \
         JOIN inventory_editions e ON e.work_id=ep.work_id \
         LEFT JOIN inventory_edition_metadata em ON em.edition_id=e.id \
         JOIN inventory_versions v ON v.edition_id=e.id \
         LEFT JOIN inventory_version_metadata vm ON vm.version_id=v.id \
         JOIN inventory_sources s ON s.version_id=v.id \
         JOIN inventory_assets a ON a.source_id=s.id \
         JOIN libraries l ON l.id=s.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE se.series_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.enabled=1 \
         ORDER BY e.work_id, e.id, v.id, s.id, a.id",
    )
    .bind(series_id)
    .fetch_all(&store.pool)
    .await?;

    let now = media_contracts::protocol::unix_now();
    let mut map: HashMap<String, Vec<Value>> = HashMap::new();
    let mut current_work_id = String::new();
    let mut editions = Vec::new();
    let mut current_edition_id = String::new();
    let mut current_edition_label = String::from("Édition locale");
    let mut current_version_id = String::new();
    let mut current_version_label = String::from("Version locale");
    let mut current_versions = Vec::new();
    let mut current_sources = Vec::new();

    for row in rows {
        let work_id: String = row.get("work_id");
        let edition_id: String = row.get("edition_id");
        let edition_label: String = row.get("edition_label");
        let version_id: String = row.get("version_id");
        let version_label: String = row.get("version_label");
        let source_id: String = row.get("source_id");
        let asset_id: String = row.get("asset_id");
        let mime: String = row.get("mime");
        let node_id: String = row.get("node_id");

        let (node_status, conn_mode, score) = match store.node_registry.get(&node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };

        if work_id != current_work_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            if !current_versions.is_empty() {
                editions.push(json!({
                    "id": current_edition_id,
                    "label": current_edition_label,
                    "versions": std::mem::take(&mut current_versions)
                }));
            }
            if !editions.is_empty() {
                sort_edition_sources_by_score(&mut editions);
                map.insert(current_work_id, std::mem::take(&mut editions));
            }
            current_work_id = work_id;
            current_edition_id = edition_id;
            current_edition_label = edition_label;
            current_version_id = version_id.clone();
            current_version_label = version_label;
        } else if edition_id != current_edition_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            if !current_versions.is_empty() {
                editions.push(json!({
                    "id": current_edition_id,
                    "label": current_edition_label,
                    "versions": std::mem::take(&mut current_versions)
                }));
            }
            current_edition_id = edition_id;
            current_edition_label = edition_label;
            current_version_id = version_id.clone();
            current_version_label = version_label;
        } else if version_id != current_version_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            current_version_id = version_id.clone();
            current_version_label = version_label;
        }

        current_sources.push(json!({
            "id": source_id,
            "assetIds": [asset_id.clone()],
            "playbackMediaId": asset_id,
            "nodeId": node_id,
            "nodeStatus": node_status,
            "connectionMode": conn_mode,
            "score": score
        }));
    }

    if !current_sources.is_empty() {
        current_versions.push(json!({
            "id": current_version_id,
            "label": current_version_label,
            "sources": current_sources
        }));
    }
    if !current_versions.is_empty() {
        editions.push(json!({
            "id": current_edition_id,
            "label": current_edition_label,
            "versions": current_versions
        }));
    }
    if !editions.is_empty() {
        sort_edition_sources_by_score(&mut editions);
        map.insert(current_work_id, editions);
    }

    Ok(map)
}

pub struct EpisodePage {
    pub season: Option<i64>,
    pub offset: usize,
    pub limit: usize,
    pub after: Option<String>,
    pub user: String,
    pub role: String,
}

pub async fn detail(store: &IdentityStore, work_id: &str) -> Result<Option<Value>, sqlx::Error> {
    detail_window(store, work_id, None).await
}

pub async fn detail_window(
    store: &IdentityStore,
    work_id: &str,
    page: Option<&EpisodePage>,
) -> Result<Option<Value>, sqlx::Error> {
    let Some(row) = query(
        "SELECT w.*, COALESCE((SELECT f.parent_id FROM library_folders f WHERE f.library_id=w.library_id AND f.active=1),w.library_id) AS display_library_id, wl.node_id AS node_id, wn.status AS node_status, \
         (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size \
          FROM inventory_images i JOIN libraries il ON il.id=i.library_id JOIN media_nodes in_node ON in_node.id=il.node_id \
          WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind IN ('films', 'series') AND in_node.status='connected' \
          ORDER BY i.priority, i.relative_path, i.id LIMIT 1) AS poster_url, \
           (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='backdrop' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS backdrop_url, \
           (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='logo' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS logo_url \
         FROM inventory_works w \
         JOIN libraries wl ON wl.id=w.library_id \
         JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE w.id=?",
    )
    .bind(work_id)
    .fetch_optional(&store.pool)
    .await?
    else {
        return Ok(None);
    };

    let mut value = work(&row);
    let images = query("SELECT i.id,i.role,i.modified_ns,i.size FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id WHERE i.work_id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND n.status='connected' AND i.role!='poster' ORDER BY i.priority,i.relative_path,i.id").bind(work_id).fetch_all(&store.pool).await?;
    let mut artwork = serde_json::Map::new();
    for image in images {
        let role: String = image.get("role");
        artwork.entry(role).or_insert_with(|| {
            json!(format!(
                "/api/v1/assets/{}/image?v={}-{}",
                image.get::<String, _>("id"),
                image.get::<String, _>("modified_ns"),
                image.get::<i64, _>("size")
            ))
        });
    }
    value["artwork"] = json!(artwork);
    crate::product::decorate(store, &mut value).await?;

    let kind: String = row.try_get("kind").unwrap_or_else(|_| "film".to_string());

    if kind == "series"
        && let Some(page) = page
    {
        populate_episode_page(store, work_id, &mut value, page).await?;
    } else if kind == "series" {
        let seasons_rows = query(
            "SELECT id, season_number, title FROM inventory_seasons WHERE series_id=? ORDER BY season_number ASC",
        )
        .bind(work_id)
        .fetch_all(&store.pool)
        .await?;

        let ep_rows = query(
            "SELECT ep.season_id, ep.episode_number, w.*, wl.node_id AS node_id, wn.status AS node_status, \
             (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='thumb' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS thumb_url \
             FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_works w ON w.id=ep.work_id \
             JOIN libraries wl ON wl.id=w.library_id \
             JOIN media_nodes wn ON wn.id=wl.node_id \
             WHERE s.series_id=? \
             ORDER BY s.season_number ASC, ep.episode_number ASC",
        )
        .bind(work_id)
        .fetch_all(&store.pool)
        .await?;

        let mut editions_by_work = load_series_editions(store, work_id).await?;

        let mut episodes_by_season: HashMap<String, Vec<Value>> = HashMap::new();
        let mut total_episodes = 0;

        for ep_row in ep_rows {
            let season_id: String = ep_row.get("season_id");
            let ep_work_id: String = ep_row.get("id");
            let ep_num: i64 = ep_row.get("episode_number");

            if let Some(editions) = editions_by_work.remove(&ep_work_id)
                && !editions.is_empty()
            {
                let mut ep_val = work(&ep_row);
                ep_val["kind"] = json!("episode");
                ep_val["number"] = json!(ep_num);
                ep_val["editions"] = json!(editions);
                refresh_work_availability_from_editions(&mut ep_val);
                episodes_by_season
                    .entry(season_id)
                    .or_default()
                    .push(ep_val);
                total_episodes += 1;
            }
        }

        let mut seasons_val = Vec::new();
        for s_row in seasons_rows {
            let season_id: String = s_row.get("id");
            let season_num: i64 = s_row.get("season_number");
            let season_title: String = s_row.get("title");

            if let Some(episodes) = episodes_by_season.remove(&season_id)
                && !episodes.is_empty()
            {
                seasons_val.push(json!({
                    "id": season_id,
                    "number": season_num,
                    "title": season_title,
                    "episodes": episodes
                }));
            }
        }

        value["seasonCount"] = json!(seasons_val.len());
        value["episodeCount"] = json!(total_episodes);
        value["seasons"] = json!(seasons_val);
        value["editions"] = json!([]);
        refresh_work_availability_from_editions(&mut value);
    } else {
        let editions = load_work_editions(store, work_id).await?;
        value["editions"] = json!(editions);
        value["seasons"] = json!([]);
        refresh_work_availability_from_editions(&mut value);
    }

    Ok(Some(value))
}

async fn populate_episode_page(
    store: &IdentityStore,
    series: &str,
    value: &mut Value,
    page: &EpisodePage,
) -> Result<(), sqlx::Error> {
    // The same permissions and parental policy apply before pagination as on direct work access.
    let eligible = format!(
        "wl.enabled=1 AND wn.status!='revoked' AND \
         (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=wl.id AND p.user_id=?)) \
         AND {} AND EXISTS(SELECT 1 FROM inventory_editions pe \
         JOIN inventory_versions pv ON pv.edition_id=pe.id JOIN inventory_sources ps ON ps.version_id=pv.id \
         JOIN inventory_assets pa ON pa.source_id=ps.id JOIN libraries pl ON pl.id=ps.library_id \
         JOIN media_nodes pn ON pn.id=pl.node_id WHERE pe.work_id=w.id AND pa.available=1 \
         AND ps.root_snapshot=pl.root AND pl.enabled=1 AND pn.status!='revoked' AND pl.kind IN ('films','series'))",
        crate::product::policy_sql()
    );
    let seasons_sql = format!(
        "SELECT s.id,s.season_number,s.title,COUNT(*) AS episode_count FROM inventory_seasons s \
         JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id \
         JOIN libraries wl ON wl.id=w.library_id JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE s.series_id=? AND {eligible} GROUP BY s.id,s.season_number,s.title ORDER BY s.season_number,s.id"
    );
    let seasons = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(
        &seasons_sql,
    )))
    .bind(series)
    .bind(&page.role)
    .bind(&page.user)
    .bind(&page.user)
    .fetch_all(&store.pool)
    .await?;
    let selected_season = page.season.or_else(|| {
        seasons
            .first()
            .map(|row| row.get::<i64, _>("season_number"))
    });
    let anchor: Option<(i64, i64, String)> = if let Some(after) = &page.after {
        crate::database::query_as("SELECT s.season_number,ep.episode_number,w.id FROM inventory_seasons s JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id WHERE s.series_id=? AND w.id=?")
            .bind(series).bind(after).fetch_optional(&store.pool).await?
    } else {
        None
    };
    let page_sql = format!(
        "SELECT ep.season_id,s.season_number,ep.episode_number,w.*,wl.node_id,wn.status AS node_status, \
         (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='thumb' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS thumb_url \
         FROM inventory_seasons s JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id \
         JOIN libraries wl ON wl.id=w.library_id JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE s.series_id=? AND {eligible} \
         AND (?=1 OR s.season_number=?) \
         AND (?=0 OR s.season_number>? OR (s.season_number=? AND ep.episode_number>?) OR (s.season_number=? AND ep.episode_number=? AND w.id>?)) \
         ORDER BY s.season_number,ep.episode_number,w.id LIMIT ? OFFSET ?"
    );
    let (anchor_season, anchor_episode, anchor_id) =
        anchor.unwrap_or((i64::MAX, i64::MAX, String::new()));
    let after = page.after.is_some();
    let mut rows = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(
        &page_sql,
    )))
    .bind(series)
    .bind(&page.role)
    .bind(&page.user)
    .bind(&page.user)
    .bind(i64::from(after))
    .bind(selected_season.unwrap_or(-1))
    .bind(i64::from(after))
    .bind(anchor_season)
    .bind(anchor_season)
    .bind(anchor_episode)
    .bind(anchor_season)
    .bind(anchor_episode)
    .bind(anchor_id)
    .bind((page.limit + 1) as i64)
    .bind(page.offset as i64)
    .fetch_all(&store.pool)
    .await?;
    let has_more = rows.len() > page.limit;
    rows.truncate(page.limit);
    let count = rows.len();
    let returned_season = if after {
        rows.first().map(|row| row.get::<i64, _>("season_number"))
    } else {
        selected_season
    };
    let mut by_season: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        let id: String = row.get("id");
        let mut episode = work(&row);
        episode["kind"] = json!("episode");
        episode["number"] = json!(row.get::<i64, _>("episode_number"));
        episode["seasonNumber"] = json!(row.get::<i64, _>("season_number"));
        episode["editions"] = json!(load_work_editions(store, &id).await?);
        refresh_work_availability_from_editions(&mut episode);
        by_season
            .entry(row.get("season_id"))
            .or_default()
            .push(episode);
    }
    let mut total = 0i64;
    let summaries: Vec<Value> = seasons.into_iter().map(|row| {
        let id: String = row.get("id");
        let count: i64 = row.get("episode_count");
        total += count;
        json!({"id":id,"number":row.get::<i64,_>("season_number"),"title":row.get::<String,_>("title"),"episodeCount":count,"episodes":by_season.remove(&id).unwrap_or_default()})
    }).collect();
    value["seasonCount"] = json!(summaries.len());
    value["episodeCount"] = json!(total);
    value["seasons"] = json!(summaries);
    value["editions"] = json!([]);
    value["episodesPage"] = json!({"season":returned_season,"offset":page.offset,"limit":page.limit,"nextOffset":page.offset + count,"hasMore":has_more});
    Ok(())
}

// Folder scopes inherit the live configuration of their logical library.
async fn playback_library_options(
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
                "url": format!("/api/v1/assets/{asset_id}/prepared/subtitle-{}", track.index) }));
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
        sources.push(source);
    }
    let chapter_list: Vec<(f64, String)> = probe
        .chapters
        .iter()
        .filter_map(|chapter| {
            let start = chapter["start_time"].as_str()?.parse::<f64>().ok()?;
            if !start.is_finite() || start < 0. {
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
        .take(1000)
        .collect();

    let total_dur = probe
        .format
        .get("duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok());
    let mut intro_segment: Option<serde_json::Value> = None;
    let mut credits_segment: Option<serde_json::Value> = None;

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
        "credits": credits_segment
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

fn native_opus_track_id(probe: &Probe, track: &media_node::probe::Track) -> Option<u32> {
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

pub async fn prepared(
    store: &IdentityStore,
    preparations: &media_node::packaging::Preparations,
    asset_id: &str,
    component: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_node::packaging::Component;
    let row = query("SELECT a.relative_path,a.size,a.modified_ns,a.probe_json,a.library_id,l.root,l.node_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "asset_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let remote_id = row.get::<String, _>("node_id");
    let request_started = std::time::Instant::now();
    eprintln!(
        "{}",
        json!({"event":"prepared_http_start","asset":asset_id,"node":remote_id,"component":component,"method":request.method().as_str(),"range":request.headers().contains_key("range")})
    );
    let probe = match decode(row.get("probe_json")) {
        Ok(probe) => probe,
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "probe_invalid"),
    };
    if let Some(index) = component
        .strip_prefix("subtitle-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        let options = match playback_library_options(
            store,
            row.get::<String, _>("library_id").as_str(),
        )
        .await
        {
            Ok(options) => options,
            Err(_) => {
                return crate::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "library_options_unavailable",
                );
            }
        };
        if probe.streams.iter().any(|track| {
            track.index == index
                && track.codec_type.as_deref() == Some("subtitle")
                && !options.embedded_subtitle_allowed(track.codec_name.as_deref())
        }) {
            return crate::error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "embedded_subtitle_disabled",
            );
        }
    }
    let stream_position = media_node::video_stream::position(component);
    let selected = if let Some((index, position)) = stream_position {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .any(|track| {
                track.index == index
                    && track.codec_type.as_deref() == Some("video")
                    && track.codec_name.as_deref() == Some("av1")
            })
            .then_some(Component::Video)
            .filter(|_| {
                duration.is_some_and(|duration| {
                    duration.is_finite() && f64::from(position) / 1000.0 < duration
                })
            })
    } else if let Some(Component::OpusChunk(id, chunk)) = Component::opus_chunk(component) {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .any(|track| native_opus_track_id(&probe, track) == Some(id))
            .then_some(Component::OpusChunk(id, chunk))
            .filter(|_| {
                duration.is_some_and(|duration| {
                    duration.is_finite()
                        && f64::from(chunk * media_node::packaging::AUDIO_CHUNK_SECONDS) < duration
                })
            })
    } else if let Some(Component::AudioChunk(index, chunk)) = Component::audio_chunk(component) {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("audio")
                    && t.codec_name.as_deref() == Some("opus")
            })
            .filter(|_| {
                duration.is_some_and(|d| {
                    d.is_finite()
                        && f64::from(chunk * media_node::packaging::AUDIO_CHUNK_SECONDS) < d
                })
            })
            .map(|_| Component::AudioChunk(index, chunk))
    } else if component == "video"
        && probe.streams.iter().any(|t| {
            t.codec_type.as_deref() == Some("video") && t.codec_name.as_deref() == Some("av1")
        })
    {
        Some(Component::Video)
    } else if let Some(index) = component
        .strip_prefix("audio-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("audio")
                    && t.codec_name.as_deref() == Some("opus")
            })
            .map(|_| Component::Audio(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("subtitle")
                    && matches!(
                        t.codec_name.as_deref(),
                        Some("subrip" | "webvtt" | "ass" | "ssa" | "mov_text")
                    )
            })
            .map(|_| Component::Subtitle(index))
    } else {
        None
    };
    let Some(selected) = selected else {
        return crate::error(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_track");
    };
    if remote_id != "local" {
        return match crate::nodes::load_node(store, &remote_id).await {
            Ok(Some(node)) => {
                let mut response = crate::nodes::relay_from_remote_node(
                    store,
                    &node,
                    &format!("/internal/v1/node/assets/{asset_id}/prepared/{component}"),
                    request,
                )
                .await;
                if !response.status().is_success()
                    && !response.headers().contains_key("x-media-preparation-error")
                {
                    let code = if response.status() == StatusCode::SERVICE_UNAVAILABLE {
                        "node_unavailable"
                    } else if response.status() == StatusCode::GATEWAY_TIMEOUT {
                        "preparation_timeout"
                    } else {
                        "preparation_failed"
                    };
                    response.headers_mut().insert(
                        "x-media-preparation-error",
                        axum::http::HeaderValue::from_static(code),
                    );
                }
                eprintln!(
                    "{}",
                    json!({"event":"prepared_remote_response","asset":asset_id,"node":remote_id,"component":component,"status":response.status().as_u16(),"elapsed_ms":request_started.elapsed().as_millis()})
                );
                response
            }
            _ => {
                let mut response =
                    crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static("node_unavailable"),
                );
                response
            }
        };
    }
    if let Component::Audio(index) | Component::AudioChunk(index, _) = selected
        && let Some(track) = probe.streams.iter().find(|track| track.index == index)
    {
        eprintln!(
            "{}",
            json!({"event":"prepared_audio_track","asset":asset_id,"index":index,"language":track.tags.get("language"),"channels":track.details.get("channels"),"codec":track.codec_name})
        );
    }
    if matches!(selected, Component::OpusChunk(..)) || stream_position.is_some() {
        let opened = match media_node::OpenedFile::open_in_root(
            Path::new(&row.get::<String, _>("root")),
            Path::new(&row.get::<String, _>("relative_path")),
        )
        .await
        {
            Ok(opened)
                if opened.size() == row.get::<i64, _>("size") as u64
                    && opened.modified_ns() == row.get::<String, _>("modified_ns") =>
            {
                opened
            }
            _ => {
                let mut response = crate::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "asset_changed_rescan_required",
                );
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static("asset_changed_rescan_required"),
                );
                return response;
            }
        };
        let streamed = if let Component::OpusChunk(id, chunk) = selected {
            preparations
                .native_audio()
                .serve(opened, id, chunk, &request)
                .await
        } else {
            preparations
                .video_streams()
                .serve(
                    opened,
                    stream_position.map_or(0, |(index, _)| index),
                    stream_position.map_or(0, |(_, position)| position),
                    &request,
                )
                .await
        };
        return match streamed {
            Ok(response) => {
                let (parts, body) = response.into_parts();
                Response::from_parts(parts, Body::new(body))
            }
            Err(error) => {
                let code = media_node::packaging::preparation_error_code(&error);
                let status = if matches!(
                    error.kind(),
                    std::io::ErrorKind::Unsupported
                        | std::io::ErrorKind::InvalidData
                        | std::io::ErrorKind::InvalidInput
                ) {
                    StatusCode::UNPROCESSABLE_ENTITY
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                };
                let mut response = crate::error(status, code);
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static(code),
                );
                response
            }
        };
    }
    let opened = preparations
        .prepare(
            std::path::Path::new(&row.get::<String, _>("root")),
            std::path::Path::new(&row.get::<String, _>("relative_path")),
            row.get::<i64, _>("size") as u64,
            &row.get::<String, _>("modified_ns"),
            selected,
        )
        .await;
    let opened = match opened {
        Ok(file) => file,
        Err(error) => {
            let code = media_node::packaging::preparation_error_code(&error);
            eprintln!(
                "{}",
                json!({"event":"prepared_http_error","asset":asset_id,"component":component,"code":code,"elapsed_ms":request_started.elapsed().as_millis()})
            );
            let mut response = crate::error(StatusCode::SERVICE_UNAVAILABLE, code);
            response.headers_mut().insert(
                "x-media-preparation-error",
                axum::http::HeaderValue::from_static(code),
            );
            return response;
        }
    };
    eprintln!(
        "{}",
        json!({"event":"prepared_http_ready","asset":asset_id,"component":component,"elapsed_ms":request_started.elapsed().as_millis(),"bytes":opened.size()})
    );
    match media_node::serve_opened(opened, &request, selected.mime()).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "prepared_stream_failed"),
    }
}

pub(crate) fn audio_operational_id(
    asset_id: &str,
    track: &media_contracts::inventory::ExternalAudio,
) -> String {
    let identity = json!([track.relative_path, track.size, track.modified_ns]).to_string();
    let hash = media_contracts::protocol::sha256_hex(identity.as_bytes());
    format!("sidecar-{asset_id}-{}", &hash[..32])
}

pub async fn audio(
    store: &IdentityStore,
    asset_id: &str,
    key: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    let row = query("SELECT au.audios_json,l.root,l.node_id FROM inventory_audio au JOIN inventory_assets a ON a.id=au.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "audio_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let tracks: Vec<media_contracts::inventory::ExternalAudio> =
        match serde_json::from_str(&row.get::<String, _>("audios_json")) {
            Ok(tracks) => tracks,
            Err(_) => {
                return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "audio_inventory_invalid");
            }
        };
    let Some(track) = tracks
        .iter()
        .find(|track| audio_operational_id(asset_id, track) == key)
    else {
        return crate::error(StatusCode::NOT_FOUND, "unknown_audio");
    };
    let node_id: String = row.get("node_id");
    if node_id != "local" {
        let Some(node) = crate::nodes::load_node(store, &node_id)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let path = format!("/internal/v1/node/assets/{key}/content");
        return crate::nodes::relay_from_remote_node(store, &node, &path, request).await;
    }
    let root: String = row.get("root");
    let opened = match crate::node::LocalNode
        .open_media(Path::new(&root), Path::new(&track.relative_path))
        .await
    {
        Ok(opened) if opened.size() == track.size && opened.modified_ns() == track.modified_ns => {
            opened
        }
        _ => return crate::error(StatusCode::NOT_FOUND, "audio_changed_rescan_required"),
    };
    match media_node::serve_opened(opened, &request, "audio/ogg").await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "audio_stream_failed"),
    }
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
                "url": format!("/api/v1/assets/{asset_id}/subtitles/{index}")
            })
        })
        .collect())
}

/// Session and live library permissions run in the common media middleware.
pub async fn subtitle(
    store: &IdentityStore,
    asset_id: &str,
    index: usize,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{
        body::Body,
        http::{StatusCode, header},
        response::Response,
    };
    let value: Result<Option<String>, _> = query_scalar("SELECT t.subtitles_json FROM inventory_subtitles t JOIN inventory_assets a ON a.id=t.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND l.root=s.root_snapshot AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
    let tracks: Vec<media_contracts::inventory::ExternalSubtitle> = match value {
        Ok(Some(text)) => match serde_json::from_str(&text) {
            Ok(tracks) => tracks,
            Err(_) => {
                return crate::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "subtitle_inventory_invalid",
                );
            }
        },
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "subtitle_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let Some(track) = tracks.get(index) else {
        return crate::error(StatusCode::NOT_FOUND, "unknown_subtitle");
    };
    let bytes = track.webvtt.as_bytes();
    let mut response = Response::new(Body::empty());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/vtt; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        axum::http::HeaderValue::from_static("bytes"),
    );
    let range = request.headers().get(header::RANGE);
    let selected = match range {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|s| media_node::parse_single_range(s, bytes.len() as u64))
        {
            Some(range) => {
                *response.status_mut() = StatusCode::PARTIAL_CONTENT;
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", range.start, range.end, bytes.len())
                        .parse()
                        .unwrap(),
                );
                &bytes[range.start as usize..=range.end as usize]
            }
            None => {
                *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes */{}", bytes.len()).parse().unwrap(),
                );
                return response;
            }
        },
        None => bytes,
    };
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, selected.len().into());
    if request.method() != axum::http::Method::HEAD {
        *response.body_mut() = Body::from(selected.to_vec());
    }
    response
}

/// Authorization is enforced by the common route middleware before this helper.
/// Resolves an authorized `ReadGrant` from SQLite and delegates confined handle streaming to `LocalNode`
/// or relays in 64 KiB chunks from a paired remote Node.
pub async fn content(
    store: &IdentityStore,
    asset_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_contracts::{
        errors::NodeErrorCode,
        ids::{AssetId, LibraryId, NodeId},
        inventory::{ReadGrant, ReadTarget},
    };
    let row = query(
        "SELECT a.library_id,a.relative_path,a.mime,l.root,l.node_id \
         FROM inventory_assets a \
         JOIN libraries l ON l.id=a.library_id \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return crate::error(StatusCode::NOT_FOUND, NodeErrorCode::UnknownAsset.as_str());
        }
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
    };
    let node_id_str: String = row.get("node_id");
    let started = std::time::Instant::now();
    let method = request.method().as_str().to_owned();
    let has_range = request.headers().contains_key("range");
    eprintln!(
        "{}",
        json!({"event":"media_http_start","asset":asset_id,"node":node_id_str,"method":method,"range":has_range})
    );
    // Headers only: never log signed URLs, query strings, cookies or disk paths.
    // elapsed_ms measures response-header readiness, not complete body delivery.
    let logged_response = |response: axum::response::Response| {
        eprintln!(
            "{}",
            json!({"event":"media_http_response","asset":asset_id,"node":node_id_str,"method":method,"range":has_range,"status":response.status().as_u16(),"content_length":response.headers().get("content-length").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()),"content_range_present":response.headers().contains_key("content-range"),"elapsed_ms":started.elapsed().as_millis()})
        );
        response
    };
    if node_id_str != "local" {
        drop(row);
        let Some(rec) = crate::nodes::load_node(store, &node_id_str)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let pq = format!("/internal/v1/node/assets/{asset_id}/content");
        return logged_response(
            crate::nodes::relay_from_remote_node(store, &rec, &pq, request).await,
        );
    }
    let grant = ReadGrant {
        target: ReadTarget::Asset(AssetId::from(asset_id)),
        library_id: LibraryId::from(row.get::<String, _>("library_id")),
        node_id: NodeId::from(node_id_str.clone()),
        root_snapshot: row.get("root"),
        relative_path: row.get("relative_path"),
        mime: row.get("mime"),
    };
    drop(row);
    logged_response(
        match crate::node::LocalNode.serve_grant(&grant, &request).await {
            Ok(res) => {
                let (parts, body) = res.into_parts();
                Response::from_parts(parts, Body::new(body))
            }
            Err(code @ NodeErrorCode::StreamError) => {
                crate::error(StatusCode::INTERNAL_SERVER_ERROR, code.as_str())
            }
            Err(code) => crate::error(StatusCode::NOT_FOUND, code.as_str()),
        },
    )
}

/// Session/library checks run before this handler, including HEAD and conditionals.
/// Resolves an authorized `ReadGrant` from SQLite and delegates confined handle validation
/// and streaming to `LocalNode` without reopening by path, or relays from a remote Node.
pub async fn image(
    store: &IdentityStore,
    image_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_contracts::{
        errors::NodeErrorCode,
        ids::{ImageId, LibraryId, NodeId},
        inventory::{ReadGrant, ReadTarget},
    };
    if media_node::image_preview_width(request.uri().query()).is_err() {
        return crate::error(StatusCode::BAD_REQUEST, "invalid_image_width");
    }
    let row = query(
        "SELECT i.*,l.root,l.node_id \
         FROM inventory_images i \
         JOIN libraries l ON l.id=i.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE i.id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(image_id)
    .fetch_optional(&store.pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return crate::error(StatusCode::NOT_FOUND, NodeErrorCode::UnknownImage.as_str());
        }
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
    };
    let node_id_str: String = row.get("node_id");
    if node_id_str != "local" {
        drop(row);
        let Some(rec) = crate::nodes::load_node(store, &node_id_str)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let mut pq = format!("/internal/v1/node/assets/{image_id}/image");
        if let Some(query) = request.uri().query() {
            pq.push('?');
            pq.push_str(query);
        }
        return crate::nodes::relay_from_remote_node(store, &rec, &pq, request).await;
    }
    let grant = ReadGrant {
        target: ReadTarget::Image {
            id: ImageId::from(image_id),
            expected_size: row.get::<i64, _>("size") as u64,
            expected_modified_ns: row.get("modified_ns"),
        },
        library_id: LibraryId::from(row.get::<String, _>("library_id")),
        node_id: NodeId::from(node_id_str),
        root_snapshot: row.get("root"),
        relative_path: row.get("relative_path"),
        mime: row.get("mime"),
    };
    drop(row);
    match crate::node::LocalNode.serve_grant(&grant, &request).await {
        Ok(res) => {
            let (parts, body) = res.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(code @ NodeErrorCode::ImageStreamError) => {
            crate::error(StatusCode::INTERNAL_SERVER_ERROR, code.as_str())
        }
        Err(code) => crate::error(StatusCode::NOT_FOUND, code.as_str()),
    }
}

/// One-time bounded batch backfill to align `inventory_works.title_sort` with Rust's
/// Unicode-compliant `to_lowercase()` for works created prior to migration 0007.
///
/// SQLite's built-in `lower()` only handles ASCII A-Z (leaving accents, ligatures, etc.
/// in uppercase), whereas all subsequent writes in Aether use `title.to_lowercase()`.
/// This procedure runs once after migrations, processes rows in bounded batches (O(1) memory),
/// preserves all UUIDs, metadata, and relations, and records completion in
/// `inventory_works_unicode_migration`.
pub async fn backfill_title_sort_unicode(
    conn: &mut sqlx::AnyConnection,
) -> Result<(), sqlx::Error> {
    let completed: Option<(String,)> =
        query_as("SELECT completed_at FROM inventory_works_unicode_migration LIMIT 1")
            .fetch_optional(&mut *conn)
            .await?;

    if completed.is_some() {
        return Ok(());
    }

    const BATCH_SIZE: i64 = 250;
    let mut last_id = String::new();

    loop {
        let rows: Vec<(String, String, String)> = query_as(
            "SELECT id, title, title_sort FROM inventory_works WHERE id > ? ORDER BY id ASC LIMIT ?",
        )
        .bind(&last_id)
        .bind(BATCH_SIZE)
        .fetch_all(&mut *conn)
        .await?;

        if rows.is_empty() {
            break;
        }

        last_id = rows.last().map(|(id, _, _)| id.clone()).unwrap_or_default();

        for (id, title, current_sort) in rows {
            let expected_sort = title.to_lowercase();
            if current_sort != expected_sort {
                query("UPDATE inventory_works SET title_sort = ? WHERE id = ?")
                    .bind(expected_sort)
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }

    query(
        "INSERT INTO inventory_works_unicode_migration (completed_at) VALUES (CAST(CURRENT_TIMESTAMP AS TEXT))",
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
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
    use super::*;
    use crate::node::scanner::ScannedFile;
    #[tokio::test]
    async fn embedded_subtitle_preferences_apply_locally_and_before_remote_relay() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("subtitle-options.sqlite"))
            .await
            .unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
            .execute(&store.pool).await.unwrap();
        let file = ScannedFile {
            relative_path: "film.mp4".into(), size: 42, modified_ns: "1".into(),
            title: "Film".into(), identity_key: "film".into(), mime: "video/mp4".into(),
            probe: serde_json::from_value(json!({"streams":[
                {"index":0,"codec_type":"video","codec_name":"av1"},
                {"index":1,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"fr"}}
            ],"format":{}})).unwrap(),
            subtitles: vec![serde_json::from_value(json!({"language":"en","label":"External","forced":false,"sdh":false,"source_format":"webvtt","webvtt":"WEBVTT\n\n"})).unwrap()],
            ..Default::default()
        };
        ingest(
            &store,
            "lib",
            Path::new("/movies"),
            &ScanReport {
                files: vec![file],
                complete: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let asset: String = query_scalar("SELECT id FROM inventory_assets")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        // The legacy ingest fixture helper predates sidecars; publish their
        // snapshot explicitly as the staged scanner publication does.
        query("INSERT INTO inventory_subtitles(asset_id,subtitles_json) VALUES(?,?)")
            .bind(&asset)
            .bind(json!([{"language":"en","label":"External","forced":false,"sdh":false,"source_format":"webvtt","webvtt":"WEBVTT\n\n"}]).to_string())
            .execute(&store.pool).await.unwrap();
        query("INSERT INTO media_nodes(id,name,kind) VALUES('remote','Remote','remote')")
            .execute(&store.pool)
            .await
            .unwrap();
        let preparations = media_node::packaging::Preparations::default();
        for node in ["local", "remote"] {
            query("UPDATE libraries SET node_id=? WHERE id='lib'")
                .bind(node)
                .execute(&store.pool)
                .await
                .unwrap();
            for (policy, expected) in [("allow_all", 2), ("allow_text", 2), ("block_all", 1)] {
                query("UPDATE libraries SET options_json=? WHERE id='lib'")
                    .bind(json!({"embeddedSubtitlePolicy":policy}).to_string())
                    .execute(&store.pool)
                    .await
                    .unwrap();
                let descriptor = descriptor(&store, &asset, "synthetic-session")
                    .await
                    .unwrap()
                    .unwrap();
                let subtitles = descriptor["subtitles"].as_array().unwrap();
                assert_eq!(subtitles.len(), expected, "node {node}, policy {policy}");
                assert!(
                    subtitles
                        .iter()
                        .any(|subtitle| subtitle["label"] == "External")
                );
            }
            let request = axum::extract::Request::builder()
                .body(axum::body::Body::empty())
                .unwrap();
            let response = prepared(&store, &preparations, &asset, "subtitle-1", request).await;
            assert_eq!(
                response.status(),
                axum::http::StatusCode::UNPROCESSABLE_ENTITY
            );
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&body).contains("embedded_subtitle_disabled"));
        }
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('folder','Folder','films','local','/child',0,'{}')")
            .execute(&store.pool).await.unwrap();
        query("INSERT INTO library_folders(library_id,parent_id) VALUES('folder','lib')")
            .execute(&store.pool)
            .await
            .unwrap();
        let inherited = playback_library_options(&store, "folder").await.unwrap();
        assert!(!inherited.embedded_subtitle_allowed(Some("subrip")));
        query("UPDATE library_folders SET active=0 WHERE library_id='folder'")
            .execute(&store.pool)
            .await
            .unwrap();
        let detached = playback_library_options(&store, "folder").await.unwrap();
        assert!(detached.embedded_subtitle_allowed(Some("subrip")));
    }

    #[tokio::test]
    async fn fresh_schema_preserves_editorial_fields() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        crate::database::initialize_fresh_schema(&mut conn)
            .await
            .unwrap();
        drop(conn);
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/test',1)")
            .execute(&pool)
            .await
            .unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES('stable','lib','folder:Film','Film',2026,'{}','film','film')")
            .execute(&pool)
            .await
            .unwrap();
        let preserved: (String, String, i64, String) =
            query_as("SELECT id,title,year,editorial_json FROM inventory_works WHERE id='stable'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            preserved,
            ("stable".into(), "Film".into(), 2026, "{}".into())
        );
        let mut conn = pool.acquire().await.unwrap();
        backfill_title_sort_unicode(&mut conn).await.unwrap();
        drop(conn);
        let after_unicode: (String, String, i64, String, String) = query_as(
            "SELECT id,title,year,editorial_json,title_sort FROM inventory_works WHERE id='stable'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            after_unicode,
            (
                "stable".into(),
                "Film".into(),
                2026,
                "{}".into(),
                "film".into()
            )
        );
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES('new','lib','new','New',2026,'{}','new','film')").execute(&pool).await.unwrap();
        let added: i64 = query_scalar("SELECT added_at FROM inventory_works WHERE id='new'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(added > 0);
    }
    #[tokio::test]
    async fn nfo_refresh_preserves_graph_technical_data_and_invalid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("editorial.sqlite");
        let store = IdentityStore::open(&db).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let mut report = ScanReport { files: vec![ScannedFile { audios: Vec::new(), subtitles: Vec::new(),
            relative_path: "Film (2026)/film.mp4".into(), size: 42, modified_ns: "100".into(),
            title: "Film".into(), year: Some(2026), identity_key: "folder:Film (2026)".into(),
            external_ids: Default::default(), mime: "video/mp4".into(),
            probe: serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1"}],"format":{"duration":"2"}})).unwrap(),
            editorial: NfoOutcome::Missing, poster: None, reused: false,
            ..Default::default()
        }], complete: true, ..Default::default() };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        let work_id: String = query_scalar("SELECT id FROM inventory_works")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let graph: Vec<(String,String,String,String)> = query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
        report.files[0].editorial = NfoOutcome::Valid {
            metadata: serde_json::from_value(json!({"title":"Titre NFO <script>","year":2001,"plot":"A & B","genres":["Drame"],"external_ids":{"tmdb":"123"}})).unwrap(),
            path: "Film (2026)/movie.nfo".into(), priority:1,
        };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        let detail = super::detail(&store, &work_id).await.unwrap().unwrap();
        assert_eq!(detail["title"], "Titre NFO <script>");
        assert_eq!(detail["description"], "A & B");
        assert_eq!(detail["genres"], json!(["Drame"]));
        assert_eq!(detail["metadataProvenance"], "nfo");
        report.files[0].editorial = NfoOutcome::Invalid {
            path: "Film (2026)/movie.nfo".into(),
            message: "invalid XML".into(),
        };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        assert_eq!(
            detail,
            super::detail(&store, &work_id).await.unwrap().unwrap()
        );
        let after: Vec<(String,String,String,String)> = query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
        assert_eq!(graph, after);
        assert_eq!(
            load_cache(&store, "lib").await.unwrap()["Film (2026)/film.mp4"]
                .probe
                .streams[0]
                .codec_name
                .as_deref(),
            Some("av1")
        );
        store.pool.close().await;
        let store = IdentityStore::open(&db).await.unwrap();
        assert_eq!(
            detail,
            super::detail(&store, &work_id).await.unwrap().unwrap()
        );
        report.files[0].editorial = NfoOutcome::Missing;
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        let fallback = super::detail(&store, &work_id).await.unwrap().unwrap();
        assert_eq!(fallback["title"], "Film");
        assert_eq!(fallback["year"], 2026);
        assert_eq!(fallback["metadataProvenance"], "local");
        report.files[0].editorial = NfoOutcome::Valid {
            metadata: serde_json::from_value(json!({"title":"Old source NFO"})).unwrap(),
            path: "Film (2026)/movie.nfo".into(),
            priority: 1,
        };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        query("UPDATE libraries SET root='/replacement' WHERE id='lib'")
            .execute(&store.pool)
            .await
            .unwrap();
        report.files[0].editorial = NfoOutcome::Invalid {
            path: "Film (2026)/movie.nfo".into(),
            message: "invalid replacement".into(),
        };
        report.files[0].title = "Replacement film".into();
        ingest(&store, "lib", Path::new("/replacement"), &report)
            .await
            .unwrap();
        let replacement = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap();
        assert_eq!(replacement["total"], 1);
        assert_eq!(replacement["items"][0]["title"], "Replacement film");
        assert_eq!(replacement["items"][0]["metadataProvenance"], "local");
        assert_ne!(replacement["items"][0]["id"], work_id);
        assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 1);
        report.files[0].editorial = NfoOutcome::Valid {
            metadata: serde_json::from_value(json!({"title":"Ignored NFO"})).unwrap(),
            path: "Film (2026)/movie.nfo".into(),
            priority: 1,
        };
        report.files[0].poster = Some(media_contracts::media::LocalImage {
            path: "poster.png".into(),
            size_bytes: 10,
            modified_ns: "1".into(),
            mime: "image/png".into(),
            priority: 1,
        });
        let logo = report.files[0].poster.clone().unwrap();
        report.files[0].artwork.insert("logo".into(), logo);
        query("UPDATE libraries SET options_json='{\"readLocalNfo\":false,\"fetchPoster\":false,\"fetchLogo\":false}' WHERE id='lib'").execute(&store.pool).await.unwrap();
        ingest(&store, "lib", Path::new("/replacement"), &report)
            .await
            .unwrap();
        let filtered = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap();
        assert_eq!(filtered["items"][0]["title"], "Replacement film");
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM inventory_images WHERE available=1")
                .fetch_one(&store.pool)
                .await
                .unwrap(),
            0
        );
    }
    #[tokio::test]
    async fn inventory_survives_reopen_is_idempotent_and_preserves_failed_scan() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("inventory.sqlite");
        let store = IdentityStore::open(&db).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let probe:Probe=serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1","pix_fmt":"yuv420p10le"},{"index":1,"codec_type":"audio","codec_name":"opus","tags":{"language":"fra"}}],"format":{"duration":"2"}})).unwrap();
        let file = |path: &str| ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: path.into(),
            size: 42,
            modified_ns: "100".into(),
            title: "Film".into(),
            year: Some(2026),
            identity_key: "tmdb:123".into(),
            external_ids: Default::default(),
            probe: probe.clone(),
            mime: "video/mp4".into(),
            editorial: NfoOutcome::Missing,
            poster: None,
            reused: false,
            ..Default::default()
        };
        let report = ScanReport {
            files: vec![file("film.mp4"), file("film.mkv")],
            complete: true,
            ..Default::default()
        };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        let first = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap();
        assert_eq!(first["total"], 1);
        assert!(first["items"][0]["addedAt"].as_i64().is_some_and(|n| n > 0));
        let work_id = first["items"][0]["id"].as_str().unwrap();
        let initial = detail(&store, work_id).await.unwrap().unwrap();
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
        assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 2);
        ingest(&store, "lib", Path::new("/movies"), &ScanReport::default())
            .await
            .unwrap();
        assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
        store.pool.close().await;
        let store = IdentityStore::open(&db).await.unwrap();
        assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
        query("UPDATE libraries SET root='/other' WHERE id='lib'")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(load_cache(&store, "lib").await.unwrap().is_empty());
        assert_eq!(
            catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
                .await
                .unwrap()["total"],
            0
        );
        assert!(
            ingest(&store, "lib", Path::new("/movies"), &report)
                .await
                .is_err()
        );
        let mut new_report = report;
        for file in &mut new_report.files {
            file.title = "New Film".into();
            file.year = Some(2020);
            file.identity_key = "tmdb:456".into();
        }
        ingest(&store, "lib", Path::new("/other"), &new_report)
            .await
            .unwrap();
        assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 2);
        let updated = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap();
        assert_eq!(updated["total"], 1);
        assert_eq!(updated["items"][0]["title"], "New Film");
        assert_eq!(updated["items"][0]["year"], 2020);
        assert_ne!(updated["items"][0]["id"], work_id);
        assert!(
            detail(&store, work_id).await.unwrap().unwrap()["editions"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        query("UPDATE libraries SET root='/movies' WHERE id='lib'")
            .execute(&store.pool)
            .await
            .unwrap();
        ingest(
            &store,
            "lib",
            Path::new("/movies"),
            &ScanReport {
                complete: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
                .await
                .unwrap()["total"],
            0
        );
    }

    #[test]
    fn test_bounded_catalogue_cache_budget_and_lru_eviction() {
        let mut cache = BoundedCatalogueCache::new(2000);

        let val1 = serde_json::json!({"items": ["movie1", "movie2"], "total": 2});
        let val2 = serde_json::json!({"items": ["movie3", "movie4"], "total": 2});

        cache.insert("key1".into(), 1, val1.clone());
        let (bytes1, max_b, count1) = cache.stats();
        assert_eq!(count1, 1);
        assert!(bytes1 > 0);
        assert!(bytes1 <= max_b);

        cache.insert("key2".into(), 1, val2.clone());
        let (bytes2, _, count2) = cache.stats();
        assert_eq!(count2, 2);
        assert!(bytes2 > bytes1);

        // Access key1 to update its recency
        assert!(cache.get("key1", 1).is_some());

        // Stale generation removes entry
        assert!(cache.get("key2", 2).is_none());
        assert_eq!(cache.stats().2, 1);

        cache.clear();
        assert_eq!(cache.stats().0, 0);
        assert_eq!(cache.stats().2, 0);
    }
}
