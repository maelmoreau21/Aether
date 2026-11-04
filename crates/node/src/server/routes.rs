//! HTTP route assembly and authenticated Node registration/lifecycle handlers.

use super::control_transport::signed_json_response;
use super::control_transport::verify_signed_core_call;
use super::playback::direct_stream_asset;
use super::playback::direct_stream_image;
use super::playback::internal_chapter_image;
use super::playback::internal_prepared;
use super::playback::internal_stream_asset;
use super::playback::internal_stream_image;
use super::playback::internal_trickplay_manifest;
use super::playback::internal_trickplay_tile;
use super::playback::stream_options;
use super::scans::cancel_remote_scan;
use super::scans::start_node_task;
use super::scans::start_remote_scan;
use super::state::NodeServerState;
use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use media_contracts::ids::NodeId;
use media_contracts::protocol::EnrollNodeRequest;
use media_contracts::protocol::EnrollNodeResponse;
use media_contracts::protocol::HDR_PROTOCOL;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::LibraryInventorySync;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::PairNodeRequest;
use media_contracts::protocol::PairNodeResponse;
use media_contracts::protocol::RotateSecretRequest;
use media_contracts::protocol::RotateSecretResponse;
use media_contracts::protocol::compute_enrollment_mutual_proof;
use media_contracts::protocol::compute_pairing_mutual_proof;
use media_contracts::protocol::compute_pairing_proof;
use media_contracts::protocol::compute_rotation_mutual_proof;
use media_contracts::protocol::constant_time_eq;
use media_contracts::protocol::derive_paired_secret;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::random_secret_hex;
use media_contracts::protocol::unix_now;
use serde::Serialize;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;

pub fn app_with_state(state: NodeServerState) -> Router {
    Router::new()
        .route("/internal/v1/health", get(health))
        .route("/internal/v1/ready", get(ready))
        .route("/internal/v1/node/pair", post(pair_node))
        .route("/internal/v1/node/heartbeat", post(node_heartbeat))
        .route("/internal/v1/node/rotate-secret", post(rotate_secret))
        .route("/internal/v1/node/revoke", post(revoke_node))
        .route("/internal/v1/node/inventory-sync", post(inventory_sync))
        .route("/internal/v1/node/scans", post(start_remote_scan))
        .route("/internal/v1/node/tasks", post(start_node_task))
        .route(
            "/internal/v1/node/scans/{scan_id}/cancel",
            post(cancel_remote_scan),
        )
        .route(
            "/internal/v1/node/assets/{id}/prepared/{component}",
            get(internal_prepared).head(internal_prepared),
        )
        .route(
            "/internal/v1/node/assets/{id}/content",
            get(internal_stream_asset).head(internal_stream_asset),
        )
        .route(
            "/internal/v1/node/assets/{id}/image",
            get(internal_stream_image).head(internal_stream_image),
        )
        .route(
            "/internal/v1/node/assets/{id}/trickplay",
            get(internal_trickplay_manifest),
        )
        .route(
            "/internal/v1/node/assets/{id}/chapter-images/{index}",
            get(internal_chapter_image).head(internal_chapter_image),
        )
        .route(
            "/internal/v1/node/assets/{id}/trickplay/{index}",
            get(internal_trickplay_tile).head(internal_trickplay_tile),
        )
        .route(
            "/stream/v1/assets/{id}/content",
            get(direct_stream_asset)
                .head(direct_stream_asset)
                .options(stream_options),
        )
        .route(
            "/stream/v1/assets/{id}/image",
            get(direct_stream_image)
                .head(direct_stream_image)
                .options(stream_options),
        )
        .with_state(state)
}

#[derive(Serialize)]
struct HealthBody {
    status: &'static str,
    protocol_version: &'static str,
    version: &'static str,
    node_id: NodeId,
    paired: bool,
    revoked: bool,
}

async fn health(State(state): State<NodeServerState>) -> impl IntoResponse {
    let id = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Json(HealthBody {
        status: "ok",
        protocol_version: PROTOCOL_VERSION,
        version: media_contracts::version::current_version(),
        node_id: id.node_id,
        paired: id.shared_secret.is_some(),
        revoked: id.revoked,
    })
}

async fn ready(State(state): State<NodeServerState>) -> impl IntoResponse {
    for root in &state.config.allowed_roots {
        if tokio::fs::metadata(root).await.is_err() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "status": "unready", "reason": "missing_allowed_root" })),
            );
        }
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready", "protocol_version": PROTOCOL_VERSION })),
    )
}

