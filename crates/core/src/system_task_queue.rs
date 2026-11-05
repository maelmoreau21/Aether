//! Existing system schedules are triggers into durable_tasks, never executors.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::IdentityStore,
    scans::ScanJobs,
    task_engine,
};
use std::{path::Path, sync::Arc, time::Duration};

const PERMANENT_FAILURE: &str = "system_task_failed_permanently";
const TIMEOUT: &str = "system_task_timeout";

/// Default execution deadlines when no trigger sets a time limit.
const SYSTEM_TASK_DEADLINE: u64 = 300;
const BACKUP_DEADLINE: u64 = 3600;

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
    query("UPDATE system_task_schedules SET last_outcome='queued' WHERE id=? AND last_outcome!='running'")
        .bind(id).execute(&mut **tx).await?;
    Ok(task)
}

/// Queue the scheduled backup (target `backup`); an active run absorbs duplicates.
pub(crate) async fn enqueue_backup(
    store: &IdentityStore,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let task = enqueue_in(
        &mut tx,
        "backup",
        "system",
        &format!("system:backup:{key}"),
        now,
        max_runtime_seconds,
    )
    .await?;
    commit(tx).await?;
    task_engine::WAKE.notify_one();
    Ok(task)
}

pub(crate) async fn enqueue_manual(
    store: &IdentityStore,
    id: &str,
    owner: &str,
    now: i64,
) -> Result<String, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let task = enqueue_in(
        &mut tx,
        id,
        owner,
        &format!("system-manual:{}", uuid::Uuid::new_v4()),
        now,
        None,
    )
    .await?;
    commit(tx).await?;
    task_engine::WAKE.notify_one();
    Ok(task)
}

/// Queue a system task fired by a non-periodic trigger (server startup).
pub(crate) async fn enqueue_triggered(
    store: &IdentityStore,
    id: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<String, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let exists: i64 = query_scalar("SELECT COUNT(*) FROM system_task_schedules WHERE id=?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if exists == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    let task = enqueue_in(
        &mut tx,
        id,
        "system",
        &format!("system:{id}:{key}"),
        now,
        max_runtime_seconds,
    )
    .await?;
    commit(tx).await?;
    task_engine::WAKE.notify_one();
    Ok(task)
}

/// Schedule advancement and queue insertion share one transaction. The next due instant is the
/// earliest next firing of the task's triggers (daily, weekly or interval).
pub(crate) async fn pump(store: &IdentityStore, now: i64) -> Result<(), sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    let due: Vec<(String, i64, i64)> = query_as("SELECT id,interval_minutes,next_due_at FROM system_task_schedules WHERE enabled=1 AND next_due_at<=? ORDER BY next_due_at,id LIMIT 5")
        .bind(now).fetch_all(&mut *tx).await?;
    for (id, interval, previous) in due {
        let advance = crate::task_triggers::advance(&mut tx, "system", &id, now, interval).await?;
        enqueue_in(
            &mut tx,
            &id,
            "system",
            &format!("system:{id}:{previous}"),
            now,
            advance.max_runtime_seconds,
        )
        .await?;
        query("UPDATE system_task_schedules SET next_due_at=?,enabled=? WHERE id=? AND next_due_at=? AND enabled=1")
            .bind(advance.next.unwrap_or(previous)).bind(i64::from(advance.next.is_some())).bind(id).bind(previous)
            .execute(&mut *tx).await?;
    }
    commit(tx).await
}

/// One leased Core operation through the existing driver's bounded dispatch pass.
pub(crate) async fn dispatch_once(
    auth: &AuthState,
    root: &Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    pump(&auth.store, timestamp).await?;
    if jobs.is_cancelled()
        || crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed) > 0
    {
        return Ok(());
    }
    let task = {
        let _writer = auth.store.task_writes.lock().await;
        let mut conn = auth.store.pool().acquire().await?;
        let mut tx = begin_write(&mut conn).await?;
        let task: Option<(String, String)> = query_as("SELECT id,target_id FROM durable_tasks WHERE category='system' AND status='queued' AND lease_until IS NULL AND available_at<=? AND failure_count<max_attempts AND NOT EXISTS(SELECT 1 FROM durable_tasks ready WHERE ready.node_id='local' AND ready.category!='system' AND ready.status='queued' AND ready.lease_until IS NULL AND ready.available_at<=? AND ready.priority>10) AND NOT EXISTS(SELECT 1 FROM durable_tasks busy WHERE busy.node_id='local' AND busy.lease_until IS NOT NULL) ORDER BY priority DESC,created_at,id LIMIT 1")
            .bind(timestamp).bind(timestamp).fetch_optional(&mut *tx).await?;
        if let Some((id, target)) = &task {
            query("UPDATE durable_tasks SET status='running',attempt=attempt+1,lease_until=?,started_at=COALESCE(started_at,?),updated_at=?,finished_at=NULL,error='' WHERE id=? AND status='queued' AND lease_until IS NULL")
                .bind(timestamp.saturating_add(60)).bind(timestamp).bind(timestamp).bind(id)
                .execute(&mut *tx).await?;
            query("UPDATE system_task_schedules SET last_outcome='queued' WHERE id=?")
                .bind(target)
                .execute(&mut *tx)
                .await?;
        }
        commit(tx).await?;
        task
    };
    let Some((id, _)) = task else {
        return Ok(());
    };
    let Some(job) = task_engine::get(&auth.store, &id).await? else {
        return Ok(());
    };
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
                        if stopping.is_cancelled() || crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed)>0 {
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
