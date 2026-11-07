//! Administrator pairing, enrollment and Node identity lifecycle.
use super::NodesState;
use super::channel::ACTIVE_CHANNELS;
use super::registry::{AdminNodeView, load_admin_nodes, load_node, refresh_node_in_registry};
use super::scans::sync_all_libraries_for_node;
use super::transport::{
    send_plain_json_post, send_signed_rotation_request, validate_http_url, validate_public_url,
};
use crate::identity::Role;
use axum::Json;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use bytes::Bytes;
use media_contracts::ids::NodeId;
use media_contracts::protocol::EnrollNodeRequest;
use media_contracts::protocol::EnrollNodeResponse;
use media_contracts::protocol::HDR_RES_NONCE;
use media_contracts::protocol::HDR_RES_SIGNATURE;
use media_contracts::protocol::HDR_RES_TIMESTAMP;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::MAX_CLOCK_SKEW_SECS;
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::PROTOCOL_VERSION;
use media_contracts::protocol::PairNodeRequest;
use media_contracts::protocol::PairNodeResponse;
use media_contracts::protocol::RotateSecretRequest;
use media_contracts::protocol::RotateSecretResponse;
use media_contracts::protocol::compute_enrollment_mutual_proof;
use media_contracts::protocol::compute_pairing_mutual_proof;
use media_contracts::protocol::compute_pairing_proof;
use media_contracts::protocol::constant_time_eq;
use media_contracts::protocol::derive_paired_secret;
use media_contracts::protocol::generate_enrollment_token;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::random_secret_hex;
use media_contracts::protocol::sha256_hex;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_response;
use serde::Deserialize;
use sqlx::Row;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairNodeAdminInput {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub public_url: Option<String>,
    pub pairing_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEnrollmentTokenInput {
    #[serde(default)]
    pub expires_in_secs: Option<i64>,
}

async fn require_admin(
    state: &NodesState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let identity = state.auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        state.auth.require_csrf(headers, &identity)?;
    }
    Ok(())
}

pub(super) async fn list_nodes(
    State(state): State<NodesState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_admin(&state, &headers, false).await?;
    let nodes = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "nodes": nodes })))
}

