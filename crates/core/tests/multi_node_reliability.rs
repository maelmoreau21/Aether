//! One Core, several Media Nodes: reliability scenarios that the happy-path suites do not cover.
//!
//! Every scenario starts a real Core and two real Node HTTP servers on loopback TCP, with two
//! libraries (one per Node, different viewers) that hold the same film:
//!
//! * equivalent copies are offered as alternatives and used for failover, but only to a viewer
//!   who is allowed to read the other library; permissions are never merged across Nodes;
//! * a copy that is *not* provably the same content (different runtime) is never substituted;
//! * a Node that goes silent without any failed request is detected through missed heartbeats;
//! * direct-streaming capability tokens are bound to a Node, an asset, an action and a live
//!   session, and are refused when tampered, forged for another viewer or after logout;
//! * a protocol-version mismatch is rejected and the Node recovers on its next valid call;
//! * commands queued for a Node survive between and behind control polls.
//!
//! The scenarios need the generated fixtures (`scripts/generate-fixtures.ps1`) and, for the
//! non-equivalent copy, an `ffmpeg` binary; they say so and return early when it is missing.

use axum::http::{Method, Request, StatusCode, header};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use media_contracts::ids::{LibraryId, NodeId};
use media_contracts::protocol::{
    HDR_BODY_SHA256, HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE, HDR_PROTOCOL, HDR_SIGNATURE,
    HDR_TIMESTAMP, HeartbeatPayload, NodeCommandEnvelope, PROTOCOL_VERSION, PlaybackTokenClaims,
    issue_playback_token, random_nonce_hex, sha256_hex, sign_internal_request, unix_now,
};
use media_node::server::{NodeServerConfig, NodeServerState, app_with_state as node_router};
use media_poc::{
    auth::AuthState, database::query, database::query_scalar, identity::IdentityStore,
    node::Inventory, nodes::ACTIVE_CHANNELS, scans::ScanJobs, secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use zeroize::Zeroizing;

const ADMIN_LOGIN: &str = "admin-reliability";
const ADMIN_PASSWORD: &str = "reliability-admin-password-2026";

async fn http_request(
    base_url: &str,
    method: Method,
    path_and_query: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    let parsed = url::Url::parse(base_url).expect("valid base url");
    let host = parsed.host_str().expect("host");
    let port = parsed.port_or_known_default().expect("port");
    let stream = TcpStream::connect((host, port)).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let mut builder = Request::builder()
        .method(method)
        .uri(path_and_query)
        .header(header::HOST, format!("{host}:{port}"));
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let res = sender
        .send_request(builder.body(Full::new(Bytes::from(body))).expect("request"))
        .await
        .expect("send");
    let (status, headers) = (res.status(), res.headers().clone());
    (
        status,
        headers,
        res.into_body().collect().await.expect("body").to_bytes(),
    )
}

struct NodeHandle {
    id: String,
    url: String,
    root: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    stopped: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl NodeHandle {
    /// Abrupt stop: the Node's own runtime is dropped, so its sockets close without any goodbye.
    async fn kill(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(stopped) = self.stopped.take() {
            let _ = stopped.await;
        }
    }
}

async fn spawn_node_server(
    name: &str,
    root: PathBuf,
    data_dir: PathBuf,
    token: &str,
) -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
) {
    let (name, token) = (name.to_owned(), token.to_owned());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("node runtime");
        runtime.block_on(async move {
            let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let public_url = format!("http://127.0.0.1:{}", addr.port());
            let state = NodeServerState::initialize(NodeServerConfig {
                bind: addr,
                data_dir,
                allowed_roots: vec![root],
                bootstrap_token: Some(token),
                configured_node_id: None,
                configured_name: Some(name),
                public_url: Some(public_url.clone()),
                batch_size: 16,
            })
            .await
            .expect("node state");
            tokio::spawn(async move {
                let _ = axum::serve(listener, node_router(state)).await;
            });
            let _ = ready_tx.send(public_url);
            let _ = stop_rx.await;
        });
        drop(runtime);
        let _ = stopped_tx.send(());
    });
    (ready_rx.await.expect("node started"), stop_tx, stopped_rx)
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated")
        .join(name)
}

const NFO: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie><title>Etoile Polaire</title><year>2026</year><plot>Copie hebergee par plusieurs Nodes.</plot>
<uniqueid type="tmdb" default="true">707</uniqueid></movie>"#;

async fn place_film(root: &Path, source: &Path) {
    let dir = root.join("Etoile Polaire (2026) [tmdb-707]");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::copy(source, dir.join("Etoile Polaire (2026).mp4"))
        .await
        .unwrap();
    tokio::fs::write(dir.join("movie.nfo"), NFO).await.unwrap();
}

struct Cluster {
    _temp: tempfile::TempDir,
    _scheduler: media_poc::scheduler::Scheduler,
    core_origin: String,
    store: IdentityStore,
    admin_cookie: String,
    alice_cookie: String,
    alice_hash: String,
    bob_cookie: String,
    bob_hash: String,
    node1: NodeHandle,
    node2: NodeHandle,
    lib1: String,
    lib2: String,
    /// Playback asset hosted by Node 1 / Node 2.
    asset1: String,
    asset2: String,
}

