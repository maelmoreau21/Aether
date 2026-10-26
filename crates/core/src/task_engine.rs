//! Durable Core-owned queue. Execution uses the existing confined Media Node scan protocol.
use crate::{
    auth::AuthState,
    database::{begin_write, query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get as route_get, post},
};
use serde::{Deserialize, Serialize};

/// Serialize control changes with the final authorization/publication of scan effects.
pub static EFFECT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub library_id: String,
    pub owner: String,
    pub node_id: String,
    pub category: String,
    pub target_id: String,
    pub component: String,
    pub width: i64,
    pub result_json: String,
    pub refresh_mode: String,
    pub status: String,
    pub priority: i64,
    pub attempt: i64,
    pub failure_count: i64,
    pub max_attempts: i64,
    pub progress: i64,
    pub processed_items: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub lease_until: Option<i64>,
    pub error: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid task input")]
    Invalid,
    #[error("library unavailable")]
    Unavailable,
    #[error("queue capacity reached")]
    Capacity,
    #[error("idempotency key already used for different input")]
    Conflict,
    #[error("task database operation failed")]
    Database(#[from] sqlx::Error),
}

/// Durable admission; a key identifies one request for its entire lifetime.
pub async fn enqueue(
    store: &IdentityStore,
    library_id: &str,
    owner: &str,
    key: &str,
    priority: i64,
    now: i64,
) -> Result<Job, EngineError> {
    enqueue_mode(store, library_id, owner, key, priority, now, "incremental").await
}

pub async fn enqueue_mode(
    store: &IdentityStore,
    library_id: &str,
    owner: &str,
    key: &str,
    priority: i64,
    now: i64,
    mode: &str,
) -> Result<Job, EngineError> {
    enqueue_spec(
        store,
        library_id,
        owner,
        key,
        priority,
        now,
        TaskSpec {
            category: "scan",
            target: "",
            component: "",
            width: 640,
            mode,
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn enqueue_task(
    store: &IdentityStore,
    library_id: &str,
    owner: &str,
    key: &str,
    priority: i64,
    now: i64,
    category: &str,
    target: &str,
    component: &str,
    width: i64,
) -> Result<Job, EngineError> {
    enqueue_spec(
        store,
        library_id,
        owner,
        key,
        priority,
        now,
        TaskSpec {
            category,
            target,
            component,
            width,
            mode: "incremental",
        },
    )
    .await
}
struct TaskSpec<'a> {
    category: &'a str,
    target: &'a str,
    component: &'a str,
    width: i64,
    mode: &'a str,
}
async fn enqueue_spec(
    store: &IdentityStore,
    library_id: &str,
    owner: &str,
    key: &str,
    priority: i64,
    now: i64,
    spec: TaskSpec<'_>,
) -> Result<Job, EngineError> {
    let TaskSpec {
        category,
        target,
        component,
        width,
        mode,
    } = spec;
    if !(0..=1000).contains(&priority)
        || !matches!(category, "scan" | "analyze" | "prepare" | "image")
        || !matches!(width, 320 | 640 | 1280)
        || target.len() > 128
        || component.len() > 64
        || (category == "scan" && (!target.is_empty() || !component.is_empty() || width != 640))
        || (category != "scan" && (target.is_empty() || mode != "incremental"))
        || (matches!(category, "analyze" | "image") && !component.is_empty())
        || (category != "image" && width != 640)
        || (category == "prepare" && !valid_component(component))
        || !matches!(
            mode,
            "incremental" | "new_only" | "missing_metadata" | "full"
        )
        || key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_:.".contains(&c))
        || owner.is_empty()
        || owner.len() > 128
    {
        return Err(EngineError::Invalid);
    }
    // Queue behind the sole writer before taking a pooled connection. This leaves
    // connections available to authentication, navigation and playback in WAL mode.
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let existing: Option<Job> =
        query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks WHERE owner=? AND idempotency_key=?")
            .bind(owner)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(job) = existing {
        if job.library_id != library_id
            || job.priority != priority
            || job.refresh_mode != mode
            || job.category != category
            || job.target_id != target
            || job.component != component
            || job.width != width
        {
            return Err(EngineError::Conflict);
        }
        crate::database::commit(tx).await?;
        return Ok(job);
    }
    let node_target: Option<(String,i64)>=query_as("SELECT l.node_id,CAST(CASE WHEN (n.id='local' OR n.status='connected') AND NOT EXISTS(SELECT 1 FROM durable_tasks busy WHERE busy.node_id=l.node_id AND busy.lease_until IS NOT NULL) THEN 1 ELSE 0 END AS BIGINT) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND l.kind IN ('films','series','music') AND n.status!='revoked' AND l.id NOT IN ('fixture-films','fixture-series')").bind(library_id).fetch_optional(&mut *tx).await?;
    let (node, can_wake) = node_target.ok_or(EngineError::Unavailable)?;
    if category != "scan" {
        let sql = if category == "image" {
            "SELECT COUNT(*) FROM inventory_images i JOIN libraries l ON l.id=i.library_id WHERE i.id=? AND i.library_id=? AND i.available=1 AND i.root_snapshot=l.root"
        } else {
            "SELECT COUNT(*) FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root"
        };
        let exists: i64 = query_scalar(sql)
            .bind(target)
            .bind(library_id)
            .fetch_one(&mut *tx)
            .await?;
        if exists != 1 {
            return Err(EngineError::Unavailable);
        }
        if category == "prepare" && component != "video" {
            let (kind, index) = component.split_once('-').ok_or(EngineError::Invalid)?;
            let index = index.parse::<i64>().map_err(|_| EngineError::Invalid)?;
            let track: Option<String> = query_scalar(
                "SELECT metadata_json FROM inventory_tracks WHERE asset_id=? AND stream_index=?",
            )
            .bind(target)
            .bind(index)
            .fetch_optional(&mut *tx)
            .await?;
            let track = track
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                .ok_or(EngineError::Unavailable)?;
            if track["codec_type"].as_str()
                != Some(if kind == "audio" { "audio" } else { "subtitle" })
            {
                return Err(EngineError::Invalid);
            }
        }
    }
    // Bounds durable pending work as well as worker memory; history is not silently deleted.
    let pending: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','paused')",
    )
    .fetch_one(&mut *tx)
    .await?;
    if pending >= 100_000 {
        return Err(EngineError::Capacity);
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,priority,created_at,updated_at,available_at,refresh_mode,category,target_id,component,width) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)").bind(&id).bind(library_id).bind(owner).bind(key).bind(node).bind(priority).bind(now).bind(now).bind(now).bind(mode).bind(category).bind(target).bind(component).bind(width).execute(&mut *tx).await?;
    let job = query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    crate::database::commit(tx).await?;
    // Busy/offline Nodes cannot accept this job yet; completion/heartbeat ticks
    // will retry without a dispatch pass for every queued submission.
    if can_wake != 0 {
        WAKE.notify_one();
    }
    Ok(job)
}

