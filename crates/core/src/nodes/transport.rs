//! Existing signed control messages, URL validation and pairing/rotation HTTP transport.
use super::NodesState;
use super::registry::{MediaNodeRecord, load_node};
use crate::error;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::Response;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
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
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::RotateSecretResponse;
use media_contracts::protocol::compute_rotation_mutual_proof;
use media_contracts::protocol::constant_time_eq;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sha256_hex;
use media_contracts::protocol::sign_internal_request;
use media_contracts::protocol::sign_internal_response;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_request;
use media_contracts::protocol::verify_internal_response;
use serde::Serialize;
use std::io;

pub(super) fn validate_http_url(raw: &str) -> Result<String, StatusCode> {
    let parsed = media_node::transport::validate_url(raw).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

pub(super) fn validate_public_url(raw: &str) -> Result<String, StatusCode> {
    let origin = validate_http_url(raw)?;
    let parsed = url::Url::parse(&origin).map_err(|_| StatusCode::BAD_REQUEST)?;
    let host = parsed.host_str().unwrap_or("").trim_matches(['[', ']']);
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if parsed.scheme() == "http" && !loopback {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(origin)
}

#[allow(clippy::result_large_err)]
pub(super) async fn verify_signed_node_call(
    state: &NodesState,
    headers: &HeaderMap,
    method: &Method,
    path_and_query: &str,
    body: &[u8],
) -> Result<(MediaNodeRecord, String), Response> {
    let version = headers
        .get(HDR_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if version != PROTOCOL_VERSION {
        return Err(error(StatusCode::UNAUTHORIZED, "unsupported_protocol"));
    }
    let header_node_id = headers
        .get(HDR_NODE_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if header_node_id.is_empty() || header_node_id == "local" {
        return Err(error(StatusCode::UNAUTHORIZED, "invalid_node_id"));
    }
    let rec = match load_node(&state.auth.store, header_node_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return Err(error(StatusCode::FORBIDDEN, "unknown_node")),
        Err(_) => return Err(error(StatusCode::INTERNAL_SERVER_ERROR, "database_error")),
    };
    if rec.status == NodeStatus::Revoked.as_str() || rec.shared_secret.is_empty() {
        return Err(error(StatusCode::FORBIDDEN, "node_revoked"));
    }
    let header_key_id = headers
        .get(HDR_KEY_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(header_key_id.as_bytes(), rec.key_id.as_bytes()) {
        return Err(error(StatusCode::UNAUTHORIZED, "invalid_key_id"));
    }
    let timestamp = headers
        .get(HDR_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let nonce = match headers.get(HDR_NONCE).and_then(|v| v.to_str().ok()) {
        Some(n) if !n.is_empty() => n.to_owned(),
        _ => return Err(error(StatusCode::UNAUTHORIZED, "missing_nonce")),
    };
    let header_body_sha = headers
        .get(HDR_BODY_SHA256)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let actual_body_sha = sha256_hex(body);
    if !constant_time_eq(header_body_sha.as_bytes(), actual_body_sha.as_bytes()) {
        return Err(error(StatusCode::UNAUTHORIZED, "body_sha256_mismatch"));
    }
    let signature = headers
        .get(HDR_SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let now = unix_now();
    verify_internal_request(
        &rec.shared_secret,
        "node-to-core",
        method.as_str(),
        path_and_query,
        &rec.id,
        &rec.key_id,
        timestamp,
        &nonce,
        &actual_body_sha,
        signature,
        now,
    )
    .map_err(|reason| error(StatusCode::UNAUTHORIZED, reason))?;

    state
        .replay_window
        .check_and_record(&rec.id, &nonce, timestamp, now)
        .map_err(|reason| error(StatusCode::UNAUTHORIZED, reason))?;

    let _ = crate::database::query(
        "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
    )
    .bind(now)
    .bind(&rec.id)
    .execute(&state.auth.store.pool)
    .await;
    state
        .auth
        .store
        .node_registry
        .record_success(&rec.id, None, now);

    Ok((rec, nonce))
}

pub(super) fn signed_core_response<T: Serialize>(
    rec: &MediaNodeRecord,
    request_nonce: &str,
    status: StatusCode,
    payload: &T,
) -> Response {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let now = unix_now();
    let res_nonce = random_nonce_hex();
    let sig = sign_internal_response(
        &rec.shared_secret,
        request_nonce,
        status.as_u16(),
        &rec.id,
        &rec.key_id,
        now,
        &res_nonce,
        &sha256_hex(&body),
    );
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = status;
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res.headers_mut()
        .insert(HDR_PROTOCOL, HeaderValue::from_static(PROTOCOL_VERSION));
    if let Ok(v) = HeaderValue::from_str(&rec.id) {
        res.headers_mut().insert(HDR_NODE_ID, v);
    }
    if let Ok(v) = HeaderValue::from_str(&rec.key_id) {
        res.headers_mut().insert(HDR_KEY_ID, v);
    }
    if let Ok(v) = HeaderValue::from_str(&now.to_string()) {
        res.headers_mut().insert(HDR_RES_TIMESTAMP, v);
    }
    if let Ok(v) = HeaderValue::from_str(&res_nonce) {
        res.headers_mut().insert(HDR_RES_NONCE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&sig) {
        res.headers_mut().insert(HDR_RES_SIGNATURE, v);
    }
    res
}

pub(super) async fn send_plain_json_post(
    base_url: &str,
    path: &str,
    body: Bytes,
) -> io::Result<(StatusCode, HeaderMap, Bytes)> {
    let parsed =
        url::Url::parse(base_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing host"))?
        .to_owned();

    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let stream = media_node::transport::connect(base_url).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = hyper::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, host_header)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(body))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let res = media_node::transport::send_request(&mut sender, req).await?;
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = media_node::transport::json_body(res.into_body()).await?;
    Ok((status, headers, bytes))
}

pub(super) async fn send_signed_rotation_request(
    base_url: &str,
    node_id: &NodeId,
    old_key_id: &str,
    old_secret: &str,
    new_key_id: &str,
    new_secret: &str,
    body: Bytes,
) -> io::Result<Bytes> {
    let parsed =
        url::Url::parse(base_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing host"))?
        .to_owned();

    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let path = "/internal/v1/node/rotate-secret";
    let now = unix_now();
    let nonce = random_nonce_hex();
    let body_sha = sha256_hex(&body);
    let sig = sign_internal_request(
        old_secret,
        "core-to-node",
        "POST",
        path,
        node_id.as_str(),
        old_key_id,
        now,
        &nonce,
        &body_sha,
    );

    let stream = media_node::transport::connect(base_url).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = hyper::Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, host_header)
        .header(header::CONTENT_TYPE, "application/json")
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .header(HDR_NODE_ID, node_id.as_str())
        .header(HDR_KEY_ID, old_key_id)
        .header(HDR_TIMESTAMP, now.to_string())
        .header(HDR_NONCE, &nonce)
        .header(HDR_BODY_SHA256, &body_sha)
        .header(HDR_SIGNATURE, sig)
        .body(Full::new(body))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    let res = media_node::transport::send_request(&mut sender, req).await?;
    let status = res.status();
    let headers = res.headers().clone();
    let collected = media_node::transport::json_body(res.into_body()).await?;

    if !status.is_success() {
        return Err(io::Error::other("rotation rejected by node"));
    }

    {
        let resp_sig = headers
            .get(HDR_RES_SIGNATURE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "missing_response_signature",
                )
            })?;
        let resp_ts = headers
            .get(HDR_RES_TIMESTAMP)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let resp_nonce = headers
            .get(HDR_RES_NONCE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        verify_internal_response(
            new_secret,
            &nonce,
            status.as_u16(),
            node_id.as_str(),
            new_key_id,
            resp_ts,
            resp_nonce,
            resp_sig,
            &sha256_hex(&collected),
            unix_now(),
        )
        .map_err(|reason| io::Error::new(io::ErrorKind::PermissionDenied, reason))?;
    }

    let rot_res: RotateSecretResponse =
        serde_json::from_slice(&collected).map_err(io::Error::other)?;
    let expected_mutual = compute_rotation_mutual_proof(
        new_secret,
        &nonce,
        node_id.as_str(),
        new_key_id,
        rot_res.timestamp,
        &rot_res.nonce,
    );
    if !constant_time_eq(expected_mutual.as_bytes(), rot_res.mutual_proof.as_bytes()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid rotation mutual proof",
        ));
    }

    Ok(collected)
}
