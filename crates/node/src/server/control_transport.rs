//! Signed control requests/responses and authenticated bounded HTTP transport.

use super::state::NodeIdentityState;
use super::state::NodeServerState;
use axum::Json;
use axum::body::Body;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
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
use media_contracts::protocol::PROTOCOL_VERSION;
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

#[allow(clippy::result_large_err)]
pub(super) fn verify_signed_core_call(
    state: &NodeServerState,
    headers: &HeaderMap,
    method: &Method,
    path_and_query: &str,
    body: &[u8],
) -> Result<(NodeIdentityState, String), Response> {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "node_revoked" })),
        )
            .into_response());
    }
    let Some(secret) = identity.shared_secret.as_deref() else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "node_unpaired" })),
        )
            .into_response());
    };
    let version = headers
        .get(HDR_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if version != PROTOCOL_VERSION {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unsupported_protocol" })),
        )
            .into_response());
    }
    let header_node_id = headers
        .get(HDR_NODE_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if header_node_id != identity.node_id.as_str() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "wrong_node_id" })),
        )
            .into_response());
    }
    let header_key_id = headers
        .get(HDR_KEY_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(header_key_id.as_bytes(), identity.key_id.as_bytes()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid_key_id" })),
        )
            .into_response());
    }
    let timestamp = headers
        .get(HDR_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let nonce = headers
        .get(HDR_NONCE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let header_body_sha = headers
        .get(HDR_BODY_SHA256)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let actual_body_sha = sha256_hex(body);
    if !constant_time_eq(header_body_sha.as_bytes(), actual_body_sha.as_bytes()) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "body_sha256_mismatch" })),
        )
            .into_response());
    }
    let signature = headers
        .get(HDR_SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let now = unix_now();
    verify_internal_request(
        secret,
        "core-to-node",
        method.as_str(),
        path_and_query,
        identity.node_id.as_str(),
        &identity.key_id,
        timestamp,
        &nonce,
        &actual_body_sha,
        signature,
        now,
    )
    .map_err(|reason| {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response()
    })?;

    state
        .replay_window
        .check_and_record(identity.node_id.as_str(), &nonce, timestamp, now)
        .map_err(|reason| {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": reason })),
            )
                .into_response()
        })?;

    Ok((identity, nonce))
}

pub(super) fn signed_json_response<T: Serialize>(
    secret: &str,
    node_id: &NodeId,
    key_id: &str,
    request_nonce: &str,
    status: StatusCode,
    payload: &T,
) -> Response {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let now = unix_now();
    let res_nonce = random_nonce_hex();
    let sig = sign_internal_response(
        secret,
        request_nonce,
        status.as_u16(),
        node_id.as_str(),
        key_id,
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
    if let Ok(v) = HeaderValue::from_str(node_id.as_str()) {
        res.headers_mut().insert(HDR_NODE_ID, v);
    }
    if let Ok(v) = HeaderValue::from_str(key_id) {
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

pub async fn post_signed_to_core<T: Serialize>(
    state: &NodeServerState,
    path: &str,
    payload: &T,
) -> io::Result<()> {
    let _ = post_signed_to_core_raw(state, path, payload).await?;
    Ok(())
}

pub async fn post_signed_to_core_raw<T: Serialize>(
    state: &NodeServerState,
    path: &str,
    payload: &T,
) -> io::Result<Bytes> {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "node is revoked",
        ));
    }
    let core_base = identity.core_internal_url.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "core_internal_url not configured",
        )
    })?;
    let secret = identity.shared_secret.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "shared_secret not configured",
        )
    })?;
    let body =
        serde_json::to_vec(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    send_signed_http_request(
        &core_base,
        "node-to-core",
        Method::POST,
        path,
        &identity.node_id,
        &identity.key_id,
        &secret,
        None,
        Bytes::from(body),
    )
    .await
    .map(|(_, _, b)| b)
}

#[allow(clippy::too_many_arguments)]
pub async fn send_signed_http_request(
    base_url: &str,
    direction: &str,
    method: Method,
    path_and_query: &str,
    node_id: &NodeId,
    key_id: &str,
    secret: &str,
    extra_headers: Option<&HeaderMap>,
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

    let now = unix_now();
    let nonce = random_nonce_hex();
    let body_sha = sha256_hex(&body);
    let sig = sign_internal_request(
        secret,
        direction,
        method.as_str(),
        path_and_query,
        node_id.as_str(),
        key_id,
        now,
        &nonce,
        &body_sha,
    );

    let stream = crate::transport::connect(base_url).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(path_and_query)
        .header(header::HOST, host_header)
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .header(HDR_NODE_ID, node_id.as_str())
        .header(HDR_KEY_ID, key_id)
        .header(HDR_TIMESTAMP, now.to_string())
        .header(HDR_NONCE, &nonce)
        .header(HDR_BODY_SHA256, &body_sha)
        .header(HDR_SIGNATURE, sig);

    if !body.is_empty() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(extra) = extra_headers {
        for (k, v) in extra.iter() {
            if k == header::RANGE || k == header::ACCEPT {
                builder = builder.header(k, v);
            }
        }
    }

    let req = builder
        .body(Full::new(body))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let res = crate::transport::send_request(&mut sender, req).await?;
    let status = res.status();
    let headers = res.headers().clone();
    let collected = crate::transport::json_body(res.into_body()).await?;

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
            secret,
            &nonce,
            status.as_u16(),
            node_id.as_str(),
            key_id,
            resp_ts,
            resp_nonce,
            resp_sig,
            &sha256_hex(&collected),
            unix_now(),
        )
        .map_err(|reason| io::Error::new(io::ErrorKind::PermissionDenied, reason))?;
    }

    if !status.is_success() {
        return Err(io::Error::other(format!(
            "HTTP {} from {base_url}{path_and_query}",
            status.as_u16()
        )));
    }
    Ok((status, headers, collected))
}
