//! Incremental scan caches, legacy ingestion and idempotent batch staging.

use super::editorial::refresh_editorial;
use super::{decode, id};
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use crate::media::Probe;
use crate::node::nfo::NfoOutcome;
use crate::node::scanner::{CachedFile, InventoryBatch, ScanReport};
use futures_util::TryStreamExt;
use serde_json::json;
use sqlx::Row;
use std::{collections::HashMap, path::Path};

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
                    content_fingerprint: None,
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
        "SELECT a.relative_path,a.size,a.modified_ns,f.fingerprint FROM inventory_assets a LEFT JOIN inventory_asset_fingerprints f ON f.asset_id=a.id AND f.size=a.size AND f.modified_ns=a.modified_ns WHERE a.library_id=? AND a.available=1 AND a.source_id IN (SELECT s.id FROM inventory_sources s JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked')",
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
                content_fingerprint: r.get::<Option<String>, _>("fingerprint"),
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
    query("DELETE FROM scan_staging_fingerprints WHERE scan_id=?")
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
        // A malformed or absent fingerprint only means "no rename hint" for this file.
        if let Some(fingerprint) = file
            .content_fingerprint
            .as_deref()
            .filter(|value| crate::node::fingerprint::is_well_formed(value))
        {
            query("INSERT INTO scan_staging_fingerprints(scan_id,relative_path,library_id,fingerprint) VALUES(?,?,?,?) ON CONFLICT(scan_id,relative_path) DO UPDATE SET fingerprint=excluded.fingerprint,library_id=excluded.library_id")
                .bind(scan_id)
                .bind(&file.relative_path)
                .bind(library_id)
                .bind(fingerprint)
                .execute(&mut *tx)
                .await?;
        }
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
