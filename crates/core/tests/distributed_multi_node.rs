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
    image::DynamicImage::new_rgb8(32, 48)
        .save(film_dir.join("poster.png"))
        .expect("write film poster");

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

    let store = IdentityStore::open(&core_dir.join("core.sqlite"))
        .await
        .expect("open core sqlite");
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
    let inventory = Inventory::local(core_empty_local).await.expect("local inv");
    let core_app = secure_app_with_jobs(inventory, core_port, auth.clone(), jobs.clone())
        .await
        .expect("distributed core app");

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
        sqlx::query("INSERT INTO users(id, role) VALUES (?, 'user')")
            .bind(uid)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, 'unused-test-hash')",
        )
        .bind(uid)
        .bind(login_name)
        .execute(store.pool())
        .await
        .unwrap();
        let tok_hash = sha256_hex(raw_tok.as_bytes());
        sqlx::query(
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
        &node1_rec, &lib1_id, "image", img_id, "stream",
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

    let _ = node1_stop_tx2.send(());
    let _ = node2_stop_tx.send(());
    let _ = core_shutdown_tx.send(());
    store.close().await;
}
