//! Bounded scan/task admission, ownership, cancellation, and playback priority.

use super::control_transport::post_signed_to_core;
use super::control_transport::post_signed_to_core_raw;
use super::control_transport::signed_json_response;
use super::control_transport::verify_signed_core_call;
use super::state::NodeServerState;
use axum::Json;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use media_contracts::errors::NodeErrorCode;
use media_contracts::inventory::CachedFile;
use media_contracts::inventory::InventoryBatch;
use media_contracts::inventory::ReadGrant;
use media_contracts::inventory::ScanFailed;
use media_contracts::inventory::ScanFinished;
use media_contracts::media::LocalImage;
use media_contracts::protocol::LibraryInventorySync;
use media_contracts::protocol::NodeTaskRequest;
use media_contracts::protocol::NodeTaskResult;
use media_contracts::protocol::RemoteBatchSubmission;
use media_contracts::protocol::RemoteScanCompleteResponse;
use media_contracts::protocol::RemoteScanCompletion;
use media_contracts::protocol::RemoteScanFailure;
use media_contracts::protocol::RemoteScanStartRequest;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) async fn start_remote_scan(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/node/scans");
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let req: RemoteScanStartRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_json" })),
            )
                .into_response();
        }
    };

    if !req.core_base_url.is_empty() {
        let mut guard = state.identity.write().unwrap_or_else(|e| e.into_inner());
        guard.core_internal_url = Some(req.core_base_url.clone());
    }

    let mut validated_root = None;
    for allowed in &state.config.allowed_roots {
        if let Ok(valid) = crate::validate_library_root(Path::new(&req.root), allowed).await {
            validated_root = Some(valid);
            break;
        }
    }
    let Some(canonical_root) = validated_root else {
        return signed_json_response(
            identity.shared_secret.as_deref().unwrap_or_default(),
            &identity.node_id,
            &identity.key_id,
            &nonce,
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": "invalid_library_root" }),
        );
    };

    let cancel = match register_scan(&state, req.scan_id.as_str()) {
        Ok(cancel) => cancel,
        Err(reason) => {
            return signed_json_response(
                identity.shared_secret.as_deref().unwrap_or_default(),
                &identity.node_id,
                &identity.key_id,
                &nonce,
                StatusCode::TOO_MANY_REQUESTS,
                &serde_json::json!({ "error": reason }),
            );
        }
    };

    let bg_state = state.clone();
    let scan_req = req.clone();
    let root_for_task = canonical_root.clone();
    tokio::spawn(async move {
        run_remote_scan_task(bg_state, scan_req, root_for_task, cancel).await;
    });

    let secret = identity.shared_secret.unwrap_or_default();
    signed_json_response(
        &secret,
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::ACCEPTED,
        &serde_json::json!({
            "status": "started",
            "scan_id": req.scan_id,
            "root_snapshot": canonical_root.to_string_lossy(),
        }),
    )
}

/// Admission is shared by inbound requests and outbound commands. Cancellation
/// retains the slot until the old worker has actually stopped.
pub(super) fn register_scan(
    state: &NodeServerState,
    scan_id: &str,
) -> Result<Arc<AtomicBool>, &'static str> {
    let mut scans = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
    if scans.contains_key(scan_id) {
        return Err("duplicate_scan");
    }
    if !scans.is_empty() || state.active_streams.load(Ordering::Acquire) > 0 {
        return Err("node_busy_playback_priority");
    }
    let cancel = Arc::new(AtomicBool::new(false));
    scans.insert(scan_id.to_owned(), cancel.clone());
    Ok(cancel)
}

pub(super) async fn start_node_task(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(uri.path());
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(response) => return response,
        };
    let req: NodeTaskRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(_) => {
            return signed_json_response(
                identity.shared_secret.as_deref().unwrap_or_default(),
                &identity.node_id,
                &identity.key_id,
                &nonce,
                StatusCode::BAD_REQUEST,
                &serde_json::json!({"error":"invalid_task"}),
            );
        }
    };
    if req.node_id != identity.node_id || task_grant(&state, &req).is_err() {
        return signed_json_response(
            identity.shared_secret.as_deref().unwrap_or_default(),
            &identity.node_id,
            &identity.key_id,
            &nonce,
            StatusCode::FORBIDDEN,
            &serde_json::json!({"error":"task_target_denied"}),
        );
    }
    let cancel = match register_scan(&state, &req.run_id) {
        Ok(cancel) => cancel,
        Err(reason) => {
            return signed_json_response(
                identity.shared_secret.as_deref().unwrap_or_default(),
                &identity.node_id,
                &identity.key_id,
                &nonce,
                StatusCode::TOO_MANY_REQUESTS,
                &serde_json::json!({"error":reason}),
            );
        }
    };
    let bg = state.clone();
    tokio::spawn(async move {
        run_node_task(bg, req, cancel).await;
    });
    signed_json_response(
        identity.shared_secret.as_deref().unwrap_or_default(),
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::ACCEPTED,
        &serde_json::json!({"status":"accepted"}),
    )
}