fn valid_component(raw: &str) -> bool {
    raw == "video"
        || raw.split_once('-').is_some_and(|(kind, index)| {
            matches!(kind, "audio" | "subtitle")
                && !index.is_empty()
                && index.bytes().all(|c| c.is_ascii_digit())
                && index.parse::<u32>().is_ok()
        })
}

/// Persist a bounded result only while the exact worker still owns the live lease.
pub async fn record_result(
    store: &IdentityStore,
    id: &str,
    attempt: i64,
    node: &str,
    now: i64,
    result: &serde_json::Value,
) -> Result<bool, sqlx::Error> {
    let encoded =
        serde_json::to_string(result).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    if encoded.len() > 64 * 1024 {
        return Ok(false);
    }
    let _writer = store.task_writes.lock().await;
    Ok(query("UPDATE durable_tasks SET result_json=?,processed_items=1,updated_at=? WHERE id=? AND attempt=? AND node_id=? AND status='running' AND lease_until>?").bind(encoded).bind(now).bind(id).bind(attempt).bind(node).bind(now).execute(store.pool()).await?.rows_affected()==1)
}

pub async fn get(store: &IdentityStore, id: &str) -> Result<Option<Job>, sqlx::Error> {
    query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks WHERE id=?")
        .bind(id)
        .fetch_optional(store.pool())
        .await
}
pub async fn leased(store: &IdentityStore, limit: i64) -> Result<Vec<Job>, sqlx::Error> {
    query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,'{}' AS result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks WHERE lease_until IS NOT NULL ORDER BY id LIMIT ?")
        .bind(limit.clamp(1, 1000))
        .fetch_all(store.pool())
        .await
}

