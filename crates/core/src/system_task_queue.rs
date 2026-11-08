//! Durable queue of Core maintenance runs (system tasks and the scheduled backup).
//!
//! Runs are admitted by the scheduler (`task_scheduler`), a startup trigger or an administrator,
//! and executed here one at a time under a durable-task lease. Light maintenance (sessions,
//! statistics, logs, caches, catalogue refresh) starts as soon as no other maintenance run is
//! active: it no longer waits for an idle Node or for playback to stop, which used to starve it
//! for hours on busy servers. Heavy work (database optimisation, backup) yields to scans and
//! playback, but never for longer than [`HEAVY_MAX_WAIT`].
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::IdentityStore,
    scans::ScanJobs,
    task_engine,
};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

const PERMANENT_FAILURE: &str = "system_task_failed_permanently";
const TIMEOUT: &str = "system_task_timeout";

/// Default execution deadlines when no trigger sets a time limit.
const SYSTEM_TASK_DEADLINE: u64 = 300;
const BACKUP_DEADLINE: u64 = 3600;

/// Longest time (seconds) a heavy run waits for an idle server before it starts anyway.
pub(crate) const HEAVY_MAX_WAIT: i64 = 3600;

/// Runs that rewrite or copy the whole database and therefore wait for a quiet server.
fn is_heavy(target: &str) -> bool {
    matches!(target, "optimize-database" | "backup")
}

/// Time limit of one queued run: the firing trigger's `maxRuntimeSeconds`, else the default.
fn deadline_seconds(job: &task_engine::Job) -> u64 {
    serde_json::from_str::<serde_json::Value>(&job.result_json)
        .ok()
        .and_then(|value| value.get("maxRuntimeSeconds")?.as_u64())
        .filter(|seconds| (60..=604_800).contains(seconds))
        .unwrap_or(if job.target_id == "backup" {
            BACKUP_DEADLINE
        } else {
            SYSTEM_TASK_DEADLINE
        })
}

/// Insert one run unless the task already has an active run or the request key was used.
async fn enqueue_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    id: &str,
    owner: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    let existing: Option<String> = query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id=? AND status IN ('queued','running','paused') ORDER BY created_at,id LIMIT 1")
        .bind(id).fetch_optional(&mut **tx).await?;
    if let Some(existing) = existing {
        return Ok(existing);
    }
    // A request key identifies one request for its whole lifetime (UNIQUE(owner,key)).
    let replayed: Option<String> =
        query_scalar("SELECT id FROM durable_tasks WHERE owner=? AND idempotency_key=?")
            .bind(owner)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(replayed) = replayed {
        return Ok(replayed);
    }
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','paused')",
    )
    .fetch_one(&mut **tx)
    .await?;
    if count >= 100_000 {
        return Err(sqlx::Error::Protocol(
            "durable queue capacity reached".into(),
        ));
    }
    let task = uuid::Uuid::new_v4().to_string();
    let options = match max_runtime_seconds {
        Some(seconds) => serde_json::json!({ "maxRuntimeSeconds": seconds }).to_string(),
        None => "{}".to_owned(),
    };
    query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,priority,created_at,updated_at,available_at,result_json) VALUES(?,NULL,?,?,'local','system',?,10,?,?,?,?)")
        .bind(&task).bind(owner).bind(key).bind(id).bind(now).bind(now).bind(now).bind(options)
        .execute(&mut **tx).await?;
    // The queued state is read from `durable_tasks`; the task keeps showing its last result.
    Ok(task)
}

async fn enqueue_with_owner(
    store: &IdentityStore,
    id: &str,
    owner: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    let task = {
        let _writer = store.task_writes.lock().await;
        let mut conn = store.pool().acquire().await?;
        let mut tx = begin_write(&mut conn).await?;
        let task = enqueue_in(&mut tx, id, owner, key, now, max_runtime_seconds).await?;
        commit(tx).await?;
        task
    };
    task_engine::WAKE.notify_one();
    Ok(task)
}