pub(super) fn task_grant(
    state: &NodeServerState,
    req: &NodeTaskRequest,
) -> io::Result<(ReadGrant, u64, String)> {
    let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "task_target_denied");
    if req.node_id != state.node_id()
        || req.run_id.is_empty()
        || req.run_id.len() > 200
        || !req
            .run_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
    {
        return Err(denied());
    }
    let inventory = state.inventory.read().unwrap_or_else(|e| e.into_inner());
    let (grant, size, mtime) = if req.category == "image" {
        let entry = inventory.images.get(&req.target_id).ok_or_else(denied)?;
        (
            inventory
                .grant_for_image(&req.node_id, &req.target_id)
                .ok_or_else(denied)?,
            entry.size,
            entry.modified_ns.clone(),
        )
    } else if matches!(req.category.as_str(), "analyze" | "prepare") {
        let entry = inventory.assets.get(&req.target_id).ok_or_else(denied)?;
        (
            inventory
                .grant_for_asset(&req.node_id, &req.target_id)
                .ok_or_else(denied)?,
            entry.size,
            entry.modified_ns.clone(),
        )
    } else {
        return Err(denied());
    };
    if grant.library_id != req.library_id
        || !state
            .config
            .allowed_roots
            .iter()
            .any(|root| Path::new(&grant.root_snapshot).starts_with(root))
    {
        return Err(denied());
    }
    Ok((grant, size, mtime))
}

pub(super) async fn run_node_task(
    state: NodeServerState,
    req: NodeTaskRequest,
    cancel: Arc<AtomicBool>,
) {
    let result = async {
        let (grant, size, mtime) = task_grant(&state, &req)?;
        crate::tasks::run_task_on_grant(
            &req.category,
            &grant,
            req.component.as_deref(),
            req.width,
            &state.preparations,
            size,
            &mtime,
        )
        .await
    };
    let result = scan_with_playback_priority(&state, &cancel, result).await;
    state
        .active_scans
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&req.run_id);
    let (success, result_json, error) = match result {
        Ok(value) => (true, value, String::new()),
        Err(error) => (
            false,
            serde_json::Value::Null,
            error.to_string().chars().take(1024).collect(),
        ),
    };
    let path = format!("/internal/v1/core/tasks/{}/complete", req.run_id);
    let output = NodeTaskResult {
        run_id: req.run_id,
        node_id: state.node_id(),
        library_id: req.library_id,
        success,
        result_json,
        error,
    };
    let _ = post_signed_to_core(&state, &path, &output).await;
}

pub(super) fn cancel_scan(state: &NodeServerState, scan_id: &str) -> bool {
    let scans = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cancel) = scans.get(scan_id) {
        cancel.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

/// Signal demand before acquiring a preparation worker, so a background warm-up
/// releases that same worker first. Blocking image decoders remain cooperative.
pub(super) async fn yield_background_for_playback(state: &NodeServerState) {
    {
        let active = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
        for cancel in active.values() {
            cancel.store(true, Ordering::Release);
        }
    }
    let _ = tokio::time::timeout(Duration::from_millis(250), async {
        loop {
            if state
                .active_scans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
}

pub(super) async fn cancel_remote_scan(
    State(state): State<NodeServerState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(uri.path());
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(response) => return response,
        };
    let found = cancel_scan(&state, &scan_id);
    signed_json_response(
        identity.shared_secret.as_deref().unwrap_or_default(),
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "cancelRequested": found }),
    )
}

/// Dropping the scan future also drops any kill-on-drop probe child. The slot
/// remains registered until this future returns and the caller removes it.
pub(super) async fn scan_with_playback_priority<T>(
    state: &NodeServerState,
    cancel: &AtomicBool,
    scan: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::pin!(scan);
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = interval.tick() => {
                if state.active_streams.load(Ordering::Acquire) > 0 {
                    cancel.store(true, Ordering::Release);
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "scan preempted for playback"));
                }
                if cancel.load(Ordering::Acquire) {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"));
                }
            }
            result = &mut scan => {
                return if cancel.load(Ordering::Acquire) {
                    Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"))
                } else {
                    result
                };
            }
        }
    }
}

