//! End-to-end distributed Core + 2 Remote Media Nodes integration suite for Milestone P2.
//!
//! Covers the full 12-step multi-Node flow over real TCP listeners:
//! 1. Start Core (`secure_app_with_jobs`) and 2 independent Nodes (`media_node::server::NodeServerState`).
//! 2. Pair both Nodes from an administrator action (`POST /api/v1/admin/nodes/pair`) + reject invalid pairing token.
//! 3. Persist and verify Node identities, statuses (`connected`), and allowed roots in SQLite (`GET /api/v1/admin/nodes`).
//! 4. Attach a `films` library to Node 1 and a `series` library to Node 2, and grant library access to a standard user.
//! 5. Request scans via `POST /api/v1/admin/libraries/{id}/scan`.
//! 6. Execute scans on the owning remote Nodes, transmit bounded batches (`POST /internal/v1/core/scans/{id}/batches`),
//!    and finalize (`POST /internal/v1/core/scans/{id}/complete`) + sync operational inventory back to each Node.
//! 7. Verify global catalogue publication (`GET /api/v1/catalog` and `GET /api/v1/catalog/{id}`) for films, series,
//!    seasons, and episodes, as well as strict RBAC permission enforcement.
//! 8. Verify media playback descriptors (`GET /api/v1/media/{id}`) expose both `directUrl` (with a short-lived
//!    node/asset/action-bound `PlaybackToken`) and `relayUrl`.
//! 9. Stream media and posters both **directly** from Node 1 / Node 2 (`/stream/v1/assets/{id}/content?token=...`)
//!    and via **Core relay** (`/api/v1/assets/{id}/content`), verifying `GET`, `HEAD`, HTTP `206 Partial Content`
//!    (`Range: bytes=...`), and HTTP `416 Range Not Satisfiable`.
//! 10. Rescan both libraries and verify UUID preservation (`work_id`, `edition_id`, `asset_id` unchanged, `reused > 0`)
//!     and zero writes in media directories.
//! 11. Simulate Node 1 unavailability: verify `POST /api/v1/admin/nodes/{id}/reconnect` marks Node 1 `disconnected`,
//!     the catalogue keeps the film with `available: false` and `nodeStatus: "disconnected"`, and reconnecting restores
//!     `available: true`.
//! 12. Verify secret rotation (`POST /api/v1/admin/nodes/{id}/rotate-secret` invalidates prior `PlaybackToken`s),
//!     Node revocation (`POST /api/v1/admin/nodes/{id}/revoke`), expired token rejection, and unknown Node identity rejection.

use axum::http::{Method, Request, StatusCode, header};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use media_contracts::{
    ids::{LibraryId, NodeId},
    protocol::{
        HDR_BODY_SHA256, HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE, HDR_PROTOCOL, HDR_SIGNATURE,
        HDR_TIMESTAMP, PROTOCOL_VERSION, PlaybackTokenClaims, issue_playback_token, sha256_hex,
        sign_internal_request, unix_now,
    },
};
use media_node::server::{NodeServerConfig, NodeServerState, app_with_state as node_router};
use media_poc::{
    auth::AuthState, identity::IdentityStore, node::Inventory, scans::ScanJobs,
    secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use zeroize::Zeroizing;

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
    let host_header = format!("{host}:{port}");
    let stream = TcpStream::connect((host, port)).await.expect("connect tcp");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .expect("http1 handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut builder = Request::builder()
        .method(method)
        .uri(path_and_query)
        .header(header::HOST, &host_header);
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let req = builder.body(Full::new(Bytes::from(body))).expect("request");
    let res = sender.send_request(req).await.expect("send request");
    let status = res.status();
    let res_headers = res.headers().clone();
    let res_bytes = res.into_body().collect().await.expect("body").to_bytes();
    (status, res_headers, res_bytes)
}

async fn populate_test_libraries(films_root: &Path, series_root: &Path) {
    let fixtures_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let mp4_src = fixtures_dir.join("av1-opus.mp4");
    let mkv_src = fixtures_dir.join("av1-opus.mkv");

    // Node 1: Films library
    let film_dir = films_root.join("Horizon Stellaire (2025) [tmdb-901]");
    tokio::fs::create_dir_all(&film_dir)
        .await
        .expect("create film dir");
    tokio::fs::copy(&mp4_src, film_dir.join("Horizon Stellaire (2025).mp4"))
        .await
        .expect("copy film mp4");
    tokio::fs::write(
        film_dir.join("Horizon Stellaire (2025).fr.forced.srt"),
        "1\n00:00:00,000 --> 00:00:10,000\nSous-titre du Node distant",
    )
    .await
    .unwrap();
    assert!(
        tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&mp4_src)
            .args(["-map", "0:a:0", "-vn", "-c:a", "copy"])
            .arg(film_dir.join("Horizon Stellaire (2025).en.opus"))
            .status()
            .await
            .unwrap()
            .success()
    );
    tokio::fs::write(
        film_dir.join("movie.nfo"),
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie>
  <title>Horizon Stellaire</title>
  <year>2025</year>
  <plot>Exploration distribuée sur le Node 1.</plot>
  <uniqueid type="tmdb" default="true">901</uniqueid>
</movie>"#,
    )
    .await
    .expect("write movie.nfo");
    image::DynamicImage::new_rgb8(960, 1440)
        .save(film_dir.join("poster.png"))
        .expect("write film poster");

    let music = films_root.join("Music");
    tokio::fs::create_dir_all(&music).await.unwrap();
    tokio::fs::copy(
        film_dir.join("Horizon Stellaire (2025).en.opus"),
        music.join("sample.opus"),
    )
    .await
    .unwrap();
    // Node 2: Series library
    let show_dir = series_root.join("Chroniques Orbitales (2024) [tvdb-777]");
    let season_dir = show_dir.join("Season 01");
    tokio::fs::create_dir_all(&season_dir)
        .await
        .expect("create season dir");
    tokio::fs::copy(
        &mkv_src,
        season_dir.join("Chroniques Orbitales - S01E01 - Premier Contact.mkv"),
    )
    .await
    .expect("copy episode 1");
    tokio::fs::copy(
        &mp4_src,
        season_dir.join("Chroniques Orbitales - S01E02 - Relais Sûr.mp4"),
    )
    .await
    .expect("copy episode 2");
    tokio::fs::write(
        show_dir.join("tvshow.nfo"),
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<tvshow>
  <title>Chroniques Orbitales</title>
  <year>2024</year>
  <plot>Série hébergée sur le Node 2.</plot>
  <uniqueid type="tvdb" default="true">777</uniqueid>
</tvshow>"#,
    )
    .await
    .expect("write tvshow.nfo");
    image::DynamicImage::new_rgb8(32, 48)
        .save(show_dir.join("poster.png"))
        .expect("write show poster");
}

async fn spawn_node_server(
    name: &str,
    root: PathBuf,
    data_dir: PathBuf,
    pairing_token: &str,
) -> (
    String,
    NodeServerState,
    tokio::sync::oneshot::Sender<()>,
    SocketAddr,
) {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind node listener");
    let addr = listener.local_addr().expect("node local addr");
    let public_url = format!("http://127.0.0.1:{}", addr.port());
    let config = NodeServerConfig {
        bind: addr,
        data_dir,
        allowed_roots: vec![root],
        bootstrap_token: Some(pairing_token.to_owned()),
        configured_node_id: None,
        configured_name: Some(name.to_owned()),
        public_url: Some(public_url.clone()),
        batch_size: 16,
    };
    let state = NodeServerState::initialize(config)
        .await
        .expect("initialize node state");
    let app = node_router(state.clone());
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    (public_url, state, tx, addr)
}

