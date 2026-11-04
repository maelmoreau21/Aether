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