/// Serialized admission prevents overbooking even under concurrent claimers.
pub async fn claim(
    store: &IdentityStore,
    node_id: &str,
    now: i64,
    lease_seconds: i64,
    node_limit: i64,
) -> Result<Option<Job>, sqlx::Error> {
    if !(1..=100).contains(&node_limit) || !(1..=3600).contains(&lease_seconds) {
        return Ok(None);
    }
    // Queue behind the sole writer before taking a pooled connection. This leaves
    // connections available to authentication, navigation and playback in WAL mode.
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let job = claim_in_transaction(&mut tx, node_id, now, lease_seconds, node_limit).await?;
    crate::database::commit(tx).await?;
    Ok(job)
}

/// Bounded production admission: one commit covers up to 100 ready Nodes.
/// No worker starts before this transaction has committed every returned lease.
pub async fn claim_batch(
    store: &IdentityStore,
    nodes: &[String],
    now: i64,
    lease_seconds: i64,
) -> Result<Vec<Job>, sqlx::Error> {
    if nodes.len() > 100 || !(1..=3600).contains(&lease_seconds) {
        return Err(sqlx::Error::Protocol("Invalid task dispatch batch".into()));
    }
    if nodes.is_empty() {
        return Ok(Vec::new());
    }
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    // Select one indexed candidate per ready Node inside SQLite/PostgreSQL. Materialize
    // before UPDATE so changing status cannot cause a second candidate on the same Node.
    // Only placeholder syntax is generated; Node identifiers remain bound values.
    let mut sql = String::with_capacity(2600 + nodes.len() * 4);
    sql.push_str("WITH ready(node_id) AS (VALUES ");
    for index in 0..nodes.len() {
        if index > 0 {
            sql.push(',');
        }
        sql.push_str("(?)");
    }
    sql.push_str(") , candidates AS MATERIALIZED (SELECT (SELECT j.id FROM durable_tasks j JOIN libraries l ON l.id=j.library_id JOIN media_nodes n ON n.id=j.node_id WHERE j.node_id=ready.node_id AND j.status='queued' AND j.lease_until IS NULL AND j.available_at<=? AND j.failure_count<j.max_attempts AND NOT EXISTS(SELECT 1 FROM automation_dependencies d JOIN durable_tasks prerequisite ON prerequisite.id=d.prerequisite_id WHERE d.task_id=j.id AND prerequisite.status!='completed') AND l.node_id=j.node_id AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND (n.status='connected' OR (n.id='local' AND n.status!='revoked')) AND NOT EXISTS(SELECT 1 FROM durable_tasks active WHERE active.library_id=j.library_id AND active.lease_until IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM scan_jobs s WHERE s.library_id=j.library_id AND s.status IN ('queued','running')) ORDER BY j.priority DESC,j.created_at,j.id LIMIT 1) AS id FROM ready WHERE NOT EXISTS(SELECT 1 FROM durable_tasks busy WHERE busy.node_id=ready.node_id AND busy.lease_until IS NOT NULL)) UPDATE durable_tasks SET status='running',attempt=attempt+1,lease_until=?,updated_at=?,started_at=COALESCE(started_at,?),finished_at=NULL,error='',result_json='{}',processed_items=0,progress=0 WHERE id IN (SELECT id FROM candidates WHERE id IS NOT NULL) RETURNING id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error");
    let mut mutation = query_as::<sqlx::Any, Job>(sqlx::AssertSqlSafe(sql));
    for node in nodes {
        mutation = mutation.bind(node);
    }
    let claimed = mutation
        .bind(now)
        .bind(now.saturating_add(lease_seconds))
        .bind(now)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
    let claimed = filter_automation_claims(&mut tx, claimed, now).await?;
    crate::database::commit(tx).await?;
    Ok(claimed)
}