async fn pair_node(
    State(state): State<NodeServerState>,
    Json(req): Json<PairNodeRequest>,
) -> Response {
    let _identity_change = state.identity_changes.lock().await;
    if req.protocol_version != PROTOCOL_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "unsupported_protocol_version" })),
        )
            .into_response();
    }
    let now = unix_now();
    if (now - req.timestamp).abs() > media_contracts::protocol::MAX_CLOCK_SKEW_SECS {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "timestamp_out_of_range" })),
        )
            .into_response();
    }

    let current = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    let mut candidate_tokens = Vec::new();
    if let Some(tok) = &state.config.bootstrap_token
        && !tok.is_empty()
        && current.shared_secret.is_none()
        && !current.revoked
    {
        candidate_tokens.push(tok.clone());
    }
    if let Some(sec) = &current.shared_secret
        && !sec.is_empty()
    {
        candidate_tokens.push(sec.clone());
    }

    let mut matched_token = None;
    for tok in candidate_tokens {
        let expected_proof = compute_pairing_proof(
            &tok,
            &req.core_id,
            &req.key_id,
            &req.core_secret_half,
            &req.core_origin,
            req.timestamp,
            &req.nonce,
        );
        if constant_time_eq(expected_proof.as_bytes(), req.pairing_proof.as_bytes()) {
            matched_token = Some(tok);
            break;
        }
    }

    let Some(pairing_token) = matched_token else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid_pairing_token" })),
        )
            .into_response();
    };

    if state
        .replay_window
        .check_and_record(current.node_id.as_str(), &req.nonce, req.timestamp, now)
        .is_err()
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "replay_detected" })),
        )
            .into_response();
    }

    let node_secret_half = random_secret_hex();
    let derived_secret =
        derive_paired_secret(&pairing_token, &req.core_secret_half, &node_secret_half);
    let res_nonce = random_nonce_hex();
    let res_ts = unix_now();
    let mutual_proof = compute_pairing_mutual_proof(
        &derived_secret,
        &req.nonce,
        current.node_id.as_str(),
        &req.key_id,
        &node_secret_half,
        res_ts,
        &res_nonce,
    );

    {
        let mut guard = state.identity.write().unwrap_or_else(|e| e.into_inner());
        guard.core_internal_url = Some(req.core_origin);
        guard.shared_secret = Some(derived_secret.clone());
        guard.key_id = req.key_id.clone();
        guard.revoked = false;
    }
    if let Err(err) = state.persist_identity().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response();
    }

    let roots = state
        .config
        .allowed_roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let resp = PairNodeResponse {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        version: Some(media_contracts::version::current_version().to_string()),
        node_id: current.node_id.clone(),
        roots,
        public_url: current.public_url,
        node_secret_half,
        timestamp: res_ts,
        nonce: res_nonce,
        mutual_proof,
    };
    signed_json_response(
        &derived_secret,
        &current.node_id,
        &req.key_id,
        &req.nonce,
        StatusCode::OK,
        &resp,
    )
}

async fn node_heartbeat(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/node/heartbeat");
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(res) => return res,
        };
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
    let roots = state
        .config
        .allowed_roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let resource_pressure = Some(state.resources.sample().await);
    let payload = HeartbeatPayload {
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
    let secret = identity.shared_secret.unwrap_or_default();
    signed_json_response(
        &secret,
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::OK,
        &payload,
    )
}

async fn rotate_secret(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let _identity_change = state.identity_changes.lock().await;
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/node/rotate-secret");
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let req: RotateSecretRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_json" })),
            )
                .into_response();
        }
    };
    {
        let mut guard = state.identity.write().unwrap_or_else(|e| e.into_inner());
        guard.shared_secret = Some(req.new_secret.clone());
        guard.key_id = req.new_key_id.clone();
        guard.revoked = false;
    }
    if state.persist_identity().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let res_ts = unix_now();
    let res_nonce = random_nonce_hex();
    let mutual_proof = compute_rotation_mutual_proof(
        &req.new_secret,
        &nonce,
        identity.node_id.as_str(),
        &req.new_key_id,
        res_ts,
        &res_nonce,
    );
    let resp = RotateSecretResponse {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        node_id: identity.node_id.clone(),
        new_key_id: req.new_key_id.clone(),
        timestamp: res_ts,
        nonce: res_nonce,
        mutual_proof,
    };
    signed_json_response(
        &req.new_secret,
        &identity.node_id,
        &req.new_key_id,
        &nonce,
        StatusCode::OK,
        &resp,
    )
}

