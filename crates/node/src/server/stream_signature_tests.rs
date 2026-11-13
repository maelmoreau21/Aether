//! Real HTTP signing, channel recovery, task ownership, and playback-priority contracts.

use super::control_transport::signed_json_response;
use super::outbound::send_stream_uplink_body;
use super::outbound::start_channel_worker;
use super::playback::ActiveStreamGuard;
use super::playback::normalize_stream_content_length;
use super::playback::packet_stream_error;
use super::routes::app_with_state;
use super::scans::cancel_remote_scan;
use super::scans::cancel_scan;
use super::scans::register_scan;
use super::scans::scan_with_playback_priority;
use super::scans::start_node_task;
use super::scans::task_grant;
use super::scans::yield_background_for_playback;
use super::state::NodeServerConfig;
use super::state::NodeServerState;
use axum::Router;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header;
use axum::response::Response;
use axum::routing::post;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use media_contracts::ids::AssetId;
use media_contracts::ids::NodeId;
use media_contracts::protocol::HDR_BODY_SHA256;
use media_contracts::protocol::HDR_KEY_ID;
use media_contracts::protocol::HDR_NODE_ID;
use media_contracts::protocol::HDR_NONCE;
use media_contracts::protocol::HDR_PROTOCOL;
use media_contracts::protocol::HDR_RES_NONCE;
use media_contracts::protocol::HDR_RES_SIGNATURE;
use media_contracts::protocol::HDR_RES_TIMESTAMP;
use media_contracts::protocol::HDR_SIGNATURE;
use media_contracts::protocol::HDR_TIMESTAMP;
use media_contracts::protocol::NodeInventoryAssetEntry;
use media_contracts::protocol::NodeTaskRequest;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sha256_hex;
use media_contracts::protocol::sign_internal_request;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_request;
use media_contracts::protocol::verify_internal_response;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use http_body_util::BodyExt;

