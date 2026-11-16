//! Process-local caches against writes of another Core (multi-Core roadmap §4, step 6).
//!
//! SQLite tests: a lone Core never re-reads its node registry nor shares telemetry. PostgreSQL
//! tests (`#[ignore]`): two stores on one isolated schema are two Cores of ONE process, both
//! heartbeating (the real `core_instances::heartbeat`, every `HEARTBEAT_INTERVAL`).
use crate::{
    auth::{AuthState, hash},
    core_instances::{HEARTBEAT_INTERVAL, heartbeat},
    database::{query, query_scalar},
    identity::IdentityStore,
    inventory::{CatalogueFilter, ScanReport, ScannedFile, catalogue_changed, catalogue_page},
    permissions::{MediaAccess, require_media},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    middleware,
    routing::get,
};
use media_contracts::{
    ids::NodeId,
    protocol::{HeartbeatPayload, unix_now},
};
use std::{
    future::Future,
    path::Path,
    time::{Duration, Instant},
};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ROOT: &str = "/movies";
/// Roadmap target for a permission change, a session revocation or a Node revocation.
const SECURITY_BOUND: Duration = Duration::from_secs(5);

async fn remote_node(store: &IdentityStore, id: &str) {
    query("INSERT INTO media_nodes(id,name,kind,status,last_seen_at) VALUES(?,'NAS','remote','connected',?)")
        .bind(id)
        .bind(unix_now())
        .execute(store.pool())
        .await
        .unwrap();
}

fn heartbeat_payload(node: &str, active_streams: u32) -> HeartbeatPayload {
    HeartbeatPayload {
        protocol_version: media_contracts::protocol::PROTOCOL_VERSION.into(),
        version: None,
        node_id: NodeId::new(node),
        roots: Vec::new(),
        public_url: None,
        active_scans: 0,
        active_scan_ids: Vec::new(),
        active_streams,
        resource_pressure: None,
        indexed_assets: 0,
        timestamp: unix_now(),
        command_acks: false,
        acked_command_ids: Vec::new(),
    }
}

async fn health_rows(store: &IdentityStore) -> i64 {
    query_scalar("SELECT COUNT(*) FROM node_health")
        .fetch_one(store.pool())
        .await
        .unwrap()
}