pub(super) async fn run_remote_scan_task(
    state: NodeServerState,
    req: RemoteScanStartRequest,
    canonical_root: PathBuf,
    cancel: Arc<AtomicBool>,
) {
    let scan_id_str = req.scan_id.as_str().to_owned();
    let root_snapshot = canonical_root.to_string_lossy().into_owned();
    let batch_size = req.batch_size.clamp(1, 128);
    let mut seq = 0u64;

    let mut scan_cache: HashMap<String, CachedFile> = HashMap::new();
    for f in req.cached_files {
        scan_cache.insert(
            f.relative_path,
            CachedFile {
                size: f.size,
                modified_ns: f.modified_ns,
                probe: media_contracts::media::Probe::default(),
            },
        );
    }
    let mut image_cache: HashMap<String, LocalImage> = HashMap::new();
    for img in req.cached_images {
        image_cache.insert(img.path.clone(), img);
    }

    let scan = async {
        if req.kind == "music" {
            crate::music::scan_with_exclusions(
                &canonical_root,
                &cancel,
                &req.excluded_patterns,
                |track| {
                    seq += 1;
                    let submission = crate::music::Submission {
                        library_id: req.library_id.as_str().into(),
                        scan_id: req.scan_id.as_str().into(),
                        root_snapshot: root_snapshot.clone(),
                        track,
                    };
                    let state = state.clone();
                    async move {
                        post_signed_to_core(&state, "/internal/v1/core/music/tracks", &submission)
                            .await
                    }
                },
            )
            .await
            .map(|count| crate::scanner::ScanReport {
                discovered: count as u64,
                probed: count as u64,
                complete: true,
                ..Default::default()
            })
        } else {
            state
                .local_node
                .scan_library_streaming(
                    &canonical_root,
                    &req.kind,
                    &scan_cache,
                    &image_cache,
                    &cancel,
                    batch_size,
                    &req.excluded_patterns,
                    |mut batch: InventoryBatch| {
                        seq += 1;
                        batch.scan_id = req.scan_id.clone();
                        batch.sequence = seq;
                        let state_ref = state.clone();
                        let scan_id = req.scan_id.clone();
                        let library_id = req.library_id.clone();
                        async move {
                            let submission = RemoteBatchSubmission {
                                node_id: state_ref.node_id(),
                                library_id,
                                batch,
                            };
                            let path =
                                format!("/internal/v1/core/scans/{}/batches", scan_id.as_str());
                            post_signed_to_core(&state_ref, &path, &submission).await
                        }
                    },
                )
                .await
        }
    };
    let result = scan_with_playback_priority(&state, &cancel, scan).await;

    {
        let mut scans = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
        scans.remove(&scan_id_str);
    }

    match result {
        Ok(report) => {
            let completion = RemoteScanCompletion {
                node_id: state.node_id(),
                library_id: req.library_id.clone(),
                root_snapshot,
                config_revision: req.config_revision,
                finished: ScanFinished {
                    scan_id: req.scan_id.clone(),
                    last_sequence: seq,
                    discovered: report.discovered,
                    probed: report.probed,
                    reused: report.reused,
                    complete: report.complete,
                    interrupted: Some(report.interrupted),
                },
            };
            let path = format!("/internal/v1/core/scans/{}/complete", req.scan_id.as_str());
            if let Ok(sync_bytes) = post_signed_to_core_raw(&state, &path, &completion).await {
                if let Ok(complete_resp) =
                    serde_json::from_slice::<RemoteScanCompleteResponse>(&sync_bytes)
                {
                    {
                        let mut inv = state.inventory.write().unwrap_or_else(|e| e.into_inner());
                        inv.apply_library_sync(complete_resp.inventory_sync);
                    }
                    let _ = state.persist_inventory().await;
                } else if let Ok(sync) = serde_json::from_slice::<LibraryInventorySync>(&sync_bytes)
                {
                    {
                        let mut inv = state.inventory.write().unwrap_or_else(|e| e.into_inner());
                        inv.apply_library_sync(sync);
                    }
                    let _ = state.persist_inventory().await;
                }
            }
        }
        Err(err) => {
            let failure = RemoteScanFailure {
                node_id: state.node_id(),
                library_id: req.library_id,
                failed: ScanFailed {
                    scan_id: req.scan_id.clone(),
                    code: NodeErrorCode::StreamError.as_str().to_owned(),
                    retryable: true,
                    config_revision: req.config_revision,
                },
                message: err.to_string(),
            };
            let path = format!("/internal/v1/core/scans/{}/fail", req.scan_id.as_str());
            let _ = post_signed_to_core(&state, &path, &failure).await;
        }
    }
}
