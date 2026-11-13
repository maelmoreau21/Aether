//! Persistent scan intervals. The process owns the worker guard; no worker is created by routers.
use crate::{auth::AuthState, identity::Role, scans::ScanJobs};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub library_id: String,
    pub interval_minutes: i64,
    // DB flags remain integers, matching the shared SQLite/PostgreSQL schema.
    pub enabled: i64,
    pub next_run_at: i64,
    pub last_run_at: Option<i64>,
    pub last_outcome: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    interval_minutes: i64,
    enabled: bool,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub async fn router(auth: AuthState) -> Result<Router, sqlx::Error> {
    crate::database::query(
        "UPDATE scheduled_scans SET last_outcome='interrupted' WHERE last_outcome='dispatching'",
    )
    .execute(&auth.store.pool)
    .await?;
    // Scan-router startup has already marked abandoned jobs as errors. Preserve
    // that terminal result immediately, even before the first worker tick.
    reconcile(&auth).await?;
    Ok(Router::new()
        .route("/api/v1/admin/tasks/schedules", get(list))
        .route(
            "/api/v1/admin/tasks/schedules/{id}",
            axum::routing::put(save).delete(remove),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth))
}
async fn authorize(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let user = auth.authenticate(headers).await?;
    if user.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &user)?;
    }
    Ok(())
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Schedule>>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    crate::database::query_as("SELECT library_id,interval_minutes,enabled,next_run_at,last_run_at,last_outcome FROM scheduled_scans ORDER BY library_id")
        .fetch_all(&auth.store.pool).await.map(Json).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)
}
async fn save(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    if !(5..=10080).contains(&input.interval_minutes) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let exists:i64 = crate::database::query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.kind IN ('films','series','music') AND l.id NOT IN ('fixture-films','fixture-series') AND n.status!='revoked'")
        .bind(&id).fetch_one(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let timestamp = now();
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::database::query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at) VALUES(?,?,?,?) ON CONFLICT(library_id) DO UPDATE SET interval_minutes=excluded.interval_minutes,enabled=excluded.enabled,next_run_at=excluded.next_run_at")
        .bind(&id).bind(input.interval_minutes).bind(i64::from(input.enabled)).bind(timestamp+input.interval_minutes*60)
        .execute(&mut *tx).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    // Compatibility endpoint: the library scan keeps exactly one interval trigger.
    crate::task_triggers::replace_with_interval(
        &mut tx,
        "scan",
        &id,
        input.interval_minutes,
        input.enabled,
        timestamp,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::database::commit(tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::database::query("DELETE FROM scheduled_scans WHERE library_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::task_triggers::remove_all(&mut tx, "scan", &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::database::commit(tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
/// Dropping the guard cancels and releases the worker; test routers never spawn background loops.
pub struct Scheduler(
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
);
impl Drop for Scheduler {
    fn drop(&mut self) {
        self.0.abort();
        self.1.abort();
        self.2.abort();
        self.3.abort();
        self.4.abort();
    }
}
impl Scheduler {
    pub fn start(auth: AuthState, root: PathBuf, jobs: Arc<ScanJobs>) -> Self {
        let application_auth = auth.clone();
        let application_jobs = jobs.clone();
        let applications = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                if application_jobs.is_cancelled() {
                    break;
                }
                if crate::acquisition::tick(&application_auth.store)
                    .await
                    .is_err()
                {
                    eprintln!("Acquisition pass failed; retrying on next tick");
                }
                if crate::artwork::migrate_one(&application_auth.store)
                    .await
                    .is_err()
                {
                    eprintln!("Artwork migration deferred; original references preserved");
                }
            }
        });
        let notification_auth = auth.clone();
        let notification_jobs = jobs.clone();
        let notifications = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            let mut ticks: u32 = 0;
            loop {
                interval.tick().await;
                if notification_jobs.is_cancelled() {
                    break;
                }
                if crate::notifications::tick(&notification_auth.store)
                    .await
                    .is_err()
                {
                    eprintln!("Notification worker pass failed; retrying on next tick");
                }
                // Every ten minutes: end idle "Watch together" parties, drop old invitations.
                if ticks.is_multiple_of(40)
                    && crate::parties::clean_expired_parties(&notification_auth.store)
                        .await
                        .is_err()
                {
                    eprintln!("Watch party expiry pass failed; retrying later");
                }
                ticks = ticks.wrapping_add(1);
            }
        });
        let artwork_auth = auth.clone();
        let artwork_jobs = jobs.clone();
        let artwork = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                if artwork_jobs.is_cancelled() {
                    break;
                }
                if crate::extensions::artwork_collection_tick(&artwork_auth)
                    .await
                    .is_err()
                {
                    eprintln!("Artwork collection pass deferred; progress remains persisted");
                }
            }
        });
        let worker_auth = auth.clone();
        let worker_root = root.clone();
        let worker_jobs = jobs.clone();
        let worker = tokio::spawn(async move {
            loop {
                if worker_jobs.is_cancelled() {
                    break;
                }
                if dispatch_once(&worker_auth, &worker_root, &worker_jobs, now())
                    .await
                    .is_err()
                {
                    eprintln!("Task dispatch pass failed; retrying");
                }
                let delay = if worker_jobs
                    .engine_busy
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    Duration::from_millis(500)
                } else {
                    Duration::from_secs(5)
                };
                tokio::select! { _ = tokio::time::sleep(delay) => {}, _ = crate::task_engine::WAKE.notified() => {} }
            }
        });
        Self(
            tokio::spawn(async move {
                // "At server startup" triggers fire once per process start.
                match crate::task_scheduler::fire_startup(&auth, now()).await {
                    Ok(0) => {}
                    Ok(count) => eprintln!("Startup triggers queued {count} task(s)"),
                    Err(error) => eprintln!("Startup triggers failed: {error}"),
                }
                let mut interval = tokio::time::interval(Duration::from_secs(15));
                let mut cache_cursor = String::new();
                let mut cache_ticks = 0u8;
                loop {
                    interval.tick().await;
                    if jobs.is_cancelled() {
                        break;
                    }
                    if run_due(&auth, &root, &jobs, now()).await.is_err() {
                        eprintln!("Scheduled scan tick failed; retrying on next tick");
                    }
                    cache_ticks += 1;
                    if cache_ticks >= 4 {
                        cache_ticks = 0;
                        if crate::automation::purge_provider_cache_batch(
                            &auth.store,
                            &mut cache_cursor,
                            now(),
                        )
                        .await
                        .is_err()
                        {
                            eprintln!("Provider cache cleanup failed; retrying on next tick");
                        }
                    }
                }
            }),
            worker,
            notifications,
            applications,
            artwork,
        )
    }
}
async fn reconcile(auth: &AuthState) -> Result<(), sqlx::Error> {
    // Aggregate the durable request, including every physical folder. A transient
    // scan_jobs error is not the outcome of a task waiting for its controlled retry.
    // Exact prefix comparison avoids LIKE wildcard collisions in library identifiers.
    crate::database::query("UPDATE scheduled_scans \
            SET last_outcome=COALESCE((SELECT CASE WHEN SUM(CASE WHEN status='error' THEN 1 \
            ELSE 0 END)>0 THEN 'error' WHEN SUM(CASE WHEN status='running' THEN 1 ELSE 0 END)>0 \
            THEN 'running' WHEN SUM(CASE WHEN status='queued' THEN 1 ELSE 0 END)>0 THEN 'queued' \
            WHEN SUM(CASE WHEN status='paused' THEN 1 ELSE 0 END)>0 THEN 'paused' WHEN SUM(CASE \
            WHEN status='cancelled' THEN 1 ELSE 0 END)>0 THEN 'cancelled' WHEN COUNT(*)>0 \
            THEN 'completed' ELSE NULL END FROM durable_tasks j WHERE j.owner='scheduler' \
            AND substr(j.idempotency_key,1,length(scheduled_scans.last_request_key)+1)=scheduled_scans.last_request_key || ':'),'interrupted') \
            WHERE last_request_key!=''")
        .execute(&auth.store.pool).await?;
    crate::database::query(
        "UPDATE scheduled_scans \
            SET last_outcome=(SELECT status FROM scan_jobs \
            WHERE scan_jobs.library_id=scheduled_scans.library_id) \
            WHERE last_request_key='' AND last_outcome IN ('queued','running') AND EXISTS(SELECT 1 \
            FROM scan_jobs WHERE scan_jobs.library_id=scheduled_scans.library_id \
            AND status IN ('completed','error'))",
    )
    .execute(&auth.store.pool)
    .await?;
    crate::database::query("UPDATE scheduled_scans SET last_outcome='interrupted' WHERE last_request_key='' AND last_outcome IN ('queued','running') AND NOT EXISTS(SELECT 1 FROM scan_jobs WHERE scan_jobs.library_id=scheduled_scans.library_id)")
        .execute(&auth.store.pool).await?;
    Ok(())
}