/// Alone (SQLite always, PostgreSQL with one live Core) the hot path reads the registry from
/// memory only and no telemetry is written; with other Cores one re-read per interval.
#[tokio::test]
async fn a_lone_core_never_rereads_its_node_registry_nor_shares_telemetry() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    remote_node(&store, "nas").await;
    crate::nodes::ensure_node_registry_warm(&store)
        .await
        .unwrap();
    for _ in 0..50 {
        crate::nodes::ensure_node_registry_warm(&store)
            .await
            .unwrap();
        crate::nodes::load_node(&store, "nas")
            .await
            .unwrap()
            .unwrap();
        crate::nodes::load_node_entry(&store, "nas")
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        store.node_registry.shared_refreshes(),
        0,
        "alone: no re-read"
    );
    let now = unix_now();
    store
        .node_registry
        .record_heartbeat("nas", &heartbeat_payload("nas", 2), Some(5), now);
    crate::nodes::health::publish(&store, "nas", now).await;
    assert_eq!(health_rows(&store).await, 0, "alone: nothing written");

    // The same code with another live Core (forced here: SQLite never enables it).
    store.coherence.set_shared(true);
    crate::nodes::health::publish(&store, "nas", now).await;
    assert_eq!(health_rows(&store).await, 1, "shared: snapshot written");
    store
        .node_registry
        .record_heartbeat("nas", &heartbeat_payload("nas", 3), Some(5), now + 1);
    crate::nodes::health::publish(&store, "nas", now + 1).await;
    let sampled: i64 = query_scalar("SELECT sampled_at FROM node_health WHERE node_id='nas'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(sampled, now, "at most one snapshot per interval");

    crate::nodes::ensure_node_registry_warm(&store)
        .await
        .unwrap();
    crate::nodes::load_node(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(store.node_registry.shared_refreshes(), 1, "one re-read");
    for _ in 0..50 {
        crate::nodes::load_node(&store, "nas")
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        store.node_registry.shared_refreshes(),
        1,
        "at most one per interval"
    );
    // Another Core revokes the Node and saw it more recently.
    query("UPDATE media_nodes SET status='revoked', last_seen_at=? WHERE id='nas'")
        .bind(now + 100)
        .execute(store.pool())
        .await
        .unwrap();
    store.node_registry.expire_shared_view();
    let record = crate::nodes::load_node(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(store.node_registry.shared_refreshes(), 2);
    assert_eq!(record.status, "revoked");
    assert_eq!(record.last_seen_at, Some(now + 100));
    store.close().await;
}

/// Telemetry of a Node received by another Core replaces older local telemetry only.
#[tokio::test]
async fn a_shared_snapshot_replaces_only_older_telemetry() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    remote_node(&store, "nas").await;
    store.coherence.set_shared(true);
    let now = unix_now();
    let snapshot = |streams: u32, at: i64| {
        query("INSERT INTO node_health(node_id,core_id,epoch,sampled_at,telemetry_json) VALUES('nas','other',1,?,?) ON CONFLICT(node_id) DO UPDATE SET sampled_at=excluded.sampled_at,telemetry_json=excluded.telemetry_json")
            .bind(at)
            .bind(format!("{{\"activeStreams\":{streams}}}"))
            .execute(store.pool())
    };
    snapshot(4, now - 30).await.unwrap();
    let entry = crate::nodes::load_node_entry(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.telemetry.reported_active_streams, 4);
    // This Core then receives a heartbeat itself: its own telemetry is newer.
    store
        .node_registry
        .record_heartbeat("nas", &heartbeat_payload("nas", 1), Some(5), now);
    store.node_registry.expire_shared_view();
    let entry = crate::nodes::load_node_entry(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.telemetry.reported_active_streams, 1, "older snapshot");
    snapshot(6, now + 1).await.unwrap();
    store.node_registry.expire_shared_view();
    let entry = crate::nodes::load_node_entry(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entry.telemetry.reported_active_streams, 6, "newer snapshot");
    store.close().await;
}

/// The last peer gone, the heartbeat re-reads once what alone mode never re-reads again.
#[tokio::test]
async fn leaving_multi_core_mode_rereads_the_node_registry_once() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    remote_node(&store, "nas").await;
    crate::nodes::ensure_node_registry_warm(&store)
        .await
        .unwrap();
    // Forced (SQLite never enables it): a peer was live, this Core re-read its view...
    store.coherence.set_shared(true);
    crate::nodes::load_node(&store, "nas")
        .await
        .unwrap()
        .unwrap();
    let refreshes = store.node_registry.shared_refreshes();
    // ...then the peer revoked the Node just before dying, within this Core's interval.
    query("UPDATE media_nodes SET status='revoked' WHERE id='nas'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !crate::core_instances::refresh_peers(&store, unix_now())
            .await
            .unwrap()
    );
    assert!(!store.shared_with_other_cores(), "alone again");
    assert_eq!(store.node_registry.shared_refreshes(), refreshes + 1);
    assert_eq!(node_status(&store, "nas").await, "revoked");
    // Alone from then on: no more re-reads.
    crate::core_instances::refresh_peers(&store, unix_now())
        .await
        .unwrap();
    assert_eq!(store.node_registry.shared_refreshes(), refreshes + 1);
    store.close().await;
}

// ---------------------------------------------------------------------------------------------
// PostgreSQL: two Cores.

struct Cores {
    pool: sqlx::PgPool,
    schema: String,
    a: IdentityStore,
    b: IdentityStore,
    beats: Vec<tokio::task::JoinHandle<()>>,
}

async fn open_core(url: &str) -> IdentityStore {
    IdentityStore::open_postgres(url).await.unwrap()
}

/// An isolated schema on the test server: admin pool, schema name, URL selecting it.
async fn postgres_schema(prefix: &str) -> (sqlx::PgPool, String, String) {
    let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("{prefix}_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    (pool, schema, url.to_string())
}

/// Two Cores on one isolated schema, heartbeating like `Scheduler` does. A gets its own host
/// name: two stores of one process share it, and B would take A for its crashed predecessor.
async fn two_cores(prefix: &str) -> Cores {
    let (pool, schema, url) = postgres_schema(prefix).await;
    let a = open_core(url.as_str()).await;
    heartbeat(&a, unix_now()).await.unwrap();
    query("UPDATE core_instances SET hostname='core-a-host' WHERE id=?")
        .bind(a.instance_id())
        .execute(a.pool())
        .await
        .unwrap();
    assert!(!a.shared_with_other_cores(), "A alone");
    let b = open_core(url.as_str()).await;
    heartbeat(&b, unix_now()).await.unwrap();
    assert!(
        b.shared_with_other_cores(),
        "B sees A as soon as it registers"
    );
    heartbeat(&a, unix_now()).await.unwrap();
    assert!(
        a.shared_with_other_cores(),
        "A sees B at its next heartbeat"
    );
    let beats = [a.clone(), b.clone()]
        .into_iter()
        .map(|store| {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
                interval.tick().await;
                loop {
                    interval.tick().await;
                    let _ = heartbeat(&store, unix_now()).await;
                }
            })
        })
        .collect();
    Cores {
        pool,
        schema,
        a,
        b,
        beats,
    }
}

