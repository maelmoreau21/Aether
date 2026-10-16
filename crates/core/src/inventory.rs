//! Persistent local inventory. Filesystem discovery remains in the Media Node.
use crate::{
    identity::IdentityStore,
    media::Probe,
    node::nfo::NfoOutcome,
    node::scanner::{CachedFile, ScanReport},
};
use serde_json::{Value, json};
use sqlx::Row;
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
    let rows = sqlx::query(
        "SELECT relative_path,size,modified_ns,probe_json FROM inventory_assets WHERE library_id=? AND available=1 AND source_id IN (SELECT s.id FROM inventory_sources s JOIN libraries l ON l.id=s.library_id WHERE s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local')",
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

pub async fn load_image_cache(
    store: &IdentityStore,
    library_id: &str,
) -> Result<HashMap<String, crate::node::images::LocalImage>, sqlx::Error> {
    let rows = sqlx::query("SELECT i.* FROM inventory_images i JOIN libraries l ON l.id=i.library_id WHERE i.library_id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND l.kind='films' AND l.node_id='local'")
        .bind(library_id).fetch_all(&store.pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let path: String = row.get("relative_path");
            (
                path.clone(),
                crate::node::images::LocalImage {
                    path,
                    size_bytes: row.get::<i64, _>("size") as u64,
                    modified_ns: row.get("modified_ns"),
                    mime: row.get("mime"),
                    priority: row.get::<i64, _>("priority") as u8,
                },
            )
        })
        .collect())
}

