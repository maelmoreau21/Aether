//! Transactional outbox feeding the existing leased task queue. No independent scheduler.
use crate::{
    database::{begin_write_serialized, query, query_as, query_scalar},
    identity::IdentityStore,
    task_engine::Job,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};

type MusicSnapshotRow = (String, i64, String, i64, String, String, String, String);
type MusicMetadataRow = (
    String,
    String,
    String,
    String,
    String,
    Option<f64>,
    String,
    Option<String>,
);
type MusicCurrentRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
);

pub struct EventInput<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub library_id: &'a str,
    pub asset_id: &'a str,
    pub fingerprint: &'a str,
    pub payload: &'a Value,
}
pub(crate) fn hash(value: &impl Serialize) -> Result<String, sqlx::Error> {
    let bytes = serde_json::to_vec(value).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
/// Call inside the transaction that changes the inventory, never after commit.
pub async fn emit(
    tx: &mut Transaction<'_, Any>,
    event: &EventInput<'_>,
    now: i64,
) -> Result<String, sqlx::Error> {
    let payload =
        serde_json::to_string(event.payload).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    if event.key.is_empty()
        || event.key.len() > 256
        || payload.len() > 65536
        || event.asset_id.len() > 128
        || !matches!(
            event.kind,
            "media.added"
                | "media.modified"
                | "media.removed"
                | "analysis.completed"
                | "metadata.missing"
                | "extension.activated"
                | "manual"
                | "scheduled"
        )
    {
        return Err(sqlx::Error::Protocol("invalid automation event".into()));
    }
    let id = hash(&(event.key, event.library_id))?;
    let existing: Option<(String, String, String, String, String,String)> = query_as(
        "SELECT id,kind,library_id,asset_id,fingerprint,payload_json FROM automation_events WHERE event_key=?",
    )
    .bind(event.key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id, kind, library, asset, fingerprint, old_payload)) = existing {
        if kind != event.kind
            || library != event.library_id
            || asset != event.asset_id
            || fingerprint != event.fingerprint
            || serde_json::from_str::<Value>(&old_payload).ok().as_ref() != Some(event.payload)
        {
            return Err(sqlx::Error::Protocol(
                "automation event key conflict".into(),
            ));
        }
        return Ok(id);
    }
    query("INSERT INTO automation_events(id,event_key,kind,library_id,asset_id,fingerprint,payload_json,created_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(event_key) DO NOTHING")
 .bind(&id).bind(event.key).bind(event.kind).bind(event.library_id).bind(event.asset_id).bind(event.fingerprint).bind(payload).bind(now).execute(&mut **tx).await?;
    Ok(id)
}
/// Cache provider responses for external APIs with TTL.
pub async fn get_cached_provider_response(
    tx: &mut Transaction<'_, Any>,
    provider: &str,
    operation: &str,
    query_val: &Value,
    now: i64,
) -> Result<Option<Value>, sqlx::Error> {
    let query_hash = hash(query_val)?;
    let cache_key = format!("{}:{}:{}", provider, operation, query_hash);
    let row: Option<(String, i64)> = query_as(
        "SELECT response_json, expires_at FROM automation_provider_cache WHERE cache_key=?",
    )
    .bind(&cache_key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((response_json, expires_at)) = row {
        if expires_at > now {
            if let Ok(val) = serde_json::from_str::<Value>(&response_json) {
                return Ok(Some(val));
            }
        } else {
            query("DELETE FROM automation_provider_cache WHERE cache_key=?")
                .bind(&cache_key)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(None)
}

pub async fn put_cached_provider_response(
    tx: &mut Transaction<'_, Any>,
    provider: &str,
    operation: &str,
    query_val: &Value,
    response: &Value,
    ttl_seconds: i64,
    now: i64,
) -> Result<(), sqlx::Error> {
    let query_hash = hash(query_val)?;
    let cache_key = format!("{}:{}:{}", provider, operation, query_hash);
    let expires_at = now.saturating_add(ttl_seconds.max(60));
    let response_json =
        serde_json::to_string(response).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    query("INSERT INTO automation_provider_cache(cache_key,provider,operation,query_hash,response_json,expires_at,created_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,expires_at=excluded.expires_at,created_at=excluded.created_at")
        .bind(cache_key)
        .bind(provider)
        .bind(operation)
        .bind(query_hash)
        .bind(response_json)
        .bind(expires_at)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn purge_expired_cache(
    tx: &mut Transaction<'_, Any>,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let affected = query("DELETE FROM automation_provider_cache WHERE expires_at<=?")
        .bind(now)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(affected)
}

/// Music snapshot emission: scans music_tracks for changes and queues media events.
pub async fn emit_music_snapshot(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let mut count = 0;
    let mut cursor = String::new();
    loop {
        let rows: Vec<MusicSnapshotRow> = query_as(
            "SELECT id,size,modified_ns,available,root_snapshot,title,artist,album FROM music_tracks WHERE library_id=? AND id>? ORDER BY id LIMIT 256",
        ).bind(library).bind(&cursor).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            break;
        }
        for (track_id, size, modified, available, root, title, artist, album) in rows {
            cursor = track_id.clone();
            let fingerprint = hash(&(
                &track_id, size, &modified, available, &root, &title, &artist, &album,
            ))?;
            let old: Option<(String, i64)> = query_as(
                "SELECT fingerprint,revision FROM automation_observations WHERE asset_id=?",
            )
            .bind(&track_id)
            .fetch_optional(&mut **tx)
            .await?;
            if old
                .as_ref()
                .is_some_and(|(previous, _)| previous == &fingerprint)
            {
                continue;
            }
            let revision = old.as_ref().map_or(1, |(_, r)| r.saturating_add(1));
            let kind = if available == 0 {
                "media.removed"
            } else if old.is_none() {
                "media.added"
            } else {
                "media.modified"
            };
            let key = hash(&(&track_id, revision))?;
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind,
                    library_id: library,
                    asset_id: &track_id,
                    fingerprint: &fingerprint,
                    payload: &serde_json::json!({"revision": revision, "mediaType": "music"}),
                },
                now,
            )
            .await?;
            query("INSERT INTO automation_observations(asset_id,library_id,fingerprint,revision) VALUES(?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,revision=excluded.revision")
                .bind(&track_id).bind(library).bind(&fingerprint).bind(revision).execute(&mut **tx).await?;
            count += 1;
        }
    }
    Ok(count)
}

pub async fn emit_snapshot(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    _scan_id: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let mut count = 0;
    let mut cursor = String::new();
    loop {
        let rows: Vec<(String, i64, String, i64,String,String)> = query_as(
        "SELECT a.id,a.size,a.modified_ns,a.available,s.root_snapshot,a.probe_json FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id WHERE a.library_id=? AND a.id>? ORDER BY a.id LIMIT 256",
    ).bind(library).bind(&cursor).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            break;
        }
        for (asset, size, modified, available, root, probe) in rows {
            cursor = asset.clone();
            let probe_digest = format!("{:x}", Sha256::digest(probe.as_bytes()));
            let fingerprint = hash(&(&asset, size, &modified, available, &root, probe_digest))?;
            let old: Option<(String, i64)> = query_as(
                "SELECT fingerprint,revision FROM automation_observations WHERE asset_id=?",
            )
            .bind(&asset)
            .fetch_optional(&mut **tx)
            .await?;
            if old
                .as_ref()
                .is_some_and(|(previous, _)| previous == &fingerprint)
            {
                continue;
            }
            let revision = old.as_ref().map_or(1, |(_, r)| r.saturating_add(1));
            let kind = if available == 0 {
                "media.removed"
            } else if old.is_none() {
                "media.added"
            } else {
                "media.modified"
            };
            let key = hash(&(&asset, revision))?;
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind,
                    library_id: library,
                    asset_id: &asset,
                    fingerprint: &fingerprint,
                    payload: &serde_json::json!({"revision":revision}),
                },
                now,
            )
            .await?;
            query("INSERT INTO automation_observations(asset_id,library_id,fingerprint,revision) VALUES(?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,revision=excluded.revision")
 .bind(&asset).bind(library).bind(fingerprint).bind(revision).execute(&mut **tx).await?;
            count += 1;
        }
    }
    Ok(count)
}
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaskLink {
    pub task_id: String,
    pub event_id: String,
    pub extension_id: String,
    pub operation: String,
    pub fingerprint: String,
    pub generation: String,
    pub provider: String,
}
pub async fn task_link(store: &IdentityStore, id: &str) -> Result<Option<TaskLink>, sqlx::Error> {
    query_as("SELECT * FROM automation_task_links WHERE task_id=?")
        .bind(id)
        .fetch_optional(store.pool())
        .await
}

