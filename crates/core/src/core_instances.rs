//! Core processes sharing one database (multi-Core, PostgreSQL only).
//!
//! Every opened store is one Core instance ([`IdentityStore::instance_id`]). It registers in
//! `core_instances` when its routes are built and refreshes `heartbeat_at` every
//! [`HEARTBEAT_INTERVAL`]. A Core silent for [`HEARTBEAT_STALE_SECONDS`] is considered dead:
//! another Core may then reclaim the local leases it owned (`durable_tasks.lease_owner`).
//! Startup cleanup is lease-based ([`reclaim_orphans`]) and never touches the work of a live Core.
use crate::{
    database::{query, query_as, query_scalar},
    identity::IdentityStore,
};
use std::{collections::HashSet, time::Duration};

/// A Core whose heartbeat is older than this (seconds) is dead.
pub const HEARTBEAT_STALE_SECONDS: i64 = 45;
/// Heartbeat refresh period of a running Core.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
/// Period of the lease-based orphan cleanup run by the scheduler worker.
pub(crate) const RECLAIM_INTERVAL: Duration = Duration::from_secs(60);
/// Rows of Cores silent for this long (seconds) are removed.
const PRUNE_AFTER_SECONDS: i64 = 86_400;
/// Reported on a scan whose executor can no longer run (same text as before multi-Core).
const INTERRUPTED_SCAN: &str = "[\"Scan interrompu par un redémarrage\"]";

/// Staging tables keyed by the scan fence (`durable_tasks.id || ':' || attempt`).
const STAGED_SCANS: [&str; 4] = [
    "SELECT DISTINCT scan_id FROM scan_batch_receipts",
    "SELECT DISTINCT scan_id FROM scan_staging_files",
    "SELECT DISTINCT scan_id FROM scan_staging_issues",
    "SELECT DISTINCT scan_id FROM scan_staging_fingerprints",
];

/// Host this Core runs on: `HOSTNAME` (set by Docker to the container id, stable across
/// restarts of the same container), else `/etc/hostname`, else `COMPUTERNAME`.
pub(crate) fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_default()
        .trim()
        .chars()
        .take(255)
        .collect()
}

/// SQL condition true while the Core whose id is in `column` is alive for this Core: its
/// heartbeat is fresh, and it is not this Core's own predecessor (same non-empty host name,
/// started earlier). A Core restarted after a crash (OOM, SIGKILL, `restart: unless-stopped`)
/// thus takes its predecessor's work back at once instead of waiting for the stale heartbeat;
/// two Cores on one host must therefore have distinct host names (`HOSTNAME`).
///
/// Binds, in order: stale cutoff (`now - HEARTBEAT_STALE_SECONDS`), [`hostname`], this
/// Core's instance id.
pub(crate) fn alive_sql(column: &str) -> String {
    format!(
        "EXISTS(SELECT 1 FROM core_instances c WHERE c.id={column} AND c.heartbeat_at>=? \
            AND NOT (c.hostname<>'' AND c.hostname=? AND c.started_at<(SELECT \
            COALESCE(MAX(m.started_at),0) FROM core_instances m WHERE m.id=?)))"
    )
}

/// Register this Core or refresh its heartbeat, then forget Cores silent for a day.
pub async fn heartbeat(store: &IdentityStore, now: i64) -> Result<(), sqlx::Error> {
    query(
        "INSERT INTO core_instances(id,started_at,heartbeat_at,hostname,version) \
            VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET heartbeat_at=excluded.heartbeat_at",
    )
    .bind(store.instance_id())
    .bind(now)
    .bind(now)
    .bind(hostname())
    .bind(env!("CARGO_PKG_VERSION"))
    .execute(store.pool())
    .await?;
    query("DELETE FROM core_instances WHERE heartbeat_at<?")
        .bind(now.saturating_sub(PRUNE_AFTER_SECONDS))
        .execute(store.pool())
        .await?;
    refresh_peers(store, now).await?;
    Ok(())
}

