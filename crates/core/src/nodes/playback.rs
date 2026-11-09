//! Direct capabilities, live playback authorization and streamed Core relay responses.
use super::NodesState;
use super::channel::{
    ACTIVE_CHANNELS, PENDING_STREAMS, RelayStreamResponse, register_pending_stream,
};
use super::registry::{MediaNodeRecord, NodeRuntimeRegistry, refresh_node_in_registry};
use super::transport::{signed_core_response, validate_public_url, verify_signed_node_call};
use crate::error;
use crate::identity::IdentityStore;
use crate::inventory;
use axum::body::Body;
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::http::header;
use axum::response::Response;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_body_util::StreamBody;
use hyper_util::rt::TokioIo;
use media_contracts::ids::LibraryId;
use media_contracts::ids::NodeId;
use media_contracts::protocol::DEFAULT_PLAYBACK_TOKEN_TTL_SECS;
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
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::PlaybackTokenClaims;
use media_contracts::protocol::StreamRelayRequest;
use media_contracts::protocol::issue_playback_token;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sha256_hex;
use media_contracts::protocol::sign_internal_request;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_response;
use std::io;
use std::sync::Arc;
use std::time::Duration;

struct ActiveCoreRelayGuard {
    registry: Arc<NodeRuntimeRegistry>,
    node_id: String,
}

impl ActiveCoreRelayGuard {
    fn enter(registry: &Arc<NodeRuntimeRegistry>, node_id: &str) -> Self {
        registry.increment_relay(node_id);
        Self {
            registry: registry.clone(),
            node_id: node_id.to_owned(),
        }
    }
}

impl Drop for ActiveCoreRelayGuard {
    fn drop(&mut self) {
        self.registry.decrement_relay(&self.node_id);
    }
}

fn track_core_relay_response(
    response: Response,
    registry: &Arc<NodeRuntimeRegistry>,
    node_id: &str,
) -> Response {
    if !response.status().is_success() {
        return response;
    }
    let guard = ActiveCoreRelayGuard::enter(registry, node_id);
    let (parts, body) = response.into_parts();
    let wrapped = StreamBody::new(futures_util::stream::unfold(
        (body, guard),
        |(mut body, guard)| async move { body.frame().await.map(|frame| (frame, (body, guard))) },
    ));
    Response::from_parts(parts, Body::new(wrapped))
}

pub fn build_direct_playback_url(
    rec: &MediaNodeRecord,
    library_id: &str,
    target_kind: &str,
    target_id: &str,
    action: &str,
    session_hash: &str,
) -> Option<(String, i64)> {
    let now = unix_now();
    // A Node that stopped heartbeating is not handed out as a direct route: the browser would
    // wait on a dead socket instead of the Core falling back to a healthy copy or the relay.
    if rec.kind != "remote"
        || rec.status != NodeStatus::Connected.as_str()
        || super::registry::node_is_stale(rec, now)
    {
        return None;
    }
    let checked_public_url = validate_public_url(rec.public_url.as_deref()?).ok()?;
    let public_url = checked_public_url.trim_end_matches('/');
    if public_url.is_empty() || rec.shared_secret.is_empty() {
        return None;
    }
    let exp = now + DEFAULT_PLAYBACK_TOKEN_TTL_SECS;
    let claims = PlaybackTokenClaims {
        v: PROTOCOL_VERSION.to_owned(),
        kid: rec.key_id.clone(),
        node_id: NodeId::new(&rec.id),
        target_kind: target_kind.to_owned(),
        target_id: target_id.to_owned(),
        library_id: LibraryId::new(library_id),
        action: action.to_owned(),
        iat: now,
        exp,
        jti: random_nonce_hex(),
        session_hash: session_hash.to_owned(),
    };
    let token = issue_playback_token(&rec.shared_secret, &claims).ok()?;
    let route = if target_kind == "image" {
        format!("{public_url}/stream/v1/assets/{target_id}/image?token={token}")
    } else {
        format!("{public_url}/stream/v1/assets/{target_id}/content?token={token}")
    };
    Some((route, exp))
}