pub(super) async fn pair_node_admin(
    State(state): State<NodesState>,
    headers: HeaderMap,
    Json(input): Json<PairNodeAdminInput>,
) -> Result<(StatusCode, Json<AdminNodeView>), StatusCode> {
    require_admin(&state, &headers, true).await?;
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let base_url = validate_http_url(&input.base_url)?;
    let explicit_public_url = match input
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(u) => Some(validate_public_url(u)?),
        None => None,
    };
    let pairing_token = input.pairing_token.trim();
    if pairing_token.is_empty() || pairing_token.len() > 256 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let core_internal_url = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| state.auth.origin().to_owned());

    let core_id = "aether-core".to_owned();
    let key_id = format!("key-{}", uuid::Uuid::new_v4().simple());
    let core_secret_half = random_secret_hex();
    let timestamp = unix_now();
    let nonce = random_nonce_hex();
    let pairing_proof = compute_pairing_proof(
        pairing_token,
        &core_id,
        &key_id,
        &core_secret_half,
        &core_internal_url,
        timestamp,
        &nonce,
    );

    let pair_req = PairNodeRequest {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        core_id,
        key_id: key_id.clone(),
        core_secret_half: core_secret_half.clone(),
        core_origin: core_internal_url,
        timestamp,
        nonce: nonce.clone(),
        pairing_proof,
    };

    let pair_started = std::time::Instant::now();
    let body = serde_json::to_vec(&pair_req).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (status, res_headers, res_bytes) =
        send_plain_json_post(&base_url, "/internal/v1/node/pair", Bytes::from(body))
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let pair_rtt_ms = pair_started.elapsed().as_millis() as u32;
    if !status.is_success() {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let pair_res: PairNodeResponse =
        serde_json::from_slice(&res_bytes).map_err(|_| StatusCode::BAD_GATEWAY)?;
    if pair_res.protocol_version != PROTOCOL_VERSION
        || pair_res.node_id.as_str() == "local"
        || pair_res.node_id.as_str().is_empty()
        || pair_res.node_id.as_str().len() > 128
    {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let derived_secret =
        derive_paired_secret(pairing_token, &core_secret_half, &pair_res.node_secret_half);
    let expected_mutual = compute_pairing_mutual_proof(
        &derived_secret,
        &nonce,
        pair_res.node_id.as_str(),
        &key_id,
        &pair_res.node_secret_half,
        pair_res.timestamp,
        &pair_res.nonce,
    );
    if !constant_time_eq(expected_mutual.as_bytes(), pair_res.mutual_proof.as_bytes()) {
        return Err(StatusCode::BAD_GATEWAY);
    }

    verify_internal_response(
        &derived_secret,
        &nonce,
        status.as_u16(),
        pair_res.node_id.as_str(),
        &key_id,
        res_headers
            .get(HDR_RES_TIMESTAMP)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        res_headers
            .get(HDR_RES_NONCE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        res_headers
            .get(HDR_RES_SIGNATURE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
        &sha256_hex(&res_bytes),
        unix_now(),
    )
    .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let final_public_url = explicit_public_url.or({
        match pair_res.public_url.as_deref() {
            Some(u) => validate_public_url(u).ok(),
            None => None,
        }
    });
    let roots_json = serde_json::to_string(&pair_res.roots).unwrap_or_else(|_| "[]".to_owned());
    let now = unix_now();

    crate::database::query(
        "INSERT INTO media_nodes(id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode)
         VALUES(?, ?, 'remote', ?, ?, 'connected', ?, ?, ?, ?, ?, NULL, ?, 'inbound')
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name,
            kind='remote',
            base_url=excluded.base_url,
            public_url=excluded.public_url,
            status='connected',
            connection_mode='inbound',
            key_id=excluded.key_id,
            shared_secret=excluded.shared_secret,
            roots_json=excluded.roots_json,
            last_seen_at=excluded.last_seen_at,
            version=excluded.version,
            revoked_at=NULL",
    )
    .bind(pair_res.node_id.as_str())
    .bind(&name)
    .bind(&base_url)
    .bind(&final_public_url)
    .bind(&key_id)
    .bind(&derived_secret)
    .bind(&roots_json)
    .bind(now)
    .bind(now)
    .bind(pair_res.version.as_deref())
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let _ = refresh_node_in_registry(&state.auth.store, pair_res.node_id.as_str()).await;
    state.auth.store.node_registry.record_success(
        pair_res.node_id.as_str(),
        Some(pair_rtt_ms),
        now,
    );

    // Push existing library inventories for this Node if any exist
    let _ = sync_all_libraries_for_node(&state.auth.store, pair_res.node_id.as_str()).await;

    let views = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let view = views
        .into_iter()
        .find(|n| n.id == pair_res.node_id.as_str())
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(view)))
}

pub(super) async fn generate_enrollment_token_admin(
    State(state): State<NodesState>,
    headers: HeaderMap,
    input: Option<Json<CreateEnrollmentTokenInput>>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    require_admin(&state, &headers, true).await?;
    let (token_plaintext, token_hash) = generate_enrollment_token();
    let token_id = format!("tok-{}", uuid::Uuid::new_v4().simple());
    let now = unix_now();
    let ttl = input
        .and_then(|Json(i)| i.expires_in_secs)
        .unwrap_or(3600)
        .clamp(60, 86400 * 7);
    let expires_at = now + ttl;

    crate::database::query(
        "INSERT INTO node_enrollment_tokens (id, token_hash, created_at, expires_at, used_at, used_by_node_id)
         VALUES (?, ?, ?, ?, NULL, NULL)",
    )
    .bind(&token_id)
    .bind(&token_hash)
    .bind(now)
    .bind(expires_at)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let core_url = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .or_else(|_| std::env::var("MEDIA_CORE_URL"))
        .unwrap_or_else(|_| state.auth.origin().to_owned());

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "tokenId": token_id,
            "token": token_plaintext,
            "createdAt": now,
            "expiresAt": expires_at,
            "coreUrl": core_url
        })),
    ))
}