async fn revoke_node(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let _identity_change = state.identity_changes.lock().await;
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/node/revoke");
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    {
        let mut scans = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
        for cancel in scans.values() {
            cancel.store(true, Ordering::SeqCst);
        }
        scans.clear();
    }
    {
        let mut guard = state.identity.write().unwrap_or_else(|e| e.into_inner());
        guard.revoked = true;
    }
    if state.persist_identity().await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let secret = identity.shared_secret.unwrap_or_default();
    signed_json_response(
        &secret,
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "revoked", "node_id": identity.node_id }),
    )
}

async fn inventory_sync(
    State(state): State<NodeServerState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/node/inventory-sync");
    let (identity, nonce) =
        match verify_signed_core_call(&state, &headers, &Method::POST, pq, &body) {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let sync: LibraryInventorySync = match serde_json::from_slice(&body) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_json" })),
            )
                .into_response();
        }
    };
    let mut validated_root = None;
    for allowed in &state.config.allowed_roots {
        if let Ok(valid) =
            crate::validate_library_root(Path::new(&sync.root_snapshot), allowed).await
        {
            validated_root = Some(valid);
            break;
        }
    }
    if validated_root.is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "invalid_library_root" })),
        )
            .into_response();
    }
    {
        let mut inv = state.inventory.write().unwrap_or_else(|e| e.into_inner());
        inv.apply_library_sync(sync);
    }
    let _ = state.persist_inventory().await;
    let secret = identity.shared_secret.unwrap_or_default();
    signed_json_response(
        &secret,
        &identity.node_id,
        &identity.key_id,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "synced" }),
    )
}

/// Self-enroll this Node with Core using a single-use enrollment token.
pub async fn enroll_with_core(
    core_url: &str,
    enrollment_token: &str,
    state: &NodeServerState,
) -> io::Result<()> {
    let _identity_change = state.identity_changes.lock().await;
    let (node_id, name, public_url) = {
        let guard = state.identity.read().unwrap_or_else(|e| e.into_inner());
        (
            guard.node_id.clone(),
            guard.name.clone(),
            guard.public_url.clone(),
        )
    };

    let node_secret_half = random_secret_hex();
    let nonce = random_nonce_hex();
    let now = unix_now();
    let roots: Vec<String> = state
        .config
        .allowed_roots
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let req = EnrollNodeRequest {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        version: Some(media_contracts::version::current_version().to_string()),
        enrollment_token: enrollment_token.to_owned(),
        node_id: Some(node_id.clone()),
        name,
        roots,
        public_url,
        node_secret_half: node_secret_half.clone(),
        timestamp: now,
        nonce: nonce.clone(),
    };

    let body =
        serde_json::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

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

    let stream = crate::transport::connect(core_url).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let http_req = hyper::Request::builder()
        .method(Method::POST)
        .uri("/api/v1/nodes/enroll")
        .header(header::HOST, host_header)
        .header(header::CONTENT_TYPE, "application/json")
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .body(Full::new(Bytes::from(body)))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    let res = crate::transport::send_request(&mut sender, http_req).await?;
    let status = res.status();
    let bytes = crate::transport::json_body(res.into_body()).await?;

    if !status.is_success() {
        return Err(io::Error::other(format!(
            "Enrollment failed with HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        )));
    }

    let enroll_res: EnrollNodeResponse = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let derived_secret = derive_paired_secret(
        enrollment_token,
        &enroll_res.core_secret_half,
        &node_secret_half,
    );

    let expected_mutual = compute_enrollment_mutual_proof(
        &derived_secret,
        &nonce,
        enroll_res.node_id.as_str(),
        &enroll_res.key_id,
        &enroll_res.core_secret_half,
        enroll_res.timestamp,
        &enroll_res.nonce,
    );

    if !constant_time_eq(
        expected_mutual.as_bytes(),
        enroll_res.mutual_proof.as_bytes(),
    ) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid enrollment mutual proof",
        ));
    }

    {
        let mut guard = state.identity.write().unwrap_or_else(|e| e.into_inner());
        guard.node_id = enroll_res.node_id;
        guard.key_id = enroll_res.key_id;
        guard.shared_secret = Some(derived_secret);
        guard.core_internal_url = Some(core_url.to_string());
        guard.revoked = false;
    }

    state.persist_identity().await?;
    Ok(())
}