#[derive(sqlx::FromRow)]
struct Candidate {
    id: String,
    package_json: String,
    generation: String,
    mode: String,
    priority: i64,
    operation: String,
    config_revision: String,
    algorithm_version: String,
    settings_json: String,
}
/// Bounded admissions; invoked by the task worker's existing tick. Cursor and jobs
/// commit together, so a crash cannot acknowledge an event without its tasks.
pub async fn drain(store: &IdentityStore, now: i64, limit: i64) -> Result<u64, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    let owner:Option<String>=query_scalar("SELECT id FROM users WHERE role='administrator' AND disabled_at IS NULL ORDER BY id LIMIT 1").fetch_optional(&mut *tx).await?;
    let Some(owner) = owner else { return Ok(0) };
    pump_triggers(&mut tx, now, limit.clamp(1, 100)).await?;
    query("UPDATE durable_tasks SET status='error',error='Prerequisite failed or cancelled',updated_at=?,finished_at=? WHERE status='queued' AND lease_until IS NULL AND EXISTS(SELECT 1 FROM automation_dependencies d JOIN durable_tasks p ON p.id=d.prerequisite_id WHERE d.task_id=durable_tasks.id AND p.status IN ('error','cancelled'))").bind(now).bind(now).execute(&mut *tx).await?;
    let mut pending: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','paused')",
    )
    .fetch_one(&mut *tx)
    .await?;
    let events:Vec<(String,String,String,String,String,String,String)>=query_as("SELECT id,kind,library_id,asset_id,fingerprint,cursor_extension,payload_json FROM automation_events WHERE dispatched_at IS NULL ORDER BY created_at,id LIMIT ?").bind(limit.clamp(1,100)).fetch_all(&mut *tx).await?;
    let mut admitted = 0;
    let budget = limit.clamp(1, 100);
    let mut examined = 0;
    for (event, kind, library, asset, file_fingerprint, cursor, payload) in events {
        let event_payload = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null);
        let selected = event_payload["extensionId"].as_str().map(str::to_owned);
        let fingerprint_ready =
            event_payload["fingerprintTaskId"].as_str().is_some() && event_payload["stale"] != true;
        if kind == "media.removed" {
            query("UPDATE durable_tasks SET status='cancelled',updated_at=?,finished_at=CASE WHEN lease_until IS NULL THEN ? ELSE NULL END WHERE target_id=? AND status IN ('queued','running','paused') AND id IN (SELECT task_id FROM automation_task_links)").bind(now).bind(now).bind(&asset).execute(&mut *tx).await?;
            query("UPDATE automation_events SET dispatched_at=? WHERE id=?")
                .bind(now)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        if kind == "media.modified" {
            query("UPDATE durable_tasks SET status='cancelled',updated_at=?,finished_at=CASE WHEN lease_until IS NULL THEN ? ELSE NULL END WHERE target_id=? AND status IN ('queued','running','paused') AND id IN (SELECT l.task_id FROM automation_task_links l JOIN automation_events original ON original.id=l.event_id WHERE original.fingerprint!=?)")
            .bind(now).bind(now).bind(&asset).bind(&file_fingerprint).execute(&mut *tx).await?;
        }
        let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
            .bind(&library)
            .fetch_one(&mut *tx)
            .await?;
        let options: Value = serde_json::from_str(&options)
            .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
        let candidates:Vec<Candidate>=query_as("SELECT e.id,e.package_json,e.generation,COALESCE(p.mode,'automatic') AS mode,COALESCE(p.priority,100) AS priority,COALESCE(p.operation,'') AS operation,COALESCE(p.config_revision,'') AS config_revision,COALESCE(p.algorithm_version,'1') AS algorithm_version,COALESCE(s.settings_json,'{}') AS settings_json FROM extensions e JOIN extension_publishers publisher ON publisher.id=e.publisher_id LEFT JOIN automation_policies p ON p.extension_id=e.id AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) LEFT JOIN extension_settings s ON s.extension_id=e.id WHERE e.enabled=1 AND publisher.active=1 AND e.id>? AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND (g.library_id=? OR EXISTS(SELECT 1 FROM library_folders granted WHERE granted.library_id=? AND granted.parent_id=g.library_id AND granted.active=1)))) ORDER BY e.id LIMIT ?").bind(&library).bind(&library).bind(&cursor).bind(&library).bind(&library).bind(budget-examined+1).fetch_all(&mut *tx).await?;
        let more = candidates.len() as i64 > budget - examined;
        for mut c in candidates.into_iter().take((budget - examined) as usize) {
            examined += 1;
            query("UPDATE automation_events SET cursor_extension=? WHERE id=?")
                .bind(&c.id)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
            if selected.as_ref().is_some_and(|id| id != &c.id) {
                continue;
            }
            if c.mode == "disabled"
                || (c.mode == "manual" && !matches!(kind.as_str(), "manual" | "metadata.missing"))
                || (c.mode == "scheduled" && kind != "scheduled")
            {
                continue;
            }
            let package: media_extension_sdk::Package = match serde_json::from_str(&c.package_json)
            {
                Ok(p) => p,
                Err(_) => continue,
            };
            let marker_plugin = package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "playback.markers");
            if kind == "analysis.completed" && marker_plugin && !fingerprint_ready {
                continue;
            }
            if fingerprint_ready && !marker_plugin {
                continue;
            }
            if !package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "metadata.read" || p == "playback.markers")
            {
                continue;
            }
            if marker_plugin {
                let episode: i64 = query_scalar("SELECT COUNT(*) FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=?")
                    .bind(&asset).fetch_one(&mut *tx).await?;
                if episode == 0 {
                    continue;
                }
                if fingerprint_ready {
                    let peers:i64=query_scalar("SELECT COUNT(DISTINCT peer.work_id) FROM inventory_assets current_asset JOIN inventory_sources current_source ON current_source.id=current_asset.source_id JOIN inventory_versions current_version ON current_version.id=current_source.version_id JOIN inventory_editions current_edition ON current_edition.id=current_version.edition_id JOIN inventory_episodes current_ep ON current_ep.work_id=current_edition.work_id JOIN inventory_episodes peer ON peer.season_id=current_ep.season_id AND peer.work_id!=current_ep.work_id JOIN inventory_editions peer_edition ON peer_edition.work_id=peer.work_id JOIN inventory_versions peer_version ON peer_version.edition_id=peer_edition.id JOIN inventory_sources peer_source ON peer_source.version_id=peer_version.id JOIN inventory_assets peer_asset ON peer_asset.source_id=peer_source.id JOIN durable_tasks fp ON fp.target_id=peer_asset.id AND fp.category='analyze' AND fp.component='fingerprint' AND fp.status='completed' WHERE current_asset.id=? AND peer_asset.available=1")
                        .bind(&asset).fetch_one(&mut *tx).await?;
                    if peers == 0 {
                        continue;
                    }
                }
            }
            if marker_plugin
                && matches!(
                    c.operation.as_str(),
                    "" | "intro.parse_chapters" | "intro.detect"
                )
            {
                c.operation = if fingerprint_ready {
                    "intro.detect"
                } else {
                    "intro.parse_chapters"
                }
                .into();
            } else if c.operation.is_empty() {
                c.operation = if marker_plugin {
                    if fingerprint_ready {
                        "intro.detect"
                    } else {
                        "intro.parse_chapters"
                    }
                } else if package.manifest.category.as_deref() == Some("Lyrics")
                    || package.manifest.permissions.iter().any(|p| p == "lyrics")
                {
                    "lyrics.get"
                } else if package.manifest.http_hosts.is_empty() {
                    "metadata.propose"
                } else {
                    "metadata.search"
                }
                .into();
            }
            if marker_plugin
                && c.operation
                    != if fingerprint_ready {
                        "intro.detect"
                    } else {
                        "intro.parse_chapters"
                    }
            {
                continue;
            }
            let settings: Value = serde_json::from_str(&c.settings_json)
                .map_err(|_| sqlx::Error::Protocol("invalid extension settings".into()))?;
            let fingerprint = hash(&(
                &asset,
                &file_fingerprint,
                &c.id,
                &c.generation,
                &package.sha256,
                &c.config_revision,
                &c.algorithm_version,
                &settings,
                &c.operation,
                &options,
            ))?;
            let exists: i64 = query_scalar(
                "SELECT COUNT(*) FROM automation_task_links WHERE extension_id=? AND fingerprint=?",
            )
            .bind(&c.id)
            .bind(&fingerprint)
            .fetch_one(&mut *tx)
            .await?;
            if exists > 0 {
                if matches!(kind.as_str(), "manual" | "metadata.missing") && pending < 100000 {
                    let restarted=query("UPDATE durable_tasks SET status='queued',failure_count=0,available_at=?,updated_at=?,finished_at=NULL,error='' WHERE status IN ('error','cancelled') AND lease_until IS NULL AND id IN (SELECT task_id FROM automation_task_links WHERE extension_id=? AND fingerprint=?)")
                    .bind(now).bind(now).bind(&c.id).bind(&fingerprint).execute(&mut *tx).await?.rows_affected();
                    pending += restarted as i64;
                    admitted += restarted;
                }
                continue;
            }
            if pending >= 100000 {
                return Ok(0);
            }
            let id = uuid::Uuid::new_v4().to_string();
            let lib_kind: String = query_scalar("SELECT kind FROM libraries WHERE id=?")
                .bind(&library)
                .fetch_one(&mut *tx)
                .await?;
            let node: Option<String> = if lib_kind == "music" {
                query_scalar("SELECT l.node_id FROM libraries l JOIN media_nodes n ON n.id=l.node_id JOIN music_tracks t ON t.library_id=l.id WHERE l.id=? AND t.id=? AND t.available=1 AND n.status!='revoked' AND (l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))")
                    .bind(&library).bind(&asset).fetch_optional(&mut *tx).await?
            } else {
                query_scalar("SELECT l.node_id FROM libraries l JOIN media_nodes n ON n.id=l.node_id JOIN inventory_assets a ON a.library_id=l.id WHERE l.id=? AND a.id=? AND a.available=1 AND n.status!='revoked' AND (l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))")
                    .bind(&library).bind(&asset).fetch_optional(&mut *tx).await?
            };
            let Some(node) = node else { continue };
            query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,priority,created_at,updated_at,available_at) VALUES(?,?,?,?,?,'analyze',?,?,?,?,?)")
 .bind(&id).bind(&library).bind(&owner).bind(format!("auto:{fingerprint}")).bind(node).bind(&asset).bind(c.priority).bind(now).bind(now).bind(now).execute(&mut *tx).await?;
            let provider = package
                .manifest
                .http_hosts
                .first()
                .cloned()
                .unwrap_or_else(|| c.id.clone());
            query("INSERT INTO automation_task_links(task_id,event_id,extension_id,operation,fingerprint,generation,provider) VALUES(?,?,?,?,?,?,?)").bind(id).bind(&event).bind(c.id).bind(c.operation).bind(fingerprint).bind(c.generation).bind(provider).execute(&mut *tx).await?;
            admitted += 1;
            pending += 1;
        }
        if !more {
            query("UPDATE automation_events SET dispatched_at=? WHERE id=?")
                .bind(now)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
        }
        if examined >= budget {
            break;
        }
    }
    crate::database::commit(tx).await?;
    if admitted > 0 {
        crate::task_engine::WAKE.notify_one();
    }
    Ok(admitted)
}
/// Store validated output as a separate automatic proposal. Manual editorial
/// fields, rights and language choices are never overwritten here.
pub async fn persist_result(
    tx: &mut Transaction<'_, Any>,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let encoded =
        serde_json::to_string(result).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    if encoded.len() > 65536 {
        return Err(sqlx::Error::Protocol("automation result too large".into()));
    }
    let is_music_target: i64 = query_scalar("SELECT COUNT(*) FROM music_tracks WHERE id=?")
        .bind(&job.target_id)
        .fetch_one(&mut **tx)
        .await?;
    let valid: i64 = if is_music_target > 0 {
        query_scalar("SELECT COUNT(*) FROM durable_tasks t JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN music_tracks a ON a.id=t.target_id WHERE t.id=? AND t.attempt=? AND t.node_id=? AND t.status='running' AND t.lease_until>? AND e.enabled=1 AND e.generation=? AND p.active=1 AND a.available=1 AND EXISTS(SELECT 1 FROM libraries lib JOIN media_nodes node ON node.id=lib.node_id WHERE lib.id=t.library_id AND node.id=t.node_id AND node.status!='revoked' AND lib.enabled=1) AND EXISTS(SELECT 1 FROM users u WHERE u.id=t.owner AND u.role='administrator' AND u.disabled_at IS NULL) AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=t.library_id AND policy.mode='disabled') AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=t.library_id))")
            .bind(&link.extension_id).bind(&job.id).bind(job.attempt).bind(&job.node_id).bind(now).bind(&link.generation).fetch_one(&mut **tx).await?
    } else {
        query_scalar("SELECT COUNT(*) FROM durable_tasks t JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN inventory_assets a ON a.id=t.target_id WHERE t.id=? AND t.attempt=? AND t.node_id=? AND t.status='running' AND t.lease_until>? AND e.enabled=1 AND e.generation=? AND p.active=1 AND a.available=1 AND EXISTS(SELECT 1 FROM libraries lib JOIN media_nodes node ON node.id=lib.node_id WHERE lib.id=t.library_id AND node.id=t.node_id AND node.status!='revoked' AND ((lib.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=lib.id)) OR EXISTS(SELECT 1 FROM library_folders own JOIN libraries parent ON parent.id=own.parent_id WHERE own.library_id=lib.id AND own.active=1 AND parent.enabled=1))) AND EXISTS(SELECT 1 FROM users u WHERE u.id=t.owner AND u.role='administrator' AND u.disabled_at IS NULL) AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=t.library_id AND policy.mode='disabled') AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND (g.library_id=t.library_id OR EXISTS(SELECT 1 FROM library_folders granted WHERE granted.library_id=t.library_id AND granted.parent_id=g.library_id AND granted.active=1))))")
            .bind(&link.extension_id).bind(&job.id).bind(job.attempt).bind(&job.node_id).bind(now).bind(&link.generation).fetch_one(&mut **tx).await?
    };
    if valid != 1 {
        return Ok(false);
    }
    if !current_fingerprint_matches(tx, &job.library_id, &job.target_id, link).await? {
        return Ok(false);
    }
    query("INSERT INTO automation_results(extension_id,asset_id,fingerprint,task_id,result_json,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(extension_id,asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,task_id=excluded.task_id,result_json=excluded.result_json,updated_at=excluded.updated_at")
 .bind(&link.extension_id).bind(&job.target_id).bind(&link.fingerprint).bind(&job.id).bind(encoded).bind(now).execute(&mut **tx).await?;
    if matches!(
        link.operation.as_str(),
        "intro.parse_chapters" | "intro.detect"
    ) {
        persist_timeline_markers(tx, job, link, result, now).await?;
    }
    if !matches!(
        link.operation.as_str(),
        "intro.parse_chapters" | "intro.detect"
    ) && can_write_catalogue(tx, &link.extension_id).await?
    {
        let is_music: i64 = query_scalar("SELECT COUNT(*) FROM music_tracks WHERE id=?")
            .bind(&job.target_id)
            .fetch_one(&mut **tx)
            .await?;
        if is_music > 0 {
            apply_missing_music_metadata(
                tx,
                &job.target_id,
                &link.extension_id,
                &link.fingerprint,
                result,
                now,
            )
            .await?;
        } else {
            apply_missing_metadata(
                tx,
                &job.target_id,
                &link.extension_id,
                &link.fingerprint,
                result,
                now,
            )
            .await?;
        }
    }
    Ok(true)
}