/// Live Cores other than this one on the database: a fresh heartbeat, and not this Core's own
/// crashed predecessor (same rule as [`alive_sql`]).
pub(crate) async fn count_live_peers(store: &IdentityStore, now: i64) -> Result<i64, sqlx::Error> {
    query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM core_instances p WHERE p.id<>? AND {}",
        alive_sql("p.id")
    )))
    .bind(store.instance_id())
    .bind(now.saturating_sub(HEARTBEAT_STALE_SECONDS))
    .bind(hostname())
    .bind(store.instance_id())
    .fetch_one(store.pool())
    .await
}

/// Multi-Core mode ([`crate::coherence::Coherence::shared`]), refreshed by every heartbeat: a
/// Core appearing is seen within [`HEARTBEAT_INTERVAL`] (it counts the Cores already running as
/// soon as it registers), a dead one after [`HEARTBEAT_STALE_SECONDS`]. SQLite is single-Core
/// by construction and never reads it.
pub(crate) async fn refresh_peers(store: &IdentityStore, now: i64) -> Result<bool, sqlx::Error> {
    let shared = store.is_postgres() && count_live_peers(store, now).await? > 0;
    let was_shared = store.coherence.shared();
    store.coherence.set_shared(shared);
    if was_shared && !shared {
        crate::coherence::left_shared_mode(store).await;
    }
    Ok(shared)
}

/// Remove this Core's row on graceful shutdown (after its heartbeat loop stopped): the other
/// Cores treat it as gone at once instead of after [`HEARTBEAT_STALE_SECONDS`], and their next
/// [`reclaim_orphans`] pass requeues the local leases it still held.
pub async fn deregister(store: &IdentityStore) -> Result<(), sqlx::Error> {
    query("DELETE FROM core_instances WHERE id=?")
        .bind(store.instance_id())
        .execute(store.pool())
        .await?;
    Ok(())
}