/// Queue the scheduled backup (target `backup`); an active run absorbs duplicates.
pub(crate) async fn enqueue_backup(
    store: &IdentityStore,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    enqueue_with_owner(
        store,
        "backup",
        "system",
        &format!("system:backup:{key}"),
        now,
        max_runtime_seconds,
    )
    .await
}

/// Queue a system task run requested by an administrator.
pub(crate) async fn enqueue_manual(
    store: &IdentityStore,
    id: &str,
    owner: &str,
    now: i64,
) -> Result<String, sqlx::Error> {
    enqueue_with_owner(
        store,
        id,
        owner,
        &format!("system-manual:{}", uuid::Uuid::new_v4()),
        now,
        None,
    )
    .await
}

/// Queue a system task fired by a trigger (`key` identifies the firing).
pub(crate) async fn enqueue(
    store: &IdentityStore,
    id: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    enqueue_with_owner(
        store,
        id,
        "system",
        &format!("system:{id}:{key}"),
        now,
        max_runtime_seconds,
    )
    .await
}

/// Whether the local server is busy for heavy maintenance: a scan or processing lease on the
/// local Node, or a stream being served.
async fn local_busy(store: &IdentityStore) -> Result<bool, sqlx::Error> {
    if crate::scans::LOCAL_STREAMS.load(Ordering::Relaxed) > 0 {
        return Ok(true);
    }
    let leased: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE node_id='local' AND category<>'system' AND lease_until IS NOT NULL")
        .fetch_one(store.pool())
        .await?;
    Ok(leased > 0)
}

/// Claim the next runnable maintenance run, if any. At most one runs at a time.
async fn claim(store: &IdentityStore, timestamp: i64) -> Result<Option<String>, sqlx::Error> {
    let candidates: Vec<(String, String, i64)> = query_as("SELECT id,target_id,created_at FROM durable_tasks WHERE category='system' AND status='queued' AND lease_until IS NULL AND available_at<=? AND failure_count<max_attempts AND NOT EXISTS(SELECT 1 FROM durable_tasks active WHERE active.category='system' AND active.lease_until IS NOT NULL) ORDER BY priority DESC,created_at,id LIMIT 16")
        .bind(timestamp)
        .fetch_all(store.pool())
        .await?;
    if candidates.is_empty() {
        return Ok(None);
    }
    let mut busy = None;
    let mut chosen = None;
    for (id, target, created) in candidates {
        if is_heavy(&target) && timestamp.saturating_sub(created) < HEAVY_MAX_WAIT {
            let busy = match busy {
                Some(value) => value,
                None => *busy.insert(local_busy(store).await?),
            };
            if busy {
                continue;
            }
        }
        chosen = Some(id);
        break;
    }
    let Some(id) = chosen else {
        return Ok(None);
    };
    let _writer = store.task_writes.lock().await;
    let claimed = query("UPDATE durable_tasks SET status='running',attempt=attempt+1,lease_until=?,started_at=COALESCE(started_at,?),updated_at=?,finished_at=NULL,error='' WHERE id=? AND status='queued' AND lease_until IS NULL AND NOT EXISTS(SELECT 1 FROM durable_tasks active WHERE active.category='system' AND active.lease_until IS NOT NULL)")
        .bind(timestamp.saturating_add(60)).bind(timestamp).bind(timestamp).bind(&id)
        .execute(store.pool()).await?
        .rows_affected();
    Ok((claimed == 1).then_some(id))
}