/// Real signed Wasm fixture, scoped to the two paired Nodes' libraries. This
/// provider receives catalogue context only; physical scans stay on the Nodes.
async fn install_automatic_metadata_fixture(store: &IdentityStore, libraries: &[&str]) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use media_extension_sdk::{Manifest, Package};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sha2::{Digest, Sha256};
    let output=json!({"apiVersion":1,"result":{"title":"Automatic proposal","summary":"Distributed signed fixture"}}).to_string();
    let data = output
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1 256)
        (data (i32.const 4096) "{data}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,
        (4096u64 << 32) | output.len() as u64
    ))
    .unwrap();
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let signing = Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap();
    let mut package = Package {
        manifest: Manifest {
            api_version: 1,
            id: "distributed.fixture".into(),
            name: "Distributed fixture".into(),
            version: "1.0.0".into(),
            description: "Legal synthetic provider".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/distributed".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "distributed-fixture".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(signing.sign(&package.signing_bytes().unwrap()).as_ref());
    media_poc::extensions::validate_package(&package, signing.public_key().as_ref()).unwrap();
    media_poc::database::query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('distributed-fixture',?,1)",
    )
    .bind(STANDARD.encode(signing.public_key().as_ref()))
    .execute(store.pool())
    .await
    .unwrap();
    media_poc::extension_storage::persist(store, &package)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('distributed.fixture','distributed-fixture',?,1,'distributed-generation')")
        .bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    for library in libraries {
        media_poc::database::query(
            "INSERT INTO extension_grants(extension_id,library_id) VALUES('distributed.fixture',?)",
        )
        .bind(*library)
        .execute(store.pool())
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn distributed_core_and_two_remote_nodes_end_to_end() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = tokio::fs::canonicalize(temp.path())
        .await
        .expect("canonicalize tempdir");

    let core_dir = base.join("core");
    let core_empty_local = base.join("core-local-empty");
    let node1_root = base.join("node1-films");
    let node1_data = base.join("node1-data");
    let node2_root = base.join("node2-series");
    let node2_data = base.join("node2-data");

    tokio::fs::create_dir_all(&core_dir).await.unwrap();
    tokio::fs::create_dir_all(&core_empty_local).await.unwrap();
    tokio::fs::create_dir_all(&node1_root).await.unwrap();
    tokio::fs::create_dir_all(&node1_data).await.unwrap();
    tokio::fs::create_dir_all(&node2_root).await.unwrap();
    tokio::fs::create_dir_all(&node2_data).await.unwrap();

    populate_test_libraries(&node1_root, &node2_root).await;

    // 1. Start Core on a real TCP listener
    let core_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind core listener");
    let core_port = core_listener.local_addr().unwrap().port();
    let core_origin = format!("http://127.0.0.1:{core_port}");

    let store = if let Ok(raw) = std::env::var("MEDIA_TEST_POSTGRES_URL") {
        let raw = Zeroizing::new(raw);
        let pool = sqlx::PgPool::connect(&raw)
            .await
            .expect("temporary PostgreSQL");
        let schema = format!("distributed_{}", uuid::Uuid::new_v4().simple());
        media_poc::database::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&pool)
            .await
            .unwrap();
        let mut url = url::Url::parse(&raw).unwrap();
        url.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let url = Zeroizing::new(url.to_string());
        pool.close().await;
        // This suite must point at a disposable PostgreSQL test database.
        IdentityStore::open_postgres(&url)
            .await
            .expect("open core postgres")
    } else {
        IdentityStore::open(&core_dir.join("core.sqlite"))
            .await
            .expect("open core sqlite")
    };
    store
        .bootstrap(
            "admin-dist",
            Zeroizing::new("distributed-admin-pass-2026".into()),
        )
        .await
        .expect("bootstrap admin");

    let auth = AuthState::new(store.clone(), core_origin.clone())
        .await
        .expect("auth state");
    let jobs = Arc::new(ScanJobs::default());
    let scheduler_root = core_empty_local.clone();
    let inventory = Inventory::local(core_empty_local).await.expect("local inv");
    let core_app = secure_app_with_jobs(inventory, core_port, auth.clone(), jobs.clone())
        .await
        .expect("distributed core app");
    let _process_scheduler =
        media_poc::scheduler::Scheduler::start(auth.clone(), scheduler_root.clone(), jobs.clone());

    let (core_shutdown_tx, core_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(core_listener, core_app)
            .with_graceful_shutdown(async move {
                let _ = core_shutdown_rx.await;
            })
            .await;
    });

    // Start Node 1 (Films) and Node 2 (Series) on real TCP listeners
    let node1_token = "pair-secret-node-1-films";
    let node2_token = "pair-secret-node-2-series";
    let (node1_url, node1_state, node1_stop_tx, node1_addr) = spawn_node_server(
        "Node 1 Films",
        node1_root.clone(),
        node1_data.clone(),
        node1_token,
    )
    .await;
    let (node2_url, _node2_state, node2_stop_tx, _node2_addr) = spawn_node_server(
        "Node 2 Series",
        node2_root.clone(),
        node2_data.clone(),
        node2_token,
    )
    .await;

    // Verify health endpoints
    let (st, _, _) = http_request(
        &core_origin,
        Method::GET,
        "/internal/v1/health",
        &[(HDR_PROTOCOL, PROTOCOL_VERSION)],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) =
        http_request(&node1_url, Method::GET, "/internal/v1/health", &[], vec![]).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) =
        http_request(&node2_url, Method::GET, "/internal/v1/health", &[], vec![]).await;
    assert_eq!(st, StatusCode::OK);

    // Log in as administrator on Core
    let (st, login_headers, login_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/auth/login",
        &[
            ("origin", &core_origin),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "login": "admin-dist",
            "password": "distributed-admin-pass-2026"
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let admin_cookie = login_headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login_json: Value = serde_json::from_slice(&login_body).unwrap();
    let admin_csrf = login_json["csrfToken"].as_str().unwrap().to_owned();

    // 2. Pair Node 1 and Node 2 (first verify wrong pairing token is rejected)
    let (bad_pair_st, _, _) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/nodes/pair",
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Node 1 Films",
            "baseUrl": node1_url,
            "publicUrl": node1_url,
            "pairingToken": "wrong-token"
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(bad_pair_st, StatusCode::BAD_GATEWAY);

    let (pair1_st, _, pair1_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/nodes/pair",
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Node 1 Films",
            "baseUrl": node1_url,
            "publicUrl": node1_url,
            "pairingToken": node1_token
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(
        pair1_st,
        StatusCode::CREATED,
        "pair1 body: {}",
        String::from_utf8_lossy(&pair1_body)
    );
    let node1_info: Value = serde_json::from_slice(&pair1_body).unwrap();
    let node1_id = node1_info["id"].as_str().unwrap().to_owned();
    assert_eq!(node1_info["status"], "connected");
    let node1_reported_root = node1_info["roots"][0].as_str().unwrap().to_owned();

    let (pair2_st, _, pair2_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/nodes/pair",
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Node 2 Series",
            "baseUrl": node2_url,
            "publicUrl": node2_url,
            "pairingToken": node2_token
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(pair2_st, StatusCode::CREATED);
    let node2_info: Value = serde_json::from_slice(&pair2_body).unwrap();
    let node2_id = node2_info["id"].as_str().unwrap().to_owned();
    assert_eq!(node2_info["status"], "connected");
    let node2_reported_root = node2_info["roots"][0].as_str().unwrap().to_owned();
    assert_ne!(node1_id, node2_id);

    // 3. Verify nodes persisted in SQLite via GET /api/v1/admin/nodes
    let (nodes_st, _, nodes_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/admin/nodes",
        &[("cookie", &admin_cookie)],
        vec![],
    )
    .await;
    assert_eq!(nodes_st, StatusCode::OK);
    let nodes_list: Value = serde_json::from_slice(&nodes_body).unwrap();
    let nodes_arr = nodes_list["nodes"].as_array().unwrap();
    assert!(
        nodes_arr
            .iter()
            .any(|n| n["id"] == node1_id && n["status"] == "connected")
    );
    assert!(
        nodes_arr
            .iter()
            .any(|n| n["id"] == node2_id && n["status"] == "connected")
    );

    // Seed two standard users: `alice` (granted access to both libraries) and `bob` (no library access)
    let alice_id = "user-alice".to_owned();
    let bob_id = "user-bob".to_owned();
    let alice_raw_token = "a".repeat(64);
    let bob_raw_token = "b".repeat(64);
    for (uid, login_name, raw_tok) in [
        (&alice_id, "alice", &alice_raw_token),
        (&bob_id, "bob", &bob_raw_token),
    ] {
        media_poc::database::query("INSERT INTO users(id, role) VALUES (?, 'user')")
            .bind(uid)
            .execute(store.pool())
            .await
            .unwrap();
        media_poc::database::query(
            "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, 'unused-test-hash')",
        )
        .bind(uid)
        .bind(login_name)
        .execute(store.pool())
        .await
        .unwrap();
        let tok_hash = sha256_hex(raw_tok.as_bytes());
        media_poc::database::query(
            "INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at) VALUES (?, ?, unixepoch(), unixepoch(), unixepoch() + 3600)",
        )
        .bind(tok_hash)
        .bind(uid)
        .execute(store.pool())
        .await
        .unwrap();
    }
    let alice_cookie = format!("media_session={alice_raw_token}");
    let bob_cookie = format!("media_session={bob_raw_token}");

    // 4. Attach Films library to Node 1 and Series library to Node 2 (granting access to `alice`)
    let (lib1_st, _, lib1_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/libraries",
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Films Distribués",
            "kind": "films",
            "nodeId": node1_id,
            "root": node1_reported_root,
            "enabled": true,
            "userIds": [&alice_id]
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(
        lib1_st,
        StatusCode::CREATED,
        "lib1: {}",
        String::from_utf8_lossy(&lib1_body)
    );
    let lib1_id = serde_json::from_slice::<Value>(&lib1_body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (lib2_st, _, lib2_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/libraries",
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
            ("content-type", "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Séries Distribuées",
            "kind": "series",
            "nodeId": node2_id,
            "root": node2_reported_root,
            "enabled": true,
            "userIds": [&alice_id]
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(
        lib2_st,
        StatusCode::CREATED,
        "lib2: {}",
        String::from_utf8_lossy(&lib2_body)
    );
    let lib2_id = serde_json::from_slice::<Value>(&lib2_body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    install_automatic_metadata_fixture(&store, &[&lib1_id, &lib2_id]).await;
    let automation_started = std::time::Instant::now();

    // 5 & 6. Trigger scans on both remote libraries and wait for completion
    for lib_id in [&lib1_id, &lib2_id] {
        let (scan_st, _, _) = http_request(
            &core_origin,
            Method::POST,
            &format!("/api/v1/admin/libraries/{lib_id}/scan"),
            &[
                ("origin", &core_origin),
                ("cookie", &admin_cookie),
                ("x-csrf-token", &admin_csrf),
            ],
            vec![],
        )
        .await;
        assert_eq!(scan_st, StatusCode::ACCEPTED);
    }

    for lib_id in [&lib1_id, &lib2_id] {
        let mut completed = false;
        for _ in 0..80 {
            let (st, _, body) = http_request(
                &core_origin,
                Method::GET,
                &format!("/api/v1/admin/libraries/{lib_id}/scan"),
                &[("cookie", &admin_cookie)],
                vec![],
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            let job: Value = serde_json::from_slice(&body).unwrap();
            if job["status"] == "completed" {
                assert!(job["discovered"].as_u64().unwrap() >= 1);
                assert_eq!(job["errorsCount"].as_u64().unwrap(), 0);
                completed = true;
                break;
            }
            assert_ne!(job["status"], "error", "scan failed: {job}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            completed,
            "remote scan for library {lib_id} did not complete in time"
        );
    }

    // Both real remote scans now feed the same durable event/extension queue.
    // The Core runs the signed Wasm worker after publication, without a manual
    // extension invocation. Distinct Node provenance must survive persistence.
    let mut automatic_count=tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            let expected:i64=media_poc::database::query_scalar("SELECT COUNT(*) FROM inventory_assets WHERE available=1 AND library_id IN (?,?)")
                .bind(&lib1_id).bind(&lib2_id).fetch_one(store.pool()).await.unwrap();
            let completed:i64=media_poc::database::query_scalar("SELECT COUNT(*) FROM automation_results r JOIN durable_tasks t ON t.id=r.task_id WHERE r.extension_id='distributed.fixture' AND t.status='completed'")
                .fetch_one(store.pool()).await.unwrap();
            if expected>=3 && completed==expected {break completed;}
            let errors:Vec<String>=media_poc::database::query_scalar("SELECT t.error FROM durable_tasks t JOIN automation_task_links a ON a.task_id=t.id WHERE t.status='error'")
                .fetch_all(store.pool()).await.unwrap();
            assert!(errors.is_empty(),"automatic extension failures: {errors:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("two real TCP Nodes did not complete automatic enrichment");
    let automatic_nodes:i64=media_poc::database::query_scalar("SELECT COUNT(DISTINCT t.node_id) FROM automation_results r JOIN durable_tasks t ON t.id=r.task_id WHERE r.extension_id='distributed.fixture'")
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(automatic_nodes, 2);
    println!(
        "REAL_TWO_NODE_AUTOMATION nodes=2 assets={automatic_count} elapsed_ms={:.3}; real TCP scans, signed Wasm subprocesses, common durable results; browser decoding latency not measured",
        automation_started.elapsed().as_secs_f64() * 1000.0
    );

    // Music is scanned on the owning remote Node and streamed under the same live library rights.
    let music_root = node1_root
        .join("Music")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (status,_,body)=http_request(&core_origin,Method::POST,"/api/v1/admin/libraries",&[("origin",&core_origin),("cookie",&admin_cookie),("x-csrf-token",&admin_csrf),("content-type","application/json")],serde_json::to_vec(&json!({"name":"Musique distante","kind":"music","nodeId":node1_id,"root":music_root,"enabled":true,"userIds":[&alice_id]})).unwrap()).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "music library: {}",
        String::from_utf8_lossy(&body)
    );
    let music_library = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        http_request(
            &core_origin,
            Method::POST,
            &format!("/api/v1/admin/libraries/{music_library}/scan"),
            &[
                ("origin", &core_origin),
                ("cookie", &admin_cookie),
                ("x-csrf-token", &admin_csrf)
            ],
            vec![]
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    let mut done = false;
    for _ in 0..100 {
        let (_, _, body) = http_request(
            &core_origin,
            Method::GET,
            &format!("/api/v1/admin/libraries/{music_library}/scan"),
            &[("cookie", &admin_cookie)],
            vec![],
        )
        .await;
        let job: Value = serde_json::from_slice(&body).unwrap();
        assert_ne!(job["status"], "error", "music scan: {job}");
        if job["status"] == "completed" {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done);
    let (status, _, body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/music",
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let music: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(music["total"], 1);
    let track = music["items"][0]["id"].as_str().unwrap();
    let path = format!("/api/v1/music/tracks/{track}/content");
    let (status, _, bytes) = http_request(
        &core_origin,
        Method::GET,
        &path,
        &[("cookie", &alice_cookie), ("range", "bytes=0-3")],
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(&bytes[..], b"OggS");
    assert_eq!(
        http_request(
            &core_origin,
            Method::GET,
            &path,
            &[("cookie", &bob_cookie)],
            vec![]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // 7. Verify global catalogue publication and permissions
    let (cat_st, _, cat_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/catalog",
        &[("cookie", &admin_cookie)],
        vec![],
    )
    .await;
    assert_eq!(cat_st, StatusCode::OK);
    let cat: Value = serde_json::from_slice(&cat_body).unwrap();
    assert_eq!(cat["total"], 2);
    let items = cat["items"].as_array().unwrap();
    let film_item = items
        .iter()
        .find(|i| i["kind"] == "film")
        .expect("film in catalog");
    let series_item = items
        .iter()
        .find(|i| i["kind"] == "series")
        .expect("series in catalog");
    assert_eq!(film_item["title"], "Horizon Stellaire");
    assert_eq!(film_item["nodeId"], node1_id);
    assert_eq!(film_item["nodeStatus"], "connected");
    assert_eq!(film_item["available"], true);
    assert!(
        film_item["posterUrl"]
            .as_str()
            .unwrap()
            .starts_with("/api/v1/assets/")
    );

    assert_eq!(series_item["title"], "Chroniques Orbitales");
    assert_eq!(series_item["nodeId"], node2_id);
    assert_eq!(series_item["nodeStatus"], "connected");
    assert_eq!(series_item["available"], true);
    assert_eq!(series_item["seasonCount"], 1);
    assert_eq!(series_item["episodeCount"], 2);
    assert!(
        series_item["posterUrl"]
            .as_str()
            .unwrap()
            .starts_with("/api/v1/assets/")
    );

    let film_work_id = film_item["id"].as_str().unwrap().to_owned();
    let series_work_id = series_item["id"].as_str().unwrap().to_owned();

    // Detail check for Film and Series
    let (fdet_st, _, fdet_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/catalog/{film_work_id}"),
        &[("cookie", &admin_cookie)],
        vec![],
    )
    .await;
    assert_eq!(fdet_st, StatusCode::OK);
    let fdet: Value = serde_json::from_slice(&fdet_body).unwrap();
    let film_edition_id = fdet["editions"][0]["id"].as_str().unwrap().to_owned();
    let film_asset_id = fdet["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_owned();

    let (sdet_st, _, sdet_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/catalog/{series_work_id}"),
        &[("cookie", &admin_cookie)],
        vec![],
    )
    .await;
    assert_eq!(sdet_st, StatusCode::OK);
    let sdet: Value = serde_json::from_slice(&sdet_body).unwrap();
    let ep1 = &sdet["seasons"][0]["episodes"][0];
    assert_eq!(ep1["number"], 1);
    let ep1_asset_id = ep1["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_owned();

    // Verify Bob (no permissions) gets 403 FORBIDDEN on catalog and media endpoints
    let (bob_cat_st, _, _) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/catalog",
        &[("cookie", &bob_cookie)],
        vec![],
    )
    .await;
    assert_eq!(bob_cat_st, StatusCode::FORBIDDEN);
    let (bob_media_st, _, _) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/media/{film_asset_id}"),
        &[("cookie", &bob_cookie)],
        vec![],
    )
    .await;
    assert_eq!(bob_media_st, StatusCode::FORBIDDEN);

    // 8 & 9. Fetch media descriptors as Alice (authorized standard user) and stream both directly from Nodes and via Core Relay
    for (asset_id, expected_node_url) in [(&film_asset_id, &node1_url), (&ep1_asset_id, &node2_url)]
    {
        let (md_st, _, md_body) = http_request(
            &core_origin,
            Method::GET,
            &format!("/api/v1/media/{asset_id}"),
            &[("cookie", &alice_cookie)],
            vec![],
        )
        .await;
        assert_eq!(md_st, StatusCode::OK);
        let md: Value = serde_json::from_slice(&md_body).unwrap();
        let prepared = md["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["prepare"] == true)
            .expect("remote prepared source");
        let audio = prepared["externalAudioUrl"].as_str().unwrap();
        let (status, _, bytes) = http_request(
            &core_origin,
            Method::GET,
            audio,
            &[("cookie", &alice_cookie), ("range", "bytes=0-3")],
            vec![],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::PARTIAL_CONTENT,
            "remote audio preparation: {}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(&bytes[..], b"OggS");
        let chunk = format!("{audio}-chunk-0");
        let (status, _, bytes) = http_request(
            &core_origin,
            Method::GET,
            &chunk,
            &[("cookie", &alice_cookie), ("range", "bytes=0-31")],
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT, "remote audio chunk");
        assert_eq!(&bytes[4..8], b"ftyp");
        let audio_window = format!("{}0", prepared["audioSegmentUrl"].as_str().unwrap());
        if asset_id == &film_asset_id {
            assert!(
                audio_window.contains("/prepared/opus-"),
                "validated MP4 uses native Opus"
            );
            assert_eq!(prepared["audioSegmentMode"], "absolute");
        }
        let progressive = format!("{}0", prepared["videoStreamUrl"].as_str().unwrap());
        for path in [&audio_window, &progressive] {
            for method in [Method::GET, Method::HEAD] {
                let (status, _, bytes) = http_request(
                    &core_origin,
                    method.clone(),
                    path,
                    &[("cookie", &alice_cookie)],
                    vec![],
                )
                .await;
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "authorized {method} {path}: {}",
                    String::from_utf8_lossy(&bytes)
                );
                if method == Method::HEAD {
                    assert!(bytes.is_empty());
                } else {
                    assert_eq!(&bytes[4..8], b"ftyp");
                }
                assert_eq!(
                    http_request(
                        &core_origin,
                        method,
                        path,
                        &[("cookie", &bob_cookie)],
                        vec![]
                    )
                    .await
                    .0,
                    StatusCode::FORBIDDEN
                );
            }
        }
        let (status, _, bytes) = http_request(
            &core_origin,
            Method::GET,
            &audio_window,
            &[("cookie", &alice_cookie), ("range", "bytes=0-31")],
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(&bytes[4..8], b"ftyp");
        assert_eq!(
            http_request(
                &core_origin,
                Method::GET,
                &progressive,
                &[("cookie", &alice_cookie), ("range", "bytes=0-31")],
                vec![]
            )
            .await
            .0,
            StatusCode::RANGE_NOT_SATISFIABLE
        );
        let video = prepared["url"].as_str().unwrap();
        let (status, headers, bytes) = http_request(
            &core_origin,
            Method::GET,
            video,
            &[("cookie", &alice_cookie), ("range", "bytes=0-31")],
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        let (fixture_name, expected_mime) = if asset_id == &film_asset_id {
            ("av1-opus.mp4", "video/mp4")
        } else {
            ("av1-opus.mkv", "video/x-matroska")
        };
        let original = tokio::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/generated")
                .join(fixture_name),
        )
        .await
        .unwrap();
        assert_eq!(video, format!("/api/v1/assets/{asset_id}/content"));
        assert_eq!(prepared["mime"], expected_mime);
        assert_eq!(prepared["prepareVideo"], false);
        assert_eq!(headers[header::CONTENT_TYPE], expected_mime);
        assert_eq!(bytes.as_ref(), &original[..32]);
        assert_eq!(
            http_request(
                &core_origin,
                Method::HEAD,
                video,
                &[("cookie", &alice_cookie)],
                vec![]
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            http_request(
                &core_origin,
                Method::GET,
                video,
                &[("cookie", &bob_cookie)],
                vec![]
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        if asset_id == &ep1_asset_id {
            // MKV decoding failures retain an explicit, authorized MP4 fallback.
            let fallback = prepared["preparedVideoUrl"].as_str().unwrap();
            assert_eq!(
                fallback,
                format!("/api/v1/assets/{asset_id}/prepared/video")
            );
            for method in [Method::GET, Method::HEAD] {
                let (status, headers, bytes) = http_request(
                    &core_origin,
                    method.clone(),
                    fallback,
                    &[("cookie", &alice_cookie), ("range", "bytes=0-31")],
                    vec![],
                )
                .await;
                assert_eq!(status, StatusCode::PARTIAL_CONTENT);
                assert_eq!(headers[header::CONTENT_TYPE], "video/mp4");
                assert_eq!(headers[header::CONTENT_LENGTH], "32");
                assert!(
                    headers[header::CONTENT_RANGE]
                        .to_str()
                        .unwrap()
                        .starts_with("bytes 0-31/")
                );
                if method == Method::GET {
                    assert_eq!(bytes.len(), 32);
                    assert_eq!(&bytes[4..8], b"ftyp");
                } else {
                    assert!(bytes.is_empty());
                }
                assert_eq!(
                    http_request(
                        &core_origin,
                        method,
                        fallback,
                        &[("cookie", &bob_cookie)],
                        vec![]
                    )
                    .await
                    .0,
                    StatusCode::FORBIDDEN
                );
            }
            assert_eq!(
                http_request(
                    &core_origin,
                    Method::GET,
                    fallback,
                    &[
                        ("cookie", &alice_cookie),
                        ("range", "bytes=999999999-1000000000")
                    ],
                    vec![],
                )
                .await
                .0,
                StatusCode::RANGE_NOT_SATISFIABLE
            );
        } else {
            assert!(prepared["preparedVideoUrl"].is_null());
        }
        if asset_id == &film_asset_id {
            let subtitle = md["subtitles"][0]["url"].as_str().unwrap();
            let (status, _, bytes) = http_request(
                &core_origin,
                Method::GET,
                subtitle,
                &[("cookie", &alice_cookie)],
                vec![],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(String::from_utf8_lossy(&bytes).contains("Node distant"));
            let audio = md["sources"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|s| s["externalAudioUrl"].as_str())
                .unwrap();
            let (status, _, bytes) = http_request(
                &core_origin,
                Method::GET,
                audio,
                &[("cookie", &alice_cookie), ("range", "bytes=0-3")],
                vec![],
            )
            .await;
            assert_eq!(status, StatusCode::PARTIAL_CONTENT);
            assert_eq!(bytes.as_ref(), b"OggS");
            assert_eq!(
                http_request(
                    &core_origin,
                    Method::GET,
                    audio,
                    &[("cookie", &bob_cookie)],
                    vec![]
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(md["available"], true);
        let src = &md["sources"][0];
        let direct_url = src["directUrl"].as_str().expect("directUrl present");
        let relay_url = src["relayUrl"].as_str().expect("relayUrl present");
        assert!(direct_url.starts_with(expected_node_url));
        assert_eq!(relay_url, format!("/api/v1/assets/{asset_id}/content"));

        let parsed_direct = url::Url::parse(direct_url).unwrap();
        let direct_pq = format!(
            "{}?{}",
            parsed_direct.path(),
            parsed_direct.query().unwrap()
        );

        // Direct HEAD on Node
        let (head_st, head_hdrs, _) = http_request(
            expected_node_url,
            Method::HEAD,
            &direct_pq,
            &[("origin", &core_origin)],
            vec![],
        )
        .await;
        assert_eq!(head_st, StatusCode::OK);
        assert_eq!(head_hdrs[header::ACCEPT_RANGES], "bytes");
        let allow_origin = head_hdrs[header::ACCESS_CONTROL_ALLOW_ORIGIN]
            .to_str()
            .unwrap();
        assert!(
            allow_origin == "*" || allow_origin == core_origin.as_str(),
            "unexpected allow-origin: {allow_origin}"
        );

        // Direct Range GET (206 Partial Content) on Node
        let (r206_st, r206_hdrs, r206_bytes) = http_request(
            expected_node_url,
            Method::GET,
            &direct_pq,
            &[("origin", &core_origin), ("range", "bytes=0-127")],
            vec![],
        )
        .await;
        assert_eq!(r206_st, StatusCode::PARTIAL_CONTENT);
        assert_eq!(r206_bytes.len(), 128);
        assert!(
            r206_hdrs[header::CONTENT_RANGE]
                .to_str()
                .unwrap()
                .starts_with("bytes 0-127/")
        );

        // Direct invalid Range (416 Range Not Satisfiable) on Node
        let (r416_st, _, _) = http_request(
            expected_node_url,
            Method::GET,
            &direct_pq,
            &[
                ("origin", &core_origin),
                ("range", "bytes=999999999-1000000000"),
            ],
            vec![],
        )
        .await;
        assert_eq!(r416_st, StatusCode::RANGE_NOT_SATISFIABLE);

        // Core Relay Range GET (206 Partial Content)
        let (relay_st, _, relay_bytes) = http_request(
            &core_origin,
            Method::GET,
            relay_url,
            &[("cookie", &alice_cookie), ("range", "bytes=0-127")],
            vec![],
        )
        .await;
        assert_eq!(relay_st, StatusCode::PARTIAL_CONTENT);
        assert_eq!(relay_bytes, r206_bytes);

        // Exercise the production queue worker while real TCP Range requests
        // continue on the film Node. A separate empty library avoids replacing
        // the inventory being streamed. This is HTTP isolation, not decoding.
        if expected_node_url == &node1_url {
            // Real signed Node execution and callback with persisted results.
            let track: i64 = media_poc::database::query_scalar("SELECT stream_index FROM inventory_tracks WHERE asset_id=? AND json_extract(metadata_json,'$.codec_type')='audio' ORDER BY stream_index LIMIT 1")
                .bind(asset_id).fetch_one(store.pool()).await.unwrap();
            let mut operations = vec![
                ("analyze", asset_id.clone(), String::new(), 640),
                ("prepare", asset_id.clone(), format!("audio-{track}"), 640),
            ];
            let image: Option<String> = media_poc::database::query_scalar(
                "SELECT id FROM inventory_images WHERE library_id=? AND available=1 LIMIT 1",
            )
            .bind(&lib1_id)
            .fetch_optional(store.pool())
            .await
            .unwrap();
            if let Some(image) = image {
                operations.push(("image", image, String::new(), 320));
            }
            for (category, target, component, width) in operations {
                let task = media_poc::task_engine::enqueue_task(
                    &store,
                    &lib1_id,
                    "integration",
                    &format!("remote-{category}"),
                    300,
                    unix_now(),
                    category,
                    &target,
                    &component,
                    width,
                )
                .await
                .unwrap();
                let finished = tokio::time::timeout(Duration::from_secs(30), async {
                    loop {
                        let row = media_poc::task_engine::get(&store, &task.id)
                            .await
                            .unwrap()
                            .unwrap();
                        if row.status == "completed" {
                            break row;
                        }
                        assert_ne!(row.status, "error", "remote {category}: {}", row.error);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                })
                .await
                .expect("remote task completion");
                let result: Value = serde_json::from_str(&finished.result_json).unwrap();
                if category == "analyze" {
                    assert!(
                        result["streams"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|s| s["codec_name"] == "av1")
                    );
                } else {
                    assert_eq!(result["cacheWarm"], true);
                    assert!(result["size"].as_u64().unwrap() > 0);
                }
                assert_eq!(finished.failure_count, 0, "{}", finished.error);
            }
            let load_root = node1_root.join("queue-isolation-empty");
            tokio::fs::create_dir_all(&load_root).await.unwrap();
            media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('queue-isolation','Queue isolation','films',?,?,1)")
                .bind(&node1_id).bind(load_root.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
            let mut tx = store.pool().begin().await.unwrap();
            let timestamp = unix_now();
            for index in 0..10_000 {
                let id = format!("queue-isolation-{index:05}");
                media_poc::database::query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES(?,'queue-isolation','integration',?,?,?,?,?)")
                    .bind(&id).bind(&id).bind(&node1_id).bind(timestamp).bind(timestamp).bind(timestamp)
                    .execute(&mut *tx).await.unwrap();
            }
            tx.commit().await.unwrap();
            let scheduler_root = base.join("core-local-empty");
            media_poc::scheduler::dispatch_once(&auth, &scheduler_root, &jobs, unix_now())
                .await
                .unwrap();
            let mut ranges = tokio::task::JoinSet::new();
            for _ in 0..32 {
                let direct_origin = expected_node_url.to_owned();
                let direct_path = direct_pq.clone();
                let origin = core_origin.clone();
                let relay_path = relay_url.to_owned();
                let cookie = alice_cookie.clone();
                let expected = r206_bytes.clone();
                ranges.spawn(async move {
                    let start = std::time::Instant::now();
                    let (status, _, bytes) = http_request(
                        &direct_origin,
                        Method::GET,
                        &direct_path,
                        &[("origin", &origin), ("range", "bytes=0-127")],
                        vec![],
                    )
                    .await;
                    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
                    assert_eq!(bytes, expected);
                    let direct_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let start = std::time::Instant::now();
                    let (status, _, bytes) = http_request(
                        &origin,
                        Method::GET,
                        &relay_path,
                        &[("cookie", &cookie), ("range", "bytes=0-127")],
                        vec![],
                    )
                    .await;
                    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
                    assert_eq!(bytes, expected);
                    (direct_ms, start.elapsed().as_secs_f64() * 1000.0)
                });
            }
            let mut direct = Vec::with_capacity(32);
            let mut relay = Vec::with_capacity(32);
            while let Some(result) = ranges.join_next().await {
                let (direct_ms, relay_ms) = result.unwrap();
                direct.push(direct_ms);
                relay.push(relay_ms);
            }
            // Require a real remote scan to finish through the production worker.
            let mut completed = 0;
            for _ in 0..100 {
                completed = media_poc::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks WHERE library_id='queue-isolation' AND status='completed'")
                    .fetch_one(store.pool()).await.unwrap();
                if completed > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(
                completed > 0,
                "production worker must complete a remote scan"
            );
            // Stop queue admission before later scenarios rescan/revoke this Node.
            media_poc::database::query("UPDATE durable_tasks SET status='cancelled' WHERE library_id='queue-isolation' AND status='queued'")
                .execute(store.pool()).await.unwrap();
            for _ in 0..100 {
                media_poc::scheduler::dispatch_once(&auth, &scheduler_root, &jobs, unix_now())
                    .await
                    .unwrap();
                let active: i64 = media_poc::database::query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE library_id='queue-isolation' AND lease_until IS NOT NULL")
                    .fetch_one(store.pool()).await.unwrap();
                if active == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            direct.sort_by(f64::total_cmp);
            relay.sort_by(f64::total_cmp);
            println!(
                "QUEUE_TCP_RANGE_ISOLATION backlog=10000 concurrency=32 real_remote_scans_completed={completed} direct_p50_ms={:.3} direct_p95_ms={:.3} direct_max_ms={:.3} relay_p50_ms={:.3} relay_p95_ms={:.3} relay_max_ms={:.3}; byte-identical206 responses, no browser decode measured",
                direct[16], direct[30], direct[31], relay[16], relay[30], relay[31]
            );
        }
    }

    // Also verify poster streaming via Core relay and direct streaming with signed token
    let film_poster_relay = film_item["posterUrl"].as_str().unwrap();
    let (p_relay_st, _, p_relay_bytes) = http_request(
        &core_origin,
        Method::GET,
        film_poster_relay,
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    assert_eq!(p_relay_st, StatusCode::OK);

    // Extract image ID from /api/v1/assets/{id}/image?v=...
    let img_id = film_poster_relay
        .strip_prefix("/api/v1/assets/")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let node1_rec = media_poc::nodes::load_node(&store, &node1_id)
        .await
        .unwrap()
        .unwrap();
    let (poster_direct_url, _) = media_poc::nodes::build_direct_playback_url(
        &node1_rec,
        &lib1_id,
        "image",
        img_id,
        "stream",
        &sha256_hex(alice_raw_token.as_bytes()),
    )
    .expect("build direct image url");
    let parsed_poster = url::Url::parse(&poster_direct_url).unwrap();
    let poster_pq = format!(
        "{}?{}",
        parsed_poster.path(),
        parsed_poster.query().unwrap()
    );
    let (p_direct_st, _, p_direct_bytes) = http_request(
        &node1_url,
        Method::GET,
        &poster_pq,
        &[("origin", &core_origin)],
        vec![],
    )
    .await;
    assert_eq!(p_direct_st, StatusCode::OK);
    assert_eq!(p_direct_bytes, p_relay_bytes);

    // The width option must survive both the Core -> Node relay query and the
    // browser -> Node query alongside the signed token. This RGB PNG becomes
    // an authorized 320px JPEG preview from the existing 960px poster.
    let preview_relay = format!(
        "{film_poster_relay}{}width=320",
        if film_poster_relay.contains('?') {
            '&'
        } else {
            '?'
        }
    );
    let (preview_relay_status, preview_relay_headers, preview_relay_bytes) = http_request(
        &core_origin,
        Method::GET,
        &preview_relay,
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    assert_eq!(preview_relay_status, StatusCode::OK);
    assert_eq!(preview_relay_headers[header::CONTENT_TYPE], "image/jpeg");
    let decoded_preview = image::load_from_memory(&preview_relay_bytes).unwrap();
    assert_eq!(
        (decoded_preview.width(), decoded_preview.height()),
        (320, 480)
    );
    assert_ne!(preview_relay_bytes, p_relay_bytes);
    let (preview_direct_status, preview_direct_headers, preview_direct_bytes) = http_request(
        &node1_url,
        Method::GET,
        &format!("{poster_pq}&width=320"),
        &[("origin", &core_origin)],
        vec![],
    )
    .await;
    assert_eq!(preview_direct_status, StatusCode::OK);
    assert_eq!(preview_direct_headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(preview_direct_bytes, preview_relay_bytes);

    // 10. Rescan Node 1 Films and verify UUID preservation + reused cache + zero writes in media folder
    let (rescan_st, _, _) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/libraries/{lib1_id}/scan"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(rescan_st, StatusCode::ACCEPTED);
    for _ in 0..80 {
        let (_, _, body) = http_request(
            &core_origin,
            Method::GET,
            &format!("/api/v1/admin/libraries/{lib1_id}/scan"),
            &[("cookie", &admin_cookie)],
            vec![],
        )
        .await;
        let job: Value = serde_json::from_slice(&body).unwrap();
        if job["status"] == "completed" {
            assert_eq!(job["reused"], 1, "rescan should reuse unchanged file probe");
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (_, _, fdet_after_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/catalog/{film_work_id}"),
        &[("cookie", &admin_cookie)],
        vec![],
    )
    .await;
    let fdet_after: Value = serde_json::from_slice(&fdet_after_body).unwrap();
    assert_eq!(fdet_after["id"], film_work_id);
    assert_eq!(fdet_after["editions"][0]["id"], film_edition_id);
    assert_eq!(
        fdet_after["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"],
        film_asset_id
    );

    // 11. Simulate Node 1 going offline, verify catalogue marks film as temporarily unavailable without deleting it,
    //     then restart Node 1 on the same port and reconnect.
    let _ = node1_stop_tx.send(());
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (reconn_offline_st, _, reconn_offline_body) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{node1_id}/reconnect"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(reconn_offline_st, StatusCode::OK);
    let reconn_offline: Value = serde_json::from_slice(&reconn_offline_body).unwrap();
    assert_eq!(reconn_offline["status"], "disconnected");

    // Catalogue must still contain both works, with film marked available=false & nodeStatus="disconnected"
    let (_, _, cat_off_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/catalog",
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    let cat_off: Value = serde_json::from_slice(&cat_off_body).unwrap();
    assert_eq!(cat_off["total"], 2);
    let film_off = cat_off["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == film_work_id)
        .unwrap();
    assert_eq!(film_off["available"], false);
    assert_eq!(film_off["nodeStatus"], "disconnected");

    // While Node 1 is actually stopped, discovery and automatic extension work
    // on Node 2 must still progress through the shared durable queue.
    let new_episode = node2_root
        .join("Chroniques Orbitales (2024) [tvdb-777]")
        .join("Season 01")
        .join("Chroniques Orbitales - S01E03 - Reprise.mkv");
    tokio::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/av1-opus.mkv"),
        &new_episode,
    )
    .await
    .unwrap();
    let failure_progress = std::time::Instant::now();
    let (scan_status, _, scan_error) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/libraries/{lib2_id}/scan"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(
        scan_status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&scan_error)
    );
    tokio::time::timeout(Duration::from_secs(30),async {
        loop {
            let completed:i64=media_poc::database::query_scalar("SELECT COUNT(*) FROM automation_results r JOIN durable_tasks t ON t.id=r.task_id WHERE r.extension_id='distributed.fixture' AND t.status='completed'")
                .fetch_one(store.pool()).await.unwrap();
            if completed==automatic_count+1 {break;}
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.expect("healthy Node did not discover/enrich new episode while peer was offline");
    automatic_count += 1;
    println!(
        "REAL_NODE_FAILURE_AUTOMATION failed_nodes=1 healthy_nodes=1 new_assets=1 elapsed_ms={:.3}; real TCP Node 2 scan and signed Wasm completion while Node 1 listener stopped",
        failure_progress.elapsed().as_secs_f64() * 1000.0
    );

    // Restart Node 1 on the same address using its persisted node_identity.json and node_inventory.json
    let listener_re = TcpListener::bind(node1_addr).await.expect("rebind node1");
    let app_re = node_router(node1_state.clone());
    let (node1_stop_tx2, node1_stop_rx2) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener_re, app_re)
            .with_graceful_shutdown(async move {
                let _ = node1_stop_rx2.await;
            })
            .await;
    });

    let (reconn_online_st, _, reconn_online_body) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{node1_id}/reconnect"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(reconn_online_st, StatusCode::OK);
    let reconn_online: Value = serde_json::from_slice(&reconn_online_body).unwrap();
    assert_eq!(reconn_online["status"], "connected");

    // 12. Test secret rotation, expired token rejection, unknown Node rejection, and Node revocation
    let (_, _, md_before_rot_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/media/{film_asset_id}"),
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    let md_before_rot: Value = serde_json::from_slice(&md_before_rot_body).unwrap();
    let old_direct_url = md_before_rot["sources"][0]["directUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_parsed = url::Url::parse(&old_direct_url).unwrap();
    let old_pq = format!("{}?{}", old_parsed.path(), old_parsed.query().unwrap());

    // Rotate secret on Node 1
    let (rot_st, _, rot_body) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{node1_id}/rotate-secret"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(rot_st, StatusCode::OK);
    let rot_json: Value = serde_json::from_slice(&rot_body).unwrap();
    assert_eq!(rot_json["status"], "connected");

    // Old PlaybackToken must now be rejected with 403 Forbidden by Node 1
    let (old_tok_st, _, _) = http_request(
        &node1_url,
        Method::GET,
        &old_pq,
        &[("origin", &core_origin)],
        vec![],
    )
    .await;
    assert_eq!(old_tok_st, StatusCode::FORBIDDEN);

    // Newly issued PlaybackToken after rotation must succeed
    let (_, _, md_after_rot_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/media/{film_asset_id}"),
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    let md_after_rot: Value = serde_json::from_slice(&md_after_rot_body).unwrap();
    let new_direct_url = md_after_rot["sources"][0]["directUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let new_parsed = url::Url::parse(&new_direct_url).unwrap();
    let new_pq = format!("{}?{}", new_parsed.path(), new_parsed.query().unwrap());
    let (new_tok_st, _, _) = http_request(
        &node1_url,
        Method::GET,
        &new_pq,
        &[("origin", &core_origin), ("range", "bytes=0-63")],
        vec![],
    )
    .await;
    assert_eq!(new_tok_st, StatusCode::PARTIAL_CONTENT);

    // Direct requests also refresh activity through the same bounded P1 worker.
    media_poc::database::query(
        "UPDATE sessions SET last_seen_at=unixepoch()-120 WHERE token_hash=?",
    )
    .bind(sha256_hex(alice_raw_token.as_bytes()))
    .execute(store.pool())
    .await
    .unwrap();
    let (status, _, _) = http_request(&node1_url, Method::HEAD, &new_pq, &[], vec![]).await;
    assert_eq!(status, StatusCode::OK);
    auth.flush_session_touches().await;
    let age: i64 = media_poc::database::query_scalar(
        "SELECT unixepoch()-last_seen_at FROM sessions WHERE token_hash=?",
    )
    .bind(sha256_hex(alice_raw_token.as_bytes()))
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(age < 60);

    let current_record = media_poc::nodes::load_node(&store, &node1_id)
        .await
        .unwrap()
        .unwrap();
    let current_token = new_parsed
        .query_pairs()
        .find(|(k, _)| k == "token")
        .unwrap()
        .1
        .to_string();
    let mut wrong_library = media_contracts::protocol::verify_playback_token(
        &current_record.shared_secret,
        &current_record.key_id,
        &node1_id,
        "asset",
        &film_asset_id,
        "stream",
        &current_token,
        unix_now(),
    )
    .unwrap();
    wrong_library.library_id = LibraryId::new(&lib2_id);
    let wrong_token = issue_playback_token(&current_record.shared_secret, &wrong_library).unwrap();
    let (status, _, _) = http_request(
        &node1_url,
        Method::GET,
        &format!("/stream/v1/assets/{film_asset_id}/content?token={wrong_token}"),
        &[],
        vec![],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    media_poc::database::query("UPDATE users SET disabled_at=unixepoch() WHERE id=?")
        .bind(&alice_id)
        .execute(store.pool())
        .await
        .unwrap();
    let (status, _, _) = http_request(&node1_url, Method::GET, &new_pq, &[], vec![]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    media_poc::database::query("UPDATE users SET disabled_at=NULL WHERE id=?")
        .bind(&alice_id)
        .execute(store.pool())
        .await
        .unwrap();

    // Previously issued direct URLs obey live P1 permissions and session revocation.
    media_poc::database::query("UPDATE libraries SET enabled=0 WHERE id=?")
        .bind(&lib1_id)
        .execute(store.pool())
        .await
        .unwrap();
    for method in [Method::GET, Method::HEAD] {
        let (status, _, _) = http_request(
            &node1_url,
            method,
            &new_pq,
            &[("range", "bytes=0-63")],
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    media_poc::database::query("UPDATE libraries SET enabled=1 WHERE id=?")
        .bind(&lib1_id)
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::database::query("DELETE FROM library_permissions WHERE library_id=? AND user_id=?")
        .bind(&lib1_id)
        .bind(&alice_id)
        .execute(store.pool())
        .await
        .unwrap();
    let (status, _, _) = http_request(&node1_url, Method::GET, &new_pq, &[], vec![]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    media_poc::database::query("INSERT INTO library_permissions(library_id,user_id) VALUES (?,?)")
        .bind(&lib1_id)
        .bind(&alice_id)
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::database::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(sha256_hex(alice_raw_token.as_bytes()))
        .execute(store.pool())
        .await
        .unwrap();
    let (status, _, _) = http_request(&node1_url, Method::HEAD, &new_pq, &[], vec![]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    media_poc::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES (?,?,unixepoch(),unixepoch(),unixepoch()+3600)")
        .bind(sha256_hex(alice_raw_token.as_bytes())).bind(&alice_id).execute(store.pool()).await.unwrap();

    // Expired token rejection test on Node 1
    let node1_rec = media_poc::nodes::load_node(&store, &node1_id)
        .await
        .unwrap()
        .unwrap();
    let expired_claims = PlaybackTokenClaims {
        v: "v1".into(),
        kid: node1_rec.key_id.clone(),
        node_id: NodeId::from(node1_id.clone()),
        library_id: LibraryId::from(lib1_id.clone()),
        target_kind: "asset".into(),
        target_id: film_asset_id.clone(),
        action: "stream".into(),
        iat: unix_now().saturating_sub(7200),
        exp: unix_now().saturating_sub(3600),
        session_hash: String::new(),
        jti: "expired-nonce-test-0001".into(),
    };
    let expired_token = issue_playback_token(&node1_rec.shared_secret, &expired_claims).unwrap();
    let (exp_st, _, _) = http_request(
        &node1_url,
        Method::GET,
        &format!("/stream/v1/assets/{film_asset_id}/content?token={expired_token}"),
        &[("origin", &core_origin)],
        vec![],
    )
    .await;
    assert_eq!(exp_st, StatusCode::FORBIDDEN);

    // Unknown node identity rejection on Core internal endpoint
    let fake_body =
        b"{\"nodeId\":\"unknown-node\",\"name\":\"Fake\",\"publicUrl\":null,\"roots\":[]}";
    let fake_hash = sha256_hex(fake_body);
    let fake_ts = unix_now();
    let fake_sig = sign_internal_request(
        "fake-secret",
        "node->core",
        "POST",
        "/internal/v1/core/heartbeat",
        "unknown-node",
        "fake-key",
        fake_ts,
        "fake-nonce-1234567890",
        &fake_hash,
    );
    let (unk_st, _, _) = http_request(
        &core_origin,
        Method::POST,
        "/internal/v1/core/heartbeat",
        &[
            (HDR_PROTOCOL, PROTOCOL_VERSION),
            (HDR_NODE_ID, "unknown-node"),
            (HDR_KEY_ID, "fake-key"),
            (HDR_TIMESTAMP, &fake_ts.to_string()),
            (HDR_NONCE, "fake-nonce-1234567890"),
            (HDR_BODY_SHA256, &fake_hash),
            (HDR_SIGNATURE, &fake_sig),
            ("content-type", "application/json"),
        ],
        fake_body.to_vec(),
    )
    .await;
    assert_eq!(unk_st, StatusCode::FORBIDDEN);

    // One logical library scans independent folders on both real Node servers.
    let mut grouped_config = json!({"name":"Films_UHD","kind":"films","enabled":true,"userIds":[&alice_id],"folders":[{"nodeId":&node1_id,"root":&node1_root},{"nodeId":&node2_id,"root":&node2_root}]});
    let admin_headers = [
        ("origin", core_origin.as_str()),
        ("cookie", admin_cookie.as_str()),
        ("x-csrf-token", admin_csrf.as_str()),
        ("content-type", "application/json"),
    ];
    let (created, _, body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/libraries",
        &admin_headers,
        serde_json::to_vec(&grouped_config).unwrap(),
    )
    .await;
    assert_eq!(
        created,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let grouped: Value = serde_json::from_slice(&body).unwrap();
    let grouped_id = grouped["id"].as_str().unwrap();
    let scan_uri = format!("/api/v1/admin/libraries/{grouped_id}/scan");
    assert_eq!(
        http_request(
            &core_origin,
            Method::POST,
            &scan_uri,
            &admin_headers,
            vec![]
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let (_, _, body) = http_request(
                &core_origin,
                Method::GET,
                &scan_uri,
                &[("cookie", &admin_cookie)],
                vec![],
            )
            .await;
            let status: Value = serde_json::from_slice(&body).unwrap();
            assert_ne!(status["status"], "error", "{status}");
            if status["status"] == "completed" {
                assert!(status["discovered"].as_i64().unwrap() >= 2);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let (_, _, body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/catalog?library={grouped_id}"),
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    let grouped_catalog: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        grouped_catalog["total"].as_i64().unwrap() >= 2,
        "{grouped_catalog}"
    );
    let items = grouped_catalog["items"].as_array().unwrap();
    assert!(items.iter().all(|item| item["libraryId"] == grouped_id));
    assert!(items.iter().any(|item| item["nodeId"] == node1_id));
    assert!(items.iter().any(|item| item["nodeId"] == node2_id));
    assert_eq!(
        http_request(
            &core_origin,
            Method::GET,
            &format!("/api/v1/catalog?library={grouped_id}"),
            &[("cookie", &bob_cookie)],
            vec![]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    grouped_config["enabled"] = json!(false);
    assert_eq!(
        http_request(
            &core_origin,
            Method::PUT,
            &format!("/api/v1/admin/libraries/{grouped_id}"),
            &admin_headers,
            serde_json::to_vec(&grouped_config).unwrap()
        )
        .await
        .0,
        StatusCode::OK
    );

    // Revoke Node 1 and verify its works are excluded from the active catalogue and direct tokens are refused
    let (rev_st, _, rev_body) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{node1_id}/revoke"),
        &[
            ("origin", &core_origin),
            ("cookie", &admin_cookie),
            ("x-csrf-token", &admin_csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(rev_st, StatusCode::OK);
    let rev_json: Value = serde_json::from_slice(&rev_body).unwrap();
    assert_eq!(rev_json["status"], "revoked");

    let (rev_stream_st, _, _) = http_request(
        &node1_url,
        Method::GET,
        &new_pq,
        &[("origin", &core_origin)],
        vec![],
    )
    .await;
    assert_eq!(rev_stream_st, StatusCode::FORBIDDEN);

    let (_, _, cat_after_rev_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/catalog",
        &[("cookie", &alice_cookie)],
        vec![],
    )
    .await;
    let cat_after_rev: Value = serde_json::from_slice(&cat_after_rev_body).unwrap();
    assert_eq!(cat_after_rev["total"], 1);
    assert_eq!(cat_after_rev["items"][0]["id"], series_work_id);

    // The unchanged rescans and actual Node 1 restart above must not duplicate
    // extension effects or discard results already committed by either Node.
    let retained: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM automation_results WHERE extension_id='distributed.fixture'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    let admitted: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM automation_task_links WHERE extension_id='distributed.fixture'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(retained, automatic_count);
    assert_eq!(
        admitted, automatic_count,
        "unchanged rescans duplicated extension work"
    );

    let _ = node1_stop_tx2.send(());
    let _ = node2_stop_tx.send(());
    let _ = core_shutdown_tx.send(());
    store.close().await;
}