/// Release the work that no live Core can finish any more, and only that work.
///
/// 1. Local leases (`node_id='local'`) that can no longer be honoured are expired, then requeued
///    by [`crate::task_engine::recover_expired`] with attempt fencing. On SQLite (a single Core
///    by construction) every local lease belongs to a stopped process at `startup`; periodic
///    passes leave them to the lease expiry of the dispatcher. On PostgreSQL only leases owned by
///    another Core whose heartbeat is stale or missing are expired; leases without an owner
///    (taken before schema v4, possibly by a Core still running) expire on their own.
/// 2. `scan_jobs` still queued/running without any leased scan task of their library end in error.
/// 3. Run fences (`active_scan_runs`, `active_task_runs`) and scan staging rows whose
///    `id:attempt` matches no leased durable task are removed.
pub(crate) async fn reclaim_orphans(
    store: &IdentityStore,
    now: i64,
    startup: bool,
) -> Result<(), sqlx::Error> {
    if store.is_postgres() {
        let _writer = store.task_writes.lock().await;
        query(sqlx::AssertSqlSafe(format!(
            "UPDATE durable_tasks SET lease_until=? WHERE node_id='local' AND lease_until>? \
                AND lease_owner IS NOT NULL AND lease_owner<>? AND NOT {}",
            alive_sql("durable_tasks.lease_owner")
        )))
        .bind(now)
        .bind(now)
        .bind(store.instance_id())
        .bind(now.saturating_sub(HEARTBEAT_STALE_SECONDS))
        .bind(hostname())
        .bind(store.instance_id())
        .execute(store.pool())
        .await?;
    } else if startup {
        let _writer = store.task_writes.lock().await;
        query("UPDATE durable_tasks SET lease_until=? WHERE node_id='local' AND lease_until>?")
            .bind(now)
            .bind(now)
            .execute(store.pool())
            .await?;
    }
    crate::task_engine::recover_expired(store, now).await?;
    // A stopped executor cannot resume a probe. Keep its last inventory and report interruption.
    query(
        "UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 \
            WHERE status IN ('queued','running') AND NOT EXISTS(SELECT 1 FROM durable_tasks j \
            WHERE j.category='scan' AND j.library_id=scan_jobs.library_id \
            AND j.lease_until IS NOT NULL)",
    )
    .bind(INTERRUPTED_SCAN)
    .execute(store.pool())
    .await?;
    // Fences and staged scans are read BEFORE the leases: a fence or staging row is written only
    // after its claim committed, so a live one is always matched by the lease read below. Fences
    // are then deleted by their exact (key, id) pair, never by a NOT EXISTS evaluated against an
    // older snapshot: a fence upserted for a newly claimed scan in the meantime is left alone.
    let scan_fences: Vec<(String, String)> =
        query_as("SELECT library_id,scan_id FROM active_scan_runs")
            .fetch_all(store.pool())
            .await?;
    let task_fences: Vec<String> = query_scalar("SELECT run_id FROM active_task_runs")
        .fetch_all(store.pool())
        .await?;
    let mut staged = HashSet::new();
    for sql in STAGED_SCANS {
        staged.extend(
            query_scalar::<sqlx::Any, String>(sql)
                .fetch_all(store.pool())
                .await?,
        );
    }
    if scan_fences.is_empty() && task_fences.is_empty() && staged.is_empty() {
        return Ok(());
    }
    let leased: HashSet<String> = query_scalar(
        "SELECT id || ':' || CAST(attempt AS TEXT) FROM durable_tasks WHERE lease_until IS NOT NULL",
    )
    .fetch_all(store.pool())
    .await?
    .into_iter()
    .collect();
    for (library, scan) in scan_fences
        .iter()
        .filter(|(_, scan)| !leased.contains(scan))
    {
        query("DELETE FROM active_scan_runs WHERE library_id=? AND scan_id=?")
            .bind(library)
            .bind(scan)
            .execute(store.pool())
            .await?;
    }
    for run in task_fences.iter().filter(|run| !leased.contains(*run)) {
        query("DELETE FROM active_task_runs WHERE run_id=?")
            .bind(run)
            .execute(store.pool())
            .await?;
    }
    for scan in staged.difference(&leased) {
        crate::inventory::clean_staging(store, scan).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{auth::AuthState, scans::ScanJobs};
    use std::sync::Arc;

    /// Far in the future of the real clock, so leases at `NOW + 60` stay valid for passes that
    /// read the real clock (the scans router).
    const NOW: i64 = 4_000_000_000;

    /// A library `films` on the local Node with a running scan leased by `owner` (`scan:1`),
    /// its run fence, staging rows and a non-scan task fence; plus a library `remote` whose scan
    /// is leased by a remote Node.
    async fn leased_scan(store: &IdentityStore, owner: Option<&str>) {
        for sql in [
            "INSERT INTO media_nodes(id,name,kind,status) VALUES('nas','NAS','remote','connected')",
            "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','/media/films',1)",
            "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('remote','Remote','films','nas','/media',1)",
            "INSERT INTO scan_jobs(library_id,status) VALUES('films','running')",
            "INSERT INTO scan_jobs(library_id,status) VALUES('remote','running')",
            "INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES('films','scan:1','local')",
            "INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES('remote','remote:1','nas')",
            "INSERT INTO active_task_runs(run_id,library_id,node_id) VALUES('prep:1','films','local')",
            "INSERT INTO scan_batch_receipts(scan_id,sequence) VALUES('scan:1',0)",
            "INSERT INTO scan_staging_fingerprints(scan_id,relative_path,library_id,fingerprint) VALUES('scan:1','a.mkv','films','f')",
            "INSERT INTO scan_batch_receipts(scan_id,sequence) VALUES('remote:1',0)",
            "INSERT INTO scan_batch_receipts(scan_id,sequence) VALUES('ghost:3',0)",
        ] {
            query(sql).execute(store.pool()).await.unwrap();
        }
        for (id, library, node, category) in [
            ("scan", "films", "local", "scan"),
            ("prep", "films", "local", "prepare"),
            ("remote", "remote", "nas", "scan"),
        ] {
            query(
                "INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,\
                    status,attempt,lease_until,lease_owner,started_at,created_at,updated_at,\
                    available_at) VALUES(?,?,'admin',?,?,?,'running',1,?,?,?,?,?,?)",
            )
            .bind(id)
            .bind(library)
            .bind(id)
            .bind(node)
            .bind(category)
            .bind(NOW + 60)
            .bind(if node == "local" {
                owner
            } else {
                Some("remote-owner")
            })
            .bind(NOW)
            .bind(NOW)
            .bind(NOW)
            .bind(NOW)
            .execute(store.pool())
            .await
            .unwrap();
        }
    }

    async fn count(store: &IdentityStore, sql: &'static str) -> i64 {
        query_scalar(sql).fetch_one(store.pool()).await.unwrap()
    }

    /// Everything `leased_scan` created for the local scan is still there and leased.
    async fn assert_intact(store: &IdentityStore, owner: &str) {
        let (status, lease, lease_owner, attempt): (String, Option<i64>, Option<String>, i64) =
            crate::database::query_as(
                "SELECT status,lease_until,lease_owner,attempt FROM durable_tasks WHERE id='scan'",
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(
            (status.as_str(), lease, lease_owner.as_deref(), attempt),
            ("running", Some(NOW + 60), Some(owner), 1)
        );
        assert_eq!(
            count(
                store,
                "SELECT COUNT(*) FROM scan_jobs WHERE library_id='films' AND status='running'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                store,
                "SELECT COUNT(*) FROM active_scan_runs WHERE scan_id='scan:1'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                store,
                "SELECT COUNT(*) FROM active_task_runs WHERE run_id='prep:1'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                store,
                "SELECT COUNT(*) FROM scan_batch_receipts WHERE scan_id='scan:1'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                store,
                "SELECT COUNT(*) FROM scan_staging_fingerprints WHERE scan_id='scan:1'"
            )
            .await,
            1
        );
    }

    /// The local scan was requeued with attempt fencing and everything it left was cleaned;
    /// the remote Node's leased scan is untouched.
    async fn assert_reclaimed(store: &IdentityStore) {
        let (status, lease, lease_owner, attempt, failures): (
            String,
            Option<i64>,
            Option<String>,
            i64,
            i64,
        ) = crate::database::query_as(
            "SELECT status,lease_until,lease_owner,attempt,failure_count FROM durable_tasks \
                WHERE id='scan'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            (status.as_str(), lease, lease_owner, attempt, failures),
            ("queued", None, None, 1, 1)
        );
        // The previous attempt is fenced: it can no longer renew nor finish.
        assert!(
            !crate::task_engine::renew(store, "scan", 1, "local", NOW, 60, 50)
                .await
                .unwrap()
        );
        assert!(
            !crate::task_engine::finish(store, "scan", 1, "local", NOW, None)
                .await
                .unwrap()
        );
        let errors: String = query_scalar(
            "SELECT errors_json FROM scan_jobs WHERE library_id='films' AND status='error'",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(errors.contains("Scan interrompu par un redémarrage"));
        for sql in [
            "SELECT COUNT(*) FROM active_scan_runs WHERE library_id='films'",
            "SELECT COUNT(*) FROM active_task_runs",
            "SELECT COUNT(*) FROM scan_batch_receipts WHERE scan_id IN ('scan:1','ghost:3')",
            "SELECT COUNT(*) FROM scan_staging_fingerprints",
        ] {
            assert_eq!(count(store, sql).await, 0, "{sql}");
        }
        for sql in [
            "SELECT COUNT(*) FROM durable_tasks WHERE id='remote' AND lease_until IS NOT NULL",
            "SELECT COUNT(*) FROM scan_jobs WHERE library_id='remote' AND status='running'",
            "SELECT COUNT(*) FROM active_scan_runs WHERE scan_id='remote:1'",
            "SELECT COUNT(*) FROM scan_batch_receipts WHERE scan_id='remote:1'",
        ] {
            assert_eq!(count(store, sql).await, 1, "{sql}");
        }
    }

    #[tokio::test]
    async fn sqlite_restart_requeues_local_leases_and_cleans_orphans() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let before = IdentityStore::open(&path).await.unwrap();
        leased_scan(&before, Some(before.instance_id())).await;
        before.close().await;
        // A second open of a SQLite database is a restart: a new Core instance.
        let after = IdentityStore::open(&path).await.unwrap();
        assert_ne!(after.instance_id(), before.instance_id());
        assert!(!after.is_postgres());
        reclaim_orphans(&after, NOW, true).await.unwrap();
        assert_reclaimed(&after).await;
        after.close().await;
    }

    #[tokio::test]
    async fn sqlite_periodic_reclaim_keeps_the_running_cores_work() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        leased_scan(&store, Some(store.instance_id())).await;
        reclaim_orphans(&store, NOW, false).await.unwrap();
        assert_intact(&store, store.instance_id()).await;
        // Only the orphan staging of an unknown attempt is gone.
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM scan_batch_receipts WHERE scan_id='ghost:3'"
            )
            .await,
            0
        );
        store.close().await;
    }

    #[tokio::test]
    async fn local_lease_renewal_requires_its_owner() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        leased_scan(&store, Some("another-core")).await;
        assert!(
            !crate::task_engine::renew(&store, "scan", 1, "local", NOW, 60, 10)
                .await
                .unwrap(),
            "a local lease of another Core is not renewed here"
        );
        // Remote Node work is fenced by id/attempt/node/lease, not by the Core owner.
        assert!(
            crate::task_engine::renew(&store, "remote", 1, "nas", NOW, 60, 10)
                .await
                .unwrap()
        );
        for owner in [None, Some(store.instance_id())] {
            query("UPDATE durable_tasks SET lease_owner=? WHERE id='scan'")
                .bind(owner)
                .execute(store.pool())
                .await
                .unwrap();
            assert!(
                crate::task_engine::renew(&store, "scan", 1, "local", NOW, 60, 10)
                    .await
                    .unwrap()
            );
        }
        // Finishing clears the owner with the lease.
        assert!(
            crate::task_engine::finish(&store, "scan", 1, "local", NOW, None)
                .await
                .unwrap()
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM durable_tasks WHERE id='scan' AND lease_owner IS NULL"
            )
            .await,
            1
        );
        store.close().await;
    }

    /// `dispatch_once` sees the scan of `films` completed: without the owner filter it would
    /// finish the lease, although the executor runs in another Core.
    async fn dispatch_leaves_foreign_local_work(store: &IdentityStore, owner: &str) {
        query("UPDATE scan_jobs SET status='completed' WHERE library_id='films'")
            .execute(store.pool())
            .await
            .unwrap();
        let foreign = crate::task_engine::foreign_local_leases(store)
            .await
            .unwrap();
        assert!(foreign.contains(&("scan".to_owned(), 1)));
        assert!(!foreign.contains(&("remote".to_owned(), 1)));
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        crate::scheduler::dispatch_once(&auth, root.path(), &Arc::new(ScanJobs::default()), NOW)
            .await
            .unwrap();
        query("UPDATE scan_jobs SET status='running' WHERE library_id='films'")
            .execute(store.pool())
            .await
            .unwrap();
        assert_intact(store, owner).await;
    }

    #[tokio::test]
    async fn sqlite_dispatch_leaves_local_work_of_another_core() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        leased_scan(&store, Some("another-core")).await;
        dispatch_leaves_foreign_local_work(&store, "another-core").await;
        store.close().await;
    }

    #[tokio::test]
    async fn heartbeat_registers_once_and_prunes_silent_cores() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        query("INSERT INTO core_instances(id,started_at,heartbeat_at) VALUES('gone',1,1)")
            .execute(store.pool())
            .await
            .unwrap();
        heartbeat(&store, NOW).await.unwrap();
        heartbeat(&store, NOW + 10).await.unwrap();
        let rows: Vec<(String, i64, i64)> =
            crate::database::query_as("SELECT id,started_at,heartbeat_at FROM core_instances")
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert_eq!(rows, vec![(store.instance_id().to_owned(), NOW, NOW + 10)]);
        store.close().await;
    }

    /// Multi-Core mode detection: alone, a peer appears, the peer dies, and a crashed
    /// predecessor on the same host never counts as a peer.
    #[tokio::test]
    async fn live_peers_are_counted_from_fresh_heartbeats_only() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        heartbeat(&store, NOW).await.unwrap();
        assert_eq!(count_live_peers(&store, NOW).await.unwrap(), 0, "alone");
        assert!(!store.shared_with_other_cores());

        query("INSERT INTO core_instances(id,started_at,heartbeat_at,hostname) VALUES('peer',?,?,'other-host')")
            .bind(NOW)
            .bind(NOW)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            count_live_peers(&store, NOW + 5).await.unwrap(),
            1,
            "peer appears"
        );
        // SQLite is single-Core by construction: the mode never switches on.
        assert!(!refresh_peers(&store, NOW + 5).await.unwrap());
        assert!(!store.shared_with_other_cores());

        assert_eq!(
            count_live_peers(&store, NOW + HEARTBEAT_STALE_SECONDS + 1)
                .await
                .unwrap(),
            0,
            "peer silent for longer than the stale limit: dead"
        );

        query("UPDATE core_instances SET hostname=? WHERE id=?")
            .bind(hostname())
            .bind(store.instance_id())
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO core_instances(id,started_at,heartbeat_at,hostname) VALUES('predecessor',?,?,?)")
            .bind(NOW - 100)
            .bind(NOW)
            .bind(hostname())
            .execute(store.pool())
            .await
            .unwrap();
        let expected = if hostname().is_empty() { 2 } else { 1 };
        assert_eq!(
            count_live_peers(&store, NOW + 5).await.unwrap(),
            expected,
            "a crashed predecessor on this host is not a peer"
        );
        store.close().await;
    }

    #[tokio::test]
    #[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
    async fn postgres_second_core_never_reclaims_a_live_cores_work() {
        let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
        let pool = sqlx::PgPool::connect(&connection).await.unwrap();
        let schema = format!("core_instances_{}", uuid::Uuid::new_v4().simple());
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
        assert!(a.is_postgres() && b.is_postgres());
        assert_ne!(a.instance_id(), b.instance_id());
        // The real clock: the scans router reclaims at the current time.
        let now = media_contracts::protocol::unix_now();
        heartbeat(&a, now).await.unwrap();
        // Two stores in one process share a host name, which would make B take A for its own
        // crashed predecessor (same host, started earlier): A runs on another machine here.
        query("UPDATE core_instances SET hostname='core-a-host' WHERE id=?")
            .bind(a.instance_id())
            .execute(a.pool())
            .await
            .unwrap();
        leased_scan(&a, Some(a.instance_id())).await;
        // A lease taken before schema v4 has no owner: it is left to expire on its own.
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('legacy','Legacy','films','local','/media/legacy',1)")
            .execute(a.pool()).await.unwrap();
        query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,status,attempt,lease_until,started_at,created_at,updated_at,available_at) VALUES('legacy','legacy','admin','legacy','local','scan','running',1,?,1,1,1,1)")
            .bind(NOW + 60).execute(a.pool()).await.unwrap();
        // NOW is far in the future of `now`: every lease stays valid in both passes below.
        assert!(NOW > now + 3600, "test clock");

        // Core B starts while A is alive: its startup cleanup leaves A's work alone.
        let auth_b = AuthState::new(b.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let _router = crate::scans::router(
            auth_b,
            root.path().to_owned(),
            Arc::new(ScanJobs::default()),
        )
        .await
        .unwrap();
        assert_intact(&b, a.instance_id()).await;
        assert_eq!(
            count(&b, "SELECT COUNT(*) FROM core_instances").await,
            2,
            "B registered at startup"
        );
        // B's dispatcher neither finishes nor renews A's local scan.
        dispatch_leaves_foreign_local_work(&b, a.instance_id()).await;
        assert!(
            !crate::task_engine::renew(&b, "scan", 1, "local", now, 60, 10)
                .await
                .unwrap()
        );

        // A dies: its heartbeat goes stale. B's next pass reclaims A's work only.
        query("UPDATE core_instances SET heartbeat_at=? WHERE id=?")
            .bind(NOW - HEARTBEAT_STALE_SECONDS - 1)
            .bind(a.instance_id())
            .execute(b.pool())
            .await
            .unwrap();
        query("UPDATE core_instances SET heartbeat_at=? WHERE id=?")
            .bind(NOW)
            .bind(b.instance_id())
            .execute(b.pool())
            .await
            .unwrap();
        reclaim_orphans(&b, NOW, false).await.unwrap();
        assert_reclaimed(&b).await;
        assert_eq!(
            count(
                &b,
                "SELECT COUNT(*) FROM durable_tasks WHERE id='legacy' AND lease_until IS NOT NULL"
            )
            .await,
            1,
            "an ownerless lease is not reclaimed early"
        );

        a.close().await;
        b.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}
