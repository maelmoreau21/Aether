//! Attempt fencing of scan effects across Cores (multi-Core roadmap, step 5).
//!
//! A scan attempt is named by its run fence: `durable_tasks.id:attempt` (a legacy run has a bare
//! id and is fenced by `active_scan_runs` alone). Each durable effect of an attempt (a staged
//! batch, the publication, its final status) is written in a transaction that first re-verifies,
//! **inside that transaction**, that the attempt still owns the scan:
//!
//! - on PostgreSQL the transaction takes an advisory lock per library ([`SCOPE`]), so two Cores
//!   never stage, publish or report one library at the same time; then it locks the attempt's
//!   `durable_tasks` row with a no-op update. A claim, an expiry or a cancellation committed
//!   meanwhile is seen after that lock (READ COMMITTED re-evaluates the condition on the newest
//!   row), and none can commit before this transaction ends. A publication is long: it checks
//!   without locking at its start, and takes that row lock only just before its commit
//!   ([`confirm`]), so lease renewals keep running while it writes;
//! - on SQLite `BEGIN IMMEDIATE` already serializes every writer; the check is the same.
//!
//! The publication also records its attempt in `scan_jobs.published_scan_id` (schema v7): a
//! completion replayed by a Node, or delivered to two Cores, finds it and publishes nothing,
//! instead of publishing an emptied staging area (which would mark every asset unavailable).
//!
//! `task_engine::EFFECT_GATE` stays as in-process ordering with control actions; correctness
//! between Cores relies only on the database.
use crate::database::{begin_write_scoped, query, query_scalar};
use crate::identity::IdentityStore;
use sqlx::{Any, AnyConnection, Transaction};

/// Advisory lock scope (with the library id) of every fenced scan effect.
pub(crate) const SCOPE: &str = "scan-publish";

/// The attempt is still the library's current run, on its current Node, and neither the
/// library nor the Node was disabled or revoked. Binds: library, node, scan id.
pub(crate) const ACTIVE_RUN_SQL: &str = "SELECT COUNT(*) \
    FROM active_scan_runs r \
    JOIN libraries l ON l.id=r.library_id \
    JOIN media_nodes n ON n.id=l.node_id \
    WHERE r.library_id=? AND r.node_id=? AND r.scan_id=? AND ((l.enabled=1 \
    AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) \
    OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent \
    ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) \
    AND l.node_id=r.node_id AND n.status!='revoked'";

/// The attempt's run fence is still in place: what the final status of a job requires (a library
/// disabled or a Node revoked meanwhile does not hide the attempt's real outcome). Binds:
/// library, node, scan id.
const RUN_FENCE_SQL: &str = "SELECT COUNT(*) FROM active_scan_runs r \
    WHERE r.library_id=? AND r.node_id=? AND r.scan_id=?";

/// Same condition as [`LIVE_ATTEMPT_SQL`], without locking the row: the start of a publication,
/// which then [`confirm`]s it before committing. Binds: as [`LIVE_ATTEMPT_SQL`].
const LIVE_ATTEMPT_PEEK_SQL: &str = "SELECT COUNT(*) FROM durable_tasks \
    WHERE id=? AND attempt=? AND node_id=? AND library_id=? AND status='running' \
    AND lease_until>? AND (node_id<>'local' OR lease_owner IS NULL OR lease_owner=?)";

/// Running attempt with a live lease, owned by this Core when it runs locally. Locks the row on
/// PostgreSQL. Binds: id, attempt, node, library, now, this Core's instance id.
const LIVE_ATTEMPT_SQL: &str = "UPDATE durable_tasks SET lease_until=lease_until \
    WHERE id=? AND attempt=? AND node_id=? AND library_id=? AND status='running' \
    AND lease_until>? AND (node_id<>'local' OR lease_owner IS NULL OR lease_owner=?)";

/// Current attempt of its task, whatever its status (an attempt stopped by an administrator still
/// reports why it stopped), not owned by another Core. Binds: id, attempt, node, library, this
/// Core's instance id.
const CURRENT_ATTEMPT_SQL: &str = "UPDATE durable_tasks SET lease_until=lease_until \
    WHERE id=? AND attempt=? AND node_id=? AND library_id=? \
    AND (node_id<>'local' OR lease_owner IS NULL OR lease_owner=?)";

/// One scan attempt, as named by the executor that produced its effects.
#[derive(Clone, Copy, Debug)]
pub struct ScanAttempt<'a> {
    pub library_id: &'a str,
    pub node_id: &'a str,
    pub scan_id: &'a str,
}

/// Effect about to be written; it decides what "still owns the scan" requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// A staged batch: running attempt, live lease, not yet published.
    Stage,
    /// The publication: running attempt, live lease.
    Publish,
    /// The final status of the library job: the attempt is still the current one. Its lease may
    /// have ended (an expired attempt reports its own error); a newer attempt never loses its
    /// status to an older one.
    Report,
}