#[tokio::test]
async fn malformed_signed_channel_responses_back_off_then_reconnect() {
    let (_data, state) = state().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    state.identity.write().unwrap().core_internal_url = Some(format!("http://{addr}"));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let recorded = calls.clone();
    let app = Router::new().route(
        "/internal/v1/core/nodes/{id}/channel",
        post(move |headers: HeaderMap| {
            let recorded = recorded.clone();
            async move {
                let count = recorded.fetch_add(1, Ordering::SeqCst);
                let nonce = headers.get(HDR_NONCE).unwrap().to_str().unwrap();
                let payload = if count == 0 {
                    serde_json::json!({})
                } else {
                    serde_json::json!({"commandId":"test","commandType":"revoke","payload":{}})
                };
                signed_json_response(
                    "test-stream-secret",
                    &NodeId::new("signature-node"),
                    "signature-key",
                    nonce,
                    StatusCode::OK,
                    &payload,
                )
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let worker = start_channel_worker(state.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "malformed responses must not cause a busy loop"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while !state.identity.read().unwrap().revoked {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    server.abort();
}

#[tokio::test]
async fn background_tasks_resolve_only_owned_inventory_targets() {
    let (_data, state) = state().await;
    let mut req = NodeTaskRequest {
        run_id: "job:1".into(),
        node_id: state.node_id(),
        library_id: media_contracts::ids::LibraryId::new("library"),
        category: "analyze".into(),
        target_id: "fixture".into(),
        component: None,
        width: None,
        trickplay: None,
        fingerprint: None,
        core_base_url: String::new(),
    };
    assert!(task_grant(&state, &req).is_ok());
    req.category = "trickplay".into();
    assert!(task_grant(&state, &req).is_ok());
    req.category = "analyze".into();
    req.library_id = media_contracts::ids::LibraryId::new("other");
    assert!(task_grant(&state, &req).is_err());
    req.library_id = media_contracts::ids::LibraryId::new("library");
    req.node_id = NodeId::new("another-node");
    assert!(task_grant(&state, &req).is_err());
    req.node_id = state.node_id();
    req.target_id = "../../outside.mp4".into();
    assert!(task_grant(&state, &req).is_err());
    let response = start_node_task(
        State(state),
        HeaderMap::new(),
        "/internal/v1/node/tasks".parse().unwrap(),
        Bytes::from(serde_json::to_vec(&req).unwrap()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn scan_admission_preserves_worker_ownership_and_playback_priority() {
    let (_data, state) = state().await;
    state.active_streams.store(1, Ordering::Release);
    assert_eq!(
        register_scan(&state, "first").unwrap_err(),
        "node_busy_playback_priority"
    );
    state.active_streams.store(0, Ordering::Release);
    let original = register_scan(&state, "first").unwrap();
    assert_eq!(
        register_scan(&state, "first").unwrap_err(),
        "duplicate_scan"
    );
    assert_eq!(
        register_scan(&state, "second").unwrap_err(),
        "node_busy_playback_priority"
    );
    assert!(!cancel_scan(&state, "unknown"));
    assert!(cancel_scan(&state, "first"));
    assert!(original.load(Ordering::Acquire));
    // Cancellation requests cannot free capacity while the worker is live.
    assert!(register_scan(&state, "second").is_err());
    state.active_scans.lock().unwrap().remove("first");
    assert!(register_scan(&state, "second").is_ok());
}

#[tokio::test]
async fn playback_preempts_a_running_scan_and_drops_its_future() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let (_data, state) = state().await;
    let cancel = AtomicBool::new(false);
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    let streams = state.active_streams.clone();
    let scan = async move {
        let _guard = guard;
        streams.store(1, Ordering::Release);
        std::future::pending::<io::Result<()>>().await
    };
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        scan_with_playback_priority(&state, &cancel, scan),
    )
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(cancel.load(Ordering::Acquire));
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test]
async fn prepared_playback_demand_releases_background_work_before_acquiring_its_slot() {
    let (_data, state) = state().await;
    let cancel = register_scan(&state, "warm:1").unwrap();
    let slot = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = slot.clone().acquire_owned().await.unwrap();
    let background_state = state.clone();
    let background_cancel = cancel.clone();
    let background = tokio::spawn(async move {
        let result =
            scan_with_playback_priority(&background_state, &background_cancel, async move {
                let _permit = permit;
                std::future::pending::<io::Result<()>>().await
            })
            .await;
        background_state
            .active_scans
            .lock()
            .unwrap()
            .remove("warm:1");
        result
    });
    let demand = ActiveStreamGuard::enter(&state.active_streams);
    yield_background_for_playback(&state).await;
    assert!(cancel.load(Ordering::Acquire));
    assert!(state.active_scans.lock().unwrap().is_empty());
    assert!(
        slot.try_acquire().is_ok(),
        "foreground preparation can acquire the released work slot"
    );
    assert_eq!(
        background.await.unwrap().unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    drop(demand);
    assert_eq!(state.active_streams.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn scan_cancel_requires_core_authentication_and_signs_its_response() {
    let (_data, state) = state().await;
    let cancel = register_scan(&state, "owned-scan").unwrap();
    let path = "/internal/v1/node/scans/owned-scan/cancel";
    let response = cancel_remote_scan(
        State(state.clone()),
        AxumPath("owned-scan".into()),
        HeaderMap::new(),
        path.parse().unwrap(),
        Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!cancel.load(Ordering::Acquire));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app_with_state(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let url = format!("http://{addr}");
    let (status, _, body) = request_and_verify(&url, Method::POST, path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["cancelRequested"],
        true
    );
    assert!(cancel.load(Ordering::Acquire));
    server.abort();
}

async fn state() -> (tempfile::TempDir, NodeServerState) {
    let data = tempfile::tempdir().unwrap();
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative");
    let state = NodeServerState::initialize(NodeServerConfig {
        data_dir: data.path().to_owned(),
        allowed_roots: vec![root],
        ..NodeServerConfig::default()
    })
    .await
    .unwrap();
    {
        let mut identity = state.identity.write().unwrap();
        identity.node_id = NodeId::new("signature-node");
        identity.key_id = "signature-key".to_owned();
        identity.shared_secret = Some("test-stream-secret".to_owned());
    }
    let root = state.config.allowed_roots[0].clone();
    let file = crate::OpenedFile::open_in_root(&root, Path::new("representative.mp4"))
        .await
        .unwrap();
    for (id, path, size, modified) in [
        (
            "fixture",
            "representative.mp4",
            file.size(),
            file.modified_ns(),
        ),
        ("removed", "not-present.mp4", 0, "0"),
    ] {
        state.inventory.write().unwrap().assets.insert(
            id.to_owned(),
            NodeInventoryAssetEntry {
                asset_id: AssetId::new(id),
                library_id: media_contracts::ids::LibraryId::new("library"),
                root_snapshot: root.to_string_lossy().into_owned(),
                relative_path: path.to_owned(),
                mime: "video/mp4".to_owned(),
                size,
                modified_ns: modified.to_owned(),
            },
        );
    }
    (data, state)
}

async fn request_and_verify(
    url: &str,
    method: Method,
    path: &str,
) -> (StatusCode, HeaderMap, Bytes) {
    let stream = crate::transport::connect(url).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let now = unix_now();
    let nonce = random_nonce_hex();
    let empty = sha256_hex(b"");
    let signature = sign_internal_request(
        "test-stream-secret",
        "core-to-node",
        method.as_str(),
        path,
        "signature-node",
        "signature-key",
        now,
        &nonce,
        &empty,
    );
    let request = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, url.strip_prefix("http://").unwrap())
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .header(HDR_NODE_ID, "signature-node")
        .header(HDR_KEY_ID, "signature-key")
        .header(HDR_TIMESTAMP, now.to_string())
        .header(HDR_NONCE, &nonce)
        .header(HDR_BODY_SHA256, &empty)
        .header(HDR_SIGNATURE, signature)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), sender.send_request(request))
        .await
        .unwrap()
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let response_digest = if path.starts_with("/internal/v1/node/scans/") {
        sha256_hex(&bytes)
    } else {
        crate::transport::stream_metadata_digest(&headers)
    };
    // This is the same verification Core applies, using actual headers
    // received over TCP after Axum/Hyper have applied HTTP body framing.
    verify_internal_response(
        "test-stream-secret",
        &nonce,
        status.as_u16(),
        "signature-node",
        "signature-key",
        headers[HDR_RES_TIMESTAMP]
            .to_str()
            .unwrap()
            .parse()
            .unwrap(),
        headers[HDR_RES_NONCE].to_str().unwrap(),
        headers[HDR_RES_SIGNATURE].to_str().unwrap(),
        &response_digest,
        unix_now(),
    )
    .expect("Core must authenticate on-wire Node error metadata");
    (status, headers, bytes)
}

#[tokio::test]
async fn real_node_http_errors_and_head_keep_their_signed_framing() {
    let (_data, state) = state().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app_with_state(state)).await.unwrap();
    });
    for (path, expected, code) in [
        (
            "/internal/v1/node/assets/absent/content",
            StatusCode::NOT_FOUND,
            None,
        ),
        (
            "/internal/v1/node/assets/removed/prepared/opus-2-chunk-0",
            StatusCode::SERVICE_UNAVAILABLE,
            Some("preparation_tool_or_source_unavailable"),
        ),
        (
            "/internal/v1/node/assets/fixture/prepared/opus-1-chunk-0",
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("unsupported_preparation"),
        ),
    ] {
        let (status, headers, bytes) = request_and_verify(&url, Method::GET, path).await;
        assert_eq!(status, expected);
        assert!(!bytes.is_empty());
        assert_eq!(headers[header::CONTENT_LENGTH], bytes.len().to_string());
        if let Some(code) = code {
            assert_eq!(headers["x-media-preparation-error"], code);
        }
    }
    let (status, headers, bytes) = request_and_verify(
        &url,
        Method::HEAD,
        "/internal/v1/node/assets/fixture/prepared/opus-2-chunk-0",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.is_empty());
    assert!(
        headers[header::CONTENT_LENGTH]
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            > 1000
    );
    server.abort();
}

#[tokio::test]
async fn real_uplink_post_binds_inferred_json_body_length_before_signing() {
    let (_data, state) = state().await;
    let identity = state.identity.read().unwrap().clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let core = Router::new().route(
        "/internal/v1/core/nodes/signature-node/stream-uplink/stream",
        post(move |headers: HeaderMap, uri: Uri, body: Bytes| {
            let tx = tx.clone();
            async move {
                let digest = crate::transport::stream_metadata_digest(&headers);
                let result = verify_internal_request(
                    "test-stream-secret",
                    "node-to-core",
                    "POST",
                    uri.path(),
                    "signature-node",
                    "signature-key",
                    headers[HDR_TIMESTAMP].to_str().unwrap().parse().unwrap(),
                    headers[HDR_NONCE].to_str().unwrap(),
                    &digest,
                    headers[HDR_SIGNATURE].to_str().unwrap(),
                    unix_now(),
                );
                let length = headers[header::CONTENT_LENGTH]
                    .to_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send((result.is_ok(), length, body.len()));
                }
                StatusCode::OK
            }
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, core).await.unwrap();
    });
    let response = packet_stream_error(&io::Error::new(io::ErrorKind::WouldBlock, "test capacity"));
    assert!(
        !response.headers().contains_key(header::CONTENT_LENGTH),
        "exercise exact Body hint, not an already explicit length"
    );
    send_stream_uplink_body(&state, &url, &identity, "stream", response)
        .await
        .unwrap();
    let (verified, length, body) = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .unwrap()
        .unwrap();
    assert!(verified);
    assert_eq!(length, body);
    assert!(body > 0);
    server.abort();
}

#[test]
fn stream_signing_preserves_head_lengths_and_bodyless_status_rules() {
    let mut head = Response::new(Body::empty());
    head.headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_static("12345"));
    normalize_stream_content_length(&mut head);
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "12345");
    for status in [StatusCode::NO_CONTENT, StatusCode::NOT_MODIFIED] {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = status;
        normalize_stream_content_length(&mut response);
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
    }
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
        let response = packet_stream_error(&io::Error::new(kind, "test"));
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.headers().contains_key("x-media-preparation-error"));
    }
}