pub async fn ingest(
    store: &IdentityStore,
    library_id: &str,
    _root: &Path,
    report: &ScanReport,
) -> Result<(), sqlx::Error> {
    let mut tx = store.pool.begin().await?;
    let configured: Option<String> = sqlx::query_scalar(
        "SELECT root FROM libraries WHERE id=? AND enabled=1 AND kind='films' AND node_id='local'",
    )
    .bind(library_id)
    .fetch_optional(&mut *tx)
    .await?;
    if configured.as_deref() != _root.to_str() {
        return Err(sqlx::Error::Protocol(
            "library configuration changed during scan".into(),
        ));
    }
    // Missing files become unavailable only after complete discovery; failed traversal preserves inventory.
    if report.complete {
        sqlx::query("UPDATE inventory_assets SET available=0 WHERE library_id=?")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
    }
    for issue in &report.errors {
        sqlx::query(
            "UPDATE inventory_assets SET available=0 WHERE library_id=? AND relative_path=?",
        )
        .bind(library_id)
        .bind(&issue.relative_path)
        .execute(&mut *tx)
        .await?;
    }
    if report.complete {
        sqlx::query("UPDATE inventory_images SET available=0 WHERE library_id=?")
            .bind(library_id)
            .execute(&mut *tx)
            .await?;
    }
    for file in &report.files {
        let identity_key = format!("{}\n{}", _root.to_string_lossy(), file.identity_key);
        let existing =
            sqlx::query("SELECT a.id,w.identity_key,s.root_snapshot FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id WHERE a.library_id=? AND a.relative_path=?")
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
                sqlx::query("SELECT id FROM inventory_works WHERE library_id=? AND identity_key=?")
                    .bind(library_id)
                    .bind(&identity_key)
                    .fetch_optional(&mut *tx)
                    .await?;
            let work_id = if let Some(row) = work {
                row.get::<String, _>("id")
            } else {
                let work_id = id();
                sqlx::query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json) VALUES(?,?,?,?,?,?)")
                    .bind(&work_id)
                    .bind(library_id)
                    .bind(&identity_key)
                    .bind(&file.title)
                    .bind(file.year.map(i64::from))
                    .bind(json!(file.external_ids).to_string())
                    .execute(&mut *tx)
                    .await?;
                work_id
            };
            let edition_id = sqlx::query_scalar::<_, String>(
                "SELECT id FROM inventory_editions WHERE work_id=? LIMIT 1",
            )
            .bind(&work_id)
            .fetch_optional(&mut *tx)
            .await?;
            let edition_id = if let Some(edition_id) = edition_id {
                edition_id
            } else {
                let edition_id = id();
                sqlx::query("INSERT INTO inventory_editions VALUES(?,?)")
                    .bind(&edition_id)
                    .bind(&work_id)
                    .execute(&mut *tx)
                    .await?;
                edition_id
            };
            let version_id = id();
            let source_id = id();
            let asset_id = previous_asset_id.unwrap_or_else(id);
            // A reused physical-path UUID after a root/identity change must not
            // inherit the editorial snapshot of the previous physical source.
            sqlx::query("DELETE FROM inventory_editorial WHERE asset_id=?")
                .bind(&asset_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO inventory_versions VALUES(?,?)")
                .bind(&version_id)
                .bind(&edition_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,?)")
                .bind(&source_id)
                .bind(&version_id)
                .bind(library_id)
                .bind(_root.to_string_lossy().as_ref())
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO inventory_assets VALUES(?,?,?,?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET source_id=excluded.source_id")
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
        sqlx::query("UPDATE inventory_assets SET size=?,modified_ns=?,mime=?,probe_json=?,available=1 WHERE id=?").bind(file.size as i64).bind(&file.modified_ns).bind(&file.mime).bind(json!(file.probe).to_string()).bind(&asset_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE inventory_sources SET root_snapshot=? WHERE id=(SELECT source_id FROM inventory_assets WHERE id=?)").bind(_root.to_string_lossy().as_ref()).bind(&asset_id).execute(&mut *tx).await?;
        if let Some(image) = &file.poster {
            let work_id: String = sqlx::query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id WHERE a.id=?")
                .bind(&asset_id).fetch_one(&mut *tx).await?;
            sqlx::query("INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available) VALUES(?,?,?,?,?,?,?,?,?,1) ON CONFLICT(work_id,root_snapshot,relative_path) DO UPDATE SET size=excluded.size,modified_ns=excluded.modified_ns,mime=excluded.mime,priority=excluded.priority,available=1")
                .bind(id()).bind(work_id).bind(library_id).bind(_root.to_string_lossy().as_ref())
                .bind(&image.path).bind(image.size_bytes as i64).bind(&image.modified_ns)
                .bind(&image.mime).bind(i64::from(image.priority)).execute(&mut *tx).await?;
        }
        let fallback =
            json!({"title":file.title,"year":file.year,"external_ids":file.external_ids});
        sqlx::query("INSERT INTO inventory_editorial(asset_id,fallback_json) VALUES(?,?) ON CONFLICT(asset_id) DO UPDATE SET fallback_json=excluded.fallback_json")
            .bind(&asset_id).bind(fallback.to_string()).execute(&mut *tx).await?;
        match &file.editorial {
            NfoOutcome::Missing => {
                sqlx::query("UPDATE inventory_editorial SET nfo_json=NULL,nfo_path=NULL,nfo_priority=NULL WHERE asset_id=?")
                    .bind(&asset_id).execute(&mut *tx).await?;
            }
            NfoOutcome::Valid {
                metadata,
                path,
                priority,
            } => {
                sqlx::query("UPDATE inventory_editorial SET nfo_json=?,nfo_path=?,nfo_priority=? WHERE asset_id=?")
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
        let previous = sqlx::query("SELECT id,stream_index FROM inventory_tracks WHERE asset_id=?")
            .bind(&asset_id)
            .fetch_all(&mut *tx)
            .await?;
        for row in previous {
            if !indexes.contains(&row.get("stream_index")) {
                sqlx::query("DELETE FROM inventory_tracks WHERE id=?")
                    .bind(row.get::<String, _>("id"))
                    .execute(&mut *tx)
                    .await?;
            }
        }
        for track in &file.probe.streams {
            sqlx::query("INSERT INTO inventory_tracks VALUES(?,?,?,?) ON CONFLICT(asset_id,stream_index) DO UPDATE SET metadata_json=excluded.metadata_json").bind(id()).bind(&asset_id).bind(i64::from(track.index)).bind(json!(track).to_string()).execute(&mut *tx).await?;
        }
    }
    refresh_editorial(&mut tx, library_id).await?;
    tx.commit().await
}

async fn refresh_editorial(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    library_id: &str,
) -> Result<(), sqlx::Error> {
    let works: Vec<String> =
        sqlx::query_scalar("SELECT id FROM inventory_works WHERE library_id=?")
            .bind(library_id)
            .fetch_all(&mut **tx)
            .await?;
    for work_id in works {
        // Choose once per Work after the complete ingest, never by traversal order.
        let rows = sqlx::query("SELECT m.*,a.relative_path FROM inventory_editorial m JOIN inventory_assets a ON a.id=m.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN libraries l ON l.id=s.library_id WHERE e.work_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind='films' AND l.node_id='local' ORDER BY m.nfo_json IS NULL,m.nfo_priority,m.nfo_path,a.relative_path")
            .bind(&work_id).fetch_all(&mut **tx).await?;
        let Some(first) = rows.first() else { continue };
        let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));
        let fallback = parse(first.get("fallback_json"));
        let nfo: Option<String> = first.get("nfo_json");
        let mut metadata = nfo.as_deref().map(parse).unwrap_or_else(|| json!({}));
        let title = metadata["title"]
            .as_str()
            .or_else(|| fallback["title"].as_str())
            .unwrap_or("Film local")
            .to_owned();
        let year = metadata["year"]
            .as_i64()
            .or_else(|| fallback["year"].as_i64());
        let mut ids = std::collections::BTreeMap::<String, String>::new();
        let mut conflicts = std::collections::BTreeSet::new();
        for row in &rows {
            let mut candidates = vec![parse(row.get("fallback_json"))];
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
        metadata["external_ids"] = json!(ids);
        metadata["provenance"] = json!(if nfo.is_some() { "nfo" } else { "local" });
        metadata["warnings"] = json!(
            conflicts
                .into_iter()
                .map(|provider| format!("Conflicting {provider} identifiers; no automatic merge"))
                .collect::<Vec<_>>()
        );
        // Private sidecar paths stay in inventory_editorial, never in catalogue output.
        sqlx::query("UPDATE inventory_works SET title=?,year=?,external_ids_json=?,editorial_json=? WHERE id=?")
            .bind(title).bind(year).bind(json!(ids).to_string()).bind(metadata.to_string()).bind(&work_id).execute(&mut **tx).await?;
    }
    Ok(())
}

pub async fn resource_library(
    store: &IdentityStore,
    kind: &str,
    resource_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let query = match kind {
        "work" => "SELECT library_id FROM inventory_works WHERE id=?",
        "image" => {
            "SELECT i.library_id FROM inventory_images i JOIN libraries l ON l.id=i.library_id WHERE i.id=? AND i.root_snapshot=l.root AND l.kind='films' AND l.node_id='local'"
        }
        "asset" | "media" => {
            "SELECT a.library_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local'"
        }
        _ => return Ok(None),
    };
    sqlx::query_scalar(query)
        .bind(resource_id)
        .fetch_optional(&store.pool)
        .await
}

fn work(row: &sqlx::sqlite::SqliteRow) -> Value {
    let metadata: Value =
        serde_json::from_str(row.get("editorial_json")).unwrap_or_else(|_| json!({}));
    json!({"id":row.get::<String,_>("id"),"libraryId":row.get::<String,_>("library_id"),"kind":"film","title":row.get::<String,_>("title"),"year":row.get::<Option<i64>,_>("year"),"description":metadata["plot"].as_str().or_else(||metadata["outline"].as_str()).unwrap_or("Média local indexé"),"plot":metadata["plot"],"genres":metadata["genres"].as_array().cloned().unwrap_or_default(),"originalTitle":metadata["original_title"],"outline":metadata["outline"],"premiered":metadata["premiered"],"rating":metadata["rating"],"mpaa":metadata["mpaa"],"externalIds":metadata["external_ids"],"metadataProvenance":metadata["provenance"].as_str().unwrap_or("local"),"metadataWarnings":metadata["warnings"].as_array().cloned().unwrap_or_default(),"synthetic":false,"posterUrl":row.try_get::<Option<String>,_>("poster_url").ok().flatten()})
}

/// Administrator scan diagnostics. Only currently available sources contribute;
/// paths and opaque internal identifiers are never included in these messages.
pub async fn editorial_warnings(
    store: &IdentityStore,
    library_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query("SELECT w.editorial_json FROM inventory_works w WHERE w.library_id=? AND EXISTS(SELECT 1 FROM inventory_editions e JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=s.library_id WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind='films' AND l.node_id='local') ORDER BY w.id")
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

pub async fn catalogue(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
) -> Result<Value, sqlx::Error> {
    let rows=sqlx::query("SELECT w.*,(SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i JOIN libraries il ON il.id=i.library_id WHERE i.work_id=w.id AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind='films' AND il.node_id='local' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS poster_url FROM inventory_works w WHERE EXISTS(SELECT 1 FROM inventory_editions e JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=s.library_id WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1)").fetch_all(&store.pool).await?;
    let mut items: Vec<Value> = rows
        .iter()
        .filter(|r| {
            visible.contains(&r.get::<String, _>("library_id"))
                && kind.is_none_or(|k| k == "film")
                && r.get::<String, _>("title")
                    .to_lowercase()
                    .contains(&search.to_lowercase())
        })
        .map(work)
        .collect();
    items.sort_by_key(|v| v["title"].as_str().unwrap_or_default().to_lowercase());
    if sort == "title-desc" {
        items.reverse();
    }
    let total = items.len();
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    Ok(
        json!({"items":items,"total":total,"offset":offset,"limit":limit,"mode":"local-library","notice":"Bibliothèques locales indexées"}),
    )
}

pub async fn detail(store: &IdentityStore, work_id: &str) -> Result<Option<Value>, sqlx::Error> {
    let Some(row) = sqlx::query("SELECT w.*,(SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i JOIN libraries il ON il.id=i.library_id WHERE i.work_id=w.id AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind='films' AND il.node_id='local' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS poster_url FROM inventory_works w WHERE w.id=?")
        .bind(work_id)
        .fetch_optional(&store.pool)
        .await?
    else {
        return Ok(None);
    };
    let mut value = work(&row);
    let mut editions = vec![];
    for edition in sqlx::query("SELECT id FROM inventory_editions WHERE work_id=?")
        .bind(work_id)
        .fetch_all(&store.pool)
        .await?
    {
        let edition_id: String = edition.get("id");
        let mut versions = vec![];
        for version in sqlx::query("SELECT id FROM inventory_versions WHERE edition_id=?")
            .bind(&edition_id)
            .fetch_all(&store.pool)
            .await?
        {
            let version_id: String = version.get("id");
            let sources:Vec<_>=sqlx::query("SELECT s.id,a.id AS asset_id,a.relative_path FROM inventory_sources s JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=s.library_id WHERE s.version_id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1").bind(&version_id).fetch_all(&store.pool).await?.iter().map(|r|json!({"id":r.get::<String,_>("id"),"assetIds":[r.get::<String,_>("asset_id")],"playbackMediaId":r.get::<String,_>("asset_id")})).collect();
            if !sources.is_empty() {
                versions.push(json!({"id":version_id,"label":"Version locale","sources":sources}));
            }
        }
        if !versions.is_empty() {
            editions.push(json!({"id":edition_id,"label":"Édition locale","versions":versions}));
        }
    }
    value["editions"] = json!(editions);
    value["seasons"] = json!([]);
    Ok(Some(value))
}

pub async fn descriptor(
    store: &IdentityStore,
    asset_id: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let row=sqlx::query("SELECT a.mime,a.probe_json,w.title FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id JOIN libraries l ON l.id=s.library_id WHERE a.id=? AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1").bind(asset_id).fetch_optional(&store.pool).await?;
    let Some(row) = row else { return Ok(None) };
    let probe = decode(row.get("probe_json"))?;
    let language = probe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .and_then(|s| s.tags.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("und");
    let language = match language {
        "fra" | "fre" => "fr",
        "eng" => "en",
        other => other,
    };
    Ok(Some(
        json!({"id":asset_id,"title":row.get::<String,_>("title"),"synthetic":false,"sources":[{"id":asset_id,"audioTrackId":"internal-default","language":language,"url":format!("/api/v1/assets/{asset_id}/content"),"mime":row.get::<String,_>("mime"),"tracks":probe.streams}],"subtitles":[]}),
    ))
}

/// Authorization is enforced by the common route middleware before this helper.
/// Confinement rechecks links before ServeFile opens; this does not eliminate TOCTOU races.
pub async fn content(
    store: &IdentityStore,
    asset_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{
        http::{HeaderValue, StatusCode},
        response::IntoResponse,
    };
    use tower::ServiceExt;
    let row=sqlx::query("SELECT a.relative_path,a.mime,l.root FROM inventory_assets a JOIN libraries l ON l.id=a.library_id JOIN inventory_sources s ON s.id=a.source_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local'").bind(asset_id).fetch_optional(&store.pool).await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "unknown_asset"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
    };
    let root: String = row.get("root");
    let relative: String = row.get("relative_path");
    let path =
        match crate::node::scanner::confined_file(Path::new(&root), Path::new(&relative)).await {
            Ok(path) => path,
            Err(_) => return crate::error(StatusCode::NOT_FOUND, "unavailable_asset"),
        };
    match tower_http::services::ServeFile::new(path)
        .with_buf_chunk_size(65536)
        .oneshot(request)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            if let Ok(mime) = HeaderValue::from_str(row.get("mime")) {
                response
                    .headers_mut()
                    .insert(axum::http::header::CONTENT_TYPE, mime);
            }
            response
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "stream_error"),
    }
}

/// Session/library checks run before this handler, including HEAD and conditionals.
/// The final ServeFile open remains subject to the documented local-writer TOCTOU race.
pub async fn image(
    store: &IdentityStore,
    image_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{
        http::{HeaderValue, StatusCode, header},
        response::IntoResponse,
    };
    use tower::ServiceExt;
    let row = sqlx::query("SELECT i.*,l.root FROM inventory_images i JOIN libraries l ON l.id=i.library_id WHERE i.id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND l.kind='films' AND l.node_id='local'")
        .bind(image_id).fetch_optional(&store.pool).await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "unknown_image"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
    };
    let root: String = row.get("root");
    let relative: String = row.get("relative_path");
    let path =
        match crate::node::scanner::confined_file(Path::new(&root), Path::new(&relative)).await {
            Ok(path) => path,
            Err(_) => return crate::error(StatusCode::NOT_FOUND, "unavailable_image"),
        };
    let checked = match crate::node::images::validate(&path).await {
        Ok(image) => image,
        Err(_) => return crate::error(StatusCode::NOT_FOUND, "invalid_image"),
    };
    if checked.size_bytes != row.get::<i64, _>("size") as u64
        || checked.modified_ns != row.get::<String, _>("modified_ns")
        || checked.mime != row.get::<String, _>("mime")
    {
        return crate::error(StatusCode::NOT_FOUND, "image_changed_rescan_required");
    }
    match tower_http::services::ServeFile::new(path)
        .with_buf_chunk_size(65536)
        .oneshot(request)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(checked.mime));
            // Never let a browser cache bypass a later permission/revocation check.
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            response
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "image_stream_error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::scanner::ScannedFile;
    #[tokio::test]
    async fn editorial_migration_preserves_existing_0004_works() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE libraries(id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0004_inventory.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO libraries VALUES('lib')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO inventory_works VALUES('stable','lib','folder:Film','Film',2026,'{}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!("../migrations/0005_film_editorial.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let preserved: (String, String, i64, String) =
            sqlx::query_as("SELECT id,title,year,editorial_json FROM inventory_works")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            preserved,
            ("stable".into(), "Film".into(), 2026, "{}".into())
        );
        sqlx::raw_sql(include_str!("../migrations/0006_local_posters.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let after: (String, String, i64, String) =
            sqlx::query_as("SELECT id,title,year,editorial_json FROM inventory_works")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(preserved, after);
    }
    #[tokio::test]
    async fn nfo_refresh_preserves_graph_technical_data_and_invalid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("editorial.sqlite");
        let store = IdentityStore::open(&db).await.unwrap();
        sqlx::query("INSERT INTO libraries VALUES('lib','Films','films','local','/movies',1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let mut report = ScanReport { files: vec![ScannedFile {
            relative_path: "Film (2026)/film.mp4".into(), size: 42, modified_ns: "100".into(),
            title: "Film".into(), year: Some(2026), identity_key: "folder:Film (2026)".into(),
            external_ids: Default::default(), mime: "video/mp4".into(),
            probe: serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1"}],"format":{"duration":"2"}})).unwrap(),
            editorial: NfoOutcome::Missing, poster: None,
        }], complete: true, ..Default::default() };
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .unwrap();
        let work_id: String = sqlx::query_scalar("SELECT id FROM inventory_works")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let graph: Vec<(String,String,String,String)> = sqlx::query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
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
        let after: Vec<(String,String,String,String)> = sqlx::query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
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
        sqlx::query("UPDATE libraries SET root='/replacement' WHERE id='lib'")
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
    }
    #[tokio::test]
    async fn inventory_survives_reopen_is_idempotent_and_preserves_failed_scan() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("inventory.sqlite");
        let store = IdentityStore::open(&db).await.unwrap();
        sqlx::query("INSERT INTO libraries VALUES('lib','Films','films','local','/movies',1)")
            .execute(&store.pool)
            .await
            .unwrap();
        let probe:Probe=serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1","pix_fmt":"yuv420p10le"},{"index":1,"codec_type":"audio","codec_name":"opus","tags":{"language":"fra"}}],"format":{"duration":"2"}})).unwrap();
        let file = |path: &str| ScannedFile {
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
        sqlx::query("UPDATE libraries SET root='/other' WHERE id='lib'")
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
        sqlx::query("UPDATE libraries SET root='/movies' WHERE id='lib'")
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
}