impl Cores {
    async fn close(self) {
        for beat in self.beats {
            beat.abort();
        }
        self.a.close().await;
        self.b.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA {} CASCADE",
            self.schema
        )))
        .execute(&self.pool)
        .await
        .unwrap();
        self.pool.close().await;
    }
}

fn film(title: &str) -> ScannedFile {
    let probe = serde_json::json!({
        "streams": [{"index": 0, "codec_type": "video", "codec_name": "av1"}],
        "format": {}
    });
    ScannedFile {
        relative_path: format!("{title}.mkv"),
        size: 1024,
        modified_ns: "1".into(),
        title: title.into(),
        identity_key: title.to_lowercase(),
        mime: "video/x-matroska".into(),
        probe: serde_json::from_value(probe).unwrap(),
        ..Default::default()
    }
}

/// Library `lib` (local Node), user `u1` granted on it with a session, two films.
async fn seed(store: &IdentityStore) {
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local',?,1)")
        .bind(ROOT)
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('u1','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('lib','u1')")
        .execute(store.pool())
        .await
        .unwrap();
    add_session(store).await;
    crate::inventory::ingest(
        store,
        "lib",
        Path::new(ROOT),
        &ScanReport {
            files: vec![film("Alpha"), film("Beta")],
            complete: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn add_session(store: &IdentityStore) {
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at,device_label) VALUES(?,'u1',unixepoch(),unixepoch(),unixepoch()+86400,'test')")
        .bind(hash(TOKEN))
        .execute(store.pool())
        .await
        .unwrap();
}

/// The media permission middleware of `store`, in front of a catalogue route.
async fn media_router(store: &IdentityStore) -> Router {
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    Router::new()
        .route("/api/v1/catalog/{id}", get(|| async { StatusCode::OK }))
        .layer(middleware::from_fn_with_state(
            MediaAccess {
                auth,
                root: ROOT.into(),
            },
            require_media,
        ))
}

async fn status(router: &Router, work: &str) -> StatusCode {
    router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/catalog/{work}"))
                .header("host", "127.0.0.1:3000")
                .header(header::COOKIE, format!("aether_session={TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

/// The catalogue page user `u1` gets from `store` (cached by that Core).
async fn page(store: &IdentityStore) -> String {
    let filter = CatalogueFilter {
        user_id: Some("u1".into()),
        ..Default::default()
    };
    catalogue_page(
        store,
        &["lib".to_owned()],
        Some("film"),
        "",
        "title-asc",
        0,
        50,
        &filter,
        true,
    )
    .await
    .unwrap()
    .to_string()
}

async fn work_id(store: &IdentityStore, title: &str) -> String {
    query_scalar("SELECT id FROM inventory_works WHERE title=?")
        .bind(title)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

/// Time from `start` until `probe` holds, polled every millisecond; fails after `bound`.
async fn observed<F, Fut>(start: Instant, bound: Duration, what: &str, mut probe: F) -> Duration
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    loop {
        if probe().await {
            return start.elapsed();
        }
        assert!(
            start.elapsed() < bound,
            "{what}: not seen by the other Core within {bound:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// The metadata edit endpoint's write: manual title, then the change is announced.
async fn rename(store: &IdentityStore, work: &str, title: &str) {
    query("UPDATE inventory_works SET manual_json=? WHERE id=?")
        .bind(serde_json::json!({ "title": title }).to_string())
        .bind(work)
        .execute(store.pool())
        .await
        .unwrap();
    catalogue_changed(store).await;
}

/// `revoke_node_admin`'s database write, then its own registry refresh.
async fn revoke_node(store: &IdentityStore, node: &str) {
    query("UPDATE media_nodes SET status='revoked', key_id='revoked-key', shared_secret='wiped', revoked_at=? WHERE id=?")
        .bind(unix_now())
        .bind(node)
        .execute(store.pool())
        .await
        .unwrap();
    let _ = crate::nodes::refresh_node_in_registry(store, node).await;
}

async fn node_status(store: &IdentityStore, node: &str) -> String {
    crate::nodes::load_node(store, node)
        .await
        .unwrap()
        .unwrap()
        .status
}

/// Warm catalogue requests and Node registry reads of `store`: statements issued (`None`
/// without the `query-metrics` feature) and registry re-reads done for other Cores.
async fn hot_path_cost(store: &IdentityStore) -> (Option<u64>, u64) {
    let statements = crate::database::statement_calls();
    let refreshes = store.node_registry.shared_refreshes();
    for _ in 0..50 {
        page(store).await;
        crate::nodes::load_node(store, "local")
            .await
            .unwrap()
            .unwrap();
    }
    (
        crate::database::statement_calls()
            .zip(statements)
            .map(|(after, before)| after - before),
        store.node_registry.shared_refreshes() - refreshes,
    )
}

/// PostgreSQL with one live Core is as fast as before: no Node registry re-read and, measured
/// with `--features query-metrics` (process-wide counter: run with `--test-threads=1`), not one
/// statement more per catalogue request than with another Core but its interval re-reads.
#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_a_lone_core_adds_nothing_to_its_hot_path() {
    let (pool, schema, url) = postgres_schema("coherence_alone").await;
    let store = open_core(&url).await;
    heartbeat(&store, unix_now()).await.unwrap();
    assert!(!store.shared_with_other_cores(), "alone");
    seed(&store).await;
    crate::nodes::ensure_node_registry_warm(&store)
        .await
        .unwrap();
    page(&store).await;

    let (alone, alone_refreshes) = hot_path_cost(&store).await;
    assert_eq!(alone_refreshes, 0, "alone: the registry is never re-read");
    assert_eq!(store.node_registry.shared_refreshes(), 0);

    // A peer on another host: one re-read per interval, nothing per request.
    query("INSERT INTO core_instances(id,started_at,heartbeat_at,hostname) VALUES('peer',?,?,'peer-host')")
        .bind(unix_now())
        .bind(unix_now())
        .execute(store.pool())
        .await
        .unwrap();
    heartbeat(&store, unix_now()).await.unwrap();
    assert!(
        store.shared_with_other_cores(),
        "peer seen at the heartbeat"
    );
    crate::nodes::ensure_node_registry_warm(&store)
        .await
        .unwrap();
    assert_eq!(store.node_registry.shared_refreshes(), 1, "first re-read");
    let (shared, shared_refreshes) = hot_path_cost(&store).await;
    assert!(shared_refreshes <= 1, "at most one re-read per interval");
    if let (Some(alone), Some(shared)) = (alone, shared) {
        println!("50 warm requests: {alone} statements alone, {shared} with a peer");
        assert!(alone > 0, "statements are counted");
        assert_eq!(
            shared,
            alone + shared_refreshes,
            "with a peer: only the interval re-reads are added"
        );
    }
    store.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

/// Multi-Core mode: alone, a peer appears, the peer stops (row removed) or dies (silent).
#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_multi_core_mode_follows_the_live_peers() {
    let cores = two_cores("coherence_mode").await;
    let (a, b) = (&cores.a, &cores.b);
    crate::core_instances::deregister(b).await.unwrap();
    heartbeat(a, unix_now()).await.unwrap();
    assert!(!a.shared_with_other_cores(), "B stopped cleanly");
    heartbeat(b, unix_now()).await.unwrap();
    heartbeat(a, unix_now()).await.unwrap();
    assert!(a.shared_with_other_cores(), "B back");
    let later = unix_now() + crate::core_instances::HEARTBEAT_STALE_SECONDS + 1;
    query("UPDATE core_instances SET heartbeat_at=? WHERE id=?")
        .bind(later)
        .bind(a.instance_id())
        .execute(a.pool())
        .await
        .unwrap();
    heartbeat(a, later).await.unwrap();
    assert!(!a.shared_with_other_cores(), "B silent: dead");
    cores.close().await;
}

/// Exit proof §6 row 6, functional part: (i) catalogue writes, (ii) a revoked library grant,
/// (iii) a revoked session and a disabled user, (iv) a revoked Node and (v) Node telemetry, each
/// written by Core A,
/// reach Core B within their bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_writes_and_revocations_of_one_core_reach_the_other() {
    let cores = two_cores("coherence_reach").await;
    let (a, b) = (&cores.a, &cores.b);
    seed(a).await;

    // (i) Catalogue. B caches the page.
    assert!(page(b).await.contains("\"Alpha\""));
    let alpha = work_id(a, "Alpha").await;
    // A write nobody announces stays invisible: B really serves its cache.
    query("UPDATE inventory_works SET manual_json='{\"title\":\"Unannounced\"}' WHERE id=?")
        .bind(&alpha)
        .execute(a.pool())
        .await
        .unwrap();
    assert!(
        !page(b).await.contains("Unannounced"),
        "B answers from cache"
    );
    // An edit (announced after its commit) and a scan (generation moved in its transaction).
    let start = Instant::now();
    rename(a, &alpha, "Alpha Edited").await;
    observed(start, SECURITY_BOUND, "edit", || async {
        page(b).await.contains("Alpha Edited")
    })
    .await;
    let generation = || async {
        query_scalar::<sqlx::Any, i64>(
            "SELECT CAST(gen AS BIGINT) FROM inventory_cache_generation WHERE id=1",
        )
        .fetch_one(a.pool())
        .await
        .unwrap()
    };
    let before = generation().await;
    let start = Instant::now();
    crate::inventory::ingest(
        a,
        "lib",
        Path::new(ROOT),
        &ScanReport {
            files: vec![film("Alpha"), film("Beta"), film("Gamma")],
            complete: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        generation().await,
        before + 1,
        "moved in the scan transaction"
    );
    observed(start, SECURITY_BOUND, "scan", || async {
        page(b).await.contains("\"Gamma\"")
    })
    .await;

    // (ii) Library grant revoked on A: B refuses.
    let router_b = media_router(b).await;
    assert_eq!(status(&router_b, &alpha).await, StatusCode::OK);
    let start = Instant::now();
    query("DELETE FROM library_permissions WHERE library_id='lib' AND user_id='u1'")
        .execute(a.pool())
        .await
        .unwrap();
    observed(start, SECURITY_BOUND, "grant revocation", || async {
        status(&router_b, &alpha).await == StatusCode::FORBIDDEN
    })
    .await;

    // (iii) Session revoked on A (sign-out): B refuses.
    let start = Instant::now();
    query("DELETE FROM sessions WHERE token_hash=?")
        .bind(hash(TOKEN))
        .execute(a.pool())
        .await
        .unwrap();
    observed(start, SECURITY_BOUND, "session revocation", || async {
        status(&router_b, &alpha).await == StatusCode::UNAUTHORIZED
    })
    .await;
    // User disabled on A (sessions kept): B refuses.
    add_session(a).await;
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('lib','u1')")
        .execute(a.pool())
        .await
        .unwrap();
    observed(Instant::now(), SECURITY_BOUND, "grant restored", || async {
        status(&router_b, &alpha).await == StatusCode::OK
    })
    .await;
    let start = Instant::now();
    query("UPDATE users SET disabled_at=? WHERE id='u1'")
        .bind(unix_now())
        .execute(a.pool())
        .await
        .unwrap();
    observed(start, SECURITY_BOUND, "user disabled", || async {
        status(&router_b, &alpha).await == StatusCode::UNAUTHORIZED
    })
    .await;

    // (iv) Node revoked on A: B's registry (warm, never told) treats it as revoked.
    remote_node(a, "nas").await;
    crate::nodes::ensure_node_registry_warm(b).await.unwrap();
    assert_eq!(node_status(b, "nas").await, "connected");
    let start = Instant::now();
    revoke_node(a, "nas").await;
    observed(start, SECURITY_BOUND, "Node revocation", || async {
        node_status(b, "nas").await == "revoked"
    })
    .await;

    // (v) Telemetry received by A (owner of the heartbeats) reaches B's ranking.
    remote_node(a, "nas2").await;
    crate::nodes::refresh_node_in_registry(a, "nas2")
        .await
        .unwrap();
    let start = Instant::now();
    let now = unix_now();
    a.node_registry
        .record_heartbeat("nas2", &heartbeat_payload("nas2", 3), Some(7), now);
    crate::nodes::health::publish(a, "nas2", now).await;
    observed(start, SECURITY_BOUND, "telemetry", || async {
        crate::nodes::load_node_entry(b, "nas2")
            .await
            .unwrap()
            .is_some_and(|entry| entry.telemetry.reported_active_streams == 3)
    })
    .await;
    cores.close().await;
}

fn percentiles(name: &str, mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let max = *samples.last().unwrap();
    println!(
        "{name:<22} n={:<4} p50={:>8.2} ms  p95={:>8.2} ms  max={:>8.2} ms",
        samples.len(),
        ms(at(0.5)),
        ms(at(0.95)),
        ms(max)
    );
    max
}

/// Exit proof §6 row 6, measurement: propagation latency between two Cores of one process,
/// from the start of A's write to B's first request that observes it, polled every
/// millisecond. `COHERENCE_ITERATIONS` (default 100) per kind; Node revocation and telemetry,
/// bounded by the registry re-read interval, run `COHERENCE_ITERATIONS / 4` times.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_cross_core_propagation_latency() {
    let iterations: usize = std::env::var("COHERENCE_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let slow_iterations = (iterations / 4).max(5);
    let cores = two_cores("coherence_latency").await;
    let (a, b) = (&cores.a, &cores.b);
    seed(a).await;
    let alpha = work_id(a, "Alpha").await;
    let router_b = media_router(b).await;

    let mut catalogue = Vec::with_capacity(iterations);
    for round in 0..iterations {
        let title = format!("Alpha r{round}");
        assert!(
            !page(b).await.contains(&title),
            "B cached the previous title"
        );
        let start = Instant::now();
        rename(a, &alpha, &title).await;
        catalogue.push(
            observed(start, SECURITY_BOUND, "edit", || async {
                page(b).await.contains(&title)
            })
            .await,
        );
    }

    let mut grants = Vec::with_capacity(iterations);
    // Reference: B's first request after the grant came back (permission caches missed).
    let mut uncached = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        assert_eq!(status(&router_b, &alpha).await, StatusCode::OK);
        uncached.push(start.elapsed());
        let start = Instant::now();
        query("DELETE FROM library_permissions WHERE library_id='lib' AND user_id='u1'")
            .execute(a.pool())
            .await
            .unwrap();
        grants.push(
            observed(start, SECURITY_BOUND, "grant revocation", || async {
                status(&router_b, &alpha).await == StatusCode::FORBIDDEN
            })
            .await,
        );
        query("INSERT INTO library_permissions(library_id,user_id) VALUES('lib','u1')")
            .execute(a.pool())
            .await
            .unwrap();
    }

    let mut sessions = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        assert_eq!(status(&router_b, &alpha).await, StatusCode::OK);
        let start = Instant::now();
        query("DELETE FROM sessions WHERE token_hash=?")
            .bind(hash(TOKEN))
            .execute(a.pool())
            .await
            .unwrap();
        sessions.push(
            observed(start, SECURITY_BOUND, "session revocation", || async {
                status(&router_b, &alpha).await == StatusCode::UNAUTHORIZED
            })
            .await,
        );
        add_session(a).await;
    }

    let mut nodes = Vec::with_capacity(slow_iterations);
    let mut telemetry = Vec::with_capacity(slow_iterations);
    for round in 0..slow_iterations {
        let node = format!("nas-{round}");
        remote_node(a, &node).await;
        // B learns about the new Node (its registry re-reads at most every interval).
        observed(Instant::now(), SECURITY_BOUND, "new Node", || async {
            crate::nodes::load_node(b, &node).await.unwrap().is_some()
        })
        .await;
        let start = Instant::now();
        revoke_node(a, &node).await;
        nodes.push(
            observed(start, SECURITY_BOUND, "Node revocation", || async {
                node_status(b, &node).await == "revoked"
            })
            .await,
        );

        let node = format!("nas-hb-{round}");
        remote_node(a, &node).await;
        crate::nodes::refresh_node_in_registry(a, &node)
            .await
            .unwrap();
        let start = Instant::now();
        let now = unix_now();
        a.node_registry
            .record_heartbeat(&node, &heartbeat_payload(&node, 2), Some(7), now);
        crate::nodes::health::publish(a, &node, now).await;
        telemetry.push(
            observed(start, SECURITY_BOUND, "telemetry", || async {
                crate::nodes::load_node_entry(b, &node)
                    .await
                    .unwrap()
                    .is_some_and(|entry| entry.telemetry.reported_active_streams == 2)
            })
            .await,
        );
    }

    println!("Cross-Core propagation (two stores, one process, PostgreSQL):");
    let maxima = [
        percentiles("catalogue edit", catalogue),
        percentiles("library grant revoked", grants),
        percentiles("session revoked", sessions),
        percentiles("Node revoked", nodes),
        percentiles("Node telemetry", telemetry),
    ];
    println!("Reference, not a propagation:");
    percentiles("uncached media request", uncached);
    assert!(maxima.iter().all(|max| *max <= SECURITY_BOUND));
    cores.close().await;
}
