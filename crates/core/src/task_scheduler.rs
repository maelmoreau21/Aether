//! The scheduler: turns due triggers into durable runs.
//!
//! `task_triggers` is the only scheduling configuration. One pass ([`fire_due`]) reads every
//! enabled trigger whose instant has come, admits one run per task into the durable queue, and
//! then advances exactly the triggers that fired (compare-and-set on `next_fire_at`, so several
//! passes or Core processes never fire one instant twice). A task that is already queued or
//! running when its trigger fires is skipped, like Jellyfin, and a missed instant after downtime
//! fires once, never as a burst of catch-up runs.
//!
//! The legacy `next_due_at` / `next_run_at` / `enabled` columns are written as a read-only mirror
//! for the older endpoints; nothing reads them to decide a run any more.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::IdentityStore,
    task_triggers::{TriggerRow, TriggerSpec, next_fire, server_time_zone},
};
use sqlx::AnyConnection;

/// One schedulable task, as named by the unified API (`backup`, `scan-<library>`, `<system id>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskRef {
    System(String),
    Backup,
    Scan(String),
    /// A media sub-task of a library (previews, intros, images): run by hand or after a scan,
    /// never by a trigger of its own.
    Media(crate::library_media_tasks::MediaTask, String),
}

impl TaskRef {
    /// Public task identifier → task.
    pub fn parse(id: &str) -> Self {
        if id == "backup" {
            Self::Backup
        } else if let Some(library) = id.strip_prefix("scan-") {
            Self::Scan(library.to_owned())
        } else if let Some((task, library)) = crate::library_media_tasks::MediaTask::parse_id(id) {
            Self::Media(task, library)
        } else {
            Self::System(id.to_owned())
        }
    }
    /// `(task_kind, task_id)` columns of `task_triggers` → task.
    pub fn from_columns(kind: &str, id: &str) -> Option<Self> {
        match kind {
            "system" => Some(Self::System(id.to_owned())),
            "backup" => Some(Self::Backup),
            "scan" => Some(Self::Scan(id.to_owned())),
            _ => None,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::System(_) => "system",
            Self::Backup => "backup",
            Self::Scan(_) => "scan",
            Self::Media(task, _) => task.key(),
        }
    }
    pub fn target(&self) -> &str {
        match self {
            Self::System(id) | Self::Scan(id) | Self::Media(_, id) => id,
            Self::Backup => "backup",
        }
    }
    /// Shortest interval a trigger of this task may use, in minutes.
    pub fn min_interval(&self) -> i64 {
        if *self == Self::Backup { 60 } else { 5 }
    }
}

/// What happened when a run was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// A new run is waiting in the durable queue.
    Queued,
    /// A run of this task is already queued or running; nothing was added.
    AlreadyActive,
    /// The task cannot run now (unknown task, disabled library, revoked Node).
    Unavailable,
}

/// Library scans count as active only for real scans: real-time monitoring polls (owner
/// `library-monitor`) are background housekeeping and never block a scheduled or manual scan.
const ACTIVE_SCAN: &str = "SELECT COUNT(*) FROM durable_tasks WHERE category='scan' AND owner<>'library-monitor' AND status IN ('queued','running','paused') AND (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?))";