/// One bounded pass, exported for deterministic integration tests and operational checks.
pub async fn run_due(
    auth: &AuthState,
    root: &std::path::Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    let _tick = jobs.scheduler_tick.lock().await;
    // Completion is retained even when the plan is disabled after dispatch.
    reconcile(auth).await?;
    if jobs.is_cancelled() {
        return Ok(());
    }
    run_library_automation(auth, timestamp).await?;
    // Every scheduled task (scans, maintenance, backup) fires from its triggers here.
    let fired = crate::task_scheduler::fire_due(auth, timestamp).await?;
    run_metadata_refresh(auth, timestamp).await?;
    if fired > 0 {
        // Keep the public deterministic pass useful without requiring a background timer.
        dispatch_once(auth, root, jobs, timestamp).await?;
    }
    Ok(())
}

/// Periodic metadata refresh of one library (library option `automaticRefreshIntervalDays`).
async fn run_metadata_refresh(auth: &AuthState, timestamp: i64) -> Result<(), sqlx::Error> {
    let due: Option<(String, String, i64)> = crate::database::query_as(
        "SELECT l.id,l.options_json,l.next_metadata_refresh_at \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) \
            AND l.enabled=1 AND l.id NOT IN ('fixture-films','fixture-series') AND l.kind!='music' \
            AND n.status!='revoked' AND l.next_metadata_refresh_at<=? \
            AND CAST(COALESCE(json_extract(l.options_json,'$.automaticRefreshIntervalDays'),'0') \
            AS BIGINT)>0 \
            ORDER BY l.next_metadata_refresh_at,l.id \
            LIMIT 1",
    )
    .bind(timestamp)
    .fetch_optional(&auth.store.pool)
    .await?;
    if let Some((id, encoded, previous)) = due {
        let options: crate::library_options::LibraryOptions = serde_json::from_str(&encoded)
            .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
        queue_scopes(auth, &id, &format!("metadata:{id}:{previous}"), timestamp).await?;
        let _claimed = crate::database::query("UPDATE libraries SET next_metadata_refresh_at=? WHERE id=? AND next_metadata_refresh_at=?")
            .bind(timestamp+i64::from(options.automatic_refresh_interval_days)*86400).bind(&id).bind(previous).execute(&auth.store.pool).await?;
    }
    Ok(())
}

