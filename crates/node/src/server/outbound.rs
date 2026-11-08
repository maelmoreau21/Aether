//! Persistent outbound Core command channel and signed streaming uplinks.

use super::control_transport::post_signed_to_core;
use super::control_transport::send_signed_http_request;
use super::playback::ActiveStreamGuard;
use super::playback::node_error_response;
use super::playback::prepare_asset;
use super::playback::{chapter_request, trickplay_request};
use super::scans::cancel_scan;
use super::scans::register_scan;
use super::scans::run_node_task;
use super::scans::run_remote_scan_task;
use super::scans::yield_background_for_playback;
use super::state::NodeIdentityState;
use super::state::NodeServerState;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::Response;
use bytes::Bytes;
use hyper_util::rt::TokioIo;
use media_contracts::errors::NodeErrorCode;
use media_contracts::inventory::ScanFailed;
use media_contracts::protocol::HDR_BODY_SHA256;
use media_contracts::protocol::HDR_KEY_ID;
use media_contracts::protocol::HDR_NODE_ID;
use media_contracts::protocol::HDR_NONCE;
use media_contracts::protocol::HDR_PROTOCOL;
use media_contracts::protocol::HDR_SIGNATURE;
use media_contracts::protocol::HDR_TIMESTAMP;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::LibraryInventorySync;
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeTaskRequest;
use media_contracts::protocol::NodeTaskResult;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::RemoteScanFailure;
use media_contracts::protocol::RemoteScanStartRequest;
use media_contracts::protocol::RotateSecretRequest;
use media_contracts::protocol::StreamRelayRequest;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sign_internal_request;
use media_contracts::protocol::unix_now;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Maintain persistent outbound control connection to Core. Automatically reconnects.
pub fn start_channel_worker(state: NodeServerState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            let identity = state
                .identity
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if identity.revoked
                || identity.shared_secret.is_none()
                || identity.core_internal_url.is_none()
            {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }

            let core_url = identity.core_internal_url.clone().unwrap();
            let secret = identity.shared_secret.clone().unwrap();
            let active_scans = state
                .active_scans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len() as u32;
            let active_streams = state.active_streams.load(Ordering::Relaxed);
            let indexed_assets = state
                .inventory
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .assets
                .len();
            let roots: Vec<String> = state
                .config
                .allowed_roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();

            let resource_pressure = Some(state.resources.sample().await);
            let hb = HeartbeatPayload {
                protocol_version: PROTOCOL_VERSION.to_owned(),
                version: Some(media_contracts::version::current_version().to_string()),
                node_id: identity.node_id.clone(),
                roots,
                public_url: identity.public_url.clone(),
                active_scans,
                active_scan_ids: state
                    .active_scans
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .keys()
                    .take(64)
                    .cloned()
                    .collect(),
                active_streams,
                resource_pressure,
                indexed_assets,
                timestamp: unix_now(),
            };

            let body = serde_json::to_vec(&hb).unwrap_or_default();
            let path = format!(
                "/internal/v1/core/nodes/{}/channel",
                identity.node_id.as_str()
            );

            match send_signed_http_request(
                &core_url,
                "node-to-core",
                Method::POST,
                &path,
                &identity.node_id,
                &identity.key_id,
                &secret,
                None,
                Bytes::from(body),
            )
            .await
            {
                Ok((status, _headers, resp_bytes)) => {
                    if status.is_success() {
                        let Ok(cmd) = serde_json::from_slice::<NodeCommandEnvelope>(&resp_bytes)
                        else {
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(5));
                            continue;
                        };
                        backoff = Duration::from_secs(1);
                        if cmd.is_expired_with_skew(unix_now()) {
                            // Delivered long after its deadline: the situation it described is gone.
                            eprintln!(
                                "{}",
                                serde_json::json!({"event":"node_command_expired","commandId":cmd.command_id,"expiresAt":cmd.expires_at})
                            );
                            continue;
                        }
                        match cmd.command_type {
                            NodeCommandType::Ping => {}
                            NodeCommandType::Task => {
                                if let Ok(req) =
                                    serde_json::from_value::<NodeTaskRequest>(cmd.payload)
                                {
                                    match register_scan(&state, &req.run_id) {
                                        Ok(cancel) => {
                                            let bg = state.clone();
                                            tokio::spawn(async move {
                                                run_node_task(bg, req, cancel).await;
                                            });
                                        }
                                        Err(reason) if reason != "duplicate_scan" => {
                                            let result = NodeTaskResult {
                                                run_id: req.run_id.clone(),
                                                node_id: state.node_id(),
                                                library_id: req.library_id,
                                                success: false,
                                                result_json: serde_json::Value::Null,
                                                error: reason.into(),
                                            };
                                            let path = format!(
                                                "/internal/v1/core/tasks/{}/complete",
                                                req.run_id
                                            );
                                            let bg = state.clone();
                                            tokio::spawn(async move {
                                                let _ =
                                                    post_signed_to_core(&bg, &path, &result).await;
                                            });
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                            NodeCommandType::CancelScan => {
                                if let Some(scan_id) =
                                    cmd.payload.get("scanId").and_then(|v| v.as_str())
                                {
                                    cancel_scan(&state, scan_id);
                                }
                            }
                            NodeCommandType::Scan => {
                                if let Ok(scan_req) =
                                    serde_json::from_value::<RemoteScanStartRequest>(cmd.payload)
                                {
                                    let bg_state = state.clone();
                                    tokio::spawn(async move {
                                        handle_channel_scan(bg_state, scan_req).await;
                                    });
                                }
                            }
                            NodeCommandType::SyncInventory => {
                                if let Ok(sync) =
                                    serde_json::from_value::<LibraryInventorySync>(cmd.payload)
                                {
                                    {
                                        let mut inv = state
                                            .inventory
                                            .write()
                                            .unwrap_or_else(|e| e.into_inner());
                                        inv.apply_library_sync(sync);
                                    }
                                    let _ = state.persist_inventory().await;
                                }
                            }
                            NodeCommandType::RotateSecret => {
                                if let Ok(rot) =
                                    serde_json::from_value::<RotateSecretRequest>(cmd.payload)
                                {
                                    {
                                        let mut guard = state
                                            .identity
                                            .write()
                                            .unwrap_or_else(|e| e.into_inner());
                                        guard.shared_secret = Some(rot.new_secret);
                                        guard.key_id = rot.new_key_id;
                                    }
                                    let _ = state.persist_identity().await;
                                }
                            }
                            NodeCommandType::Revoke => {
                                {
                                    let mut guard =
                                        state.identity.write().unwrap_or_else(|e| e.into_inner());
                                    guard.revoked = true;
                                }
                                {
                                    let mut scans = state
                                        .active_scans
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner());
                                    for cancel in scans.values() {
                                        cancel.store(true, Ordering::SeqCst);
                                    }
                                    scans.clear();
                                }
                                let _ = state.persist_identity().await;
                            }
                            NodeCommandType::StreamRelay => {
                                if let Ok(relay_req) =
                                    serde_json::from_value::<StreamRelayRequest>(cmd.payload)
                                {
                                    let bg_state = state.clone();
                                    tokio::spawn(async move {
                                        handle_channel_stream_relay(bg_state, relay_req).await;
                                    });
                                }
                            }
                        }
                    } else {
                        if status == StatusCode::FORBIDDEN
                            && String::from_utf8_lossy(&resp_bytes).contains("node_revoked")
                        {
                            // The Core refuses a revoked identity at the door, so the in-band
                            // Revoke command may never be delivered. Stop polling instead of
                            // retrying forever with a credential that can never work again.
                            {
                                let mut guard =
                                    state.identity.write().unwrap_or_else(|e| e.into_inner());
                                guard.revoked = true;
                            }
                            let _ = state.persist_identity().await;
                            continue;
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(5));
                    }
                }
                Err(_err) => {
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
    })
}

async fn handle_channel_scan(state: NodeServerState, req: RemoteScanStartRequest) {
    let mut validated_root = None;
    for allowed in &state.config.allowed_roots {
        if let Ok(valid) = crate::validate_library_root(Path::new(&req.root), allowed).await {
            validated_root = Some(valid);
            break;
        }
    }
    let Some(canonical_root) = validated_root else {
        return;
    };
    let cancel = match register_scan(&state, req.scan_id.as_str()) {
        Ok(cancel) => cancel,
        Err(reason) => {
            // A repeated delivery must not fail the original live attempt.
            if reason != "duplicate_scan" {
                let failure = RemoteScanFailure {
                    node_id: state.node_id(),
                    library_id: req.library_id,
                    failed: ScanFailed {
                        scan_id: req.scan_id.clone(),
                        code: NodeErrorCode::StreamError.as_str().to_owned(),
                        retryable: true,
                        config_revision: req.config_revision,
                    },
                    message: reason.into(),
                };
                let path = format!("/internal/v1/core/scans/{}/fail", req.scan_id.as_str());
                let _ = post_signed_to_core(&state, &path, &failure).await;
            }
            return;
        }
    };
    run_remote_scan_task(state, req, canonical_root, cancel).await;
}

async fn handle_channel_stream_relay(state: NodeServerState, req: StreamRelayRequest) {
    let _stream_guard = ActiveStreamGuard::enter(&state.active_streams);
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked || identity.shared_secret.is_none() || identity.core_internal_url.is_none()
    {
        return;
    }
    let core_url = identity.core_internal_url.as_ref().unwrap();

    let grant = if req.target_kind == "image" {
        state
            .inventory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .grant_for_image(&identity.node_id, &req.target_id)
    } else {
        state
            .inventory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .grant_for_asset(&identity.node_id, &req.target_id)
    };

    let Some(grant) = grant else {
        let _ = send_stream_uplink_error(
            &state,
            core_url,
            &identity,
            &req.stream_id,
            StatusCode::NOT_FOUND,
        )
        .await;
        return;
    };

    let mut builder =
        hyper::Request::builder().method(if req.head { Method::HEAD } else { Method::GET });
    if let Some(ref r) = req.range {
        builder = builder.header(header::RANGE, r);
    }
    if req.target_kind == "image" {
        if let Some(width) = req.image_width {
            builder = builder.uri(format!(
                "/internal/v1/node/assets/{}/image?width={width}",
                req.target_id
            ));
        }
        if let Some(ref etag) = req.if_none_match {
            builder = builder.header(header::IF_NONE_MATCH, etag);
        }
    }
    let synthetic_req = builder.body(Body::empty()).unwrap_or_default();

    if let Some(parameters) = req.target_kind.strip_prefix("chapter-image:") {
        let query = parameters
            .split_once('?')
            .map(|(_, query)| query)
            .unwrap_or("");
        let mut response = if let Some(request) = chapter_request(&state, &req.target_id, query) {
            crate::chapter_images::response(&state.trickplay, request, synthetic_req).await
        } else {
            node_error_response(NodeErrorCode::UnknownAsset, false)
        };
        if req.head {
            encode_head_uplink(&mut response);
        }
        let _ =
            send_stream_uplink_body(&state, core_url, &identity, &req.stream_id, response).await;
        return;
    }

    if let Some(component) = req.target_kind.strip_prefix("prepared:") {
        yield_background_for_playback(&state).await;
        let mut response = prepare_asset(&state, &req.target_id, component, synthetic_req).await;
        if req.head {
            encode_head_uplink(&mut response);
        }
        let _ =
            send_stream_uplink_body(&state, core_url, &identity, &req.stream_id, response).await;
        return;
    }
    let (preview_target, preview_query) = req
        .target_kind
        .split_once('?')
        .unwrap_or((&req.target_kind, ""));
    if let Some(index_str) = preview_target.strip_prefix("trickplay:") {
        if let Ok(index) = index_str.parse::<usize>() {
            let Some(trickplay_req) = trickplay_request(&state, &req.target_id, preview_query)
            else {
                let _ = send_stream_uplink_error(
                    &state,
                    core_url,
                    &identity,
                    &req.stream_id,
                    StatusCode::NOT_FOUND,
                )
                .await;
                return;
            };
            match state
                .trickplay
                .open_sheet_with_mime(&trickplay_req, index)
                .await
            {
                Ok((opened, mime)) => {
                    if let Ok(response) =
                        crate::stream::serve_opened(opened, &synthetic_req, &mime).await
                    {
                        let (mut parts, body) = response.into_parts();
                        parts.headers.insert(
                            axum::http::header::X_CONTENT_TYPE_OPTIONS,
                            axum::http::HeaderValue::from_static("nosniff"),
                        );
                        parts.headers.insert(
                            axum::http::header::CACHE_CONTROL,
                            axum::http::HeaderValue::from_static("public, max-age=86400"),
                        );
                        let mut axum_res = Response::from_parts(parts, Body::new(body));
                        if req.head {
                            encode_head_uplink(&mut axum_res);
                        }
                        let _ = send_stream_uplink_body(
                            &state,
                            core_url,
                            &identity,
                            &req.stream_id,
                            axum_res,
                        )
                        .await;
                        return;
                    }
                }
                Err(_) => {
                    let _ = send_stream_uplink_error(
                        &state,
                        core_url,
                        &identity,
                        &req.stream_id,
                        StatusCode::NOT_FOUND,
                    )
                    .await;
                    return;
                }
            }
        }
    } else if preview_target == "trickplay" {
        let Some(trickplay_req) = trickplay_request(&state, &req.target_id, preview_query) else {
            let _ = send_stream_uplink_error(
                &state,
                core_url,
                &identity,
                &req.stream_id,
                StatusCode::NOT_FOUND,
            )
            .await;
            return;
        };
        let body = if let Some(cached) = state.trickplay.check_native_cache(&trickplay_req).await {
            serde_json::json!({
                "status": "ready",
                "source": "aether",
                "intervalMs": cached.interval_ms,
                "width": cached.width,
                "height": cached.height,
                "columns": cached.columns,
                "rows": cached.rows,
                "sheetCount": cached.sheet_count,
                "mime": cached.mime,
                "thumbnailCount": cached.thumbnail_count,
                "urlPrefix": format!("/api/v1/assets/{}/trickplay/", req.target_id)
            })
        } else if let Some(jellyfin) = state.trickplay.check_jellyfin(&trickplay_req).await {
            serde_json::json!({
                "status": "ready",
                "source": "jellyfin",
                "intervalMs": jellyfin.interval_ms,
                "width": jellyfin.width,
                "height": jellyfin.height,
                "columns": jellyfin.columns,
                "rows": jellyfin.rows,
                "sheetCount": jellyfin.sheet_count,
                "mime": jellyfin.mime,
                "thumbnailCount": jellyfin.thumbnail_count,
                "urlPrefix": format!("/api/v1/assets/{}/trickplay/", req.target_id)
            })
        } else {
            let queue_status = state.trickplay.enqueue(trickplay_req);
            let status = match queue_status {
                crate::trickplay::QueueStatus::Queued => "generating",
                crate::trickplay::QueueStatus::Busy => "busy",
                crate::trickplay::QueueStatus::Failed => "failed",
                crate::trickplay::QueueStatus::Disabled => "disabled",
            };
            serde_json::json!({ "status": status })
        };
        let json_bytes = serde_json::to_vec(&body).unwrap_or_default();
        let mut response = Response::new(Body::from(json_bytes));
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        let _ =
            send_stream_uplink_body(&state, core_url, &identity, &req.stream_id, response).await;
        return;
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &synthetic_req)
        .await
    {
        Ok(serve_res) => {
            let (parts, body) = serve_res.into_parts();
            let mut axum_res = Response::from_parts(parts, Body::new(body));
            if req.head {
                encode_head_uplink(&mut axum_res);
            }
            let _ = send_stream_uplink_body(&state, core_url, &identity, &req.stream_id, axum_res)
                .await;
        }
        Err(code) => {
            let status = match code {
                NodeErrorCode::UnknownAsset | NodeErrorCode::UnknownImage => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let _ =
                send_stream_uplink_error(&state, core_url, &identity, &req.stream_id, status).await;
        }
    }
}

// Carry HEAD's resource length in existing signed range metadata while uploading zero bytes.
fn encode_head_uplink(response: &mut Response) {
    if response.status().is_success()
        && !response.headers().contains_key(header::CONTENT_RANGE)
        && let Some(length) = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
        && let Ok(value) = HeaderValue::from_str(&format!("bytes */{length}"))
    {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    *response.body_mut() = Body::empty();
}

pub(super) async fn send_stream_uplink_body(
    _state: &NodeServerState,
    core_url: &str,
    identity: &NodeIdentityState,
    stream_id: &str,
    serve_res: Response,
) -> io::Result<()> {
    let parsed =
        url::Url::parse(core_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing host"))?
        .to_owned();
    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let path = format!(
        "/internal/v1/core/nodes/{}/stream-uplink/{}",
        identity.node_id.as_str(),
        stream_id
    );
    let now = unix_now();
    let nonce = random_nonce_hex();
    let secret = identity.shared_secret.as_deref().unwrap_or("");

    let mut headers_to_send = HeaderMap::new();
    headers_to_send.insert(
        header::HeaderName::from_static("x-media-stream-status"),
        HeaderValue::from_str(&serve_res.status().as_u16().to_string()).unwrap(),
    );
    for name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::CACHE_CONTROL,
        axum::http::HeaderName::from_static("x-media-preparation-error"),
    ] {
        if let Some(v) = serve_res.headers().get(&name) {
            headers_to_send.insert(name, v.clone());
        }
    }

    // This is an HTTP POST upload, even when the original representation was
    // a bodyless status or HEAD. Sign its actual upload framing, not a header
    // that Hyper will subsequently infer from the request body's exact size.
    if !headers_to_send.contains_key(header::CONTENT_LENGTH)
        && let Some(length) = http_body::Body::size_hint(serve_res.body()).exact()
        && let Ok(value) = HeaderValue::from_str(&length.to_string())
    {
        headers_to_send.insert(header::CONTENT_LENGTH, value);
    }

    let digest = crate::transport::stream_metadata_digest(&headers_to_send);
    let sig = sign_internal_request(
        secret,
        "node-to-core",
        "POST",
        &path,
        identity.node_id.as_str(),
        &identity.key_id,
        now,
        &nonce,
        &digest,
    );

    let stream = crate::transport::connect(core_url).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut builder = hyper::Request::builder()
        .method(Method::POST)
        .uri(&path)
        .header(header::HOST, host_header)
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .header(HDR_NODE_ID, identity.node_id.as_str())
        .header(HDR_KEY_ID, &identity.key_id)
        .header(HDR_TIMESTAMP, now.to_string())
        .header(HDR_NONCE, &nonce)
        .header(HDR_BODY_SHA256, &digest)
        .header(HDR_SIGNATURE, sig);

    for (k, v) in headers_to_send {
        if let Some(key) = k {
            builder = builder.header(key, v);
        }
    }

    let req = builder
        .body(serve_res.into_body())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let _ = sender.send_request(req).await.map_err(io::Error::other)?;
    Ok(())
}

async fn send_stream_uplink_error(
    state: &NodeServerState,
    core_url: &str,
    identity: &NodeIdentityState,
    stream_id: &str,
    status: StatusCode,
) -> io::Result<()> {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = status;
    send_stream_uplink_body(state, core_url, identity, stream_id, res).await
}
