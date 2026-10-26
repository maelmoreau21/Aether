//! Transactional outbox feeding the existing leased task queue. No independent scheduler.
use crate::{
    database::{begin_write, query, query_as, query_scalar},
    identity::IdentityStore,
    task_engine::Job,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};

pub struct EventInput<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub library_id: &'a str,
    pub asset_id: &'a str,
    pub fingerprint: &'a str,
    pub payload: &'a Value,
}
fn hash(value: &impl Serialize) -> Result<String, sqlx::Error> {
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
/// Compare committed inventory revisions. Repeated delivery produces no new event;
/// removal followed by reappearance gets a new revision even for identical bytes.
pub async fn emit_snapshot(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    _scan_id: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let mut count = 0;
    let mut cursor = String::new();
    loop {
        let rows: Vec<(String, i64, String, i64,String)> = query_as(
        "SELECT a.id,a.size,a.modified_ns,a.available,s.root_snapshot FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id WHERE a.library_id=? AND a.id>? ORDER BY a.id LIMIT 256",
    ).bind(library).bind(&cursor).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            break;
        }
        for (asset, size, modified, available, root) in rows {
            cursor = asset.clone();
            let fingerprint = hash(&(&asset, size, &modified, available, &root))?;
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
    let mut tx = begin_write(&mut conn).await?;
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
        let selected = serde_json::from_str::<Value>(&payload)
            .ok()
            .and_then(|v| v["extensionId"].as_str().map(str::to_owned));
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
                || (c.mode == "manual" && kind != "manual")
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
            }
            if c.operation.is_empty() {
                c.operation = if marker_plugin {
                    "intro.parse_chapters"
                } else if package.manifest.http_hosts.is_empty() {
                    "metadata.propose"
                } else {
                    "metadata.search"
                }
                .into();
            }
            if marker_plugin && c.operation != "intro.parse_chapters" {
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
                if kind == "manual" && pending < 100000 {
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
            let node:Option<String>=query_scalar("SELECT l.node_id FROM libraries l JOIN media_nodes n ON n.id=l.node_id JOIN inventory_assets a ON a.library_id=l.id WHERE l.id=? AND a.id=? AND a.available=1 AND n.status!='revoked' AND (l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))").bind(&library).bind(&asset).fetch_optional(&mut *tx).await?;
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
    let valid:i64=query_scalar("SELECT COUNT(*) FROM durable_tasks t JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN inventory_assets a ON a.id=t.target_id WHERE t.id=? AND t.attempt=? AND t.node_id=? AND t.status='running' AND t.lease_until>? AND e.enabled=1 AND e.generation=? AND p.active=1 AND a.available=1 AND EXISTS(SELECT 1 FROM libraries lib JOIN media_nodes node ON node.id=lib.node_id WHERE lib.id=t.library_id AND node.id=t.node_id AND node.status!='revoked' AND ((lib.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=lib.id)) OR EXISTS(SELECT 1 FROM library_folders own JOIN libraries parent ON parent.id=own.parent_id WHERE own.library_id=lib.id AND own.active=1 AND parent.enabled=1))) AND EXISTS(SELECT 1 FROM users u WHERE u.id=t.owner AND u.role='administrator' AND u.disabled_at IS NULL) AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=t.library_id AND policy.mode='disabled') AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND (g.library_id=t.library_id OR EXISTS(SELECT 1 FROM library_folders granted WHERE granted.library_id=t.library_id AND granted.parent_id=g.library_id AND granted.active=1))))")
 .bind(&link.extension_id).bind(&job.id).bind(job.attempt).bind(&job.node_id).bind(now).bind(&link.generation).fetch_one(&mut **tx).await?;
    if valid != 1 {
        return Ok(false);
    }
    if !current_fingerprint_matches(tx, &job.library_id, &job.target_id, link).await? {
        return Ok(false);
    }
    query("INSERT INTO automation_results(extension_id,asset_id,fingerprint,task_id,result_json,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(extension_id,asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,task_id=excluded.task_id,result_json=excluded.result_json,updated_at=excluded.updated_at")
 .bind(&link.extension_id).bind(&job.target_id).bind(&link.fingerprint).bind(&job.id).bind(encoded).bind(now).execute(&mut **tx).await?;
    if link.operation != "intro.parse_chapters"
        && can_write_catalogue(tx, &link.extension_id).await?
    {
        apply_missing_metadata(tx, &job.target_id, result).await?;
    }
    Ok(true)
}

/// Merge only absent automatic fields. Manual corrections remain a separate
/// higher-precedence document and are never modified by this path.
async fn apply_missing_metadata(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    result: &Value,
) -> Result<(), sqlx::Error> {
    let rows:Vec<(String,String)>=query_as("SELECT w.id,w.editorial_json FROM inventory_works w JOIN inventory_editions e ON e.work_id=w.id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=?").bind(asset).fetch_all(&mut **tx).await?;
    for (work, raw) in rows {
        let mut metadata: Value =
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
        let Some(fields) = metadata.as_object_mut() else {
            continue;
        };
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
            if fields.get(destination).is_some_and(|v| {
                !v.is_null() && v.as_str() != Some("") && v.as_array().is_none_or(|a| !a.is_empty())
            }) {
                continue;
            }
            if let Some(value) = sources
                .iter()
                .filter_map(|key| proposal.get(*key))
                .find(|v| !v.is_null() && v.as_str() != Some(""))
            {
                fields.insert(destination.into(), value.clone());
                changed = true;
            }
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
    let mut tx = begin_write(&mut conn).await?;
    if !persist_result(&mut tx, job, link, result, now).await? {
        return Ok(false);
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
    let mut tx = begin_write(&mut conn).await?;
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
        let rows:Vec<(String,i64,String,i64,String)>=query_as("SELECT a.id,a.size,a.modified_ns,a.available,s.root_snapshot FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id WHERE a.library_id=? AND a.available=1 AND a.id>? ORDER BY a.id LIMIT ?").bind(&library).bind(&cursor).bind(limit+1).fetch_all(&mut **tx).await?;
        let more = rows.len() as i64 > limit;
        for (asset, size, modified, available, root) in rows.into_iter().take(limit as usize) {
            let fingerprint = hash(&(&asset, size, &modified, available, &root))?;
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
            query("UPDATE automation_sweeps SET cursor_asset=? WHERE id=?")
                .bind(asset)
                .bind(&id)
                .execute(&mut **tx)
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
                apply_missing_metadata(tx, &asset, &result).await?;
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
);

async fn current_fingerprint_matches(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    asset: &str,
    link: &TaskLink,
) -> Result<bool, sqlx::Error> {
    let current:Option<FingerprintState>=query_as("SELECT e.package_json,COALESCE(p.config_revision,''),COALESCE(p.algorithm_version,'1'),COALESCE(s.settings_json,'{}'),a.modified_ns,CAST(a.size AS TEXT),CAST(a.available AS TEXT),source.root_snapshot,COALESCE(p.operation,''),lib.options_json FROM extensions e LEFT JOIN automation_policies p ON p.extension_id=e.id AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) LEFT JOIN extension_settings s ON s.extension_id=e.id JOIN inventory_assets a ON a.id=? JOIN inventory_sources source ON source.id=a.source_id JOIN libraries lib ON lib.id=a.library_id WHERE e.id=? AND source.root_snapshot=lib.root AND COALESCE(p.mode,'automatic')!='disabled'")
 .bind(library).bind(library).bind(asset).bind(&link.extension_id).fetch_optional(&mut **tx).await?;
    let Some((
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
    )) = current
    else {
        return Ok(false);
    };
    let package: media_extension_sdk::Package = serde_json::from_str(&package)
        .map_err(|_| sqlx::Error::Protocol("invalid extension package".into()))?;
    let operation = if operation.is_empty() {
        if package.manifest.http_hosts.is_empty() {
            "metadata.propose"
        } else {
            "metadata.search"
        }
    } else {
        &operation
    };
    if operation != link.operation {
        return Ok(false);
    }
    let size = size
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source size".into()))?;
    let available = available
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source availability".into()))?;
    let file_fingerprint = hash(&(asset, size, &modified, available, &root))?;
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