/// After a failed monitoring poll, the library is not polled again for this long (seconds).
const MONITOR_FAILURE_BACKOFF: i64 = 1800;

/// Monitoring polls through the Node scan queue, so Core never traverses remote roots.
/// One admission per pass, low priority and at most once a minute per library, never while
/// another scan of the library is pending, and not again soon after a failed poll.
async fn run_library_automation(auth: &AuthState, timestamp: i64) -> Result<(), sqlx::Error> {
    let watch: Option<String> = crate::database::query_scalar(
        "SELECT l.id \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE l.enabled=1 AND n.status!='revoked' \
            AND l.id NOT IN ('fixture-films','fixture-series') AND NOT EXISTS(SELECT 1 \
            FROM library_folders f WHERE f.library_id=l.id) \
            AND CAST(COALESCE(json_extract(l.options_json,'$.realtimeMonitoring'),'0') AS TEXT) IN \
            ('1','true') AND NOT EXISTS(SELECT 1 FROM durable_tasks t \
            WHERE t.owner='library-monitor' AND (t.library_id=l.id \
            OR t.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=l.id \
            AND active=1)) AND (t.status IN ('queued','running','paused') OR t.created_at>? \
            OR (t.status='error' AND COALESCE(t.finished_at,t.updated_at)>?))) \
            AND NOT EXISTS(SELECT 1 FROM durable_tasks a WHERE a.category='scan' \
            AND a.status IN ('queued','running','paused') AND (a.library_id=l.id \
            OR a.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=l.id))) \
            ORDER BY COALESCE((SELECT MAX(t.created_at) FROM durable_tasks t \
            WHERE t.library_id=l.id AND t.owner='library-monitor'),0),l.id \
            LIMIT 1",
    )
    .bind(timestamp - 60)
    .bind(timestamp - MONITOR_FAILURE_BACKOFF)
    .fetch_optional(&auth.store.pool)
    .await?;
    if let Some(library) = watch {
        queue_scopes_with(
            auth,
            &library,
            &format!("monitor:{}", timestamp / 60),
            timestamp,
            "library-monitor",
            10,
        )
        .await?;
        // A monitoring poll is periodic: a failed poll waits for the next one (after
        // MONITOR_FAILURE_BACKOFF) instead of being replayed three times with backoff.
        crate::database::query("UPDATE durable_tasks SET max_attempts=1 WHERE owner='library-monitor' AND status='queued' AND attempt=0 AND max_attempts>1")
            .execute(&auth.store.pool)
            .await?;
    }
    let online: Option<(String, String)> = crate::database::query_as(
        "SELECT l.id,l.options_json \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE l.enabled=1 AND n.status!='revoked' \
            AND l.id NOT IN ('fixture-films','fixture-series') AND NOT EXISTS(SELECT 1 \
            FROM library_folders f WHERE f.library_id=l.id) \
            AND CAST(COALESCE(json_extract(l.options_json,'$.onlineRefreshIntervalDays'),'0') AS \
            BIGINT)>0 AND NOT EXISTS(SELECT 1 FROM automation_sweeps s WHERE s.library_id=l.id \
            AND s.kind='scheduled' AND s.extension_id='' AND (s.completed_at IS NULL \
            OR s.created_at>?-CAST(json_extract(l.options_json,'$.onlineRefreshIntervalDays') AS \
            BIGINT)*86400)) \
            ORDER BY COALESCE((SELECT MAX(s.created_at) FROM automation_sweeps s \
            WHERE s.library_id=l.id AND s.kind='scheduled' AND s.extension_id=''),0),l.id \
            LIMIT 1",
    )
    .bind(timestamp)
    .fetch_optional(&auth.store.pool)
    .await?;
    if let Some((library, _)) = online {
        let mut tx = auth.store.pool.begin().await?;
        crate::automation::schedule_library(
            &mut tx,
            &library,
            "scheduled",
            &format!("online:{timestamp}"),
            None,
            timestamp,
        )
        .await?;
        tx.commit().await?;
    }
    Ok(())
}

