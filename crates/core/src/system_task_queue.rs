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
    database::{begin_write_scoped, commit, query, query_as, query_scalar},
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
/// Returns the run's id and whether it was inserted (`false`: an existing run absorbed it).
async fn enqueue_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    id: &str,
    owner: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<(String, bool), sqlx::Error> {
    let existing: Option<String> = query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id=? AND status IN ('queued','running','paused') ORDER BY created_at,id LIMIT 1")
        .bind(id).fetch_optional(&mut **tx).await?;
    if let Some(existing) = existing {
        return Ok((existing, false));
    }
    // A request key identifies one request for its whole lifetime (UNIQUE(owner,key)).
    let replayed: Option<String> =
        query_scalar("SELECT id FROM durable_tasks WHERE owner=? AND idempotency_key=?")
            .bind(owner)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(replayed) = replayed {
        return Ok((replayed, false));
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
    Ok((task, true))
}

/// Queue one run of system task `id` (see [`enqueue_in`]). `task_writes` orders the writers of
/// this process; the per-target lock makes "active run? request key used? else insert" atomic
/// across Cores sharing a PostgreSQL database (SQLite: `BEGIN IMMEDIATE` already serializes).
async fn enqueue_with_owner(
    store: &IdentityStore,
    id: &str,
    owner: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<(String, bool), sqlx::Error> {
    let enqueued = {
        let _writer = store.task_writes.lock().await;
        let mut conn = store.pool().acquire().await?;
        let mut tx = begin_write_scoped(&mut conn, "system-task-enqueue", id).await?;
        match enqueue_in(&mut tx, id, owner, key, now, max_runtime_seconds).await {
            Ok(enqueued) => {
                commit(tx).await?;
                enqueued
            }
            // The same request key raced under another target's lock: that request exists.
            Err(error)
                if error
                    .as_database_error()
                    .is_some_and(|database| database.is_unique_violation()) =>
            {
                tx.rollback().await?;
                let existing: Option<String> = query_scalar(
                    "SELECT id FROM durable_tasks WHERE owner=? AND idempotency_key=?",
                )
                .bind(owner)
                .bind(key)
                .fetch_optional(&mut *conn)
                .await?;
                (existing.ok_or(error)?, false)
            }
            Err(error) => return Err(error),
        }
    };
    task_engine::WAKE.notify_one();
    Ok(enqueued)
}

/// Queue the scheduled backup (target `backup`); an active run absorbs duplicates.
/// Returns the run's id and whether a new run was inserted.
pub(crate) async fn enqueue_backup(
    store: &IdentityStore,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<(String, bool), sqlx::Error> {
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
) -> Result<(String, bool), sqlx::Error> {
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
/// Returns the run's id and whether a new run was inserted.
pub(crate) async fn enqueue(
    store: &IdentityStore,
    id: &str,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<(String, bool), sqlx::Error> {
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
    let candidates: Vec<(String, String, i64)> = query_as("SELECT id,target_id,created_at \
            FROM durable_tasks \
            WHERE category='system' AND status='queued' AND lease_until IS NULL AND available_at<=? \
            AND failure_count<max_attempts AND NOT EXISTS(SELECT 1 FROM durable_tasks active \
            WHERE active.category='system' AND active.lease_until IS NOT NULL) \
            ORDER BY priority DESC,created_at,id \
            LIMIT 16")
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
    Ok(lease(store, &id, timestamp).await?.then_some(id))
}

/// Lease run `id` to this Core unless any maintenance run is already leased (by any Core).
async fn lease(store: &IdentityStore, id: &str, timestamp: i64) -> Result<bool, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    // The single exclusion point of maintenance runs. One lock for every system claim: on
    // PostgreSQL two Cores could otherwise both pass NOT EXISTS(active lease) and lease two runs.
    let mut tx = begin_write_scoped(&mut conn, "system-task-claim", "global").await?;
    let claimed = query("UPDATE durable_tasks \
            SET status='running',attempt=attempt+1,lease_until=?,lease_owner=?,started_at=COALESCE(started_at,?),updated_at=?,finished_at=NULL,error='' \
            WHERE id=? AND status='queued' AND lease_until IS NULL AND NOT EXISTS(SELECT 1 \
            FROM durable_tasks active WHERE active.category='system' \
            AND active.lease_until IS NOT NULL)")
        .bind(timestamp.saturating_add(60)).bind(store.instance_id()).bind(timestamp).bind(timestamp).bind(id)
        .execute(&mut *tx).await?
        .rows_affected();
    commit(tx).await?;
    Ok(claimed == 1)
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
                    result = &mut operation => break result.map_err(|code| match code {
                        axum::http::StatusCode::UNPROCESSABLE_ENTITY => PERMANENT_FAILURE,
                        // Exclusion busy (another backup running): not a failure of this run.
                        axum::http::StatusCode::LOCKED => task_engine::DEFERRED_BUSY,
                        _ => "system_task_failed",
                    }),
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
        // A deferred run never started: the backup that holds the exclusion is still running.
        if error.is_some_and(|e| e != task_engine::DEFERRED_BUSY) && job.target_id == "backup" {
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
            Ok(true) if error.is_some_and(|e| e != PERMANENT_FAILURE && e != task_engine::DEFERRED_BUSY) => {
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

    async fn count(store: &IdentityStore, sql: &'static str) -> i64 {
        query_scalar(sql).fetch_one(store.pool()).await.unwrap()
    }

    const ACTIVE: &str = "SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND status IN ('queued','running','paused')";
    const LEASED: &str =
        "SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND lease_until IS NOT NULL";

    #[tokio::test]
    async fn an_active_run_or_a_used_key_absorbs_another_enqueue() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("queue.sqlite3"))
            .await
            .unwrap();
        let (first, inserted) = enqueue(&store, "clean-cache", "a", 100, None)
            .await
            .unwrap();
        assert!(inserted);
        let (second, inserted) = enqueue(&store, "clean-cache", "b", 101, None)
            .await
            .unwrap();
        assert_eq!((second.as_str(), inserted), (first.as_str(), false));
        // Concurrent requests with different keys: one row, both callers get its id.
        let (x, y) = tokio::join!(
            enqueue(&store, "clean-sessions", "c", 102, None),
            enqueue(&store, "clean-sessions", "d", 102, None)
        );
        let (x, y) = (x.unwrap(), y.unwrap());
        assert_eq!(x.0, y.0);
        assert!(x.1 != y.1, "exactly one insert: {x:?} {y:?}");
        assert_eq!(count(&store, ACTIVE).await, 2);
        // Once the run ended, a replayed key still names it; a new key queues a new run.
        query("UPDATE durable_tasks SET status='completed' WHERE id=?")
            .bind(&first)
            .execute(store.pool())
            .await
            .unwrap();
        let (replayed, inserted) = enqueue(&store, "clean-cache", "a", 103, None)
            .await
            .unwrap();
        assert_eq!((replayed.as_str(), inserted), (first.as_str(), false));
        let (third, inserted) = enqueue(&store, "clean-cache", "e", 104, None)
            .await
            .unwrap();
        assert!(inserted && third != first);
        // The backup trigger learns from the locked enqueue whether it queued a run.
        assert!(
            crate::admin_backups::trigger_backup(&store, "k1", 105, None)
                .await
                .unwrap()
        );
        assert!(
            !crate::admin_backups::trigger_backup(&store, "k2", 106, None)
                .await
                .unwrap()
        );
        assert_eq!(count(&store, ACTIVE).await, 3);
        store.close().await;
    }

    #[tokio::test]
    async fn one_maintenance_run_is_leased_at_a_time() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("claim.sqlite3"))
            .await
            .unwrap();
        let (cache, _) = enqueue(&store, "clean-cache", "a", 100, None)
            .await
            .unwrap();
        let (sessions, _) = enqueue(&store, "clean-sessions", "b", 100, None)
            .await
            .unwrap();
        let (x, y) = tokio::join!(claim(&store, 200), claim(&store, 200));
        let claimed: Vec<String> = [x.unwrap(), y.unwrap()].into_iter().flatten().collect();
        assert_eq!(claimed.len(), 1, "{claimed:?}");
        let other = if claimed[0] == cache {
            &sessions
        } else {
            &cache
        };
        // Leasing the other queued run directly is refused too.
        assert!(!lease(&store, other, 200).await.unwrap());
        assert_eq!(count(&store, LEASED).await, 1);
        let owner: Option<String> =
            query_scalar("SELECT lease_owner FROM durable_tasks WHERE id=?")
                .bind(&claimed[0])
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(owner.as_deref(), Some(store.instance_id()));
        query("UPDATE durable_tasks SET status='completed',lease_until=NULL,lease_owner=NULL WHERE id=?")
            .bind(&claimed[0])
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(claim(&store, 201).await.unwrap().as_ref(), Some(other));
        store.close().await;
    }

    #[tokio::test]
    #[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
    async fn postgres_two_cores_never_queue_or_lease_twice() {
        let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
        let pool = sqlx::PgPool::connect(&connection).await.unwrap();
        let schema = format!("system_queue_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&pool)
            .await
            .unwrap();
        let mut url = url::Url::parse(&connection).unwrap();
        url.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        // Two stores on one database are two Cores.
        let a = IdentityStore::open_postgres(url.as_str()).await.unwrap();
        let b = IdentityStore::open_postgres(url.as_str()).await.unwrap();
        let now = media_contracts::protocol::unix_now();

        // Both Cores queue the same task at once, with different request keys.
        for round in 0..20 {
            let target = format!("race-{round}");
            let (x, y) = tokio::join!(
                enqueue(&a, &target, "from-a", now, None),
                enqueue(&b, &target, "from-b", now, None)
            );
            let (x, y) = (x.unwrap(), y.unwrap());
            assert_eq!(x.0, y.0, "round {round}");
            assert!(x.1 != y.1, "round {round}: exactly one insert");
        }
        assert_eq!(count(&a, ACTIVE).await, 20);
        // One request key raced on two targets (two different enqueue locks): no error, one row.
        let (x, y) = tokio::join!(
            enqueue_with_owner(&a, "key-a", "system", "shared-key", now, None),
            enqueue_with_owner(&b, "key-b", "system", "shared-key", now, None)
        );
        let (x, y) = (x.unwrap(), y.unwrap());
        assert_eq!(x.0, y.0);
        assert!(x.1 != y.1, "exactly one insert: {x:?} {y:?}");
        assert_eq!(count(&a, ACTIVE).await, 21);

        // Both Cores lease a different queued run at once: never two system leases.
        for round in 0..10 {
            let queued: Vec<String> = query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND status='queued' ORDER BY id LIMIT 2")
                .fetch_all(a.pool())
                .await
                .unwrap();
            let (x, y) = tokio::join!(lease(&a, &queued[0], now), lease(&b, &queued[1], now));
            let leased = [x.unwrap(), y.unwrap()];
            assert_eq!(
                leased.iter().filter(|leased| **leased).count(),
                1,
                "round {round}: {leased:?}"
            );
            assert_eq!(count(&a, LEASED).await, 1, "round {round}");
            query("UPDATE durable_tasks SET status='completed',lease_until=NULL,lease_owner=NULL WHERE category='system' AND lease_until IS NOT NULL")
                .execute(a.pool())
                .await
                .unwrap();
        }

        // A due trigger seen by both Cores' scheduling passes queues one run.
        let auth_a = AuthState::new(a.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let auth_b = AuthState::new(b.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES('hourly','backup','backup','interval',60,1,0,0)")
            .execute(a.pool())
            .await
            .unwrap();
        let (x, y) = tokio::join!(
            crate::task_scheduler::fire_due(&auth_a, now),
            crate::task_scheduler::fire_due(&auth_b, now)
        );
        assert_eq!(x.unwrap() + y.unwrap(), 1);
        assert_eq!(
            count(
                &a,
                "SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id='backup'"
            )
            .await,
            1
        );

        drop((auth_a, auth_b));
        a.close().await;
        b.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}
