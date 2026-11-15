use super::channel::register_pending_stream;
use super::router;
use super::scans::{dispatch_remote_task, scan_run_authorized};
use super::transport::validate_public_url;
use crate::auth::AuthState;
use crate::identity::IdentityStore;
use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use axum::http::StatusCode;
use media_contracts::ids::LibraryId;
use media_contracts::ids::NodeId;
use media_contracts::protocol::HDR_BODY_SHA256;
use media_contracts::protocol::HDR_KEY_ID;
use media_contracts::protocol::HDR_NODE_ID;
use media_contracts::protocol::HDR_NONCE;
use media_contracts::protocol::HDR_PROTOCOL;
use media_contracts::protocol::HDR_SIGNATURE;
use media_contracts::protocol::HDR_TIMESTAMP;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::ReplayWindow;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sha256_hex;
use media_contracts::protocol::sign_internal_request;
use media_contracts::protocol::unix_now;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

#[tokio::test]
async fn scan_results_require_current_node_library_and_live_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("fencing.sqlite"))
        .await
        .unwrap();
    crate::libraries::initialize_local(&store).await.unwrap();
    crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic-root',1)").execute(store.pool()).await.unwrap();
    crate::database::query("INSERT INTO active_scan_runs(library_id,node_id,scan_id) VALUES('films','local','legacy-current')").execute(store.pool()).await.unwrap();
    assert!(scan_run_authorized(&store, "films", "local", "legacy-current").await);
    assert!(!scan_run_authorized(&store, "films", "local", "legacy-old").await);
    assert!(!scan_run_authorized(&store, "films", "wrong-node", "legacy-current").await);
    assert!(!scan_run_authorized(&store, "wrong-library", "local", "legacy-current").await);
    let now = unix_now();
    let job =
        crate::task_engine::enqueue(&store, "films", "administrator", "fence-proof", 100, now)
            .await
            .unwrap();
    let claimed = crate::task_engine::claim(&store, "local", now, 30, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.id, claimed.id);
    let scan = format!("{}:{}", job.id, claimed.attempt);
    crate::database::query("UPDATE active_scan_runs SET scan_id=? WHERE library_id='films'")
        .bind(&scan)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(scan_run_authorized(&store, "films", "local", &scan).await);
    assert!(!scan_run_authorized(&store, "films", "local", &format!("{}:0", job.id)).await);
    crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id=?")
        .bind(now)
        .bind(&job.id)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!scan_run_authorized(&store, "films", "local", &scan).await);
    crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id=?")
        .bind(now + 30)
        .bind(&job.id)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        crate::task_engine::control(&store, &job.id, "cancel", now)
            .await
            .unwrap()
    );
    assert!(!scan_run_authorized(&store, "films", "local", &scan).await);
    crate::database::query(
        "UPDATE active_scan_runs SET scan_id='legacy-current' WHERE library_id='films'",
    )
    .execute(store.pool())
    .await
    .unwrap();
    crate::database::query("UPDATE libraries SET enabled=0 WHERE id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!scan_run_authorized(&store, "films", "local", "legacy-current").await);
    crate::database::query("UPDATE libraries SET enabled=1 WHERE id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('child','Child','films','local','child-root',0)").execute(store.pool()).await.unwrap();
    crate::database::query(
        "INSERT INTO library_folders(library_id,parent_id,active) VALUES('child','films',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    crate::database::query("INSERT INTO active_scan_runs(library_id,node_id,scan_id) VALUES('child','local','child-current')").execute(store.pool()).await.unwrap();
    assert!(scan_run_authorized(&store, "child", "local", "child-current").await);
    // A historical enabled flag cannot override the parent or removed folder.
    crate::database::query("UPDATE libraries SET enabled=1 WHERE id='child'")
        .execute(store.pool())
        .await
        .unwrap();
    crate::database::query("UPDATE libraries SET enabled=0 WHERE id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!scan_run_authorized(&store, "child", "local", "child-current").await);
    crate::database::query("UPDATE libraries SET enabled=1 WHERE id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    crate::database::query("UPDATE library_folders SET active=0 WHERE library_id='child'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!scan_run_authorized(&store, "child", "local", "child-current").await);
    crate::database::query("UPDATE media_nodes SET status='revoked' WHERE id='local'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!scan_run_authorized(&store, "films", "local", "legacy-current").await);
    store.close().await;
}
#[tokio::test]
async fn old_scan_completion_cannot_revert_a_changed_library_root() {
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("changed-root.sqlite"))
        .await
        .unwrap();
    let secret = "synthetic-test-node-secret-not-a-deployment-secret";
    crate::database::query("INSERT INTO media_nodes(id,name,kind,status,key_id,shared_secret) VALUES('config-node','Config','remote','connected','test-key',?)").bind(secret).execute(store.pool()).await.unwrap();
    crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('config-library','Config','films','config-node','new-root',1)").execute(store.pool()).await.unwrap();
    crate::database::query(
        "INSERT INTO scan_jobs(library_id,status) VALUES('config-library','running')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    crate::database::query("INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES('config-library','old-config-scan','config-node')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = router(auth, Arc::new(ReplayWindow::new()));
    let path = "/internal/v1/core/scans/old-config-scan/complete";
    let body=serde_json::to_vec(&serde_json::json!({"nodeId":"config-node","libraryId":"config-library","rootSnapshot":"old-root","configRevision":"config-library:old-root","finished":{"scan_id":"old-config-scan","last_sequence":0,"discovered":0,"probed":0,"reused":0,"complete":true}})).unwrap();
    let now = unix_now();
    let nonce = random_nonce_hex();
    let digest = sha256_hex(&body);
    let signature = sign_internal_request(
        secret,
        "node-to-core",
        "POST",
        path,
        "config-node",
        "test-key",
        now,
        &nonce,
        &digest,
    );
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(HDR_PROTOCOL, PROTOCOL_VERSION)
                .header(HDR_NODE_ID, "config-node")
                .header(HDR_KEY_ID, "test-key")
                .header(HDR_TIMESTAMP, now.to_string())
                .header(HDR_NONCE, nonce)
                .header(HDR_BODY_SHA256, digest)
                .header(HDR_SIGNATURE, signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let root: String =
        crate::database::query_scalar("SELECT root FROM libraries WHERE id='config-library'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(root, "new-root");
    let status: String = crate::database::query_scalar(
        "SELECT status FROM scan_jobs WHERE library_id='config-library'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(status, "error");
    store.close().await;
}
#[tokio::test]
async fn unavailable_task_node_is_not_retried_until_reconnected() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("offline-task.sqlite"))
        .await
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    crate::database::query("INSERT INTO media_nodes(id,name,kind,status,base_url) VALUES('offline-task','Offline','remote','connected',?)").bind(base).execute(store.pool()).await.unwrap();
    let request = media_contracts::protocol::NodeTaskRequest {
        run_id: "test:1".into(),
        node_id: NodeId::new("offline-task"),
        library_id: LibraryId::new("scope"),
        category: "analyze".into(),
        target_id: "asset".into(),
        component: None,
        width: None,
        trickplay: None,
        fingerprint: None,
        core_base_url: "http://127.0.0.1:3000".into(),
    };
    assert_eq!(
        dispatch_remote_task(&store, &request).await.unwrap_err(),
        "Node task unavailable; waiting for reconnection"
    );
    let status: String =
        crate::database::query_scalar("SELECT status FROM media_nodes WHERE id='offline-task'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(status, "disconnected");
    store.close().await;
}
#[test]
fn relay_pending_is_bounded_and_reclaims_cancelled_receivers() {
    let mut pending = HashMap::new();
    let mut receivers = Vec::new();
    for index in 0..64 {
        let (sender, receiver) = oneshot::channel();
        receivers.push(receiver);
        assert!(register_pending_stream(
            &mut pending,
            index.to_string(),
            sender
        ));
    }
    let (sender, _receiver) = oneshot::channel();
    assert!(!register_pending_stream(
        &mut pending,
        "overflow".into(),
        sender
    ));
    assert_eq!(pending.len(), 64);
    receivers.clear();
    let (sender, _receiver) = oneshot::channel();
    assert!(register_pending_stream(
        &mut pending,
        "after-cancel".into(),
        sender
    ));
    assert_eq!(pending.len(), 1);
}
#[test]
fn playback_bearers_require_https_except_local_loopback() {
    for raw in [
        "http://192.168.1.20:4101",
        "http://node.lan",
        "https://user:pass@node.lan",
        "https://node.lan/path",
    ] {
        assert!(validate_public_url(raw).is_err());
    }
    for raw in [
        "https://node.lan",
        "http://127.0.0.1:4101",
        "http://localhost:4101",
        "http://[::1]:4101",
    ] {
        assert!(validate_public_url(raw).is_ok());
    }
}

/// Signed Node -> Core call, as a Media Node sends it.
async fn node_call(
    app: &axum::Router,
    secret: &str,
    node: &str,
    path: &str,
    body: Vec<u8>,
) -> StatusCode {
    use tower::ServiceExt;
    let now = unix_now();
    let nonce = random_nonce_hex();
    let digest = sha256_hex(&body);
    let signature = sign_internal_request(
        secret,
        "node-to-core",
        "POST",
        path,
        node,
        "test-key",
        now,
        &nonce,
        &digest,
    );
    app.clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(HDR_PROTOCOL, PROTOCOL_VERSION)
                .header(HDR_NODE_ID, node)
                .header(HDR_KEY_ID, "test-key")
                .header(HDR_TIMESTAMP, now.to_string())
                .header(HDR_NONCE, nonce)
                .header(HDR_BODY_SHA256, digest)
                .header(HDR_SIGNATURE, signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

/// Multi-Core step 5: a remote batch of a stale attempt is refused with nothing staged, and a
/// completion replayed by the Node (or received by a second Core) publishes once: it never
/// republishes the emptied staging area, which would hide every asset of the library.
#[tokio::test]
async fn remote_scan_effects_are_fenced_by_the_durable_attempt() {
    use media_contracts::inventory::{InventoryBatch, ScannedFile};
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("remote-fence.sqlite"))
        .await
        .unwrap();
    let secret = "synthetic-test-node-secret-not-a-deployment-secret";
    crate::database::query("INSERT INTO media_nodes(id,name,kind,status,key_id,shared_secret) VALUES('nas','NAS','remote','connected','test-key',?)").bind(secret).execute(store.pool()).await.unwrap();
    crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','nas','/volume1/films',1)").execute(store.pool()).await.unwrap();
    crate::database::query("INSERT INTO scan_jobs(library_id,status) VALUES('films','running')")
        .execute(store.pool())
        .await
        .unwrap();
    let now = unix_now();
    crate::database::query(
        "INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,status,\
            attempt,lease_until,started_at,created_at,updated_at,available_at) \
            VALUES('scan','films','admin','scan','nas','scan','running',2,?,?,?,?,?)",
    )
    .bind(now + 600)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(store.pool())
    .await
    .unwrap();
    crate::database::query(
        "INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES('films','scan:2','nas')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = router(auth, Arc::new(ReplayWindow::new()));
    let batch = |scan: &str| {
        serde_json::to_vec(&media_contracts::protocol::RemoteBatchSubmission {
            node_id: NodeId::new("nas"),
            library_id: LibraryId::new("films"),
            batch: InventoryBatch {
                scan_id: scan.into(),
                sequence: 0,
                files: (0..3)
                    .map(|index| ScannedFile {
                        relative_path: format!("Film {index} (2020).mkv"),
                        title: format!("Film {index}"),
                        identity_key: format!("film-{index}"),
                        size: 10 + index,
                        modified_ns: "1".into(),
                        mime: "video/x-matroska".into(),
                        probe: serde_json::from_value(serde_json::json!({
                            "format": {"duration": "60"},
                            "streams": [{"index": 0, "codec_type": "video", "codec_name": "av1"}]
                        }))
                        .unwrap(),
                        ..Default::default()
                    })
                    .collect(),
                errors: vec![],
                warnings: vec![],
            },
        })
        .unwrap()
    };
    let staged = || async {
        crate::database::query_scalar::<sqlx::Any, i64>("SELECT COUNT(*) FROM scan_staging_files")
            .fetch_one(store.pool())
            .await
            .unwrap()
    };
    // Superseded attempt: refused, nothing staged.
    crate::database::query("INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES('films','scan:1','nas') ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id")
        .execute(store.pool()).await.unwrap();
    assert_eq!(
        node_call(
            &app,
            secret,
            "nas",
            "/internal/v1/core/scans/scan:1/batches",
            batch("scan:1")
        )
        .await,
        StatusCode::CONFLICT
    );
    crate::database::query("UPDATE active_scan_runs SET scan_id='scan:2' WHERE library_id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    // Expired lease: refused, nothing staged.
    crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id='scan'")
        .bind(now - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        node_call(
            &app,
            secret,
            "nas",
            "/internal/v1/core/scans/scan:2/batches",
            batch("scan:2")
        )
        .await,
        StatusCode::CONFLICT
    );
    assert_eq!(staged().await, 0);
    crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id='scan'")
        .bind(now + 600)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        node_call(
            &app,
            secret,
            "nas",
            "/internal/v1/core/scans/scan:2/batches",
            batch("scan:2")
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(staged().await, 3);

    let completion = serde_json::to_vec(&serde_json::json!({"nodeId":"nas","libraryId":"films","rootSnapshot":"/volume1/films","configRevision":"films:/volume1/films","finished":{"scan_id":"scan:2","last_sequence":0,"discovered":3,"probed":3,"reused":0,"complete":true}})).unwrap();
    let available = || async {
        crate::database::query_scalar::<sqlx::Any, i64>(
            "SELECT COUNT(*) FROM inventory_assets WHERE library_id='films' AND available=1",
        )
        .fetch_one(store.pool())
        .await
        .unwrap()
    };
    for _ in 0..2 {
        assert_eq!(
            node_call(
                &app,
                secret,
                "nas",
                "/internal/v1/core/scans/scan:2/complete",
                completion.clone()
            )
            .await,
            StatusCode::OK
        );
        assert_eq!(
            available().await,
            3,
            "a replayed completion publishes nothing"
        );
    }
    let (status, published): (String, Option<String>) = crate::database::query_as(
        "SELECT status,published_scan_id FROM scan_jobs WHERE library_id='films'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        (status.as_str(), published.as_deref()),
        ("completed", Some("scan:2"))
    );
    // Once the attempt lost its lease, its completion is refused.
    crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id='scan'")
        .bind(now - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        node_call(
            &app,
            secret,
            "nas",
            "/internal/v1/core/scans/scan:2/complete",
            completion
        )
        .await,
        StatusCode::CONFLICT
    );
    assert_eq!(available().await, 3);
    store.close().await;
}
