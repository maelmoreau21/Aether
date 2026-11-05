//! Outbound Node control channels, bounded relay uplinks and heartbeats.
use super::NodesState;
use super::registry::{load_node, refresh_node_in_registry};
use super::transport::{signed_core_response, verify_signed_node_call};
use crate::error;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use media_contracts::protocol::HDR_KEY_ID;
use media_contracts::protocol::HDR_NONCE;
use media_contracts::protocol::HDR_SIGNATURE;
use media_contracts::protocol::HDR_TIMESTAMP;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_request;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

pub struct RelayStreamResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Body,
}

pub static ACTIVE_CHANNELS: LazyLock<RwLock<HashMap<String, mpsc::Sender<NodeCommandEnvelope>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub static PENDING_STREAMS: LazyLock<Mutex<HashMap<String, oneshot::Sender<RelayStreamResponse>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn register_pending_stream(
    pending: &mut HashMap<String, oneshot::Sender<RelayStreamResponse>>,
    id: String,
    sender: oneshot::Sender<RelayStreamResponse>,
) -> bool {
    // A disconnected browser can drop the receiver before Node sends headers.
    // Reap those entries before admitting another request, keeping abandoned
    // metadata requests bounded even when the Node is slow or disconnected.
    pending.retain(|_, sender| !sender.is_closed());
    if pending.len() >= 64 {
        return false;
    }
    pending.insert(id, sender);
    true
}

pub(super) async fn node_channel(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/nodes/channel");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(v) => v,
            Err(res) => return res,
        };
    if rec.status == NodeStatus::Revoked.as_str() {
        return error(StatusCode::FORBIDDEN, "node_revoked");
    }

    let hb: Result<HeartbeatPayload, _> = serde_json::from_slice(&body);
    let now = unix_now();
    if let Ok(ref h) = hb {
        let roots_json = serde_json::to_string(&h.roots).unwrap_or_else(|_| rec.roots_json.clone());
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=?, roots_json=?, version=COALESCE(?, version) WHERE id=? AND status != 'revoked'",
        )
        .bind(now)
        .bind(roots_json)
        .bind(h.version.as_deref())
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_heartbeat(&id, h, Some(1), now);
    } else {
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(now)
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_success(&id, Some(1), now);
    }

    let (tx, mut rx) = mpsc::channel::<NodeCommandEnvelope>(8);
    ACTIVE_CHANNELS.write().await.insert(id.clone(), tx);

    let command = match tokio::time::timeout(Duration::from_secs(25), rx.recv()).await {
        Ok(Some(cmd)) => cmd,
        _ => NodeCommandEnvelope {
            command_id: "ping".to_owned(),
            command_type: NodeCommandType::Ping,
            payload: serde_json::Value::Null,
        },
    };

    {
        let mut channels = ACTIVE_CHANNELS.write().await;
        if let Some(s) = channels.get(&id)
            && s.is_closed()
        {
            channels.remove(&id);
        }
    }

    signed_core_response(&rec, &nonce, StatusCode::OK, &command)
}

pub(super) async fn receive_stream_uplink(
    State(state): State<NodesState>,
    AxumPath((id, stream_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    body: Body,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/nodes/stream-uplink");
    let rec = match load_node(&state.auth.store, &id).await {
        Ok(Some(r)) => r,
        _ => return error(StatusCode::UNAUTHORIZED, "unknown_node"),
    };
    if rec.status == NodeStatus::Revoked.as_str() {
        return error(StatusCode::FORBIDDEN, "node_revoked");
    }

    let digest = media_node::transport::stream_metadata_digest(&headers);
    let req_ts = headers
        .get(HDR_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let req_nonce = headers
        .get(HDR_NONCE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let req_sig = headers
        .get(HDR_SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let req_kid = headers
        .get(HDR_KEY_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if req_kid != rec.key_id {
        return error(StatusCode::UNAUTHORIZED, "invalid_key_id");
    }

    if verify_internal_request(
        &rec.shared_secret,
        "node-to-core",
        Method::POST.as_str(),
        pq,
        &id,
        &rec.key_id,
        req_ts,
        req_nonce,
        &digest,
        req_sig,
        unix_now(),
    )
    .is_err()
    {
        return error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }

    let stream_status = headers
        .get("x-media-stream-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u16>().ok())
        .and_then(|s| StatusCode::from_u16(s).ok())
        .unwrap_or(StatusCode::OK);

    let sender = PENDING_STREAMS.lock().await.remove(&stream_id);
    if let Some(tx) = sender {
        let _ = tx.send(RelayStreamResponse {
            status: stream_status,
            headers: headers.clone(),
            body,
        });
        StatusCode::OK.into_response()
    } else {
        error(StatusCode::NOT_FOUND, "stream_not_found")
    }
}

pub(super) async fn receive_node_heartbeat(
    State(state): State<NodesState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/heartbeat");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    if let Ok(hb) = serde_json::from_slice::<HeartbeatPayload>(&body) {
        let now = unix_now();
        let roots_json =
            serde_json::to_string(&hb.roots).unwrap_or_else(|_| rec.roots_json.clone());
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', roots_json=?, version=COALESCE(?, version), last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(roots_json)
        .bind(hb.version.as_deref())
        .bind(now)
        .bind(&rec.id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &rec.id).await;
        state
            .auth
            .store
            .node_registry
            .record_heartbeat(&rec.id, &hb, Some(1), now);
    }
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "ok" }),
    )
}