/// Bounded streaming relay from a remote Node to the browser when direct Node streaming
/// is not used or as a fallback. Never loads the full asset into Core RAM.
pub async fn relay_from_remote_node(
    store: &IdentityStore,
    rec: &MediaNodeRecord,
    path_and_query: &str,
    request: Request,
) -> Response {
    if rec.status == NodeStatus::Revoked.as_str() {
        return error(StatusCode::SERVICE_UNAVAILABLE, "node_revoked");
    }
    let relay_started = std::time::Instant::now();

    // Only the response headers are bounded here; the media body remains streamed.
    let component = path_and_query
        .split("/prepared/")
        .nth(1)
        .and_then(|value| value.split('?').next());
    let progressive = component.is_some_and(|value| {
        media_node::video_stream::position(value).is_some()
            || media_node::transcode::request(value).is_some()
            || media_node::packaging::Component::opus_chunk(value).is_some()
            || media_node::packaging::Component::audio_chunk(value).is_some()
    });
    let head_wait = if progressive {
        // Bounded windows and live pipes must not inherit the hour-long budget
        // retained for explicit legacy full-file preparations.
        Duration::from_secs(35)
    } else if path_and_query.contains("/prepared/") {
        Duration::from_secs(media_node::packaging::MAX_PREPARATION_SECONDS + 5)
    } else {
        Duration::from_secs(6)
    };
    let is_channel_active = super::channel::channel_is_active(&rec.id).await;
    if is_channel_active || rec.base_url.is_none() {
        let is_image = path_and_query.contains("/image");
        let prepared = path_and_query
            .split("/prepared/")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .map(|s| format!("prepared:{s}"));
        let target_kind_str = if let Some(parameters) =
            path_and_query.split("/chapter-images/").nth(1)
        {
            format!("chapter-image:{parameters}")
        } else if let Some(index) = path_and_query.split("/trickplay/").nth(1) {
            format!("trickplay:{index}")
        } else if path_and_query.ends_with("/trickplay") || path_and_query.contains("/trickplay?") {
            path_and_query.split_once('?').map_or_else(
                || "trickplay".to_string(),
                |(_, query)| format!("trickplay?{query}"),
            )
        } else if let Some(ref s) = prepared {
            s.clone()
        } else if is_image {
            "image".to_string()
        } else {
            "asset".to_string()
        };
        let target_kind = target_kind_str.as_str();
        let parts: Vec<&str> = path_and_query
            .split('?')
            .next()
            .unwrap_or("")
            .split('/')
            .collect();
        let target_id = parts
            .iter()
            .position(|&p| p == "assets")
            .and_then(|idx| parts.get(idx + 1))
            .copied()
            .unwrap_or("");

        let range = request
            .headers()
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let stream_id = format!("str-{}", uuid::Uuid::new_v4().simple());
        let (tx, rx) = tokio::sync::oneshot::channel::<RelayStreamResponse>();
        if !register_pending_stream(&mut *PENDING_STREAMS.lock().await, stream_id.clone(), tx) {
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_relay_busy");
        }

        let is_outbound = rec.base_url.is_none();
        let mut sent = false;
        let deadline = if is_outbound {
            tokio::time::Instant::now() + Duration::from_millis(3000)
        } else {
            tokio::time::Instant::now() + Duration::from_millis(100)
        };

        while tokio::time::Instant::now() <= deadline {
            let channel_sender = {
                let channels = ACTIVE_CHANNELS.read().await;
                channels.get(&rec.id).cloned()
            };
            if let Some(sender) = channel_sender
                && !sender.is_closed()
            {
                let cmd = NodeCommandEnvelope {
                    command_id: format!("cmd-{}", uuid::Uuid::new_v4().simple()),
                    command_type: NodeCommandType::StreamRelay,
                    expires_at: None,
                    payload: serde_json::to_value(StreamRelayRequest {
                        head: request.method() == Method::HEAD,
                        stream_id: stream_id.clone(),
                        target_kind: target_kind.to_owned(),
                        target_id: target_id.to_owned(),
                        range: range.clone(),
                        image_width: if is_image {
                            request.uri().query().and_then(|q| {
                                url::form_urlencoded::parse(q.as_bytes())
                                    .find(|(k, _)| k == "width")
                                    .and_then(|(_, v)| v.parse().ok())
                            })
                        } else {
                            None
                        },
                        if_none_match: request
                            .headers()
                            .get(header::IF_NONE_MATCH)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned),
                    })
                    .unwrap_or_default(),
                };
                if sender.send(cmd).await.is_ok() {
                    sent = true;
                    break;
                }
            }
            if !is_outbound {
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }

        if sent {
            match tokio::time::timeout(head_wait, rx).await {
                Ok(Ok(stream_res)) => {
                    let head = request.method() == Method::HEAD;
                    let mut out = Response::new(if head { Body::empty() } else { stream_res.body });
                    *out.status_mut() = stream_res.status;
                    for header_name in [
                        header::CONTENT_TYPE,
                        header::CONTENT_LENGTH,
                        header::CONTENT_RANGE,
                        header::ACCEPT_RANGES,
                        header::CACHE_CONTROL,
                        axum::http::HeaderName::from_static("x-media-preparation-error"),
                    ] {
                        if let Some(val) = stream_res.headers.get(&header_name) {
                            out.headers_mut().insert(header_name, val.clone());
                        }
                    }
                    if head
                        && out.status().is_success()
                        && let Some(range) = out
                            .headers()
                            .get(header::CONTENT_RANGE)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned)
                    {
                        let length = if let Some(total) = range.strip_prefix("bytes */") {
                            total.parse::<u64>().ok()
                        } else {
                            range
                                .strip_prefix("bytes ")
                                .and_then(|v| v.split('/').next())
                                .and_then(|v| v.split_once('-'))
                                .and_then(|(start, end)| {
                                    end.parse::<u64>()
                                        .ok()?
                                        .checked_sub(start.parse::<u64>().ok()?)?
                                        .checked_add(1)
                                })
                        };
                        if let Some(length) =
                            length.and_then(|v| HeaderValue::from_str(&v.to_string()).ok())
                        {
                            out.headers_mut().insert(header::CONTENT_LENGTH, length);
                        }
                        if out.status() == StatusCode::OK {
                            out.headers_mut().remove(header::CONTENT_RANGE);
                        }
                    }
                    let rtt_ms = relay_started.elapsed().as_millis() as u32;
                    store
                        .node_registry
                        .record_success(&rec.id, Some(rtt_ms), unix_now());
                    return track_core_relay_response(out, &store.node_registry, &rec.id);
                }
                _ => {
                    PENDING_STREAMS.lock().await.remove(&stream_id);
                    if rec.base_url.is_none() {
                        let _ = crate::database::query(
                            "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
                        )
                        .bind(&rec.id)
                        .execute(&store.pool)
                        .await;
                        store.node_registry.record_failure(&rec.id, true);
                        let _ = refresh_node_in_registry(store, &rec.id).await;
                        return error(StatusCode::GATEWAY_TIMEOUT, "stream_timeout");
                    }
                }
            }
        } else {
            PENDING_STREAMS.lock().await.remove(&stream_id);
            if rec.base_url.is_none() {
                let _ = crate::database::query(
                    "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
                )
                .bind(&rec.id)
                .execute(&store.pool)
                .await;
                store.node_registry.record_failure(&rec.id, true);
                let _ = refresh_node_in_registry(store, &rec.id).await;
                return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
            }
        }
    }

    let Some(base_url) = rec.base_url.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
    };
    let parsed = match url::Url::parse(base_url) {
        Ok(u) => u,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
    };
    let Some(host) = parsed.host_str().map(str::to_owned) else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
    };

    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let stream = match tokio::time::timeout(
        Duration::from_secs(3),
        media_node::transport::connect(base_url),
    )
    .await
    {
        Ok(Ok(s)) => s,
        _ => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&rec.id)
            .execute(&store.pool)
            .await;
            store.node_registry.record_failure(&rec.id, true);
            let _ = refresh_node_in_registry(store, &rec.id).await;
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        }
    };

    let io = TokioIo::new(stream);
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(ok) => ok,
        Err(_) => {
            store.node_registry.record_failure(&rec.id, true);
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        }
    };
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let method = if *request.method() == Method::HEAD {
        Method::HEAD
    } else {
        Method::GET
    };
    let now = unix_now();
    let nonce = random_nonce_hex();
    let empty_sha = sha256_hex(b"");
    let sig = sign_internal_request(
        &rec.shared_secret,
        "core-to-node",
        method.as_str(),
        path_and_query,
        &rec.id,
        &rec.key_id,
        now,
        &nonce,
        &empty_sha,
    );

    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(path_and_query)
        .header(header::HOST, host_header)
        .header(HDR_PROTOCOL, PROTOCOL_VERSION)
        .header(HDR_NODE_ID, &rec.id)
        .header(HDR_KEY_ID, &rec.key_id)
        .header(HDR_TIMESTAMP, now.to_string())
        .header(HDR_NONCE, &nonce)
        .header(HDR_BODY_SHA256, &empty_sha)
        .header(HDR_SIGNATURE, sig);

    if let Some(range) = request.headers().get(header::RANGE) {
        builder = builder.header(header::RANGE, range);
    }
    if let Some(condition) = request.headers().get(header::IF_NONE_MATCH) {
        builder = builder.header(header::IF_NONE_MATCH, condition);
    }

    let upstream_req = match builder.body(Full::new(Bytes::new())) {
        Ok(r) => r,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "stream_error"),
    };

    let upstream_res =
        match tokio::time::timeout(head_wait, sender.send_request(upstream_req)).await {
            Ok(Ok(r)) => r,
            _ => {
                let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&rec.id)
            .execute(&store.pool)
            .await;
                store.node_registry.record_failure(&rec.id, true);
                let _ = refresh_node_in_registry(store, &rec.id).await;
                return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
            }
        };

    let rtt_ms = relay_started.elapsed().as_millis() as u32;
    let _ = crate::database::query(
        "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
    )
    .bind(now)
    .bind(&rec.id)
    .execute(&store.pool)
    .await;
    store
        .node_registry
        .record_success(&rec.id, Some(rtt_ms), now);

    let h = upstream_res.headers();
    let response_signature = h
        .get(HDR_RES_SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let response_nonce = h
        .get(HDR_RES_NONCE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let response_timestamp = h
        .get(HDR_RES_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if verify_internal_response(
        &rec.shared_secret,
        &nonce,
        upstream_res.status().as_u16(),
        &rec.id,
        &rec.key_id,
        response_timestamp,
        response_nonce,
        response_signature,
        &media_node::transport::stream_metadata_digest(h),
        unix_now(),
    )
    .is_err()
    {
        return error(StatusCode::BAD_GATEWAY, "invalid_node_response");
    }
    let (parts, incoming) = upstream_res.into_parts();
    let stream_body = StreamBody::new(
        incoming
            .into_data_stream()
            .map(|res| res.map(hyper::body::Frame::data).map_err(io::Error::other)),
    );
    let mut out = Response::new(Body::new(stream_body));
    *out.status_mut() = parts.status;
    for header_name in [
        header::CONTENT_TYPE,
        header::CONTENT_LENGTH,
        header::CONTENT_RANGE,
        header::ACCEPT_RANGES,
        header::CACHE_CONTROL,
        axum::http::HeaderName::from_static("x-media-preparation-error"),
    ] {
        if let Some(val) = parts.headers.get(&header_name) {
            out.headers_mut().insert(header_name, val.clone());
        }
    }
    track_core_relay_response(out, &store.node_registry, &rec.id)
}

/// Online, fail-closed authorization for every new direct GET/HEAD/Range request.
pub(super) async fn authorize_playback(
    State(state): State<NodesState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, uri.path(), &body).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let claims: PlaybackTokenClaims = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_claims"),
    };
    let library =
        inventory::resource_library(&state.auth.store, &claims.target_kind, &claims.target_id)
            .await;
    let valid_target = matches!(library, Ok(Some(ref l)) if l == claims.library_id.as_str());
    let allowed = crate::database::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sessions s JOIN users u ON u.id=s.user_id JOIN libraries l ON l.id=? \
         WHERE s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-604800 \
         AND u.disabled_at IS NULL AND l.enabled=1 AND l.node_id=? \
         AND ((u.role='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=u.id))")
        .bind(claims.library_id.as_str()).bind(&claims.session_hash).bind(&rec.id)
        .fetch_one(&state.auth.store.pool).await.unwrap_or(0) == 1;
    // Resolve the live identity once per authorization request, including its
    // access schedule. Reuse that validated identity for the parental check.
    let session = state.auth.session_from_hash(&claims.session_hash).await;
    let valid_session = session.is_ok();
    let parental = match session {
        Ok(user) => crate::product::target_allowed(
            &state.auth.store,
            &user.id,
            &claims.target_kind,
            &claims.target_id,
        )
        .await
        .unwrap_or(false),
        Err(_) => false,
    };
    let valid = valid_session
        && parental
        && valid_target
        && allowed
        && claims.node_id.as_str() == rec.id
        && claims.kid == rec.key_id
        && claims.action == "stream"
        && claims.exp > unix_now();
    signed_core_response(
        &rec,
        &nonce,
        if valid {
            StatusCode::OK
        } else {
            StatusCode::FORBIDDEN
        },
        &serde_json::json!({"authorized":valid}),
    )
}