/// One bounded pass of the driver: start the next maintenance run, if one may start.
pub(crate) async fn dispatch_once(
    auth: &AuthState,
    root: &Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    if jobs.is_cancelled() {
        return Ok(());
    }
    let Some(id) = claim(&auth.store, timestamp).await? else {
        return Ok(());
    };
    let Some(job) = task_engine::get(&auth.store, &id).await? else {
        return Ok(());
    };
    // Heavy runs that started on a quiet server step aside for playback; once they have waited
    // their maximum they run to completion.
    let preemptible =
        is_heavy(&job.target_id) && timestamp.saturating_sub(job.created_at) < HEAVY_MAX_WAIT;
    let auth = auth.clone();
    let root = root.to_owned();
    let stopping = jobs.clone();
    jobs.track_control(tokio::spawn(async move {
        let state = crate::system_tasks::execution_state(auth.clone(), root);
        let outcome = {
            let operation = crate::system_tasks::execute_leased(&state, &job.target_id, &job);
            tokio::pin!(operation);
            let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
            let deadline = tokio::time::sleep(Duration::from_secs(deadline_seconds(&job)));
            tokio::pin!(deadline);
            let mut last_renewed = timestamp;
            loop {
                tokio::select! {
                    result = &mut operation => break result.map_err(|code| if code == axum::http::StatusCode::UNPROCESSABLE_ENTITY { PERMANENT_FAILURE } else { "system_task_failed" }),
                    _ = &mut deadline => break Err(TIMEOUT),
                    _ = heartbeat.tick() => {
                        let now = media_contracts::protocol::unix_now();
                        if stopping.is_cancelled() || (preemptible && crate::scans::LOCAL_STREAMS.load(Ordering::Relaxed) > 0) {
                            break Err("playback_preempted");
                        }
                        match task_engine::get(&auth.store, &job.id).await {
                            Ok(Some(current)) if current.status=="running" && current.attempt==job.attempt && current.lease_until.is_some_and(|until|until>now) => {},
                            _ => break Err("system_task_cancelled"),
                        }
                        if now.saturating_sub(last_renewed)>=10 {
                            match task_engine::renew(&auth.store,&job.id,job.attempt,"local",now,60,0).await {
                                Ok(true) => last_renewed=now,
                                _ => break Err("system_task_lease_lost"),
                            }
                        }
                    }
                }
            }
        };
        let error = outcome.err();
        if error.is_some() && job.target_id == "backup" {
            // The executor could not record its own outcome (timeout, preemption, lease loss).
            if let Err(failure) = query("UPDATE backup_schedule SET last_outcome='interrupted',last_completed_at=? WHERE singleton=1 AND last_outcome='running'")
                .bind(media_contracts::protocol::unix_now()).execute(auth.store.pool()).await {
                eprintln!("backup interruption could not be persisted: {failure}");
            }
        }
        let finished = task_engine::finish(&auth.store,&job.id,job.attempt,"local",media_contracts::protocol::unix_now(),error).await;
        if matches!(finished, Ok(true)) && matches!(error, Some(PERMANENT_FAILURE | TIMEOUT)) {
            // Deterministic failures and exhausted time limits are not replayed with backoff.
            let _writer = auth.store.task_writes.lock().await;
            let now = media_contracts::protocol::unix_now();
            if let Err(failure) = query("UPDATE durable_tasks SET status='error',failure_count=max_attempts,finished_at=?,updated_at=? WHERE id=? AND attempt=? AND status='queued' AND lease_until IS NULL")
                .bind(now).bind(now).bind(&job.id).bind(job.attempt).execute(auth.store.pool()).await {
                eprintln!("system task termination could not be persisted: {failure}");
            }
        }
        match finished {
            Ok(true) if error.is_some() && error != Some(PERMANENT_FAILURE) => {
                if let Err(failure) = query("UPDATE system_task_schedules SET last_outcome='interrupted',last_completed_at=? WHERE id=? AND last_outcome='running' AND EXISTS(SELECT 1 FROM durable_tasks WHERE id=? AND attempt=? AND lease_until IS NULL)")
                    .bind(media_contracts::protocol::unix_now()).bind(&job.target_id).bind(&job.id).bind(job.attempt).execute(auth.store.pool()).await {
                    eprintln!("system task interruption could not be persisted: {failure}");
                }
            }
            Err(failure) => eprintln!("system task lease result could not be persisted: {failure}"),
            _ => {},
        }
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_database_wide_work_is_heavy() {
        assert!(is_heavy("optimize-database"));
        assert!(is_heavy("backup"));
        for light in [
            "clean-sessions",
            "clean-statistics",
            "clean-activity-log",
            "clean-cache",
            "clean-playlists",
            "update-extensions",
        ] {
            assert!(!is_heavy(light), "{light}");
        }
    }
}