impl Cluster {
    /// Node 1 holds `primary`, Node 2 holds `secondary`. Alice may read only library 1 (Node 1),
    /// Bob only library 2 (Node 2); the administrator reads both.
    async fn start(primary: &Path, secondary: &Path) -> Cluster {
        Self::start_with(primary, secondary, 0, true).await
    }

    /// `extra_films` additional titles are placed on Node 1; `scan: false` leaves both libraries
    /// unscanned (the scenario drives the scan itself).
    async fn start_with(
        primary: &Path,
        secondary: &Path,
        extra_films: usize,
        scan: bool,
    ) -> Cluster {
        let temp = tempfile::tempdir().unwrap();
        let base = tokio::fs::canonicalize(temp.path()).await.unwrap();
        let dirs = |n: &str| {
            (
                base.join(format!("{n}-root")),
                base.join(format!("{n}-data")),
            )
        };
        let (r1, d1) = dirs("n1");
        let (r2, d2) = dirs("n2");
        let (core_dir, core_local) = (base.join("core"), base.join("core-local"));
        for d in [&r1, &d1, &r2, &d2, &core_dir, &core_local] {
            tokio::fs::create_dir_all(d).await.unwrap();
        }
        place_film(&r1, primary).await;
        place_film(&r2, secondary).await;
        for index in 0..extra_films {
            let dir = r1.join(format!(
                "Film numero {index} (2026) [tmdb-{}]",
                5_000 + index
            ));
            tokio::fs::create_dir_all(&dir).await.unwrap();
            tokio::fs::copy(primary, dir.join(format!("Film numero {index} (2026).mp4")))
                .await
                .unwrap();
            tokio::fs::write(
                dir.join("movie.nfo"),
                format!(
                    "<movie><title>Film numero {index}</title><year>2026</year><uniqueid type=\"tmdb\" default=\"true\">{}</uniqueid></movie>",
                    5_000 + index
                ),
            )
            .await
            .unwrap();
        }

        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let core_origin = format!("http://127.0.0.1:{port}");
        let store = IdentityStore::open(&core_dir.join("core.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(ADMIN_LOGIN, Zeroizing::new(ADMIN_PASSWORD.into()))
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), core_origin.clone())
            .await
            .unwrap();
        let jobs = Arc::new(ScanJobs::default());
        let inventory = Inventory::local(core_local).await.unwrap();
        let scheduler_root = inventory.root().to_owned();
        let app = secure_app_with_jobs(inventory, port, auth.clone(), jobs.clone())
            .await
            .unwrap();
        // The production binary runs the scheduler beside the router; lease renewal, expiry and
        // retry of interrupted scans live there.
        let scheduler = media_poc::scheduler::Scheduler::start(auth, scheduler_root, jobs);
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (u1, s1, w1) = spawn_node_server("Node A", r1.clone(), d1, "pair-a").await;
        let (u2, s2, w2) = spawn_node_server("Node B", r2.clone(), d2, "pair-b").await;

        let (st, headers, body) = http_request(
            &core_origin,
            Method::POST,
            "/api/v1/auth/login",
            &[
                ("origin", &core_origin),
                ("content-type", "application/json"),
            ],
            serde_json::to_vec(&json!({"login": ADMIN_LOGIN, "password": ADMIN_PASSWORD})).unwrap(),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let admin_cookie = headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = serde_json::from_slice::<Value>(&body).unwrap()["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned();
        let admin = [
            ("origin", core_origin.as_str()),
            ("cookie", admin_cookie.as_str()),
            ("x-csrf-token", csrf.as_str()),
            ("content-type", "application/json"),
        ];

        let mut nodes = Vec::new();
        for (name, url, token) in [("Node A", &u1, "pair-a"), ("Node B", &u2, "pair-b")] {
            let (st, _, body) = http_request(
                &core_origin,
                Method::POST,
                "/api/v1/admin/nodes/pair",
                &admin,
                serde_json::to_vec(&json!({
                    "name": name, "baseUrl": url, "publicUrl": url, "pairingToken": token
                }))
                .unwrap(),
            )
            .await;
            assert_eq!(st, StatusCode::CREATED, "pair {name}");
            let value: Value = serde_json::from_slice(&body).unwrap();
            nodes.push((
                value["id"].as_str().unwrap().to_owned(),
                value["roots"][0].as_str().unwrap().to_owned(),
            ));
        }

        let mut users = Vec::new();
        for (uid, login) in [("user-alice", "alice"), ("user-bob", "bob")] {
            let raw = login[..1].repeat(64);
            query("INSERT INTO users(id, role) VALUES (?, 'user')")
                .bind(uid)
                .execute(store.pool())
                .await
                .unwrap();
            query("INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, 'unused')")
                .bind(uid)
                .bind(login)
                .execute(store.pool())
                .await
                .unwrap();
            let hash = sha256_hex(raw.as_bytes());
            query("INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at) VALUES (?, ?, unixepoch(), unixepoch(), unixepoch() + 3600)")
                .bind(&hash)
                .bind(uid)
                .execute(store.pool())
                .await
                .unwrap();
            users.push((format!("media_session={raw}"), hash));
        }

        let mut libs = Vec::new();
        for (idx, (node_id, root)) in nodes.iter().enumerate() {
            let (st, _, body) = http_request(
                &core_origin,
                Method::POST,
                "/api/v1/admin/libraries",
                &admin,
                serde_json::to_vec(&json!({
                    "name": format!("Films {idx}"), "kind": "films", "nodeId": node_id,
                    "root": root, "enabled": true,
                    "userIds": [if idx == 0 { "user-alice" } else { "user-bob" }]
                }))
                .unwrap(),
            )
            .await;
            assert_eq!(
                st,
                StatusCode::CREATED,
                "{}",
                String::from_utf8_lossy(&body)
            );
            libs.push(
                serde_json::from_slice::<Value>(&body).unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        for lib in libs.iter().filter(|_| scan) {
            let uri = format!("/api/v1/admin/libraries/{lib}/scan");
            let (st, _, _) = http_request(&core_origin, Method::POST, &uri, &admin, vec![]).await;
            assert_eq!(st, StatusCode::ACCEPTED);
            tokio::time::timeout(Duration::from_secs(90), async {
                loop {
                    let (_, _, body) = http_request(
                        &core_origin,
                        Method::GET,
                        &uri,
                        &[("cookie", &admin_cookie)],
                        vec![],
                    )
                    .await;
                    let status: Value = serde_json::from_slice(&body).unwrap();
                    assert_ne!(status["status"], "error", "scan failed: {status}");
                    if status["status"] == "completed" {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("scan timed out");
        }

        // The two libraries share the TMDB id, so the catalogue consolidates them into one work.
        let (asset1, asset2) = if scan {
            let (_, _, body) = http_request(
                &core_origin,
                Method::GET,
                "/api/v1/catalog?kind=film",
                &[("cookie", &admin_cookie)],
                vec![],
            )
            .await;
            let catalog: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(catalog["total"], 1, "one consolidated work: {catalog}");
            let work = catalog["items"][0]["id"].as_str().unwrap().to_owned();
            let (_, _, body) = http_request(
                &core_origin,
                Method::GET,
                &format!("/api/v1/catalog/{work}"),
                &[("cookie", &admin_cookie)],
                vec![],
            )
            .await;
            let detail: Value = serde_json::from_slice(&body).unwrap();
            let mut by_node = std::collections::HashMap::new();
            for edition in detail["editions"].as_array().unwrap() {
                for version in edition["versions"].as_array().unwrap() {
                    for source in version["sources"].as_array().unwrap() {
                        by_node.insert(
                            source["nodeId"].as_str().unwrap().to_owned(),
                            source["playbackMediaId"].as_str().unwrap().to_owned(),
                        );
                    }
                }
            }
            (
                by_node.get(&nodes[0].0).expect("node 1 asset").clone(),
                by_node.get(&nodes[1].0).expect("node 2 asset").clone(),
            )
        } else {
            (String::new(), String::new())
        };

        // Both Nodes just proved they are alive through signed calls; give them fresh telemetry.
        let now = unix_now();
        for (id, _) in &nodes {
            store.node_registry().record_success(id, Some(2), now);
        }

        Cluster {
            _temp: temp,
            _scheduler: scheduler,
            core_origin,
            store,
            admin_cookie,
            alice_cookie: users[0].0.clone(),
            alice_hash: users[0].1.clone(),
            bob_cookie: users[1].0.clone(),
            bob_hash: users[1].1.clone(),
            node1: NodeHandle {
                id: nodes[0].0.clone(),
                url: u1,
                root: nodes[0].1.clone(),
                stop: Some(s1),
                stopped: Some(w1),
            },
            node2: NodeHandle {
                id: nodes[1].0.clone(),
                url: u2,
                root: nodes[1].1.clone(),
                stop: Some(s2),
                stopped: Some(w2),
            },
            lib1: libs[0].clone(),
            lib2: libs[1].clone(),
            asset1,
            asset2,
        }
    }

    async fn descriptor(&self, cookie: &str, asset: &str) -> (StatusCode, Value) {
        let (status, _, body) = http_request(
            &self.core_origin,
            Method::GET,
            &format!("/api/v1/media/{asset}"),
            &[("cookie", cookie)],
            vec![],
        )
        .await;
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// The Core stops hearing from a Node, the way it would after a power cut: no request fails,
    /// the last sign of life simply gets older than the heartbeat tolerance.
    async fn silence(&self, node: &str) {
        let stale = unix_now() - 3_600;
        self.store
            .node_registry()
            .record_success(node, None, stale);
        let _ = query("UPDATE media_nodes SET last_seen_at=? WHERE id=?")
            .bind(stale)
            .bind(node)
            .execute(self.store.pool())
            .await;
    }

    async fn node_secret(&self, node: &str) -> (String, String) {
        let secret = query_scalar::<_, String>("SELECT shared_secret FROM media_nodes WHERE id=?")
            .bind(node)
            .fetch_one(self.store.pool())
            .await
            .unwrap();
        let key = query_scalar::<_, String>("SELECT key_id FROM media_nodes WHERE id=?")
            .bind(node)
            .fetch_one(self.store.pool())
            .await
            .unwrap();
        (secret, key)
    }

    /// POST as a Node would: correctly signed, with a chosen protocol version.
    async fn signed_node_post(
        &self,
        node: &NodeHandle,
        path: &str,
        body: Vec<u8>,
        protocol: &str,
    ) -> (StatusCode, Bytes) {
        let (secret, key_id) = self.node_secret(&node.id).await;
        let (now, nonce, sha) = (unix_now(), random_nonce_hex(), sha256_hex(&body));
        let signature = sign_internal_request(
            &secret,
            "node-to-core",
            "POST",
            path,
            &node.id,
            &key_id,
            now,
            &nonce,
            &sha,
        );
        let (status, _, bytes) = http_request(
            &self.core_origin,
            Method::POST,
            path,
            &[
                (HDR_PROTOCOL, protocol),
                (HDR_NODE_ID, &node.id),
                (HDR_KEY_ID, &key_id),
                (HDR_TIMESTAMP, &now.to_string()),
                (HDR_NONCE, &nonce),
                (HDR_BODY_SHA256, &sha),
                (HDR_SIGNATURE, &signature),
                ("content-type", "application/json"),
            ],
            body,
        )
        .await;
        (status, bytes)
    }

    fn heartbeat(&self, node: &NodeHandle) -> Vec<u8> {
        serde_json::to_vec(&HeartbeatPayload {
            protocol_version: PROTOCOL_VERSION.into(),
            version: Some("v1.0.0".into()),
            node_id: node.id.clone().into(),
            roots: vec![node.root.clone()],
            public_url: Some(node.url.clone()),
            active_scans: 0,
            active_scan_ids: Vec::new(),
            active_streams: 0,
            resource_pressure: None,
            indexed_assets: 1,
            timestamp: unix_now(),
        })
        .unwrap()
    }
}

fn identical_copy() -> Option<PathBuf> {
    let path = fixture("av1-opus.mp4");
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipped: generated fixtures are missing ({path:?})");
        None
    }
}

/// A shorter cut of the same film, produced by stream-copying the first seconds.
fn shorter_cut(source: &Path, into: &Path) -> Option<PathBuf> {
    let target = into.join("short-cut.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-y", "-i"])
        .arg(source)
        .args([
            "-t",
            "3",
            "-map",
            "0",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(&target)
        .status()
        .ok()?;
    status.success().then_some(target)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn equivalent_copy_is_a_failover_target_only_for_viewers_allowed_to_read_it() {
    let Some(mp4) = identical_copy() else { return };
    let mut c = Cluster::start(&mp4, &mp4).await;

    // The administrator reads both libraries: the other Node's copy is an offered alternative.
    let (status, admin_view) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admin_view["fallbackMediaIds"], json!([c.asset2]));
    assert_eq!(admin_view["alternatives"][0]["id"], c.asset2);
    assert_eq!(admin_view["alternatives"][0]["nodeId"], c.node2.id);
    assert_eq!(admin_view["alternatives"][0]["nodeName"], "Node B");
    assert_eq!(admin_view["alternatives"][0]["equivalence"], "identical");

    // Alice may only read library 1. Nothing about Node B leaks into her descriptor, and she
    // cannot ask for Node B's copy directly either.
    let (status, alice_view) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alice_view["fallbackMediaIds"], json!([]));
    assert_eq!(alice_view["alternatives"], json!([]));
    let (status, _) = c.descriptor(&c.alice_cookie, &c.asset2).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
        "alice must not read library 2: {status}"
    );
    // Bob is the mirror image.
    let (_, bob_view) = c.descriptor(&c.bob_cookie, &c.asset2).await;
    assert_eq!(bob_view["alternatives"], json!([]));

    // Node A goes silent without a single failed request.
    c.silence(&c.node1.id).await;

    // Before the first playback request the Core already knows: the administrator is moved to
    // the equivalent copy, with no direct URL handed out for the silent Node.
    let (_, moved) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(
        moved["id"], c.asset2,
        "silent Node must trigger failover: {moved}"
    );
    assert_eq!(moved["failoverFrom"], c.asset1);
    assert_eq!(moved["nodeId"], c.node2.id);

    // Alice is *not* moved to a copy she has no right to read: she gets an honest "unavailable".
    let (_, stuck) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    assert_eq!(stuck["id"], c.asset1);
    assert!(stuck.get("failoverFrom").is_none(), "{stuck}");
    assert_eq!(stuck["available"], false);
    assert!(stuck["sources"][0].get("directUrl").is_none(), "{stuck}");
    assert_eq!(stuck["alternatives"], json!([]));

    // Reconnection: one valid signed heartbeat from the Node restores it, and the original
    // asset is served again without any failover marker.
    let (status, _) = c
        .signed_node_post(
            &c.node1,
            "/internal/v1/core/heartbeat",
            c.heartbeat(&c.node1),
            PROTOCOL_VERSION,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, back) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(back["id"], c.asset1);
    assert!(back.get("failoverFrom").is_none(), "{back}");
    assert_eq!(back["nodeStatus"], "connected");
    c.node1.kill().await;
    c.node2.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_copy_with_another_runtime_is_never_substituted() {
    let Some(mp4) = identical_copy() else { return };
    let scratch = tempfile::tempdir().unwrap();
    let Some(short) = shorter_cut(&mp4, scratch.path()) else {
        eprintln!("skipped: ffmpeg is not available to cut a shorter version");
        return;
    };
    let mut c = Cluster::start(&mp4, &short).await;

    // Same work (same TMDB id), but 3 s against 8 s: not the same content, so no alternative.
    let (_, admin_view) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(admin_view["fallbackMediaIds"], json!([]), "{admin_view}");
    assert_eq!(admin_view["alternatives"], json!([]), "{admin_view}");

    // With Node A gone the administrator is told the title is unavailable instead of being
    // silently moved to a different cut whose timeline the saved position does not match.
    c.silence(&c.node1.id).await;
    let (_, view) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(view["id"], c.asset1);
    assert!(view.get("failoverFrom").is_none(), "{view}");
    assert_eq!(view["available"], false);
    c.node1.kill().await;
    c.node2.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_tokens_are_bound_authorized_online_and_revoked_with_the_session() {
    let Some(mp4) = identical_copy() else { return };
    let mut c = Cluster::start(&mp4, &mp4).await;

    let (_, view) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    let direct = view["sources"][0]["directUrl"].as_str().unwrap().to_owned();
    let url = url::Url::parse(&direct).unwrap();
    let token = url
        .query_pairs()
        .find(|(k, _)| k == "token")
        .unwrap()
        .1
        .to_string();
    let path =
        |asset: &str, token: &str| format!("/stream/v1/assets/{asset}/content?token={token}");
    let origin = [("origin", c.core_origin.as_str())];
    let get = |p: String, range: Option<&'static str>| {
        let url = c.node1.url.clone();
        let origin = origin[0];
        async move {
            let mut headers = vec![origin];
            if let Some(range) = range {
                headers.push(("range", range));
            }
            http_request(&url, Method::GET, &p, &headers, vec![]).await
        }
    };

    // Range requests: a window, an open-ended tail, a suffix, and the refusals.
    let (st, h, body) = get(path(&c.asset1, &token), Some("bytes=0-15")).await;
    assert_eq!((st, body.len()), (StatusCode::PARTIAL_CONTENT, 16));
    assert!(
        h[header::CONTENT_RANGE]
            .to_str()
            .unwrap()
            .starts_with("bytes 0-15/")
    );
    let (st, _, body) = get(path(&c.asset1, &token), Some("bytes=-8")).await;
    assert_eq!((st, body.len()), (StatusCode::PARTIAL_CONTENT, 8));
    let (st, ..) = get(path(&c.asset1, &token), Some("bytes=999999999-")).await;
    assert_eq!(st, StatusCode::RANGE_NOT_SATISFIABLE);
    let (st, ..) = get(path(&c.asset1, &token), Some("bytes=0-1,4-5")).await;
    assert_eq!(
        st,
        StatusCode::RANGE_NOT_SATISFIABLE,
        "multi-range is refused"
    );

    // Missing, tampered, swapped-asset and truncated tokens.
    let (st, ..) = get(format!("/stream/v1/assets/{}/content", c.asset1), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let tampered = format!("{}x", token);
    let (st, ..) = get(path(&c.asset1, &tampered), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, ..) = get(path(&c.asset2, &token), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "a token is bound to its asset");
    let (st, ..) = get(path(&c.asset1, "no-dot-in-this-token"), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Properly signed claims that the Core must still refuse (the Node secret is the only thing
    // an attacker would lack; these simulate a bug in whatever issued the token).
    let (secret, key_id) = c.node_secret(&c.node1.id).await;
    let claims =
        |session: &str, library: &str, iat: i64, exp: i64, action: &str| PlaybackTokenClaims {
            v: PROTOCOL_VERSION.into(),
            kid: key_id.clone(),
            node_id: NodeId::new(&c.node1.id),
            target_kind: "asset".into(),
            target_id: c.asset1.clone(),
            library_id: LibraryId::new(library),
            action: action.into(),
            iat,
            exp,
            jti: random_nonce_hex(),
            session_hash: session.into(),
        };
    let now = unix_now();
    let forge = |claims: PlaybackTokenClaims| issue_playback_token(&secret, &claims).unwrap();
    // Expired, over-long lifetime, wrong action.
    for (label, token) in [
        (
            "expired",
            forge(claims(
                &c.alice_hash,
                &c.lib1,
                now - 1_000,
                now - 1,
                "stream",
            )),
        ),
        (
            "lifetime above the cap",
            forge(claims(&c.alice_hash, &c.lib1, now, now + 86_400, "stream")),
        ),
        (
            "wrong action",
            forge(claims(&c.alice_hash, &c.lib1, now, now + 60, "download")),
        ),
    ] {
        let (st, ..) = get(path(&c.asset1, &token), None).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{label}");
    }
    // Bob holds a valid token for a library he cannot read: the Core's live check refuses it.
    let bob_token = forge(claims(&c.bob_hash, &c.lib1, now, now + 60, "stream"));
    let (st, ..) = get(path(&c.asset1, &bob_token), None).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "permissions are checked online, not trusted from the token"
    );
    // A token naming the other library than the one that holds the asset.
    let wrong_library = forge(claims(&c.alice_hash, &c.lib2, now, now + 60, "stream"));
    let (st, ..) = get(path(&c.asset1, &wrong_library), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // The legitimate token still works until the session ends...
    let (st, ..) = get(path(&c.asset1, &token), Some("bytes=0-3")).await;
    assert_eq!(st, StatusCode::PARTIAL_CONTENT);
    // ...and stops working with it, although the token itself has not expired.
    query("DELETE FROM sessions WHERE token_hash=?")
        .bind(&c.alice_hash)
        .execute(c.store.pool())
        .await
        .unwrap();
    let (st, ..) = get(path(&c.asset1, &token), Some("bytes=0-3")).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "logout revokes direct access for new requests"
    );
    c.node1.kill().await;
    c.node2.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protocol_mismatch_is_refused_and_the_node_recovers_with_a_valid_call() {
    let Some(mp4) = identical_copy() else { return };
    let mut c = Cluster::start(&mp4, &mp4).await;

    // A Node from a different protocol generation is refused on every control endpoint, even
    // with a perfectly valid signature.
    for path in [
        "/internal/v1/core/heartbeat".to_owned(),
        format!("/internal/v1/core/nodes/{}/channel", c.node1.id),
    ] {
        let (status, body) = c
            .signed_node_post(&c.node1, &path, c.heartbeat(&c.node1), "v2")
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert!(String::from_utf8_lossy(&body).contains("unsupported_protocol"));
    }
    // An empty or missing version is refused the same way.
    let (status, _) = c
        .signed_node_post(
            &c.node1,
            "/internal/v1/core/heartbeat",
            c.heartbeat(&c.node1),
            "",
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // The refusal did not change what the Core believes about the Node...
    c.silence(&c.node1.id).await;
    let (status, _) = c
        .signed_node_post(
            &c.node1,
            "/internal/v1/core/heartbeat",
            c.heartbeat(&c.node1),
            "v2",
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, view) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(view["id"], c.asset2, "a refused call is not a sign of life");
    // ...and the first valid call after the upgrade/downgrade brings it back.
    let (status, _) = c
        .signed_node_post(
            &c.node1,
            "/internal/v1/core/heartbeat",
            c.heartbeat(&c.node1),
            PROTOCOL_VERSION,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, view) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(view["id"], c.asset1);
    c.node1.kill().await;
    c.node2.kill().await;
}

fn command(id: &str, kind: &str, extra: Value) -> NodeCommandEnvelope {
    let mut value = json!({"commandId": id, "commandType": kind, "payload": null});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().cloned().unwrap_or_default());
    serde_json::from_value(value).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_queued_behind_a_poll_or_between_polls_are_delivered_in_order() {
    let Some(mp4) = identical_copy() else { return };
    let mut c = Cluster::start(&mp4, &mp4).await;
    let channel_path = format!("/internal/v1/core/nodes/{}/channel", c.node1.id);
    let command_id = |body: &Bytes| {
        serde_json::from_slice::<Value>(body).unwrap()["commandId"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    // Poll 1 parks on the Core waiting for work.
    let first = {
        let (origin, node, body, path) = (
            c.core_origin.clone(),
            (c.node1.id.clone(), c.node_secret(&c.node1.id).await),
            c.heartbeat(&c.node1),
            channel_path.clone(),
        );
        tokio::spawn(async move { poll_once(&origin, &node.0, &node.1, &path, body).await })
    };
    let channel = loop {
        if let Some(channel) = ACTIVE_CHANNELS.read().await.get(&c.node1.id).cloned() {
            break channel;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // Three commands arrive while only one poll is open. The former design delivered the first
    // and dropped the other two together with the poll's receiver.
    for id in ["cmd-a", "cmd-b", "cmd-c"] {
        channel
            .try_send(command(id, "ping", json!({"expiresAt": unix_now() + 120})))
            .unwrap_or_else(|_| panic!("{id} refused"));
    }
    assert_eq!(command_id(&first.await.unwrap()), "cmd-a");

    // The Node re-polls straight away and receives the rest, in order, without waiting 25 s.
    let started = std::time::Instant::now();
    let mut delivered = Vec::new();
    for _ in 0..2 {
        let body = c
            .signed_node_post(
                &c.node1,
                &channel_path,
                c.heartbeat(&c.node1),
                PROTOCOL_VERSION,
            )
            .await
            .1;
        delivered.push(command_id(&body));
    }
    assert_eq!(delivered, ["cmd-b", "cmd-c"]);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    c.node1.kill().await;
    c.node2.kill().await;
}

async fn poll_once(
    origin: &str,
    node_id: &str,
    secret: &(String, String),
    path: &str,
    body: Vec<u8>,
) -> Bytes {
    let (now, nonce, sha) = (unix_now(), random_nonce_hex(), sha256_hex(&body));
    let signature = sign_internal_request(
        &secret.0,
        "node-to-core",
        "POST",
        path,
        node_id,
        &secret.1,
        now,
        &nonce,
        &sha,
    );
    http_request(
        origin,
        Method::POST,
        path,
        &[
            (HDR_PROTOCOL, PROTOCOL_VERSION),
            (HDR_NODE_ID, node_id),
            (HDR_KEY_ID, &secret.1),
            (HDR_TIMESTAMP, &now.to_string()),
            (HDR_NONCE, &nonce),
            (HDR_BODY_SHA256, &sha),
            (HDR_SIGNATURE, &signature),
            ("content-type", "application/json"),
        ],
        body,
    )
    .await
    .2
}

/// Open a request and hand back the body without collecting it, so a test can read part of it.
async fn open_body(
    base_url: &str,
    path_and_query: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, hyper::body::Incoming) {
    let parsed = url::Url::parse(base_url).expect("valid base url");
    let host = parsed.host_str().expect("host");
    let port = parsed.port_or_known_default().expect("port");
    let stream = TcpStream::connect((host, port)).await.expect("connect");
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(path_and_query)
        .header(header::HOST, format!("{host}:{port}"));
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let res = sender
        .send_request(builder.body(Full::new(Bytes::new())).expect("request"))
        .await
        .expect("send");
    let (status, headers) = (res.status(), res.headers().clone());
    (status, headers, res.into_body())
}

/// About 1 600 s of stream-copied video: far more bytes than the loopback socket buffers hold,
/// so a Node that dies mid-response has certainly not delivered the whole body.
fn long_copy(source: &Path, into: &Path) -> Option<PathBuf> {
    let target = into.join("long.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-stream_loop",
            "200",
            "-i",
        ])
        .arg(source)
        .args(["-map", "0", "-c", "copy", "-movflags", "+faststart"])
        .arg(&target)
        .status()
        .ok()?;
    status.success().then_some(target)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_killed_mid_read_interrupts_that_response_and_the_next_request_continues_elsewhere()
{
    let Some(mp4) = identical_copy() else { return };
    let scratch = tempfile::tempdir().unwrap();
    let Some(long) = long_copy(&mp4, scratch.path()) else {
        eprintln!("skipped: ffmpeg is not available to build a long file");
        return;
    };
    let on_disk = tokio::fs::read(&long).await.unwrap();
    let mut c = Cluster::start(&long, &long).await;

    let (_, view) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    let direct = url::Url::parse(view["sources"][0]["directUrl"].as_str().unwrap()).unwrap();
    let path_and_query = format!("{}?{}", direct.path(), direct.query().unwrap());
    let (status, headers, mut body) =
        open_body(&c.node1.url, &path_and_query, &[("origin", &c.core_origin)]).await;
    assert_eq!(status, StatusCode::OK);
    let total: usize = headers[header::CONTENT_LENGTH]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(total, on_disk.len());

    // Read the beginning, then pull the plug on the Node while the browser is mid-response.
    let mut received: Vec<u8> = Vec::new();
    while received.len() < 128 * 1024 {
        let frame = body
            .frame()
            .await
            .expect("body ended early")
            .expect("frame");
        if let Ok(data) = frame.into_data() {
            received.extend_from_slice(&data);
        }
    }
    // Orchestration is the Core's, the bytes are the Node's: nothing is relayed by the Core.
    assert_eq!(
        c.store
            .node_registry()
            .get(&c.node1.id)
            .unwrap()
            .telemetry
            .core_active_relays,
        0,
        "a direct read must not be carried by the Core"
    );
    c.node1.kill().await;
    let interrupted;
    loop {
        match body.frame().await {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    received.extend_from_slice(&data);
                }
            }
            Some(Err(_)) | None => {
                interrupted = received.len() < total;
                break;
            }
        }
    }
    // What the viewer experiences: the response stops short. It is not silently continued.
    assert!(
        interrupted,
        "the response must stop short, got {} of {total}",
        received.len()
    );
    assert_eq!(
        received[..],
        on_disk[..received.len()],
        "bytes before the cut are intact"
    );

    // Once the Core stops hearing from the Node (here: the heartbeat tolerance elapsed), the
    // player's next request lands on the equivalent copy...
    c.silence(&c.node1.id).await;
    let (_, moved) = c.descriptor(&c.admin_cookie, &c.asset1).await;
    assert_eq!(moved["id"], c.asset2, "{moved}");
    let (_, alice_view) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    assert_eq!(
        alice_view["available"], false,
        "alice has no right to the other copy"
    );

    // ...and resuming at the offset already received yields exactly the bytes that follow.
    let direct = url::Url::parse(moved["sources"][0]["directUrl"].as_str().unwrap()).unwrap();
    let path_and_query = format!("{}?{}", direct.path(), direct.query().unwrap());
    let start = received.len();
    let range = format!("bytes={start}-{}", start + 4_095);
    let (status, _, resumed) = http_request(
        &c.node2.url,
        Method::GET,
        &path_and_query,
        &[("origin", &c.core_origin), ("range", &range)],
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(resumed[..], on_disk[start..start + 4_096]);
    c.node2.kill().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "about 90 s: waits for the scan lease of a killed Node to expire; run explicitly"]
async fn a_node_killed_during_a_scan_ends_the_scan_without_a_partial_publication() {
    let Some(mp4) = identical_copy() else { return };
    let mut c = Cluster::start_with(&mp4, &mp4, 150, false).await;
    let scan_uri = |lib: &str| format!("/api/v1/admin/libraries/{lib}/scan");
    let admin_cookie = c.admin_cookie.clone();
    let core_origin = c.core_origin.clone();
    let status_of = |lib: String| {
        let (origin, cookie) = (core_origin.clone(), admin_cookie.clone());
        async move {
            let (_, _, body) = http_request(
                &origin,
                Method::GET,
                &format!("/api/v1/admin/libraries/{lib}/scan"),
                &[("cookie", &cookie)],
                vec![],
            )
            .await;
            serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null)
        }
    };
    // Admin mutations need the CSRF token: log in again to fetch one.
    let (_, headers, body) = http_request(
        &c.core_origin,
        Method::POST,
        "/api/v1/auth/login",
        &[
            ("origin", &c.core_origin),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({"login": ADMIN_LOGIN, "password": ADMIN_PASSWORD})).unwrap(),
    )
    .await;
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = serde_json::from_slice::<Value>(&body).unwrap()["csrfToken"]
        .as_str()
        .unwrap()
        .to_owned();
    let admin = [
        ("origin", c.core_origin.as_str()),
        ("cookie", cookie.as_str()),
        ("x-csrf-token", csrf.as_str()),
        ("content-type", "application/json"),
    ];

    let (st, _, _) = http_request(
        &c.core_origin,
        Method::POST,
        &scan_uri(&c.lib1),
        &admin,
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    // Let the Node start probing, then kill it.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    let before_kill = status_of(c.lib1.clone()).await;
    c.node1.kill().await;

    let started = std::time::Instant::now();
    let terminal = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let status = status_of(c.lib1.clone()).await;
            if matches!(status["status"].as_str(), Some("completed" | "error")) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("scan still not terminal 180 s after the Node was killed; status before kill: {before_kill}"));
    eprintln!(
        "scan_after_node_kill: terminal={} after {:?} (before kill: {})",
        terminal["status"],
        started.elapsed(),
        before_kill["status"]
    );
    assert_eq!(terminal["status"], "error", "{terminal}");

    // Nothing from the interrupted scan was published.
    let (_, _, body) = http_request(
        &c.core_origin,
        Method::GET,
        "/api/v1/catalog?kind=film",
        &[("cookie", &c.admin_cookie)],
        vec![],
    )
    .await;
    let catalog: Value = serde_json::from_slice(&body).unwrap();
    // (Library 2 is scanned automatically when created and may already have published its one
    // title; what must never appear is any title from the interrupted library.)
    let items = catalog["items"].as_array().unwrap();
    assert!(
        items.iter().all(|item| {
            !item["title"]
                .as_str()
                .unwrap_or("")
                .starts_with("Film numero")
                && item["libraryId"] != c.lib1.as_str()
        }),
        "no partial publication from the killed Node: {catalog}"
    );

    // The surviving Node is unaffected: its library scans and publishes normally.
    let (st, _, _) = http_request(
        &c.core_origin,
        Method::POST,
        &scan_uri(&c.lib2),
        &admin,
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let done = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let status = status_of(c.lib2.clone()).await;
            if matches!(status["status"].as_str(), Some("completed" | "error")) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the healthy Node's scan must finish");
    assert_eq!(done["status"], "completed", "{done}");
    c.node2.kill().await;
}

/// Reads `length` bytes of a response, sampling how many relays the Core is carrying.
async fn drain(
    mut body: hyper::body::Incoming,
    store: &IdentityStore,
    node: &str,
    started: std::time::Instant,
) -> (Vec<u8>, Duration, u32) {
    let mut bytes = Vec::new();
    let mut first_byte = None;
    let mut peak_relays = 0;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.expect("frame").into_data() {
            first_byte.get_or_insert_with(|| started.elapsed());
            bytes.extend_from_slice(&data);
            let relays = store
                .node_registry()
                .get(node)
                .map_or(0, |entry| entry.telemetry.core_active_relays);
            peak_relays = peak_relays.max(relays);
        }
    }
    (bytes, first_byte.unwrap_or_default(), peak_relays)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_streaming_bypasses_the_core_and_the_relay_costs_are_measured() {
    let Some(mp4) = identical_copy() else { return };
    let scratch = tempfile::tempdir().unwrap();
    let Some(long) = long_copy(&mp4, scratch.path()) else {
        eprintln!("skipped: ffmpeg is not available to build a long file");
        return;
    };
    let mut c = Cluster::start(&long, &long).await;
    let window = 16 * 1024 * 1024 - 1;
    let range = format!("bytes=0-{window}");

    let (_, view) = c.descriptor(&c.alice_cookie, &c.asset1).await;
    let direct = url::Url::parse(view["sources"][0]["directUrl"].as_str().unwrap()).unwrap();
    let direct_pq = format!("{}?{}", direct.path(), direct.query().unwrap());

    let started = std::time::Instant::now();
    let (status, _, body) = open_body(
        &c.node1.url,
        &direct_pq,
        &[("origin", &c.core_origin), ("range", &range)],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    let (direct_bytes, direct_ttfb, direct_peak) =
        drain(body, &c.store, &c.node1.id, started).await;
    let direct_total = started.elapsed();

    let started = std::time::Instant::now();
    let (status, _, body) = open_body(
        &c.core_origin,
        &format!("/api/v1/assets/{}/content", c.asset1),
        &[("cookie", &c.alice_cookie), ("range", &range)],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    let (relay_bytes, relay_ttfb, relay_peak) = drain(body, &c.store, &c.node1.id, started).await;
    let relay_total = started.elapsed();

    assert_eq!(direct_bytes.len(), 16 * 1024 * 1024);
    assert_eq!(
        sha256_hex(&direct_bytes),
        sha256_hex(&relay_bytes),
        "same bytes either way"
    );
    assert_eq!(
        direct_peak, 0,
        "the direct path never touches the Core's relay accounting"
    );
    assert!(
        relay_peak >= 1,
        "the relay is visible in the Core's accounting"
    );
    let mib = |d: Duration| 16.0 / d.as_secs_f64();
    eprintln!(
        "measure: {}",
        json!({
            "window_mib": 16,
            "direct": {"ttfb_ms": direct_ttfb.as_millis(), "total_ms": direct_total.as_millis(), "mib_s": mib(direct_total), "core_relays_peak": direct_peak},
            "relay": {"ttfb_ms": relay_ttfb.as_millis(), "total_ms": relay_total.as_millis(), "mib_s": mib(relay_total), "core_relays_peak": relay_peak},
            "build": "debug, loopback, Core and Nodes in one process"
        })
    );
    c.node1.kill().await;
    c.node2.kill().await;
}