/// Queue one library scan (every physical folder) outside its periodic schedule: startup
/// triggers and the unified "run" action.
pub(crate) async fn queue_library_scan(
    auth: &AuthState,
    library: &str,
    key: &str,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    queue_scopes(auth, library, key, timestamp).await?;
    crate::task_engine::WAKE.notify_one();
    Ok(())
}

/// Enqueue each physical folder independently, retaining one visible library.
async fn queue_scopes(
    auth: &AuthState,
    library: &str,
    key: &str,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    queue_scopes_with(auth, library, key, timestamp, "scheduler", 100).await
}
async fn queue_scopes_with(
    auth: &AuthState,
    library: &str,
    key: &str,
    timestamp: i64,
    owner: &str,
    priority: i64,
) -> Result<(), sqlx::Error> {
    let mut scopes: Vec<String> = crate::database::query_scalar(
        "SELECT library_id FROM library_folders WHERE parent_id=? AND active=1 ORDER BY library_id",
    )
    .bind(library)
    .fetch_all(&auth.store.pool)
    .await?;
    scopes.push(library.to_owned());
    for scope in scopes {
        crate::task_engine::enqueue(
            &auth.store,
            &scope,
            owner,
            &format!("{key}:{scope}"),
            priority,
            timestamp,
        )
        .await
        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    }
    Ok(())
}

