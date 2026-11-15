//! Publication and staging fencing (multi-Core roadmap, step 5).
use super::fence::failpoint::{self, Action};
use super::{
    InventoryBatch, Publication, ScanAttempt, ScannedFile, publish_fenced_scan, stage_batch,
    stage_fenced_batch,
};
use crate::database::{query, query_scalar};
use crate::identity::IdentityStore;
use media_contracts::protocol::unix_now;
use std::path::Path;

const LIBRARY: &str = "films";
const ROOT: &str = "/read-only/films";

/// Library `films` on `node` with its job row.
async fn library(store: &IdentityStore, node: &str) {
    if node != "local" {
        query("INSERT INTO media_nodes(id,name,kind,status) VALUES(?,'NAS','remote','connected')")
            .bind(node)
            .execute(store.pool())
            .await
            .unwrap();
    }
    query(
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,'Films','films',?,?,1)",
    )
    .bind(LIBRARY)
    .bind(node)
    .bind(ROOT)
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO scan_jobs(library_id,status) VALUES(?,'running')")
        .bind(LIBRARY)
        .execute(store.pool())
        .await
        .unwrap();
}

/// Scan task `task` running as `attempt` on `node`, leased until `until` by `owner`, and the
/// library's run fence pointing at it (as a claim followed by `scans::enqueue_one` leaves them).
async fn lease(
    store: &IdentityStore,
    task: &str,
    attempt: i64,
    node: &str,
    owner: Option<&str>,
    until: i64,
) -> String {
    let now = unix_now();
    query(
        "INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,status,\
            attempt,lease_until,lease_owner,started_at,created_at,updated_at,available_at) \
            VALUES(?,?,'admin',?,?,'scan','running',?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE \
            SET status='running',attempt=excluded.attempt,lease_until=excluded.lease_until,\
            lease_owner=excluded.lease_owner,node_id=excluded.node_id",
    )
    .bind(task)
    .bind(LIBRARY)
    .bind(task)
    .bind(node)
    .bind(attempt)
    .bind(until)
    .bind(owner)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(store.pool())
    .await
    .unwrap();
    let scan = format!("{task}:{attempt}");
    query(
        "INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES(?,?,?) \
            ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id,node_id=excluded.node_id",
    )
    .bind(LIBRARY)
    .bind(&scan)
    .bind(node)
    .execute(store.pool())
    .await
    .unwrap();
    scan
}

fn batch(scan: &str, sequence: u64, films: std::ops::Range<usize>) -> InventoryBatch {
    InventoryBatch {
        scan_id: scan.into(),
        sequence,
        files: films
            .map(|index| ScannedFile {
                relative_path: format!("Film {index:04} (2020)/Film {index:04} (2020).mkv"),
                title: format!("Film {index:04}"),
                identity_key: format!("film-{index:04}"),
                size: 1_000 + index as u64,
                modified_ns: "1".into(),
                mime: "video/x-matroska".into(),
                probe: serde_json::from_value(serde_json::json!({
                    "format": {"duration": "120"},
                    "streams": [{"index": 0, "codec_type": "video", "codec_name": "av1"}]
                }))
                .unwrap(),
                ..Default::default()
            })
            .collect(),
        errors: vec![],
        warnings: vec![],
    }
}

fn attempt<'a>(node: &'a str, scan: &'a str) -> ScanAttempt<'a> {
    ScanAttempt {
        library_id: LIBRARY,
        node_id: node,
        scan_id: scan,
    }
}

async fn publish(store: &IdentityStore, node: &str, scan: &str) -> Publication {
    publish_fenced_scan(store, attempt(node, scan), Path::new(ROOT), true)
        .await
        .unwrap()
}

/// `[works, editions, versions, sources, assets, available assets]`.
async fn inventory(store: &IdentityStore) -> [i64; 6] {
    let mut counts = [0; 6];
    for (slot, sql) in [
        "SELECT COUNT(*) FROM inventory_works",
        "SELECT COUNT(*) FROM inventory_editions",
        "SELECT COUNT(*) FROM inventory_versions",
        "SELECT COUNT(*) FROM inventory_sources",
        "SELECT COUNT(*) FROM inventory_assets",
        "SELECT COUNT(*) FROM inventory_assets WHERE available=1",
    ]
    .into_iter()
    .enumerate()
    {
        counts[slot] = query_scalar(sql).fetch_one(store.pool()).await.unwrap();
    }
    counts
}

/// Stage the rescan of films 50..150 for `scan`.
async fn rescan(store: &IdentityStore, scan: String) -> String {
    for (sequence, range) in [(0, 50..100), (1, 100..150)] {
        assert!(
            stage_fenced_batch(
                store,
                attempt("local", &scan),
                &batch(&scan, sequence, range)
            )
            .await
            .unwrap()
        );
    }
    scan
}