/// Request one run of `task`. `key` identifies the request: admitting the same key twice never
/// queues twice, so a pass interrupted before advancing its triggers can safely run again.
pub(crate) async fn admit(
    auth: &AuthState,
    task: &TaskRef,
    key: &str,
    now: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<Admission, sqlx::Error> {
    let pool = auth.store.pool();
    match task {
        TaskRef::System(id) => {
            let known: i64 = query_scalar("SELECT COUNT(*) FROM system_task_schedules WHERE id=?")
                .bind(id)
                .fetch_one(pool)
                .await?;
            if known == 0 {
                return Ok(Admission::Unavailable);
            }
            let active: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id=? AND status IN ('queued','running','paused')")
                .bind(id).fetch_one(pool).await?;
            if active > 0 {
                return Ok(Admission::AlreadyActive);
            }
            crate::system_task_queue::enqueue(&auth.store, id, key, now, max_runtime_seconds)
                .await?;
            Ok(Admission::Queued)
        }
        TaskRef::Backup => {
            if crate::admin_backups::trigger_backup(&auth.store, key, now, max_runtime_seconds)
                .await?
            {
                Ok(Admission::Queued)
            } else {
                Ok(Admission::AlreadyActive)
            }
        }
        TaskRef::Scan(library) => {
            let available: i64 = query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films','series','music') AND l.id NOT IN ('fixture-films','fixture-series') AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id)")
                .bind(library).fetch_one(pool).await?;
            if available == 0 {
                return Ok(Admission::Unavailable);
            }
            let active: i64 = query_scalar(ACTIVE_SCAN)
                .bind(library)
                .bind(library)
                .fetch_one(pool)
                .await?;
            let legacy_running: i64 = query_scalar(
                "SELECT COUNT(*) FROM scan_jobs WHERE library_id=? AND status IN ('queued','running')",
            )
            .bind(library)
            .fetch_one(pool)
            .await?;
            if active > 0 || legacy_running > 0 {
                return Ok(Admission::AlreadyActive);
            }
            crate::scheduler::queue_library_scan(auth, library, key, now).await?;
            Ok(Admission::Queued)
        }
        // Media sub-tasks carry no trigger: they run after scans or on an administrator's request
        // (`library_media_tasks::admit`), which needs the administrator's identity.
        TaskRef::Media(..) => Ok(Admission::Unavailable),
    }
}

/// Idempotency key of the run fired by a trigger instant. Scan keys keep the historical
/// `schedule:<library>:<instant>` shape that the legacy outcome reconciliation reads.
fn fire_key(task: &TaskRef, instant: i64) -> String {
    match task {
        TaskRef::Scan(library) => format!("schedule:{library}:{instant}"),
        _ => format!("trigger:{instant}"),
    }
}

/// Most due triggers handled in one pass; the rest wait for the next pass (seconds later).
const FIRE_BATCH: i64 = 64;

/// One scheduling pass. Returns the number of runs queued.
pub async fn fire_due(auth: &AuthState, now: i64) -> Result<usize, sqlx::Error> {
    repair(&auth.store, now).await?;
    let due: Vec<TriggerRow> = query_as("SELECT id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at FROM task_triggers WHERE enabled=1 AND trigger_type<>'startup' AND next_fire_at IS NOT NULL AND next_fire_at<=? ORDER BY next_fire_at,id LIMIT ?")
        .bind(now)
        .bind(FIRE_BATCH)
        .fetch_all(auth.store.pool())
        .await?;
    // Group by task, keeping the order of the earliest due instant.
    let mut groups: Vec<(String, String, Vec<TriggerRow>)> = Vec::new();
    for row in due {
        match groups
            .iter_mut()
            .find(|(kind, id, _)| *kind == row.task_kind && *id == row.task_id)
        {
            Some((.., rows)) => rows.push(row),
            None => groups.push((row.task_kind.clone(), row.task_id.clone(), vec![row])),
        }
    }
    let mut queued = 0;
    for (kind, id, rows) in groups {
        let instant = rows
            .iter()
            .filter_map(|row| row.next_fire_at)
            .min()
            .unwrap_or(now);
        // The run gets the largest time limit among the triggers that fired together.
        let limit = rows.iter().filter_map(|row| row.max_runtime_seconds).max();
        if let Some(task) = TaskRef::from_columns(&kind, &id) {
            match admit(auth, &task, &fire_key(&task, instant), now, limit).await {
                Ok(Admission::Queued) => {
                    queued += 1;
                    if let TaskRef::Scan(library) = &task {
                        record_scan_dispatch(&auth.store, library, &fire_key(&task, instant), now)
                            .await?;
                    }
                }
                Ok(Admission::AlreadyActive | Admission::Unavailable) => {}
                Err(error) => {
                    // Transient failure: the triggers stay due and the next pass retries.
                    eprintln!("scheduled task {kind}:{id} could not be queued: {error}");
                    continue;
                }
            }
        }
        advance(&auth.store, &kind, &id, &rows, now).await?;
    }
    Ok(queued)
}

/// Move each fired trigger to its next instant after `now`, unless another pass already did.
async fn advance(
    store: &IdentityStore,
    kind: &str,
    id: &str,
    rows: &[TriggerRow],
    now: i64,
) -> Result<(), sqlx::Error> {
    let zone = server_time_zone();
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    for row in rows {
        let next = row.spec().and_then(|spec| next_fire(&spec, now, &zone));
        query("UPDATE task_triggers SET next_fire_at=? WHERE id=? AND next_fire_at=?")
            .bind(next)
            .bind(&row.id)
            .bind(row.next_fire_at)
            .execute(&mut *tx)
            .await?;
    }
    mirror(&mut tx, kind, id).await?;
    commit(tx).await
}

/// Legacy scan plans report their last scheduled dispatch; the outcome is reconciled later from
/// the durable tasks that share the request key.
async fn record_scan_dispatch(
    store: &IdentityStore,
    library: &str,
    key: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    query("UPDATE scheduled_scans SET last_run_at=?,last_request_key=?,last_outcome='queued' WHERE library_id=?")
        .bind(now)
        .bind(key)
        .bind(library)
        .execute(store.pool())
        .await?;
    Ok(())
}

/// Keep the trigger table consistent: give every enabled timed trigger an instant and drop the
/// triggers of tasks that no longer exist (deleted library, retired system task).
async fn repair(store: &IdentityStore, now: i64) -> Result<(), sqlx::Error> {
    let missing: Vec<TriggerRow> = query_as("SELECT id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at FROM task_triggers WHERE enabled=1 AND trigger_type<>'startup' AND next_fire_at IS NULL")
        .fetch_all(store.pool())
        .await?;
    let orphans: i64 = query_scalar("SELECT COUNT(*) FROM task_triggers t WHERE (t.task_kind='scan' AND NOT EXISTS(SELECT 1 FROM libraries l WHERE l.id=t.task_id)) OR (t.task_kind='system' AND NOT EXISTS(SELECT 1 FROM system_task_schedules s WHERE s.id=t.task_id))")
        .fetch_one(store.pool())
        .await?;
    if missing.is_empty() && orphans == 0 {
        return Ok(());
    }
    let zone = server_time_zone();
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    for row in &missing {
        let next = row.spec().and_then(|spec| next_fire(&spec, now, &zone));
        query("UPDATE task_triggers SET next_fire_at=? WHERE id=? AND next_fire_at IS NULL")
            .bind(next)
            .bind(&row.id)
            .execute(&mut *tx)
            .await?;
        mirror(&mut tx, &row.task_kind, &row.task_id).await?;
    }
    if orphans > 0 {
        query("DELETE FROM task_triggers WHERE (task_kind='scan' AND NOT EXISTS(SELECT 1 FROM libraries l WHERE l.id=task_triggers.task_id)) OR (task_kind='system' AND NOT EXISTS(SELECT 1 FROM system_task_schedules s WHERE s.id=task_triggers.task_id))")
            .execute(&mut *tx)
            .await?;
    }
    commit(tx).await
}

/// Write the legacy "earliest next run" columns from the task's triggers (read-only mirror for
/// the older endpoints) and remember that the task's legacy interval is converted.
pub(crate) async fn mirror(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let rows = crate::task_triggers::load(conn, kind, id).await?;
    let mut earliest: Option<i64> = None;
    let mut first_interval = None;
    for row in rows.iter().filter(|row| row.timed()) {
        if first_interval.is_none() {
            first_interval = row.interval_minutes;
        }
        earliest = match (earliest, row.next_fire_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    let enabled = i64::from(earliest.is_some());
    match kind {
        "system" => {
            query("UPDATE system_task_schedules SET enabled=?,next_due_at=COALESCE(?,next_due_at),interval_minutes=COALESCE(?,interval_minutes) WHERE id=?")
                .bind(enabled).bind(earliest).bind(first_interval.filter(|m| (5..=10080).contains(m))).bind(id)
                .execute(&mut *conn).await?;
        }
        "backup" => {
            query("UPDATE backup_schedule SET enabled=?,next_due_at=COALESCE(?,next_due_at),interval_minutes=COALESCE(?,interval_minutes) WHERE singleton=1")
                .bind(enabled).bind(earliest).bind(first_interval.filter(|m| (60..=10080).contains(m)))
                .execute(&mut *conn).await?;
        }
        "scan" => {
            if let Some(next) = earliest {
                query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at) VALUES(?,?,1,?) ON CONFLICT(library_id) DO UPDATE SET enabled=1,next_run_at=excluded.next_run_at,interval_minutes=excluded.interval_minutes")
                    .bind(id).bind(first_interval.unwrap_or(1440).clamp(5, 10080)).bind(next)
                    .execute(&mut *conn).await?;
            } else {
                query("UPDATE scheduled_scans SET enabled=0 WHERE library_id=?")
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        _ => {}
    }
    query("INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at) VALUES(?,?,0) ON CONFLICT(task_kind,task_id) DO NOTHING")
        .bind(kind).bind(id).execute(&mut *conn).await?;
    Ok(earliest)
}

/// Give every system task that was never converted its legacy interval as a trigger. The SQL
/// migration does the same at start-up; this also covers tasks seeded later by Rust code.
pub(crate) async fn adopt_legacy_system_tasks(
    store: &IdentityStore,
    now: i64,
) -> Result<(), sqlx::Error> {
    let pending: Vec<(String, i64, i64, i64)> = query_as("SELECT s.id,s.interval_minutes,s.enabled,s.next_due_at FROM system_task_schedules s WHERE NOT EXISTS(SELECT 1 FROM task_trigger_migrations m WHERE m.task_kind='system' AND m.task_id=s.id)")
        .fetch_all(store.pool())
        .await?;
    if pending.is_empty() {
        return Ok(());
    }
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write(&mut conn).await?;
    for (id, interval, enabled, next_due) in pending {
        if enabled == 1 && TriggerSpec::interval(interval).normalized(5).is_ok() {
            query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES(?,'system',?,'interval',?,1,?,?) ON CONFLICT(id) DO NOTHING")
                .bind(format!("legacy-system-{id}")).bind(&id).bind(interval).bind(next_due.max(now)).bind(now)
                .execute(&mut *tx).await?;
        }
        query("INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at) VALUES('system',?,?) ON CONFLICT(task_kind,task_id) DO NOTHING")
            .bind(&id).bind(now).execute(&mut *tx).await?;
    }
    commit(tx).await
}

/// Queue, once per process start, every task carrying an enabled "server startup" trigger.
pub async fn fire_startup(auth: &AuthState, now: i64) -> Result<usize, sqlx::Error> {
    let rows: Vec<(String, String, Option<i64>)> = query_as("SELECT task_kind,task_id,MAX(max_runtime_seconds) FROM task_triggers WHERE trigger_type='startup' AND enabled=1 GROUP BY task_kind,task_id ORDER BY task_kind,task_id")
        .fetch_all(auth.store.pool()).await?;
    let mut fired = 0;
    for (kind, id, limit) in rows {
        let Some(task) = TaskRef::from_columns(&kind, &id) else {
            continue;
        };
        let key = match &task {
            TaskRef::Scan(library) => format!("startup:{library}:{now}"),
            _ => format!("startup:{now}"),
        };
        match admit(auth, &task, &key, now, limit).await {
            Ok(Admission::Queued) => fired += 1,
            Ok(_) => {}
            Err(error) => eprintln!("startup trigger for {kind}:{id} failed: {error}"),
        }
    }
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_references_round_trip() {
        assert_eq!(TaskRef::parse("backup"), TaskRef::Backup);
        assert_eq!(TaskRef::parse("scan-lib-1"), TaskRef::Scan("lib-1".into()));
        assert_eq!(
            TaskRef::parse("clean-cache"),
            TaskRef::System("clean-cache".into())
        );
        assert_eq!(TaskRef::parse("scan-lib-1").target(), "lib-1");
        assert_eq!(
            TaskRef::from_columns("scan", "lib-1"),
            Some(TaskRef::Scan("lib-1".into()))
        );
        assert_eq!(TaskRef::from_columns("other", "x"), None);
        assert_eq!(TaskRef::Backup.min_interval(), 60);
    }

    #[test]
    fn fire_keys_are_valid_queue_keys() {
        let key = fire_key(&TaskRef::Scan("films".into()), 1_700_000_000);
        assert_eq!(key, "schedule:films:1700000000");
        assert_eq!(fire_key(&TaskRef::Backup, 5), "trigger:5");
        assert!(
            key.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_:.".contains(&c))
        );
    }
}