async fn claim_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    node_id: &str,
    now: i64,
    lease_seconds: i64,
    node_limit: i64,
) -> Result<Option<Job>, sqlx::Error> {
    let active: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE node_id=? AND lease_until IS NOT NULL",
    )
    .bind(node_id)
    .fetch_one(&mut **tx)
    .await?;
    if active >= node_limit {
        return Ok(None);
    }
    let id:Option<String>=query_scalar("SELECT j.id FROM durable_tasks j JOIN libraries l ON l.id=j.library_id JOIN media_nodes n ON n.id=j.node_id WHERE j.node_id=? AND j.status='queued' AND j.lease_until IS NULL AND j.available_at<=? AND j.failure_count<j.max_attempts AND NOT EXISTS(SELECT 1 FROM automation_dependencies d JOIN durable_tasks prerequisite ON prerequisite.id=d.prerequisite_id WHERE d.task_id=j.id AND prerequisite.status!='completed') AND l.node_id=j.node_id AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND (n.status='connected' OR (n.id='local' AND n.status!='revoked')) AND NOT EXISTS(SELECT 1 FROM durable_tasks active WHERE active.library_id=j.library_id AND active.lease_until IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM scan_jobs s WHERE s.library_id=j.library_id AND s.status IN ('queued','running')) ORDER BY j.priority DESC,j.created_at,j.id LIMIT 1").bind(node_id).bind(now).fetch_optional(&mut **tx).await?;
    let Some(id) = id else { return Ok(None) };
    query("UPDATE durable_tasks SET status='running',attempt=attempt+1,lease_until=?,updated_at=?,started_at=COALESCE(started_at,?),finished_at=NULL,error='',result_json='{}',processed_items=0,progress=0 WHERE id=? AND status='queued'").bind(now.saturating_add(lease_seconds)).bind(now).bind(now).bind(&id).execute(&mut **tx).await?;
    let job = query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks WHERE id=?")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(filter_automation_claims(tx, vec![job], now).await?.pop())
}

/// Reservations commit with leases; two Nodes cannot oversubscribe a provider.
async fn filter_automation_claims(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    claimed: Vec<Job>,
    now: i64,
) -> Result<Vec<Job>, sqlx::Error> {
    let mut accepted = Vec::with_capacity(claimed.len());
    for job in claimed {
        let link: Option<(String, String)> =
            query_as("SELECT extension_id,provider FROM automation_task_links WHERE task_id=?")
                .bind(&job.id)
                .fetch_optional(&mut **tx)
                .await?;
        let Some((extension, provider)) = link else {
            accepted.push(job);
            continue;
        };
        query("INSERT INTO automation_provider_limits(provider) VALUES(?) ON CONFLICT(provider) DO NOTHING")
            .bind(&provider).execute(&mut **tx).await?;
        let (next, interval, provider_limit): (i64,i64,i64) = query_as("SELECT next_allowed_at,min_interval_seconds,max_concurrent FROM automation_provider_limits WHERE provider=?")
            .bind(&provider).fetch_one(&mut **tx).await?;
        let extension_limit: i64 = query_scalar("SELECT COALESCE((SELECT max_concurrent FROM automation_policies WHERE extension_id=? AND library_id=COALESCE((SELECT parent_id FROM library_folders WHERE library_id=? AND active=1),?)),1)")
            .bind(&extension).bind(&job.library_id).bind(&job.library_id).fetch_one(&mut **tx).await?;
        let provider_active: i64 = query_scalar("SELECT COUNT(*) FROM automation_task_links a JOIN durable_tasks t ON t.id=a.task_id WHERE a.provider=? AND t.lease_until IS NOT NULL")
            .bind(&provider).fetch_one(&mut **tx).await?;
        let extension_active: i64 = query_scalar("SELECT COUNT(*) FROM automation_task_links a JOIN durable_tasks t ON t.id=a.task_id WHERE a.extension_id=? AND t.lease_until IS NOT NULL")
            .bind(&extension).fetch_one(&mut **tx).await?;
        let library_active: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks t WHERE t.lease_until IS NOT NULL AND COALESCE((SELECT parent_id FROM library_folders WHERE library_id=t.library_id AND active=1),t.library_id)=COALESCE((SELECT parent_id FROM library_folders WHERE library_id=? AND active=1),?)")
            .bind(&job.library_id).bind(&job.library_id).fetch_one(&mut **tx).await?;
        if next > now
            || provider_active > provider_limit
            || extension_active > extension_limit
            || library_active > 1
        {
            query("UPDATE durable_tasks SET status='queued',attempt=attempt-1,lease_until=NULL,available_at=?,started_at=NULL WHERE id=?")
                .bind(next.max(now.saturating_add(1))).bind(&job.id).execute(&mut **tx).await?;
            continue;
        }
        query("UPDATE automation_provider_limits SET next_allowed_at=? WHERE provider=?")
            .bind(now.saturating_add(interval))
            .bind(&provider)
            .execute(&mut **tx)
            .await?;
        accepted.push(job);
    }
    Ok(accepted)
}

