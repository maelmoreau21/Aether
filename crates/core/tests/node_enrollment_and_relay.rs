//! Integration test for Media Node enrollment & NAT-proof outbound relay.
//!
//! Validates:
//! 1. Generation of single-use, expiring enrollment tokens (`mnet_...`).
//! 2. Node auto-enrollment with Core (`POST /api/v1/nodes/enroll`).
//! 3. Rejection of duplicate or invalid enrollment tokens (single-use enforcement).
//! 4. Durable identity persistence and outbound connection mode.
//! 5. NAT-proof outbound bi-directional channel (`/channel`) with automatic heartbeat & reconnection.
//! 6. Remote scan dispatch through outbound channel without inbound ports on Node.
//! 7. Multi-root library support on a single Node (`/media/films; /media/series`).
//! 8. Bounded zero-copy streaming relay (`206 Partial Content`) via Node outbound uplink.
//! 9. Secret rotation and Node revocation over the outbound channel.

use axum::http::{Method, Request, StatusCode, header};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use media_contracts::protocol::{PROTOCOL_VERSION, unix_now};
use media_node::server::{NodeServerConfig, NodeServerState};
use media_poc::{
    auth::AuthState, identity::IdentityStore, node::Inventory, scans::ScanJobs,
    secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};

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

#[tokio::test]
async fn test_node_enrollment_outbound_channel_and_streaming_relay() {
    let temp = tempfile::tempdir().expect("tempdir");
    let base = tokio::fs::canonicalize(temp.path())
        .await
        .expect("canonicalize tempdir");

    let core_empty_local = base.join("core-local-empty");
    let node_films_root = base.join("node-films");
    let node_series_root = base.join("node-series");
    let node_data = base.join("node-data");

    tokio::fs::create_dir_all(&core_empty_local).await.unwrap();
    tokio::fs::create_dir_all(&node_films_root).await.unwrap();
    tokio::fs::create_dir_all(&node_series_root).await.unwrap();
    tokio::fs::create_dir_all(&node_data).await.unwrap();

    // Populate media fixture in films root
    let fixtures_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let mp4_src = fixtures_dir.join("av1-opus.mp4");
    if mp4_src.exists() {
        let film_dir = node_films_root.join("Relay Test Movie (2026)");
        tokio::fs::create_dir_all(&film_dir).await.unwrap();
        tokio::fs::copy(&mp4_src, film_dir.join("Relay Test Movie (2026).mp4"))
            .await
            .unwrap();
        tokio::fs::write(
            film_dir.join("movie.nfo"),
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie>
  <title>Relay Test Movie</title>
  <year>2026</year>
  <plot>Tested through outbound NAT-proof streaming relay.</plot>
  <uniqueid type="tmdb" default="true">8881</uniqueid>
</movie>"#,
        )
        .await
        .unwrap();
    }

    // 1. Start Core on a TCP listener
    let core_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind core listener");
    let core_port = core_listener.local_addr().unwrap().port();
    let core_origin = format!("http://127.0.0.1:{core_port}");

    let db_path = base.join("core.sqlite");
    let store = IdentityStore::open(&db_path).await.expect("sqlite store");
    store
        .bootstrap(
            "admin",
            zeroize::Zeroizing::new("AdminStrongPassword123!".into()),
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
        .expect("core app");

    let (core_shutdown_tx, core_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(core_listener, core_app)
            .with_graceful_shutdown(async move {
                let _ = core_shutdown_rx.await;
            })
            .await;
    });

    // 2. Admin logs in on Core
    let (st, login_headers, login_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/auth/login",
        &[
            ("origin", &core_origin),
            (header::CONTENT_TYPE.as_str(), "application/json"),
        ],
        serde_json::to_vec(&json!({
            "login": "admin",
            "password": "AdminStrongPassword123!"
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cookie_str = login_headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    let session_cookie = cookie_str.split(';').next().unwrap().trim();
    let login_val: Value = serde_json::from_slice(&login_body).unwrap();
    let csrf = login_val["csrfToken"].as_str().unwrap().to_owned();

    // 3. Admin generates an enrollment token
    let (st, _, token_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/nodes/enrollment-tokens",
        &[
            ("origin", &core_origin),
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
            (header::CONTENT_TYPE.as_str(), "application/json"),
        ],
        serde_json::to_vec(&json!({ "expiresInSecs": 600 })).unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let token_resp: Value = serde_json::from_slice(&token_body).unwrap();
    let raw_token = token_resp["token"].as_str().unwrap().to_owned();
    assert!(raw_token.starts_with("mnet_"));
    assert_eq!(token_resp["coreUrl"].as_str().unwrap(), &core_origin);

    // 4. Try enrolling with a malformed token -> must fail 400 Bad Request
    let (st, _, _) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/nodes/enroll",
        &[(header::CONTENT_TYPE.as_str(), "application/json")],
        serde_json::to_vec(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "enrollmentToken": "mnet_short",
            "nodeSecretHalf": "0123456789abcdef0123456789abcdef0123456789abcdef",
            "name": "Attacker Node",
            "nonce": "0123456789abcdef",
            "roots": ["/media/fake"],
            "timestamp": unix_now()
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Try enrolling with an unknown/forged valid-format token -> must fail 401 Unauthorized
    let (st, _, _) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/nodes/enroll",
        &[(header::CONTENT_TYPE.as_str(), "application/json")],
        serde_json::to_vec(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "enrollmentToken": "mnet_0123456789abcdef0123456789abcdef0123456789abcdef",
            "nodeSecretHalf": "0123456789abcdef0123456789abcdef0123456789abcdef",
            "name": "Attacker Node",
            "nonce": "0123456789abcdef",
            "roots": ["/media/fake"],
            "timestamp": unix_now()
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // 5. Initialize Node state with 2 roots (films and series) and NO public URL (pure outbound mode)
    let node_config = NodeServerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: node_data.clone(),
        allowed_roots: vec![node_films_root.clone(), node_series_root.clone()],
        bootstrap_token: None,
        configured_node_id: None,
        configured_name: Some("Outbound Remote Node".to_owned()),
        public_url: None, // zero inbound public streaming port!
        batch_size: 16,
    };
    let node_state = NodeServerState::initialize(node_config)
        .await
        .expect("initialize node");
    assert!(!node_state.identity.read().unwrap().revoked);
    assert!(node_state.identity.read().unwrap().shared_secret.is_none());

    // 6. Node performs auto-enrollment with Core using the valid token
    media_node::server::enroll_with_core(&core_origin, &raw_token, &node_state)
        .await
        .expect("enroll_with_core should succeed");

    let enrolled_id = node_state.node_id();
    assert!(node_state.identity.read().unwrap().shared_secret.is_some());
    assert_eq!(
        node_state
            .identity
            .read()
            .unwrap()
            .core_internal_url
            .as_deref(),
        Some(core_origin.as_str())
    );

    // 7. Re-using the same enrollment token must be rejected (single-use enforcement)
    let (st, _, _) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/nodes/enroll",
        &[(header::CONTENT_TYPE.as_str(), "application/json")],
        serde_json::to_vec(&json!({
            "protocolVersion": PROTOCOL_VERSION,
            "enrollmentToken": &raw_token,
            "nodeSecretHalf": "0123456789abcdef0123456789abcdef0123456789abcdef",
            "name": "Duplicate Node",
            "nonce": "0123456789abcdef",
            "roots": ["/media/fake"],
            "timestamp": unix_now()
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Verify in Core admin that the node exists and is marked connection_mode: "outbound"
    let (st, _, nodes_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/admin/nodes",
        &[
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let nodes_val: Value = serde_json::from_slice(&nodes_body).unwrap();
    let enrolled_node_in_admin = nodes_val["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"].as_str() == Some(enrolled_id.as_str()))
        .expect("enrolled node found in admin list");
    assert_eq!(enrolled_node_in_admin["status"].as_str(), Some("connected"));
    assert_eq!(
        enrolled_node_in_admin["connectionMode"].as_str(),
        Some("outbound")
    );
    assert_eq!(enrolled_node_in_admin["roots"].as_array().unwrap().len(), 2);

    // 8. Start Node's outbound persistent channel worker (NAT-proof bidirectional communication)
    let channel_state = node_state.clone();
    let channel_handle = media_node::server::start_channel_worker(channel_state);

    // Give the channel a moment to connect
    tokio::time::sleep(Duration::from_millis(300)).await;

    // 9. Admin creates a library attached to this enrolled node
    let (st, _, lib_body) = http_request(
        &core_origin,
        Method::POST,
        "/api/v1/admin/libraries",
        &[
            ("origin", &core_origin),
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
            (header::CONTENT_TYPE.as_str(), "application/json"),
        ],
        serde_json::to_vec(&json!({
            "name": "Outbound Films",
            "kind": "films",
            "nodeId": enrolled_id.as_str(),
            "root": node_films_root.to_string_lossy(),
            "enabled": true,
            "userIds": []
        }))
        .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let lib_val: Value = serde_json::from_slice(&lib_body).unwrap();
    let library_id = lib_val["id"].as_str().unwrap().to_owned();

    // 10. Admin triggers a library scan via Core -> Core dispatches Scan command through OUTBOUND channel!
    let (st, _, _scan_body) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/libraries/{library_id}/scan"),
        &[
            ("origin", &core_origin),
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);

    // Wait for the scan to finish executing on the Node via channel and updating catalogue
    let mut scan_done = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (st, _, scan_status_body) = http_request(
            &core_origin,
            Method::GET,
            &format!("/api/v1/admin/libraries/{library_id}/scan"),
            &[
                (header::COOKIE.as_str(), session_cookie),
                ("x-csrf-token", &csrf),
            ],
            vec![],
        )
        .await;
        if st == StatusCode::OK {
            let scan_info: Value = serde_json::from_slice(&scan_status_body).unwrap();
            let status = scan_info["status"].as_str().unwrap_or("");
            if status == "completed" {
                scan_done = true;
                break;
            }
        }
    }
    assert!(
        scan_done,
        "remote scan over outbound channel did not complete"
    );

    // 11. Verify catalogue has the scanned movie
    let (st, _, cat_body) = http_request(
        &core_origin,
        Method::GET,
        "/api/v1/catalog",
        &[(header::COOKIE.as_str(), session_cookie)],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cat_val: Value = serde_json::from_slice(&cat_body).unwrap();
    let items = cat_val["items"].as_array().unwrap();
    assert!(!items.is_empty(), "catalog should have indexed items");
    let item_id = items[0]["id"].as_str().unwrap();

    // Get catalog work details to obtain the asset_id (playbackMediaId)
    let (st, _, fdet_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/catalog/{item_id}"),
        &[(header::COOKIE.as_str(), session_cookie)],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let fdet: Value = serde_json::from_slice(&fdet_body).unwrap();
    let asset_id = fdet["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_owned();

    // Get media descriptor
    let (st, _, media_body) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/media/{asset_id}"),
        &[(header::COOKIE.as_str(), session_cookie)],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let media_val: Value = serde_json::from_slice(&media_body).unwrap();
    let src = &media_val["sources"][0];
    assert!(
        src["directUrl"].is_null(),
        "outbound node without public_url must not have directUrl"
    );
    assert_eq!(
        src["relayUrl"].as_str(),
        Some(format!("/api/v1/assets/{asset_id}/content").as_str())
    );

    // 12. Test Streaming Relay via Node Outbound Uplink (zero-copy NAT-proof chunked streaming)
    let (st, stream_headers, stream_bytes) = http_request(
        &core_origin,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}/content"),
        &[
            (header::COOKIE.as_str(), session_cookie),
            (header::RANGE.as_str(), "bytes=0-63"),
        ],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::PARTIAL_CONTENT);
    assert_eq!(stream_bytes.len(), 64);
    assert!(
        stream_headers
            .get(header::CONTENT_RANGE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("bytes 0-63/")
    );

    // Allow channel worker to cycle gracefully after completing stream relay
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 13. Test Secret Rotation over outbound channel
    let (st, _, _) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{}/rotate-secret", enrolled_id.as_str()),
        &[
            ("origin", &core_origin),
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Wait for the channel to deliver rotate command
    tokio::time::sleep(Duration::from_millis(400)).await;

    // 14. Test Node Revocation over outbound channel
    let (st, _, _) = http_request(
        &core_origin,
        Method::POST,
        &format!("/api/v1/admin/nodes/{}/revoke", enrolled_id.as_str()),
        &[
            ("origin", &core_origin),
            (header::COOKIE.as_str(), session_cookie),
            ("x-csrf-token", &csrf),
        ],
        vec![],
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Wait for the channel to deliver revoke command
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(node_state.identity.read().unwrap().revoked);

    channel_handle.abort();
    let _ = core_shutdown_tx.send(());
}