pub(super) async fn list_enrollment_tokens_admin(
    State(state): State<NodesState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_admin(&state, &headers, false).await?;
    let now = unix_now();
    let rows = crate::database::query(
        "SELECT id, created_at, expires_at, used_at, used_by_node_id FROM node_enrollment_tokens WHERE expires_at > ? AND used_at IS NULL ORDER BY created_at DESC",
    )
    .bind(now)
    .fetch_all(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tokens: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "createdAt": r.get::<i64, _>("created_at"),
                "expiresAt": r.get::<i64, _>("expires_at"),
                "used": r.get::<Option<i64>, _>("used_at").is_some(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tokens": tokens })))
}

pub(super) async fn delete_enrollment_token_admin(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    require_admin(&state, &headers, true).await?;
    crate::database::query("DELETE FROM node_enrollment_tokens WHERE id=?")
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn enroll_node(
    State(state): State<NodesState>,
    Json(req): Json<EnrollNodeRequest>,
) -> Result<(StatusCode, Json<EnrollNodeResponse>), (StatusCode, Json<serde_json::Value>)> {
    if req.protocol_version != PROTOCOL_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "unsupported_protocol_version" })),
        ));
    }
    let now = unix_now();
    if (now - req.timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "timestamp_out_of_range" })),
        ));
    }
    let token = req.enrollment_token.trim();
    if !token.starts_with("mnet_") || token.len() < 40 || token.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid_enrollment_token_format" })),
        ));
    }

    let token_hash = sha256_hex(token.as_bytes());
    let requested_id = req
        .node_id
        .as_ref()
        .map(|n| n.as_str().trim().to_owned())
        .filter(|s| !s.is_empty() && s != "local" && s.len() <= 128);
    let final_node_id =
        requested_id.unwrap_or_else(|| format!("node-{}", uuid::Uuid::new_v4().simple()));

    // Atomically claim the single-use token:
    let update_res = crate::database::query(
        "UPDATE node_enrollment_tokens SET used_at=?, used_by_node_id=? WHERE token_hash=? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(&final_node_id)
    .bind(&token_hash)
    .bind(now)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "database_error" }))))?;

    if update_res.rows_affected() == 0 {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid_or_expired_enrollment_token" })),
        ));
    }

    let core_secret_half = random_secret_hex();
    let key_id = format!("key-{}", uuid::Uuid::new_v4().simple());
    let derived_secret = derive_paired_secret(token, &core_secret_half, &req.node_secret_half);

    let explicit_public_url = match req
        .public_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(u) => validate_public_url(u).ok(),
        None => None,
    };
    let connection_mode = if explicit_public_url.is_some() {
        "direct"
    } else {
        "outbound"
    };

    let name = req.name.trim();
    let final_name = if name.is_empty() {
        final_node_id.clone()
    } else {
        name.to_owned()
    };
    let roots_json = serde_json::to_string(&req.roots).unwrap_or_else(|_| "[]".to_owned());

    crate::database::query(
        "INSERT INTO media_nodes (id, name, kind, base_url, public_url, status, version, connection_mode, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at)
         VALUES (?, ?, 'remote', NULL, ?, 'connected', ?, ?, ?, ?, ?, ?, ?, NULL)
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name,
            kind='remote',
            public_url=excluded.public_url,
            status='connected',
            version=excluded.version,
            connection_mode=excluded.connection_mode,
            key_id=excluded.key_id,
            shared_secret=excluded.shared_secret,
            roots_json=excluded.roots_json,
            last_seen_at=excluded.last_seen_at,
            revoked_at=NULL",
    )
    .bind(&final_node_id)
    .bind(&final_name)
    .bind(&explicit_public_url)
    .bind(req.version.as_deref())
    .bind(connection_mode)
    .bind(&key_id)
    .bind(&derived_secret)
    .bind(&roots_json)
    .bind(now)
    .bind(now)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "database_error" }))))?;

    let _ = refresh_node_in_registry(&state.auth.store, &final_node_id).await;
    state
        .auth
        .store
        .node_registry
        .record_success(&final_node_id, Some(1), now);

    let res_nonce = random_nonce_hex();
    let mutual_proof = compute_enrollment_mutual_proof(
        &derived_secret,
        &req.nonce,
        &final_node_id,
        &key_id,
        &core_secret_half,
        now,
        &res_nonce,
    );

    let core_origin = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .or_else(|_| std::env::var("MEDIA_CORE_URL"))
        .unwrap_or_else(|_| state.auth.origin().to_owned());

    Ok((
        StatusCode::CREATED,
        Json(EnrollNodeResponse {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            version: Some(media_contracts::version::current_version().to_string()),
            node_id: NodeId::new(&final_node_id),
            key_id,
            core_secret_half,
            core_origin,
            timestamp: now,
            nonce: res_nonce,
            mutual_proof,
        }),
    ))
}