async fn duplicated_paths(store: &IdentityStore) -> i64 {
    query_scalar(
        "SELECT COUNT(*) FROM (SELECT relative_path FROM inventory_assets \
            GROUP BY library_id,relative_path HAVING COUNT(*)>1) d",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn staged(store: &IdentityStore, scan: &str) -> i64 {
    query_scalar("SELECT COUNT(*) FROM scan_staging_files WHERE scan_id=?")
        .bind(scan)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn publication_is_refused_once_the_attempt_no_longer_owns_the_scan() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    library(&store, "local").await;
    let me = store.instance_id().to_owned();
    let now = unix_now();
    let scan = lease(&store, "scan", 1, "local", Some(&me), now + 600).await;
    assert!(
        stage_fenced_batch(&store, attempt("local", &scan), &batch(&scan, 0, 0..10))
            .await
            .unwrap()
    );
    assert_eq!(staged(&store, &scan).await, 10);

    // Lease expired, another Core's lease, cancelled, superseded, wrong Node: nothing written.
    let refusals: [(&str, i64, Option<&str>, i64); 3] = [
        ("expired", 1, Some(&me), now - 1),
        ("foreign owner", 1, Some("another-core"), now + 600),
        ("superseded", 2, Some(&me), now + 600),
    ];
    for (case, number, owner, until) in refusals {
        query("UPDATE durable_tasks SET attempt=?,lease_owner=?,lease_until=? WHERE id='scan'")
            .bind(number)
            .bind(owner)
            .bind(until)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            publish(&store, "local", &scan).await,
            Publication::Fenced,
            "{case}"
        );
        assert_eq!(inventory(&store).await, [0; 6], "{case}");
    }
    query("UPDATE durable_tasks SET attempt=1,lease_owner=?,lease_until=?,status='cancelled' WHERE id='scan'")
        .bind(&me)
        .bind(now + 600)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(publish(&store, "local", &scan).await, Publication::Fenced);
    query("UPDATE durable_tasks SET status='running' WHERE id='scan'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(publish(&store, "nas", &scan).await, Publication::Fenced);
    assert_eq!(inventory(&store).await, [0; 6]);
    assert_eq!(
        staged(&store, &scan).await,
        10,
        "a refusal leaves staging alone"
    );

    // The owner publishes once; a replay finds the marker and keeps every asset available
    // (an unfenced replay would publish the emptied staging area and hide all of them).
    assert_eq!(
        publish(&store, "local", &scan).await,
        Publication::Published
    );
    assert_eq!(inventory(&store).await, [10, 10, 10, 10, 10, 10]);
    assert_eq!(
        publish(&store, "local", &scan).await,
        Publication::AlreadyPublished
    );
    assert_eq!(inventory(&store).await, [10, 10, 10, 10, 10, 10]);
    // A batch after the publication of its attempt is stale too.
    assert!(
        !stage_fenced_batch(&store, attempt("local", &scan), &batch(&scan, 1, 10..20))
            .await
            .unwrap()
    );
    store.close().await;
}

#[tokio::test]
async fn stale_batches_are_refused_without_any_staging() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    library(&store, "nas").await;
    let now = unix_now();
    let current = lease(&store, "scan", 2, "nas", Some("remote-owner"), now + 600).await;
    let stale = "scan:1";
    for (scan, node) in [
        (stale, "nas"),
        (current.as_str(), "local"),
        ("scan:x", "nas"),
    ] {
        assert!(
            !stage_fenced_batch(&store, attempt(node, scan), &batch(scan, 0, 0..5))
                .await
                .unwrap(),
            "{scan} on {node}"
        );
    }
    query("UPDATE durable_tasks SET lease_until=? WHERE id='scan'")
        .bind(now - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !stage_fenced_batch(&store, attempt("nas", &current), &batch(&current, 0, 0..5))
            .await
            .unwrap(),
        "expired lease"
    );
    let receipts: i64 = query_scalar("SELECT COUNT(*) FROM scan_batch_receipts")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let staging: i64 = query_scalar("SELECT COUNT(*) FROM scan_staging_files")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let processed: i64 = query_scalar("SELECT processed_items FROM durable_tasks WHERE id='scan'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!((receipts, staging, processed), (0, 0, 0));
    store.close().await;
}

/// A publication stopped midway (error, then a dropped future holding its transaction) leaves
/// the previous inventory exactly as it was; the next attempt publishes once.
#[tokio::test]
async fn aborted_publication_leaves_nothing_and_the_next_attempt_publishes_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    library(&store, "local").await;
    let me = store.instance_id().to_owned();
    let now = unix_now();
    // A first scan publishes films 0..100.
    let first = lease(&store, "first", 1, "local", Some(&me), now + 600).await;
    assert!(
        stage_fenced_batch(&store, attempt("local", &first), &batch(&first, 0, 0..100))
            .await
            .unwrap()
    );
    assert_eq!(
        publish(&store, "local", &first).await,
        Publication::Published
    );
    query("UPDATE durable_tasks SET status='completed',lease_until=NULL WHERE id='first'")
        .execute(store.pool())
        .await
        .unwrap();
    let before = inventory(&store).await;
    assert_eq!(before, [100, 100, 100, 100, 100, 100]);
    let available: Vec<String> =
        query_scalar("SELECT id FROM inventory_assets WHERE available=1 ORDER BY id")
            .fetch_all(store.pool())
            .await
            .unwrap();

    // The rescan sees films 50..150 (complete): it would add 50 and hide 50.
    let failed = rescan(
        &store,
        lease(&store, "second", 1, "local", Some(&me), now + 600).await,
    )
    .await;
    failpoint::arm(&failed, Action::Fail);
    assert!(
        publish_fenced_scan(&store, attempt("local", &failed), Path::new(ROOT), true)
            .await
            .is_err()
    );
    assert_eq!(inventory(&store).await, before, "zero partial publication");

    let hung = rescan(
        &store,
        lease(&store, "second", 2, "local", Some(&me), now + 600).await,
    )
    .await;
    let reached = failpoint::arm(&hung, Action::Hang);
    let task = {
        let store = store.clone();
        let hung = hung.clone();
        tokio::spawn(async move {
            publish_fenced_scan(&store, attempt("local", &hung), Path::new(ROOT), true).await
        })
    };
    reached.notify.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(inventory(&store).await, before, "zero partial publication");
    let still: Vec<String> =
        query_scalar("SELECT id FROM inventory_assets WHERE available=1 ORDER BY id")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(still, available);

    // The dropped attempt lost its lease; the retry is a new attempt and publishes once.
    query("UPDATE durable_tasks SET lease_until=? WHERE id='second'")
        .bind(now - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(publish(&store, "local", &hung).await, Publication::Fenced);
    let retry = rescan(
        &store,
        lease(&store, "second", 3, "local", Some(&me), now + 600).await,
    )
    .await;
    assert_eq!(
        publish(&store, "local", &retry).await,
        Publication::Published
    );
    let after = [150, 150, 150, 150, 150, 100];
    assert_eq!(inventory(&store).await, after);
    assert_eq!(
        publish(&store, "local", &retry).await,
        Publication::AlreadyPublished
    );
    assert_eq!(publish(&store, "local", &hung).await, Publication::Fenced);
    assert_eq!(inventory(&store).await, after, "zero double publication");
    assert_eq!(duplicated_paths(&store).await, 0);
    store.close().await;
}

/// Two stores (two Cores) on one isolated PostgreSQL schema, in one process.
async fn postgres_cores() -> (sqlx::PgPool, String, IdentityStore, IdentityStore) {
    let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("scan_fence_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let a = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    let b = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    (pool, schema, a, b)
}

async fn drop_schema(pool: sqlx::PgPool, schema: String, a: IdentityStore, b: IdentityStore) {
    a.close().await;
    b.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// Exit proof §6 row 5 (concurrency): the same completion received by two Cores publishes
/// once; a stale attempt racing the current one publishes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_two_cores_publish_one_library_exactly_once() {
    let (pool, schema, a, b) = postgres_cores().await;
    library(&a, "nas").await;
    let now = unix_now();
    let scan = lease(&a, "scan", 1, "nas", Some("remote-owner"), now + 600).await;
    for (sequence, start) in [(0, 0), (1, 100), (2, 200)] {
        assert!(
            stage_fenced_batch(
                &a,
                attempt("nas", &scan),
                &batch(&scan, sequence, start..start + 100)
            )
            .await
            .unwrap()
        );
    }
    let (first, second) = tokio::join!(publish(&a, "nas", &scan), publish(&b, "nas", &scan));
    let mut outcomes = [first, second];
    outcomes.sort_by_key(|outcome| *outcome as u8);
    assert_eq!(
        outcomes,
        [Publication::Published, Publication::AlreadyPublished]
    );
    assert_eq!(inventory(&b).await, [300, 300, 300, 300, 300, 300]);
    assert_eq!(duplicated_paths(&b).await, 0);

    // Attempt 2 replaces attempt 1; attempt 1's staging is left behind (as when its Node
    // keeps sending after a lease loss). Both race: only the current one publishes.
    let current = lease(&b, "scan", 2, "nas", Some("remote-owner"), now + 600).await;
    stage_batch(&a, &scan, LIBRARY, &batch(&scan, 9, 0..300))
        .await
        .unwrap();
    for (sequence, start) in [(0, 100), (1, 200), (2, 300)] {
        assert!(
            stage_fenced_batch(
                &b,
                attempt("nas", &current),
                &batch(&current, sequence, start..start + 100)
            )
            .await
            .unwrap()
        );
    }
    let (stale, fresh) = tokio::join!(publish(&a, "nas", &scan), publish(&b, "nas", &current));
    assert_eq!(
        (stale, fresh),
        (Publication::Fenced, Publication::Published)
    );
    assert_eq!(inventory(&a).await, [400, 400, 400, 400, 400, 300]);
    assert_eq!(duplicated_paths(&a).await, 0);
    drop_schema(pool, schema, a, b).await;
}

/// Exit proof §6 row 5 (brutal stop): Core A's publication is cut midway by killing its
/// PostgreSQL connection (as the death of A's process would), then its future is dropped. Its
/// lease expires and Core B's new attempt publishes the library once and completely.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_brutal_stop_mid_publication_then_the_other_core_publishes_once() {
    let (pool, schema, a, b) = postgres_cores().await;
    library(&a, "local").await;
    let now = unix_now();
    let scan = lease(&a, "scan", 1, "local", Some(a.instance_id()), now + 600).await;
    for (sequence, start) in [(0, 0), (1, 100)] {
        assert!(
            stage_fenced_batch(
                &a,
                attempt("local", &scan),
                &batch(&scan, sequence, start..start + 100)
            )
            .await
            .unwrap()
        );
    }
    let reached = failpoint::arm(&scan, Action::Hang);
    let task = {
        let a = a.clone();
        let scan = scan.clone();
        tokio::spawn(async move {
            publish_fenced_scan(&a, attempt("local", &scan), Path::new(ROOT), true).await
        })
    };
    reached.notify.notified().await;
    let backend = reached
        .backend
        .lock()
        .unwrap()
        .expect("PostgreSQL backend pid");
    // B waits for A's per-library publication lock, then is refused (A's local lease).
    let blocked = {
        let b = b.clone();
        let scan = scan.clone();
        tokio::spawn(async move { publish(&b, "local", &scan).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!blocked.is_finished(), "B waits for A's publication lock");
    // The publication holds no lock on its task row while it writes: the dispatcher keeps
    // renewing its lease (a long publication must not outlive its own lease).
    let renewed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::task_engine::renew(&a, "scan", 1, "local", now, 600, 0),
    )
    .await
    .expect("renewal returns while the publication runs")
    .unwrap();
    assert!(renewed, "the publishing attempt's lease is renewed");
    let killed: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
        .bind(i32::try_from(backend).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(killed);
    task.abort();
    let _ = task.await;
    assert_eq!(blocked.await.unwrap(), Publication::Fenced);
    assert_eq!(inventory(&b).await, [0; 6], "zero partial publication");

    // A's lease expires; B requeues and claims the next attempt, rescans and publishes.
    query("UPDATE durable_tasks SET lease_until=? WHERE id='scan'")
        .bind(now - 1)
        .execute(b.pool())
        .await
        .unwrap();
    crate::task_engine::recover_expired(&b, now).await.unwrap();
    let retry = lease(&b, "scan", 2, "local", Some(b.instance_id()), now + 600).await;
    for (sequence, start) in [(0, 0), (1, 100)] {
        assert!(
            stage_fenced_batch(
                &b,
                attempt("local", &retry),
                &batch(&retry, sequence, start..start + 100)
            )
            .await
            .unwrap()
        );
    }
    let (zombie, owner) = tokio::join!(publish(&a, "local", &scan), publish(&b, "local", &retry));
    assert_eq!(
        (zombie, owner),
        (Publication::Fenced, Publication::Published)
    );
    assert_eq!(
        publish(&b, "local", &retry).await,
        Publication::AlreadyPublished
    );
    assert_eq!(
        publish(&a, "local", &retry).await,
        Publication::Fenced,
        "B's local lease"
    );
    assert_eq!(inventory(&a).await, [200, 200, 200, 200, 200, 200]);
    assert_eq!(duplicated_paths(&a).await, 0);
    let published: Option<String> =
        query_scalar("SELECT published_scan_id FROM scan_jobs WHERE library_id=?")
            .bind(LIBRARY)
            .fetch_one(a.pool())
            .await
            .unwrap();
    assert_eq!(published.as_deref(), Some(retry.as_str()));
    drop_schema(pool, schema, a, b).await;
}