async fn persist_timeline_markers(
    tx: &mut Transaction<'_, Any>,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let season:Option<String>=query_scalar("SELECT ep.season_id FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=? AND a.available=1")
        .bind(&job.target_id).fetch_optional(&mut **tx).await?;
    let Some(season) = season else {
        return Err(sqlx::Error::Protocol("marker episode missing".into()));
    };
    let results: Vec<(String, &Vec<Value>)> = if link.operation == "intro.parse_chapters" {
        vec![(
            job.target_id.clone(),
            result
                .as_array()
                .ok_or_else(|| sqlx::Error::Protocol("invalid marker result".into()))?,
        )]
    } else {
        result
            .as_array()
            .ok_or_else(|| sqlx::Error::Protocol("invalid marker result".into()))?
            .iter()
            .map(|episode| {
                let id = episode["episodeId"]
                    .as_str()
                    .ok_or_else(|| sqlx::Error::Protocol("missing episode id".into()))?;
                let markers = episode["markers"]
                    .as_array()
                    .ok_or_else(|| sqlx::Error::Protocol("missing markers".into()))?;
                Ok((id.to_owned(), markers))
            })
            .collect::<Result<_, sqlx::Error>>()?
    };
    for (asset, markers) in results {
        let state:Option<(String,String,String)>=query_as("SELECT ep.season_id,obs.fingerprint,a.probe_json FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN automation_observations obs ON obs.asset_id=a.id WHERE a.id=? AND a.available=1")
            .bind(&asset).fetch_optional(&mut **tx).await?;
        let Some((candidate_season, source_fingerprint, probe)) = state else {
            return Err(sqlx::Error::Protocol("marker source missing".into()));
        };
        if candidate_season != season {
            return Err(sqlx::Error::Protocol("marker season mismatch".into()));
        }
        let duration = serde_json::from_str::<Value>(&probe)
            .ok()
            .and_then(|v| {
                v["format"]["duration"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| sqlx::Error::Protocol("marker duration missing".into()))?;
        for marker in markers {
            let Some(kind) = marker["markerType"]
                .as_str()
                .filter(|kind| matches!(*kind, "intro" | "credits"))
            else {
                continue;
            };
            let (Some(start), Some(end), Some(confidence)) = (
                marker["start"].as_f64(),
                marker["end"].as_f64(),
                marker["confidence"].as_f64(),
            ) else {
                return Err(sqlx::Error::Protocol("invalid marker".into()));
            };
            if !start.is_finite()
                || !end.is_finite()
                || !confidence.is_finite()
                || start < 0.0
                || end <= start
                || end > duration
                || end - start > 600.0
                || !(0.0..=1.0).contains(&confidence)
            {
                return Err(sqlx::Error::Protocol("marker outside timeline".into()));
            }
            query("INSERT INTO automatic_timeline_markers(asset_id,extension_id,marker_type,start_seconds,end_seconds,confidence,source_fingerprint,generation,task_id,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(asset_id,extension_id,marker_type) DO UPDATE SET start_seconds=excluded.start_seconds,end_seconds=excluded.end_seconds,confidence=excluded.confidence,source_fingerprint=excluded.source_fingerprint,generation=excluded.generation,task_id=excluded.task_id,updated_at=excluded.updated_at")
                .bind(&asset).bind(&link.extension_id).bind(kind).bind(start).bind(end).bind(confidence).bind(&source_fingerprint).bind(&link.generation).bind(&job.id).bind(now).execute(&mut **tx).await?;
        }
    }
    Ok(())
}

/// Merge only absent automatic fields. Manual corrections remain a separate
/// higher-precedence document and are never modified by this path.
pub async fn apply_missing_metadata(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    extension: &str,
    fingerprint: &str,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let rows:Vec<(String,String,String,String)>=query_as("SELECT w.id,w.editorial_json,COALESCE(w.manual_json,'{}'),a.library_id FROM inventory_works w JOIN inventory_editions e ON e.work_id=w.id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=?").bind(asset).fetch_all(&mut **tx).await?;
    for (work, raw, manual_raw, library) in rows {
        let mut metadata: Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
        let Some(fields) = metadata.as_object_mut() else {
            continue;
        };
        let manual: Value = serde_json::from_str(&manual_raw).unwrap_or(Value::Null);
        let priority:i64=query_scalar("SELECT COALESCE((SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=?),(SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)),100)")
            .bind(extension).bind(&library).bind(extension).bind(&library).fetch_one(&mut **tx).await?;
        let provider_id = result["providerId"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(256)
            .collect::<String>();
        let proposal = result
            .get("metadata")
            .filter(|v| v.is_object())
            .unwrap_or(result);
        let mut changed = false;
        for (destination, sources) in [
            ("plot", &["plot", "summary", "overview"][..]),
            ("outline", &["outline", "tagline"][..]),
            ("genres", &["genres"][..]),
        ] {
            if manual
                .as_object()
                .is_some_and(|m| m.contains_key(destination))
            {
                continue;
            }
            let value = sources
                .iter()
                .filter_map(|key| proposal.get(*key))
                .find(|v| match destination {
                    "genres" => v.as_array().is_some_and(|items| {
                        !items.is_empty()
                            && items.len() <= 50
                            && items.iter().all(|item| {
                                item.as_str()
                                    .is_some_and(|s| !s.is_empty() && s.len() <= 128)
                            })
                    }),
                    _ => v
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 16384),
                });
            let Some(value) = value else { continue };
            let current = fields.get(destination);
            let empty = current.is_none_or(|v| {
                v.is_null() || v.as_str() == Some("") || v.as_array().is_some_and(Vec::is_empty)
            });
            let previous:Option<(String,i64,String)>=query_as("SELECT extension_id,priority,value_json FROM automatic_metadata_provenance WHERE work_id=? AND field_name=?")
                .bind(&work).bind(destination).fetch_optional(&mut **tx).await?;
            let replace = empty
                || previous
                    .as_ref()
                    .is_some_and(|(owner, old_priority, old_value)| {
                        serde_json::from_str::<Value>(old_value)
                            .is_ok_and(|ov| current == Some(&ov))
                            && (owner == extension || priority > *old_priority)
                    });
            if !replace {
                continue;
            }
            fields.insert(destination.into(), value.clone());
            query("INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at")
                .bind(&work).bind(destination).bind(extension).bind(&provider_id).bind(priority).bind(value.to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
            changed = true;
        }
        if changed {
            query("UPDATE inventory_works SET editorial_json=? WHERE id=?")
                .bind(metadata.to_string())
                .bind(work)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

/// Fence, persist output and complete the leased job atomically.
pub async fn apply_missing_music_metadata(
    tx: &mut Transaction<'_, Any>,
    track_id: &str,
    extension: &str,
    fingerprint: &str,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let row: Option<MusicMetadataRow> = query_as(
        "SELECT library_id, title, artist, album, album_artist, duration, lyrics, cover_path FROM music_tracks WHERE id=?"
    )
    .bind(track_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((library, _title, artist, album, _album_artist, _duration, lyrics, cover_path)) = row
    else {
        return Ok(());
    };
    let priority: i64 = query_scalar(
        "SELECT COALESCE((SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=?),(SELECT priority FROM automation_policies WHERE extension_id=? AND library_id=(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)),100)"
    )
    .bind(extension).bind(&library).bind(extension).bind(&library).fetch_one(&mut **tx).await?;

    let provider_id = result["providerId"]
        .as_str()
        .or_else(|| result["id"].as_str())
        .unwrap_or("")
        .chars()
        .take(256)
        .collect::<String>();

    // 1. Lyrics enrichment: preserve manual lyrics, allow higher priority
    let proposal_lyrics = result["synced"]
        .as_str()
        .or_else(|| result["plain"].as_str())
        .or_else(|| result["lyrics"].as_str());

    if let Some(new_lyrics) = proposal_lyrics.filter(|s| !s.trim().is_empty()) {
        let has_lyrics_provenance: i64 = query_scalar(
            "SELECT COUNT(*) FROM automatic_metadata_provenance WHERE work_id=? AND field_name='lyrics'"
        )
        .bind(track_id)
        .fetch_one(&mut **tx)
        .await?;

        let can_update_lyrics = lyrics.is_empty() || (has_lyrics_provenance > 0);
        if can_update_lyrics {
            let previous_prio: Option<i64> = query_scalar(
                "SELECT priority FROM automatic_metadata_provenance WHERE work_id=? AND field_name='lyrics'"
            )
            .bind(track_id)
            .fetch_optional(&mut **tx)
            .await?;

            if previous_prio.is_none_or(|p| priority >= p) {
                query("UPDATE music_tracks SET lyrics=? WHERE id=?")
                    .bind(new_lyrics)
                    .bind(track_id)
                    .execute(&mut **tx)
                    .await?;
                query(
                    "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
                )
                .bind(track_id).bind("lyrics").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_lyrics).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
            }
        }
    }

    // 2. Artist & Album enrichment if missing
    let proposal_artist = result["artist"]
        .as_str()
        .or_else(|| result["artistName"].as_str());
    if let Some(new_artist) = proposal_artist.filter(|s| !s.trim().is_empty())
        && (artist.is_empty() || artist == "Unknown Artist")
    {
        query("UPDATE music_tracks SET artist=? WHERE id=?")
            .bind(new_artist)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("artist").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_artist).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    let proposal_album = result["album"]
        .as_str()
        .or_else(|| result["albumName"].as_str());
    if let Some(new_album) = proposal_album.filter(|s| !s.trim().is_empty())
        && (album.is_empty() || album == "Unknown Album")
    {
        query("UPDATE music_tracks SET album=? WHERE id=?")
            .bind(new_album)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("album").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(new_album).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    // 3. Cover art enrichment: preserve local album cover
    let proposal_cover = result["coverUrl"]
        .as_str()
        .or_else(|| result["cover_url"].as_str())
        .or_else(|| result["poster_url"].as_str())
        .or_else(|| result["coverPath"].as_str());
    if let Some(cover_url) = proposal_cover.filter(|s| !s.trim().is_empty())
        && cover_path.is_none()
    {
        query("UPDATE music_tracks SET cover_path=? WHERE id=?")
            .bind(cover_url)
            .bind(track_id)
            .execute(&mut **tx)
            .await?;
        query(
            "INSERT INTO automatic_metadata_provenance(work_id,field_name,extension_id,provider_id,priority,value_json,source_fingerprint,updated_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(work_id,field_name) DO UPDATE SET extension_id=excluded.extension_id,provider_id=excluded.provider_id,priority=excluded.priority,value_json=excluded.value_json,source_fingerprint=excluded.source_fingerprint,updated_at=excluded.updated_at"
        )
        .bind(track_id).bind("cover").bind(extension).bind(&provider_id).bind(priority).bind(serde_json::json!(cover_url).to_string()).bind(fingerprint).bind(now).execute(&mut **tx).await?;
    }

    Ok(())
}

pub async fn complete(
    store: &IdentityStore,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    if !persist_result(&mut tx, job, link, result, now).await? {
        return Ok(false);
    }
    if link.operation == "intro.parse_chapters" {
        let found: std::collections::HashSet<&str> = result
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|marker| marker["markerType"].as_str())
            .collect();
        if !found.contains("intro") {
            let source: Option<String> =
                query_scalar("SELECT fingerprint FROM automation_observations WHERE asset_id=?")
                    .bind(&job.target_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(source) = source {
                let key = format!("fp:{}", hash(&(&job.target_id, &source, "chromaprint-v1"))?);
                query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,component,priority,created_at,updated_at,available_at) VALUES(?,?,?,?,?,'analyze',?,'fingerprint',20,?,?,?) ON CONFLICT(owner,idempotency_key) DO NOTHING")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&job.library_id).bind(&job.owner).bind(key).bind(&job.node_id).bind(&job.target_id).bind(now).bind(now).bind(now).execute(&mut *tx).await?;
            }
        }
    }
    query("UPDATE durable_tasks SET status='completed',result_json=?,progress=100,processed_items=1,lease_until=NULL,finished_at=?,updated_at=? WHERE id=? AND attempt=? AND status='running'")
 .bind(result.to_string()).bind(now).bind(now).bind(&job.id).bind(job.attempt).execute(&mut *tx).await?;
    let key = format!("analysis:{}", job.id);
    let fingerprint: String = query_scalar("SELECT fingerprint FROM automation_events WHERE id=?")
        .bind(&link.event_id)
        .fetch_one(&mut *tx)
        .await?;
    emit(
        &mut tx,
        &EventInput {
            key: &key,
            kind: "analysis.completed",
            library_id: &job.library_id,
            asset_id: &job.target_id,
            fingerprint: &fingerprint,
            payload: &serde_json::json!({"taskId":job.id}),
        },
        now,
    )
    .await?;
    crate::database::commit(tx).await?;
    crate::task_engine::WAKE.notify_one();
    Ok(true)
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub extension_id: String,
    pub library_id: String,
    pub mode: String,
    pub priority: i64,
    pub operation: String,
    pub config_revision: String,
    pub algorithm_version: String,
    pub interval_seconds: i64,
    pub next_run_at: i64,
    pub max_concurrent: i64,
}
pub async fn policies(store: &IdentityStore) -> Result<Vec<Policy>, sqlx::Error> {
    query_as("SELECT * FROM automation_policies ORDER BY library_id,extension_id LIMIT 1000")
        .fetch_all(store.pool())
        .await
}
pub async fn set_policy(store: &IdentityStore, p: &Policy, now: i64) -> Result<(), sqlx::Error> {
    if !matches!(
        p.mode.as_str(),
        "automatic" | "manual" | "disabled" | "scheduled"
    ) || !(0..=1000).contains(&p.priority)
        || !(1..=16).contains(&p.max_concurrent)
        || !(60..=31536000).contains(&p.interval_seconds)
        || p.config_revision.len() > 128
        || p.algorithm_version.len() > 128
        || !matches!(
            p.operation.as_str(),
            "metadata.propose" | "metadata.search" | "intro.parse_chapters"
        )
    {
        return Err(sqlx::Error::Protocol("invalid automation policy".into()));
    }
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    query("INSERT INTO automation_policies(extension_id,library_id,mode,priority,operation,config_revision,algorithm_version,interval_seconds,next_run_at,max_concurrent) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(extension_id,library_id) DO UPDATE SET mode=excluded.mode,priority=excluded.priority,operation=excluded.operation,config_revision=excluded.config_revision,algorithm_version=excluded.algorithm_version,interval_seconds=excluded.interval_seconds,next_run_at=excluded.next_run_at,max_concurrent=excluded.max_concurrent")
 .bind(&p.extension_id).bind(&p.library_id).bind(&p.mode).bind(p.priority).bind(&p.operation).bind(&p.config_revision).bind(&p.algorithm_version).bind(p.interval_seconds).bind(p.next_run_at).bind(p.max_concurrent).execute(&mut *tx).await?;
    if p.mode == "automatic" {
        schedule_library(
            &mut tx,
            &p.library_id,
            "extension.activated",
            &uuid::Uuid::new_v4().to_string(),
            Some(&p.extension_id),
            now,
        )
        .await?;
    }
    crate::database::commit(tx).await?;
    crate::task_engine::WAKE.notify_one();
    Ok(())
}
/// Persist a bounded, keyset-paginated fanout request, not one future per asset.
pub async fn schedule_library(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    kind: &str,
    key: &str,
    extension: Option<&str>,
    now: i64,
) -> Result<String, sqlx::Error> {
    if !matches!(
        kind,
        "manual" | "scheduled" | "extension.activated" | "metadata.missing"
    ) {
        return Err(sqlx::Error::Protocol("invalid automation trigger".into()));
    }
    let id = hash(&(library, kind, key, extension))?;
    query("INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) VALUES(?,?,?,?,?) ON CONFLICT(id) DO NOTHING")
 .bind(&id).bind(library).bind(extension.unwrap_or("")).bind(kind).bind(now).execute(&mut **tx).await?;
    query("INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) SELECT ? || ':' || library_id,library_id,?,?,? FROM library_folders WHERE parent_id=? AND active=1 ON CONFLICT(id) DO NOTHING")
    .bind(&id).bind(extension.unwrap_or("")).bind(kind).bind(now).bind(library).execute(&mut **tx).await?;
    Ok(id)
}
/// Called in the same transaction as extension activation/configuration changes.
pub async fn emit_extension(
    tx: &mut Transaction<'_, Any>,
    extension: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let nonce = uuid::Uuid::new_v4().to_string();
    Ok(query("INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) SELECT ? || ':' || l.id,l.id,e.id,'extension.activated',? FROM extensions e JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND (g.library_id=l.id OR EXISTS(SELECT 1 FROM library_folders granted WHERE granted.library_id=l.id AND granted.parent_id=g.library_id AND granted.active=1)))) WHERE e.id=? AND e.enabled=1 ON CONFLICT(id) DO NOTHING")
 .bind(nonce).bind(now).bind(extension).execute(&mut **tx).await?.rows_affected())
}
async fn pump_triggers(
    tx: &mut Transaction<'_, Any>,
    now: i64,
    limit: i64,
) -> Result<(), sqlx::Error> {
    let ready:Vec<(String,String,String,String,String)>=query_as("SELECT t.id,t.library_id,t.target_id,t.idempotency_key,obs.fingerprint FROM durable_tasks t JOIN automation_observations obs ON obs.asset_id=t.target_id WHERE t.category='analyze' AND t.component='fingerprint' AND t.status='completed' AND NOT EXISTS(SELECT 1 FROM automation_events ev WHERE ev.event_key='fingerprint-ready:' || t.id) ORDER BY t.finished_at,t.id LIMIT ?")
        .bind(limit).fetch_all(&mut **tx).await?;
    for (task, library, asset, key, source) in ready {
        let stale = key != format!("fp:{}", hash(&(&asset, &source, "chromaprint-v1"))?);
        emit(
            tx,
            &EventInput {
                key: &format!("fingerprint-ready:{task}"),
                kind: "analysis.completed",
                library_id: &library,
                asset_id: &asset,
                fingerprint: &source,
                payload: &serde_json::json!({"fingerprintTaskId":task,"stale":stale}),
            },
            now,
        )
        .await?;
    }
    let due:Vec<(String,String,i64,i64)>=query_as("SELECT extension_id,library_id,interval_seconds,next_run_at FROM automation_policies WHERE mode='scheduled' AND next_run_at<=? ORDER BY next_run_at,extension_id,library_id LIMIT ?").bind(now).bind(limit).fetch_all(&mut **tx).await?;
    for (extension, library, interval, due) in due {
        schedule_library(
            tx,
            &library,
            "scheduled",
            &due.to_string(),
            Some(&extension),
            now,
        )
        .await?;
        query("UPDATE automation_policies SET next_run_at=? WHERE extension_id=? AND library_id=?")
            .bind(now.saturating_add(interval))
            .bind(extension)
            .bind(library)
            .execute(&mut **tx)
            .await?;
    }
    let sweeps:Vec<(String,String,String,String,String)>=query_as("SELECT id,library_id,extension_id,kind,cursor_asset FROM automation_sweeps WHERE completed_at IS NULL ORDER BY created_at,id LIMIT 1").fetch_all(&mut **tx).await?;
    for (id, library, extension, kind, cursor) in sweeps {
        let rows:Vec<(String,String,String,String)>=query_as("SELECT a.id,obs.fingerprint,w.editorial_json,COALESCE(w.manual_json,'{}') FROM inventory_assets a JOIN automation_observations obs ON obs.asset_id=a.id JOIN inventory_sources source ON source.id=a.source_id JOIN inventory_versions version ON version.id=source.version_id JOIN inventory_editions edition ON edition.id=version.edition_id JOIN inventory_works w ON w.id=edition.work_id WHERE a.library_id=? AND a.available=1 AND a.id>? ORDER BY a.id LIMIT ?").bind(&library).bind(&cursor).bind(limit+1).fetch_all(&mut **tx).await?;
        let more = rows.len() as i64 > limit;
        for (asset, fingerprint, editorial, manual) in rows.into_iter().take(limit as usize) {
            query("UPDATE automation_sweeps SET cursor_asset=? WHERE id=?")
                .bind(&asset)
                .bind(&id)
                .execute(&mut **tx)
                .await?;
            if kind == "metadata.missing" && !metadata_has_missing_fields(&editorial, &manual) {
                continue;
            }
            let key = hash(&(&id, &asset))?;
            let payload = if extension.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({"extensionId":extension})
            };
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind: &kind,
                    library_id: &library,
                    asset_id: &asset,
                    fingerprint: &fingerprint,
                    payload: &payload,
                },
                now,
            )
            .await?;
        }
        if !more {
            query("UPDATE automation_sweeps SET completed_at=? WHERE id=?")
                .bind(now)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

fn metadata_has_missing_fields(editorial: &str, manual: &str) -> bool {
    let editorial = serde_json::from_str::<Value>(editorial).unwrap_or(Value::Null);
    let manual = serde_json::from_str::<Value>(manual).unwrap_or(Value::Null);
    ["plot", "outline", "genres"].iter().any(|key| {
        !manual
            .as_object()
            .is_some_and(|fields| fields.contains_key(*key))
            && editorial.get(*key).is_none_or(|value| {
                value.is_null()
                    || value.as_str() == Some("")
                    || value.as_array().is_some_and(Vec::is_empty)
            })
    })
}

/// Restore still-current automatic fields after the scanner rebuilds editorial
/// metadata. Revalidation prevents reusing outputs from changed source/config.
pub async fn reapply_results(
    tx: &mut Transaction<'_, Any>,
    work_ids: &[String],
) -> Result<(), sqlx::Error> {
    for work in work_ids {
        let rows:Vec<(String,String,String,String,String)>=query_as("SELECT r.asset_id,r.result_json,r.task_id,r.fingerprint,l.extension_id FROM automation_results r JOIN automation_task_links l ON l.task_id=r.task_id JOIN inventory_assets a ON a.id=r.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN extensions x ON x.id=l.extension_id JOIN automation_events event ON event.id=l.event_id JOIN automation_observations o ON o.asset_id=a.id WHERE e.work_id=? AND a.available=1 AND x.enabled=1 AND x.generation=l.generation AND o.fingerprint=event.fingerprint AND (x.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=x.id AND (g.library_id=a.library_id OR EXISTS(SELECT 1 FROM library_folders granted WHERE granted.library_id=a.library_id AND granted.parent_id=g.library_id AND granted.active=1)))) ORDER BY r.updated_at DESC LIMIT 100")
 .bind(work).fetch_all(&mut **tx).await?;
        for (asset, raw, task, _, _) in rows {
            let link: TaskLink = query_as("SELECT * FROM automation_task_links WHERE task_id=?")
                .bind(&task)
                .fetch_one(&mut **tx)
                .await?;
            let library: String =
                query_scalar("SELECT library_id FROM inventory_assets WHERE id=?")
                    .bind(&asset)
                    .fetch_one(&mut **tx)
                    .await?;
            if !current_fingerprint_matches(tx, &library, &asset, &link).await? {
                continue;
            }
            if can_write_catalogue(tx, &link.extension_id).await?
                && let Ok(result) = serde_json::from_str::<Value>(&raw)
            {
                apply_missing_metadata(
                    tx,
                    &asset,
                    &link.extension_id,
                    &link.fingerprint,
                    &result,
                    media_contracts::protocol::unix_now(),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn can_write_catalogue(
    tx: &mut Transaction<'_, Any>,
    extension: &str,
) -> Result<bool, sqlx::Error> {
    let raw: Option<String> = query_scalar("SELECT package_json FROM extensions WHERE id=?")
        .bind(extension)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(raw
        .and_then(|raw| serde_json::from_str::<media_extension_sdk::Package>(&raw).ok())
        .is_some_and(|package| {
            package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "metadata.write")
        }))
}

type FingerprintState = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

async fn current_fingerprint_matches(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    asset: &str,
    link: &TaskLink,
) -> Result<bool, sqlx::Error> {
    let current:Option<FingerprintState>=query_as("SELECT e.package_json,COALESCE(p.config_revision,''),COALESCE(p.algorithm_version,'1'),COALESCE(s.settings_json,'{}'),a.modified_ns,CAST(a.size AS TEXT),CAST(a.available AS TEXT),source.root_snapshot,COALESCE(p.operation,''),lib.options_json,a.probe_json FROM extensions e LEFT JOIN automation_policies p ON p.extension_id=e.id AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) LEFT JOIN extension_settings s ON s.extension_id=e.id JOIN inventory_assets a ON a.id=? JOIN inventory_sources source ON source.id=a.source_id JOIN libraries lib ON lib.id=a.library_id WHERE e.id=? AND source.root_snapshot=lib.root AND COALESCE(p.mode,'automatic')!='disabled'")
 .bind(library).bind(library).bind(asset).bind(&link.extension_id).fetch_optional(&mut **tx).await?;
    let (
        package,
        config,
        algorithm,
        settings,
        modified,
        size,
        available,
        root,
        operation,
        options,
        probe,
    ) = match current {
        Some(c) => c,
        None => {
            let music_curr: Option<MusicCurrentRow> = query_as(
                "SELECT e.package_json,COALESCE(p.config_revision,''),COALESCE(p.algorithm_version,'1'),COALESCE(s.settings_json,'{}'),t.modified_ns,t.size,t.available,t.root_snapshot,COALESCE(p.operation,''),lib.options_json,t.title || ':' || t.artist || ':' || t.album FROM extensions e LEFT JOIN automation_policies p ON p.extension_id=e.id AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) LEFT JOIN extension_settings s ON s.extension_id=e.id JOIN music_tracks t ON t.id=? JOIN libraries lib ON lib.id=t.library_id WHERE e.id=? AND t.root_snapshot=lib.root AND COALESCE(p.mode,'automatic')!='disabled'"
            )
            .bind(library).bind(library).bind(asset).bind(&link.extension_id).fetch_optional(&mut **tx).await?;
            let Some((pkg, cfg, alg, sett, modif, sz, avail, rt, op, opt, tag)) = music_curr else {
                return Ok(false);
            };
            (
                pkg,
                cfg,
                alg,
                sett,
                modif,
                sz.to_string(),
                avail.to_string(),
                rt,
                op,
                opt,
                tag,
            )
        }
    };
    let package: media_extension_sdk::Package = serde_json::from_str(&package)
        .map_err(|_| sqlx::Error::Protocol("invalid extension package".into()))?;
    let operation = if operation.is_empty() {
        if package
            .manifest
            .permissions
            .iter()
            .any(|p| p == "playback.markers")
        {
            "intro.parse_chapters"
        } else if package.manifest.http_hosts.is_empty() {
            "metadata.propose"
        } else {
            "metadata.search"
        }
    } else {
        &operation
    };
    if operation != link.operation
        && !(operation == "intro.parse_chapters" && link.operation == "intro.detect")
    {
        return Ok(false);
    }
    let size = size
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source size".into()))?;
    let available = available
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source availability".into()))?;
    let probe_digest = format!("{:x}", Sha256::digest(probe.as_bytes()));
    let file_fingerprint = hash(&(asset, size, &modified, available, &root, probe_digest))?;
    let settings: Value = serde_json::from_str(&settings)
        .map_err(|_| sqlx::Error::Protocol("invalid extension settings".into()))?;
    let options: Value = serde_json::from_str(&options)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
    let fingerprint = hash(&(
        asset,
        &file_fingerprint,
        &link.extension_id,
        &link.generation,
        &package.sha256,
        &config,
        &algorithm,
        &settings,
        &link.operation,
        &options,
    ))?;
    Ok(fingerprint == link.fingerprint)
}

/// Dependencies are attached before admission to a worker. Reject cycles and
/// edits to running jobs; prerequisite failures become explicit terminal errors.
pub async fn add_dependency(
    tx: &mut Transaction<'_, Any>,
    task: &str,
    prerequisite: &str,
) -> Result<(), sqlx::Error> {
    if task == prerequisite {
        return Err(sqlx::Error::Protocol("dependency cycle".into()));
    }
    let ready: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE id=? AND status='queued' AND lease_until IS NULL",
    )
    .bind(task)
    .fetch_one(&mut **tx)
    .await?;
    if ready != 1 {
        return Err(sqlx::Error::Protocol(
            "dependency task is not queued".into(),
        ));
    }
    let cycle:i64=query_scalar("WITH RECURSIVE ancestors(id) AS (SELECT prerequisite_id FROM automation_dependencies WHERE task_id=? UNION SELECT d.prerequisite_id FROM automation_dependencies d JOIN ancestors a ON d.task_id=a.id) SELECT COUNT(*) FROM ancestors WHERE id=?")
 .bind(prerequisite).bind(task).fetch_one(&mut **tx).await?;
    if cycle != 0 {
        return Err(sqlx::Error::Protocol("dependency cycle".into()));
    }
    query("INSERT INTO automation_dependencies(task_id,prerequisite_id) VALUES(?,?) ON CONFLICT(task_id,prerequisite_id) DO NOTHING").bind(task).bind(prerequisite).execute(&mut **tx).await?;
    Ok(())
}