pub(super) async fn reconnect_node(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<AdminNodeView>, StatusCode> {
    require_admin(&state, &headers, true).await?;
    if id == "local" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let rec = load_node(&state.auth.store, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if rec.status == NodeStatus::Revoked.as_str() {
        return Err(StatusCode::CONFLICT);
    }
    let is_channel_active = super::channel::channel_is_active(&id).await;
    if is_channel_active {
        let now = unix_now();
        crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=?",
        )
        .bind(now)
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_success(&id, Some(1), now);

        let _ = sync_all_libraries_for_node(&state.auth.store, &id).await;

        let views = load_admin_nodes(&state.auth.store)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return views
            .into_iter()
            .find(|n| n.id == id)
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND);
    }

    let Some(base_url) = rec.base_url.as_deref() else {
        crate::database::query(
            "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
        )
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        state.auth.store.node_registry.record_failure(&id, true);
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;

        let views = load_admin_nodes(&state.auth.store)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return views
            .into_iter()
            .find(|n| n.id == id)
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND);
    };

    let hb_started = std::time::Instant::now();
    let hb_res = media_node::server::send_signed_http_request(
        base_url,
        "core-to-node",
        Method::POST,
        "/internal/v1/node/heartbeat",
        &NodeId::new(&rec.id),
        &rec.key_id,
        &rec.shared_secret,
        None,
        Bytes::from_static(b"{}"),
    )
    .await;
    let hb_rtt_ms = hb_started.elapsed().as_millis() as u32;

    let now = unix_now();
    match hb_res {
        Ok((_, _, bytes)) => {
            if let Ok(hb) = serde_json::from_slice::<HeartbeatPayload>(&bytes) {
                let roots_json =
                    serde_json::to_string(&hb.roots).unwrap_or_else(|_| rec.roots_json.clone());
                let pub_url = hb
                    .public_url
                    .clone()
                    .or(rec.public_url)
                    .and_then(|u| validate_public_url(&u).ok());
                crate::database::query(
                    "UPDATE media_nodes SET status='connected', roots_json=?, public_url=?, version=COALESCE(?, version), last_seen_at=? WHERE id=?",
                )
                .bind(roots_json)
                .bind(pub_url)
                .bind(hb.version.as_deref())
                .bind(now)
                .bind(&id)
                .execute(&state.auth.store.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let _ = refresh_node_in_registry(&state.auth.store, &id).await;
                state
                    .auth
                    .store
                    .node_registry
                    .record_heartbeat(&id, &hb, Some(hb_rtt_ms), now);
            } else {
                crate::database::query(
                    "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=?",
                )
                .bind(now)
                .bind(&id)
                .execute(&state.auth.store.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let _ = refresh_node_in_registry(&state.auth.store, &id).await;
                state
                    .auth
                    .store
                    .node_registry
                    .record_success(&id, Some(hb_rtt_ms), now);
            }
            let _ = sync_all_libraries_for_node(&state.auth.store, &id).await;
        }
        Err(_) => {
            crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&id)
            .execute(&state.auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            state.auth.store.node_registry.record_failure(&id, true);
            let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        }
    }

    let views = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    views
        .into_iter()
        .find(|n| n.id == id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub(super) async fn rotate_node_secret(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<AdminNodeView>, StatusCode> {
    require_admin(&state, &headers, true).await?;
    if id == "local" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let rec = load_node(&state.auth.store, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if rec.status == NodeStatus::Revoked.as_str() {
        return Err(StatusCode::CONFLICT);
    }

    let new_key_id = format!("key-{}", uuid::Uuid::new_v4().simple());
    let new_secret = random_secret_hex();
    let req_payload = RotateSecretRequest {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        node_id: NodeId::new(&rec.id),
        new_key_id: new_key_id.clone(),
        new_secret: new_secret.clone(),
        timestamp: unix_now(),
        nonce: random_nonce_hex(),
    };

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
            channels.get(&id).cloned()
        };
        if let Some(sender) = channel_sender
            && !sender.is_closed()
        {
            let cmd = NodeCommandEnvelope {
                command_id: format!("rotate-{new_key_id}"),
                command_type: NodeCommandType::RotateSecret,
                expires_at: None,
                payload: serde_json::to_value(&req_payload).unwrap_or_default(),
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
        let now = unix_now();
        crate::database::query(
            "UPDATE media_nodes SET key_id=?, shared_secret=?, status='connected', last_seen_at=? WHERE id=?",
        )
        .bind(&new_key_id)
        .bind(&new_secret)
        .bind(now)
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_success(&id, Some(1), now);

        let views = load_admin_nodes(&state.auth.store)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return views
            .into_iter()
            .find(|n| n.id == id)
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND);
    }

    let Some(base_url) = rec.base_url.as_deref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    let body = serde_json::to_vec(&req_payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let res_bytes = send_signed_rotation_request(
        base_url,
        &NodeId::new(&rec.id),
        &rec.key_id,
        &rec.shared_secret,
        &new_key_id,
        &new_secret,
        Bytes::from(body),
    )
    .await
    .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let rot_res: RotateSecretResponse =
        serde_json::from_slice(&res_bytes).map_err(|_| StatusCode::BAD_GATEWAY)?;
    if rot_res.new_key_id != new_key_id {
        return Err(StatusCode::BAD_GATEWAY);
    }

    let now = unix_now();
    crate::database::query(
        "UPDATE media_nodes SET key_id=?, shared_secret=?, status='connected', last_seen_at=? WHERE id=?",
    )
    .bind(&new_key_id)
    .bind(&new_secret)
    .bind(now)
    .bind(&id)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = refresh_node_in_registry(&state.auth.store, &id).await;
    state
        .auth
        .store
        .node_registry
        .record_success(&id, None, now);

    let views = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    views
        .into_iter()
        .find(|n| n.id == id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub(super) async fn revoke_node_admin(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<AdminNodeView>, StatusCode> {
    require_admin(&state, &headers, true).await?;
    if id == "local" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let rec = load_node(&state.auth.store, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Best-effort notify Node of revocation via channel or HTTP
    {
        let channels = ACTIVE_CHANNELS.read().await;
        if let Some(sender) = channels.get(&id) {
            let _ = sender
                .send(NodeCommandEnvelope {
                    command_id: format!("revoke-{}", unix_now()),
                    command_type: NodeCommandType::Revoke,
                    expires_at: None,
                    payload: serde_json::Value::Null,
                })
                .await;
        }
    }
    ACTIVE_CHANNELS.write().await.remove(&id);

    if let Some(base_url) = rec.base_url.as_deref() {
        let _ = media_node::server::send_signed_http_request(
            base_url,
            "core-to-node",
            Method::POST,
            "/internal/v1/node/revoke",
            &NodeId::new(&rec.id),
            &rec.key_id,
            &rec.shared_secret,
            None,
            Bytes::from_static(b"{}"),
        )
        .await;
    }

    let now = unix_now();
    let revoked_key_id = format!("revoked-{}", uuid::Uuid::new_v4().simple());
    let wiped_secret = random_secret_hex();
    crate::database::query(
        "UPDATE media_nodes SET status='revoked', key_id=?, shared_secret=?, revoked_at=? WHERE id=?",
    )
    .bind(&revoked_key_id)
    .bind(&wiped_secret)
    .bind(now)
    .bind(&id)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let _ = refresh_node_in_registry(&state.auth.store, &id).await;

    // Mark any running scan jobs on this Node's libraries as interrupted
    crate::database::query(
        "UPDATE scan_jobs SET status='error', errors_json='[\"Node révoqué pendant le scan\"]', errors_count=1 WHERE library_id IN (SELECT id FROM libraries WHERE node_id=?) AND status IN ('queued','running')",
    )
    .bind(&id)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let views = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    views
        .into_iter()
        .find(|n| n.id == id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}