pub async fn renew(
    store: &IdentityStore,
    id: &str,
    attempt: i64,
    node_id: &str,
    now: i64,
    lease_seconds: i64,
    progress: i64,
) -> Result<bool, sqlx::Error> {
    if !(1..=3600).contains(&lease_seconds) || !(0..=100).contains(&progress) {
        return Ok(false);
    }
    let _writer = store.task_writes.lock().await;
    Ok(query("UPDATE durable_tasks SET lease_until=?,updated_at=?,progress=CASE WHEN progress>? THEN progress ELSE ? END WHERE id=? AND attempt=? AND node_id=? AND status='running' AND lease_until>?").bind(now.saturating_add(lease_seconds)).bind(now).bind(progress).bind(progress).bind(id).bind(attempt).bind(node_id).bind(now).execute(store.pool()).await?.rows_affected()==1)
}

/// A stopped worker acknowledges paused/cancelled work without reviving it.
pub async fn finish(
    store: &IdentityStore,
    id: &str,
    attempt: i64,
    node_id: &str,
    now: i64,
    error: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let error = error.unwrap_or("");
    let error: String = error.chars().take(1024).collect();
    let failed = i64::from(!error.is_empty() && error != "playback_preempted");
    let delay = if failed == 0 {
        1
    } else {
        // Stable bounded jitter avoids synchronized retries after provider outages.
        let jitter = id.bytes().fold(attempt as u64, |n, b| {
            n.wrapping_mul(31).wrapping_add(u64::from(b))
        }) % 6;
        5_i64
            .saturating_mul(1_i64.checked_shl(attempt.clamp(0, 6) as u32).unwrap_or(64))
            .min(300)
            .saturating_add(jitter as i64)
    };
    let _writer = store.task_writes.lock().await;
    let changed=query("UPDATE durable_tasks SET status=CASE WHEN status IN ('paused','cancelled') THEN status WHEN ?='' THEN 'completed' WHEN failure_count+? >= max_attempts THEN 'error' ELSE 'queued' END,failure_count=failure_count+CASE WHEN status='running' THEN ? ELSE 0 END,progress=CASE WHEN status='running' AND ?='' THEN 100 ELSE progress END,lease_until=NULL,updated_at=?,finished_at=CASE WHEN status='cancelled' OR (status='running' AND (?='' OR failure_count+? >= max_attempts)) THEN ? ELSE NULL END,available_at=CASE WHEN status IN ('paused','cancelled') THEN ? ELSE ? END,error=? WHERE id=? AND attempt=? AND node_id=? AND lease_until>? AND status IN ('running','paused','cancelled')").bind(&error).bind(failed).bind(failed).bind(&error).bind(now).bind(&error).bind(failed).bind(now).bind(now).bind(now.saturating_add(delay)).bind(error).bind(id).bind(attempt).bind(node_id).bind(now).execute(store.pool()).await?.rows_affected()==1;
    if changed {
        WAKE.notify_one();
    }
    Ok(changed)
}

/// Expiry fences prior attempts. Restart only requeues work after the lease expires.
pub async fn recover_expired(store: &IdentityStore, now: i64) -> Result<u64, sqlx::Error> {
    let expired: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE lease_until<=?")
        .bind(now)
        .fetch_one(store.pool())
        .await?;
    if expired == 0 {
        return Ok(0);
    }
    let _effects = EFFECT_GATE.lock().await;
    let _writer = store.task_writes.lock().await;
    Ok(query("UPDATE durable_tasks SET status=CASE WHEN status IN ('paused','cancelled') THEN status WHEN failure_count+1>=max_attempts THEN 'error' ELSE 'queued' END,failure_count=failure_count+CASE WHEN status='running' THEN 1 ELSE 0 END,lease_until=NULL,updated_at=?,available_at=?,finished_at=CASE WHEN (status='running' AND failure_count+1>=max_attempts) OR status='cancelled' THEN ? ELSE NULL END,error='Worker lease expired; retry fenced' WHERE lease_until<=?").bind(now).bind(now.saturating_add(5)).bind(now).bind(now).execute(store.pool()).await?.rows_affected())
}