/// Result of the in-transaction check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hold {
    Held,
    /// The attempt still owns the scan and already published it.
    Published,
    Lost,
}

/// Outcome of a fenced publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publication {
    Published,
    /// This attempt was already published (replayed completion): nothing written.
    AlreadyPublished,
    /// The attempt no longer owns the scan (lease expired, superseded, cancelled, another
    /// Core's lease, library or Node changed): nothing written.
    Fenced,
}

/// Start the write transaction of a fenced effect and check, inside it, that the attempt still
/// owns the scan. The caller writes only on [`Hold::Held`] (or [`Hold::Published`] where
/// meaningful) and otherwise drops or rolls back the transaction.
pub(crate) async fn begin<'c>(
    conn: &'c mut AnyConnection,
    attempt: ScanAttempt<'_>,
    effect: Effect,
    instance: &str,
    now: i64,
) -> Result<(Transaction<'c, Any>, Hold), sqlx::Error> {
    let mut tx = begin_write_scoped(conn, SCOPE, attempt.library_id).await?;
    let hold = check(&mut tx, attempt, effect, instance, now).await?;
    Ok((tx, hold))
}

async fn check(
    conn: &mut AnyConnection,
    attempt: ScanAttempt<'_>,
    effect: Effect,
    instance: &str,
    now: i64,
) -> Result<Hold, sqlx::Error> {
    if let Some((id, number)) = attempt.scan_id.split_once(':') {
        let Ok(number) = number.parse::<i64>() else {
            return Ok(Hold::Lost);
        };
        let held = match effect {
            Effect::Report => query(CURRENT_ATTEMPT_SQL)
                .bind(id)
                .bind(number)
                .bind(attempt.node_id)
                .bind(attempt.library_id)
                .bind(instance)
                .execute(&mut *conn)
                .await?
                .rows_affected(),
            Effect::Stage => live_attempt(conn, id, number, attempt, instance, now).await?,
            // No row lock for the whole publication: renewals of this very lease must go on
            // while it writes. The locking check is `confirm`, just before the commit.
            Effect::Publish => u64::try_from(
                query_scalar::<_, i64>(LIVE_ATTEMPT_PEEK_SQL)
                    .bind(id)
                    .bind(number)
                    .bind(attempt.node_id)
                    .bind(attempt.library_id)
                    .bind(now)
                    .bind(instance)
                    .fetch_one(&mut *conn)
                    .await?,
            )
            .unwrap_or(0),
        };
        if held != 1 {
            return Ok(Hold::Lost);
        }
    }
    let active: i64 = query_scalar(if effect == Effect::Report {
        RUN_FENCE_SQL
    } else {
        ACTIVE_RUN_SQL
    })
    .bind(attempt.library_id)
    .bind(attempt.node_id)
    .bind(attempt.scan_id)
    .fetch_one(&mut *conn)
    .await?;
    if active != 1 {
        return Ok(Hold::Lost);
    }
    // Every scan run has its library job row (`scans::enqueue_one`); without it there is no
    // current scan to publish nor to report.
    let published: Option<Option<String>> =
        query_scalar("SELECT published_scan_id FROM scan_jobs WHERE library_id=?")
            .bind(attempt.library_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(match published {
        None => Hold::Lost,
        Some(Some(scan)) if scan == attempt.scan_id => match effect {
            Effect::Stage => Hold::Lost,
            Effect::Publish | Effect::Report => Hold::Published,
        },
        Some(_) => Hold::Held,
    })
}

async fn live_attempt(
    conn: &mut AnyConnection,
    id: &str,
    number: i64,
    attempt: ScanAttempt<'_>,
    instance: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    Ok(query(LIVE_ATTEMPT_SQL)
        .bind(id)
        .bind(number)
        .bind(attempt.node_id)
        .bind(attempt.library_id)
        .bind(now)
        .bind(instance)
        .execute(&mut *conn)
        .await?
        .rows_affected())
}

/// Last check of a publication, just before its commit: locks the attempt's `durable_tasks`
/// row (PostgreSQL) and re-verifies its live lease at the current time. From here to the commit
/// no claim, expiry or cancellation of that attempt can commit; if it was lost meanwhile the
/// caller rolls the whole publication back. Legacy runs (bare id) have no lease to confirm.
pub(crate) async fn confirm(
    conn: &mut AnyConnection,
    attempt: ScanAttempt<'_>,
    instance: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let Some((id, number)) = attempt.scan_id.split_once(':') else {
        return Ok(true);
    };
    let Ok(number) = number.parse::<i64>() else {
        return Ok(false);
    };
    Ok(live_attempt(conn, id, number, attempt, instance, now).await? == 1)
}

/// Record, in the publication transaction, that this attempt published its library.
pub(crate) async fn mark_published(
    conn: &mut AnyConnection,
    attempt: ScanAttempt<'_>,
) -> Result<(), sqlx::Error> {
    query("UPDATE scan_jobs SET published_scan_id=? WHERE library_id=?")
        .bind(attempt.scan_id)
        .bind(attempt.library_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Final status of a scan, written to its library job only while the attempt is current.
#[derive(Clone, Copy, Debug)]
pub(crate) enum JobReport<'a> {
    /// `status='error'` with one message; `reset_warnings` also clears the warning count.
    Failed {
        errors_json: &'a str,
        reset_warnings: bool,
    },
    /// Counters and issues of a finished films/series scan.
    Finished {
        status: &'a str,
        discovered: i64,
        probed: i64,
        reused: i64,
        errors_json: &'a str,
        errors_count: i64,
        warnings_count: i64,
    },
    /// A completed music scan (tracks are published one by one).
    Counted { discovered: i64, probed: i64 },
}

/// Write `report` to the attempt's library job in a fenced transaction ([`Effect::Report`]).
/// Returns whether it was written.
pub(crate) async fn report(
    store: &IdentityStore,
    attempt: ScanAttempt<'_>,
    report: JobReport<'_>,
) -> Result<bool, sqlx::Error> {
    let mut conn = store.pool.acquire().await?;
    let (mut tx, hold) = begin(
        &mut conn,
        attempt,
        Effect::Report,
        store.instance_id(),
        media_contracts::protocol::unix_now(),
    )
    .await?;
    if hold == Hold::Lost {
        tx.rollback().await?;
        return Ok(false);
    }
    let library = attempt.library_id;
    let update = match report {
        JobReport::Failed {
            errors_json,
            reset_warnings: false,
        } => query(
            "UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1 WHERE library_id=?",
        )
        .bind(errors_json)
        .bind(library),
        JobReport::Failed {
            errors_json,
            reset_warnings: true,
        } => query(
            "UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 \
                WHERE library_id=?",
        )
        .bind(errors_json)
        .bind(library),
        JobReport::Finished {
            status,
            discovered,
            probed,
            reused,
            errors_json,
            errors_count,
            warnings_count,
        } => query(
            "UPDATE scan_jobs SET status=?,discovered=?,probed=?,reused=?,errors_json=?,\
                errors_count=?,warnings_count=? WHERE library_id=?",
        )
        .bind(status)
        .bind(discovered)
        .bind(probed)
        .bind(reused)
        .bind(errors_json)
        .bind(errors_count)
        .bind(warnings_count)
        .bind(library),
        JobReport::Counted { discovered, probed } => query(
            "UPDATE scan_jobs SET status='completed',discovered=?,probed=?,reused=0 \
                WHERE library_id=?",
        )
        .bind(discovered)
        .bind(probed)
        .bind(library),
    };
    update.execute(&mut *tx).await?;
    crate::database::commit(tx).await?;
    Ok(true)
}

/// Test-only hook that stops a publication midway, after its inventory writes and before its
/// commit, to prove that nothing of it survives.
#[cfg(test)]
pub(crate) mod failpoint {
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Action {
        /// The publication returns an error (its transaction is rolled back).
        Fail,
        /// The publication never returns: the test kills its connection or drops its future.
        Hang,
    }

    #[derive(Default)]
    pub(crate) struct Reached {
        pub notify: tokio::sync::Notify,
        /// `pg_backend_pid()` of the publication's connection (PostgreSQL only).
        pub backend: Mutex<Option<i64>>,
    }

    type Armed = (String, Action, Arc<Reached>);
    static ARMED: Mutex<Vec<Armed>> = Mutex::new(Vec::new());

    /// Arm the hook for one scan id only (tests run in parallel in one process).
    pub(crate) fn arm(scan_id: &str, action: Action) -> Arc<Reached> {
        let reached = Arc::new(Reached::default());
        ARMED
            .lock()
            .unwrap()
            .push((scan_id.to_owned(), action, reached.clone()));
        reached
    }

    pub(crate) async fn hit(
        conn: &mut sqlx::AnyConnection,
        scan_id: &str,
    ) -> Result<(), sqlx::Error> {
        let armed = {
            let mut list = ARMED.lock().unwrap();
            list.iter()
                .position(|(scan, _, _)| scan == scan_id)
                .map(|index| list.swap_remove(index))
        };
        let Some((_, action, reached)) = armed else {
            return Ok(());
        };
        if conn.backend_name() == "PostgreSQL" {
            let pid: i64 = crate::database::query_scalar("SELECT CAST(pg_backend_pid() AS BIGINT)")
                .fetch_one(&mut *conn)
                .await?;
            *reached.backend.lock().unwrap() = Some(pid);
        }
        reached.notify.notify_one();
        match action {
            Action::Fail => Err(sqlx::Error::Protocol(
                "failpoint: publication aborted".into(),
            )),
            Action::Hang => std::future::pending().await,
        }
    }
}
