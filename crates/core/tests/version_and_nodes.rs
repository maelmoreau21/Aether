use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use media_contracts::protocol::{HeartbeatPayload, PROTOCOL_VERSION};
use media_poc::{
    auth::AuthState,
    identity::IdentityStore,
    node::Inventory,
    nodes::{load_admin_nodes, load_node, router},
    secure_app,
};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_core_health_and_config_expose_version() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir).await.unwrap();
    let app = secure_app(inventory, 3000, auth).await.unwrap();

    // Test /api/v1/health
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["version"], media_contracts::version::current_version());

    // Test /api/v1/config
    let res2 = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .header("Host", "127.0.0.1:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let bytes2 = axum::body::to_bytes(res2.into_body(), 1024 * 64)
        .await
        .unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
    assert_eq!(
        json2["version"],
        media_contracts::version::current_version()
    );
}

#[tokio::test]
async fn test_nodes_version_tracking_local_and_remote() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("db.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();

    // 1. Local node always defaults to the running Core/Node component version
    let views = load_admin_nodes(&store).await.unwrap();
    let local = views.iter().find(|n| n.id == "local").unwrap();
    assert_eq!(
        local.version.as_deref(),
        Some(media_contracts::version::current_version())
    );

    // 2. Insert a remote node with an initial version (e.g. 0.0.9)
    let now = media_contracts::protocol::unix_now();
    media_poc::database::query(
        "INSERT INTO media_nodes(id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version)
         VALUES('remote-node-1', 'NAS Remote 1', 'remote', 'http://127.0.0.1:4101', NULL, 'connected', 'k1', 'secret', '[]', ?, ?, NULL, '0.0.9')",
    )
    .bind(now)
    .bind(now)
    .execute(store.pool())
    .await
    .unwrap();

    let views = load_admin_nodes(&store).await.unwrap();
    let remote1 = views.iter().find(|n| n.id == "remote-node-1").unwrap();
    assert_eq!(remote1.version.as_deref(), Some("0.0.9"));

    // 3. Heartbeat from remote node updates version to 0.1.0-beta
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let replay_window = Arc::new(media_contracts::protocol::ReplayWindow::new());
    let nodes_router = router(auth, replay_window);

    let hb = HeartbeatPayload {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        version: Some("0.1.0-beta".to_string()),
        node_id: media_contracts::ids::NodeId::new("remote-node-1"),
        roots: vec!["/media".into()],
        public_url: None,
        active_scans: 0,
        indexed_assets: 10,
        timestamp: media_contracts::protocol::unix_now(),
    };
    let body_bytes = serde_json::to_vec(&hb).unwrap();
    let ts = media_contracts::protocol::unix_now();
    let nonce = "nonce-version-test-1234567890123456";
    let body_sha = media_contracts::protocol::sha256_hex(&body_bytes);
    let sig = media_contracts::protocol::sign_internal_request(
        "secret",
        "node-to-core",
        "POST",
        "/internal/v1/core/heartbeat",
        "remote-node-1",
        "k1",
        ts,
        nonce,
        &body_sha,
    );

    let res = nodes_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v1/core/heartbeat")
                .header(
                    media_contracts::protocol::HDR_PROTOCOL,
                    media_contracts::protocol::PROTOCOL_VERSION,
                )
                .header(media_contracts::protocol::HDR_NODE_ID, "remote-node-1")
                .header(media_contracts::protocol::HDR_KEY_ID, "k1")
                .header(media_contracts::protocol::HDR_TIMESTAMP, ts.to_string())
                .header(media_contracts::protocol::HDR_NONCE, nonce)
                .header(media_contracts::protocol::HDR_BODY_SHA256, body_sha)
                .header(media_contracts::protocol::HDR_SIGNATURE, sig)
                .header("Content-Type", "application/json")
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    // Verify database record has updated version
    let rec = load_node(&store, "remote-node-1")
        .await
        .unwrap()
        .expect("node must exist");
    assert_eq!(rec.version.as_deref(), Some("0.1.0-beta"));

    let views = load_admin_nodes(&store).await.unwrap();
    let remote1_updated = views.iter().find(|n| n.id == "remote-node-1").unwrap();
    assert_eq!(remote1_updated.version.as_deref(), Some("0.1.0-beta"));

    // 4. Legacy heartbeat without version field preserves previous version
    let legacy_hb = HeartbeatPayload {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        version: None,
        node_id: media_contracts::ids::NodeId::new("remote-node-1"),
        roots: vec!["/media".into()],
        public_url: None,
        active_scans: 0,
        indexed_assets: 10,
        timestamp: media_contracts::protocol::unix_now(),
    };
    let body_bytes = serde_json::to_vec(&legacy_hb).unwrap();
    let ts = media_contracts::protocol::unix_now();
    let nonce = "nonce-version-test-legacy-12345678";
    let body_sha = media_contracts::protocol::sha256_hex(&body_bytes);
    let sig = media_contracts::protocol::sign_internal_request(
        "secret",
        "node-to-core",
        "POST",
        "/internal/v1/core/heartbeat",
        "remote-node-1",
        "k1",
        ts,
        nonce,
        &body_sha,
    );

    let res = nodes_router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v1/core/heartbeat")
                .header(
                    media_contracts::protocol::HDR_PROTOCOL,
                    media_contracts::protocol::PROTOCOL_VERSION,
                )
                .header(media_contracts::protocol::HDR_NODE_ID, "remote-node-1")
                .header(media_contracts::protocol::HDR_KEY_ID, "k1")
                .header(media_contracts::protocol::HDR_TIMESTAMP, ts.to_string())
                .header(media_contracts::protocol::HDR_NONCE, nonce)
                .header(media_contracts::protocol::HDR_BODY_SHA256, body_sha)
                .header(media_contracts::protocol::HDR_SIGNATURE, sig)
                .header("Content-Type", "application/json")
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let rec_preserved = load_node(&store, "remote-node-1")
        .await
        .unwrap()
        .expect("node must exist");
    assert_eq!(rec_preserved.version.as_deref(), Some("0.1.0-beta"));
}