/// One bounded queue pass. Only leased tasks get executors; a backlog gets no Tokio task.
pub async fn dispatch_once(
    auth: &AuthState,
    root: &std::path::Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    let _dispatch = jobs.engine_tick.lock().await;
    if jobs.is_cancelled() {
        return Ok(());
    }
    crate::automation::drain(&auth.store, timestamp, 64).await?;
    crate::system_task_queue::dispatch_once(auth, root, jobs, timestamp).await?;
    let leased = crate::task_engine::leased(&auth.store, 100).await?;
    jobs.engine_busy
        .store(!leased.is_empty(), std::sync::atomic::Ordering::Relaxed);
    let statuses: Vec<(String,String,String)> = crate::database::query_as(
        "SELECT r.scan_id,s.status,s.errors_json FROM active_scan_runs r JOIN scan_jobs s ON s.library_id=r.library_id WHERE EXISTS(SELECT 1 FROM durable_tasks j WHERE j.library_id=r.library_id AND j.lease_until IS NOT NULL) UNION ALL SELECT t.run_id,t.status,t.error FROM active_task_runs t")
        .fetch_all(&auth.store.pool).await?;
    let registry = auth.store.node_registry.snapshot_all();
    let expired: Vec<_> = leased
        .iter()
        .filter(|j| j.lease_until.is_some_and(|until| until <= timestamp))
        .map(|j| format!("{}:{}", j.id, j.attempt))
        .collect();
    for job in leased {
        if job.category == "system" {
            continue;
        }
        if crate::automation::task_link(&auth.store, &job.id)
            .await?
            .is_some()
        {
            continue;
        }
        let scan = format!("{}:{}", job.id, job.attempt);
        let preempted = job.node_id == "local"
            && crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed) > 0;
        if preempted {
            jobs.cancel_scan(&scan);
        }
        let stopping = job.status != "running";
        if stopping {
            jobs.cancel_scan(&scan);
            if job.node_id == "local" {
                if jobs.scan_active(&scan) {
                    continue;
                }
            } else {
                // Results are fenced immediately. The slot stays reserved until lease
                // expiry, since transport delivery is not proof that a probe stopped.
                if registry.get(&job.node_id).is_some_and(|n| {
                    n.record.last_seen_at.is_some_and(|t| t > job.updated_at)
                        && !n.telemetry.active_scan_ids.contains(&scan)
                }) {
                    crate::task_engine::finish(
                        &auth.store,
                        &job.id,
                        job.attempt,
                        &job.node_id,
                        timestamp,
                        Some("Stopped by administrator"),
                    )
                    .await?;
                    continue;
                }
                if timestamp.saturating_sub(job.updated_at) % 5 == 0 {
                    let store = auth.store.clone();
                    let node = job.node_id.clone();
                    let scan = scan.clone();
                    jobs.track_control(tokio::spawn(async move {
                        let _ = tokio::time::timeout(
                            Duration::from_millis(200),
                            crate::nodes::cancel_remote_scan(&store, &node, &scan),
                        )
                        .await;
                    }));
                }
                continue;
            }
            crate::task_engine::finish(
                &auth.store,
                &job.id,
                job.attempt,
                &job.node_id,
                timestamp,
                Some("Stopped by administrator"),
            )
            .await?;
            continue;
        }
        if let Some((_, status, errors)) = statuses.iter().find(|(id, _, _)| id == &scan)
            && matches!(status.as_str(), "completed" | "error")
        {
            crate::task_engine::finish(
                &auth.store,
                &job.id,
                job.attempt,
                &job.node_id,
                timestamp,
                if status == "completed" {
                    None
                } else {
                    Some(
                        if preempted
                            || errors.contains("preempted for playback")
                            || errors.contains("node_busy_playback_priority")
                            || errors.contains("preparation_busy")
                            || errors.contains("cancelled")
                            || errors.contains("Scan arrêté")
                            || errors.contains("Music scan interrupted")
                        {
                            "playback_preempted"
                        } else {
                            errors.as_str()
                        },
                    )
                },
            )
            .await?;
            continue;
        }
        // Renew only live executors; a Core restart cannot renew an abandoned local job.
        let live = if job.node_id == "local" {
            jobs.scan_active(&scan)
        } else {
            registry.get(&job.node_id).is_some_and(|n| {
                n.record.status == "connected"
                    && n.record.last_seen_at.is_some_and(|t| timestamp - t <= 30)
                    && n.telemetry.active_scan_ids.contains(&scan)
                    && statuses.iter().any(|(id, _, _)| id == &scan)
            })
        };
        if live && job.lease_until.is_some_and(|until| until - timestamp <= 30) {
            crate::task_engine::renew(
                &auth.store,
                &job.id,
                job.attempt,
                &job.node_id,
                timestamp,
                60,
                job.progress,
            )
            .await?;
        }
    }
    crate::task_engine::recover_expired(&auth.store, timestamp).await?;
    // Task fences end with their lease: finished, recovered or cancelled runs.
    crate::database::query("DELETE FROM active_task_runs WHERE NOT EXISTS(SELECT 1 FROM durable_tasks j WHERE active_task_runs.run_id=j.id || ':' || CAST(j.attempt AS TEXT) AND j.lease_until IS NOT NULL)")
        .execute(&auth.store.pool).await?;
    for scan in expired {
        let _ = crate::inventory::clean_staging(&auth.store, &scan).await;
    }
    // Old scan status cannot block a safely fenced retry forever after a crash.
    crate::database::query(
        "UPDATE scan_jobs \
            SET status='error',errors_json='[\"Worker lease expired\"]',errors_count=1 \
            WHERE status IN ('queued','running') AND EXISTS(SELECT 1 FROM active_scan_runs r \
            JOIN durable_tasks j ON j.library_id=r.library_id \
            WHERE r.library_id=scan_jobs.library_id \
            AND r.scan_id=j.id || ':' || CAST(j.attempt AS TEXT) AND j.lease_until IS NULL)",
    )
    .execute(&auth.store.pool)
    .await?;
    let nodes: Vec<String> = crate::database::query_scalar("SELECT DISTINCT node_id FROM durable_tasks WHERE status='queued' AND lease_until IS NULL AND available_at<=? LIMIT 100")
        .bind(timestamp).fetch_all(&auth.store.pool).await?;
    let mut ready = Vec::with_capacity(nodes.len());
    for node in nodes {
        if node == "local"
            && resource_pressure_blocks(
                &jobs
                    .local_resources
                    .get_or_init(|| {
                        media_node::resources::Sampler::new(
                            auth.store
                                .plugins_path()
                                .parent()
                                .unwrap_or(root)
                                .to_owned(),
                        )
                    })
                    .sample()
                    .await,
                timestamp,
            )
        {
            continue;
        }
        if node == "local"
            && crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed) > 0
        {
            continue;
        }
        if registry.get(&node).is_some_and(|n| {
            n.telemetry.reported_active_streams > 0 || n.telemetry.core_active_relays > 0
        }) {
            continue;
        }
        if registry
            .get(&node)
            .and_then(|n| n.telemetry.resources.as_ref())
            .is_some_and(|r| resource_pressure_blocks(r, timestamp))
        {
            continue;
        }
        ready.push(node);
    }
    for job in crate::task_engine::claim_batch(&auth.store, &ready, timestamp, 60).await? {
        let node = job.node_id.clone();
        jobs.engine_busy
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let scan = format!("{}:{}", job.id, job.attempt);
        if crate::automation_execution::dispatch(auth, jobs, &job).await? {
            continue;
        }
        if job.category != "scan" {
            if crate::task_execution::dispatch(auth, root, jobs, &job)
                .await
                .is_err()
            {
                crate::task_engine::finish(
                    &auth.store,
                    &job.id,
                    job.attempt,
                    &node,
                    timestamp,
                    Some("Task operation admission failed"),
                )
                .await?;
            }
            continue;
        }
        if crate::scans::execute_leased(
            auth,
            root.to_owned(),
            jobs.clone(),
            job.library_id.clone(),
            scan,
            serde_json::from_value(serde_json::Value::String(job.refresh_mode.clone()))
                .map_err(|_| sqlx::Error::Protocol("Invalid task refresh mode".into()))?,
        )
        .await
        .is_err()
        {
            crate::task_engine::finish(
                &auth.store,
                &job.id,
                job.attempt,
                &node,
                timestamp,
                Some("Scan admission failed"),
            )
            .await?;
        }
    }
    Ok(())
}

/// Missing/stale telemetry is unknown, never fabricated as free capacity.
fn resource_pressure_blocks(
    r: &media_contracts::protocol::ResourcePressure,
    timestamp: i64,
) -> bool {
    if timestamp.saturating_sub(r.sampled_at) > 30 {
        return false;
    }
    r.cpu_usage_percent.is_some_and(|p| p >= 85)
        || r.io_pressure_percent.is_some_and(|p| p >= 50)
        || r.memory_available_bytes.is_some_and(|available| {
            available < 128 * 1024 * 1024
                || r.memory_total_bytes
                    .is_some_and(|total| available < total / 20)
        })
        || r.scratch_available_bytes
            .is_some_and(|bytes| bytes < 128 * 1024 * 1024)
        || r.network_bytes_per_second
            .is_some_and(|bps| network_budget().is_some_and(|limit| bps >= limit))
}

fn network_budget() -> Option<u64> {
    static LIMIT: std::sync::LazyLock<Option<u64>> = std::sync::LazyLock::new(|| {
        std::env::var("MEDIA_TASK_NETWORK_LIMIT_BPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|v| *v > 0)
    });
    *LIMIT
}