/// Running controls retain capacity until stop acknowledgment or lease expiry.
pub async fn control(
    store: &IdentityStore,
    id: &str,
    action: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let _effects = EFFECT_GATE.lock().await;
    let _writer = store.task_writes.lock().await;
    let sql = match action {
        "pause" => {
            "UPDATE durable_tasks SET status='paused',updated_at=? WHERE id=? AND status IN ('queued','running')"
        }
        "cancel" => {
            "UPDATE durable_tasks SET status='cancelled',finished_at=CASE WHEN lease_until IS NULL THEN ? ELSE NULL END,updated_at=? WHERE id=? AND status IN ('queued','running','paused')"
        }
        "resume" => {
            "UPDATE durable_tasks SET status='queued',updated_at=? WHERE id=? AND status='paused' AND lease_until IS NULL AND failure_count<max_attempts"
        }
        _ => return Ok(false),
    };
    let mutation = query(sql).bind(now);
    let mutation = if action == "cancel" {
        mutation.bind(now)
    } else {
        mutation
    };
    let changed = mutation
        .bind(id)
        .execute(store.pool())
        .await?
        .rows_affected()
        == 1;
    if changed {
        WAKE.notify_one();
    }
    Ok(changed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    library_id: String,
    idempotency_key: String,
    #[serde(default = "default_priority")]
    priority: i64,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_category")]
    category: String,
    #[serde(default)]
    target_id: String,
    #[serde(default)]
    component: String,
    #[serde(default = "default_width")]
    width: i64,
}
fn default_category() -> String {
    "scan".to_owned()
}
fn default_width() -> i64 {
    640
}
fn default_mode() -> String {
    "incremental".to_owned()
}
fn default_priority() -> i64 {
    100
}
#[derive(Deserialize)]
struct Page {
    limit: Option<i64>,
    offset: Option<i64>,
}
fn now() -> i64 {
    media_contracts::protocol::unix_now()
}
async fn authorize(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<String, StatusCode> {
    let user = auth.authenticate(headers).await?;
    if user.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &user)?;
    }
    Ok(user.user.id)
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/tasks/jobs", route_get(list).post(create))
        .route("/api/v1/admin/tasks/jobs/{id}", route_get(detail))
        .route("/api/v1/admin/tasks/jobs/{id}/{action}", post(action))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
async fn detail(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Job>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    get(&auth.store, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    let limit = page.limit.unwrap_or(100).clamp(1, 200);
    let offset = page.offset.unwrap_or(0).clamp(0, 100_000);
    let jobs: Vec<Job> =
        query_as("SELECT id,COALESCE(library_id,'') AS library_id,owner,COALESCE(node_id,'') AS node_id,category,target_id,component,width,'{}' AS result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,error FROM durable_tasks ORDER BY created_at DESC,id LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(auth.store.pool())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        serde_json::json!({"jobs":jobs,"limit":limit,"offset":offset}),
    ))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Job>), StatusCode> {
    let owner = authorize(&auth, &headers, true).await?;
    let timestamp = now();
    let parent = enqueue_spec(
        &auth.store,
        &input.library_id,
        &owner,
        &input.idempotency_key,
        input.priority,
        timestamp,
        TaskSpec {
            category: &input.category,
            target: &input.target_id,
            component: &input.component,
            width: input.width,
            mode: &input.mode,
        },
    )
    .await
    .map_err(engine_status)?;
    if input.category == "scan" {
        let folders: Vec<String> = query_scalar(
        "SELECT library_id FROM library_folders WHERE parent_id=? AND active=1 ORDER BY library_id",
    )
    .bind(&input.library_id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for folder in folders {
            let key = media_contracts::protocol::sha256_hex(
                format!("{}:{folder}", input.idempotency_key).as_bytes(),
            );
            enqueue_mode(
                &auth.store,
                &folder,
                &owner,
                &key,
                input.priority,
                timestamp,
                &input.mode,
            )
            .await
            .map_err(engine_status)?;
        }
    }
    Ok((StatusCode::ACCEPTED, Json(parent)))
}
fn engine_status(e: EngineError) -> StatusCode {
    match e {
        EngineError::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
        EngineError::Unavailable => StatusCode::NOT_FOUND,
        EngineError::Capacity => StatusCode::TOO_MANY_REQUESTS,
        EngineError::Conflict => StatusCode::CONFLICT,
        EngineError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
async fn action(
    State(auth): State<AuthState>,
    Path((id, action)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    if !matches!(action.as_str(), "pause" | "resume" | "cancel") {
        return Err(StatusCode::BAD_REQUEST);
    }
    if control(&auth.store, &id, &action, now())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::CONFLICT)
    }
}
