//! Distributed Media Node management, mutual HMAC authentication, remote scan ingestion,
//! and bounded Core relay streaming for Milestone P2.

use crate::{
    auth::AuthState,
    error,
    identity::{IdentityStore, Role},
    inventory,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper_util::rt::TokioIo;
use media_contracts::{
    ids::{AssetId, ImageId, LibraryId, NodeId, ScanId},
    inventory::BatchAck,
    protocol::{
        DEFAULT_PLAYBACK_TOKEN_TTL_SECS, EnrollNodeRequest, EnrollNodeResponse, HDR_BODY_SHA256,
        HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE, HDR_PROTOCOL, HDR_RES_NONCE, HDR_RES_SIGNATURE,
        HDR_RES_TIMESTAMP, HDR_SIGNATURE, HDR_TIMESTAMP, HeartbeatPayload, LibraryInventorySync,
        MAX_CLOCK_SKEW_SECS, NodeCommandEnvelope, NodeCommandType, NodeInventoryAssetEntry,
        NodeInventoryImageEntry, NodeStatus, PROTOCOL_VERSION, PairNodeRequest, PairNodeResponse,
        PlaybackTokenClaims, RemoteBatchSubmission, RemoteScanCompleteResponse,
        RemoteScanCompletion, RemoteScanFailure, RemoteScanStartRequest, ReplayWindow,
        RotateSecretRequest, RotateSecretResponse, StreamRelayRequest,
        compute_enrollment_mutual_proof, compute_pairing_mutual_proof, compute_pairing_proof,
        compute_rotation_mutual_proof, constant_time_eq, derive_paired_secret,
        generate_enrollment_token, issue_playback_token, random_nonce_hex, random_secret_hex,
        sha256_hex, sign_internal_request, sign_internal_response, unix_now,
        verify_internal_request, verify_internal_response,
    },
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{
    collections::HashMap,
    io,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};

pub struct RelayStreamResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Body,
}

pub static ACTIVE_CHANNELS: LazyLock<RwLock<HashMap<String, mpsc::Sender<NodeCommandEnvelope>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub static PENDING_STREAMS: LazyLock<Mutex<HashMap<String, oneshot::Sender<RelayStreamResponse>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
pub struct NodesState {
    pub auth: AuthState,
    pub replay_window: Arc<ReplayWindow>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct MediaNodeRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub public_url: Option<String>,
    pub status: String,
    pub version: Option<String>,
    pub connection_mode: String,
    pub key_id: String,
    pub shared_secret: String,
    pub roots_json: String,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLibraryRef {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub root: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeScanSummary {
    pub library_id: String,
    pub status: String,
    pub discovered: i64,
    pub probed: i64,
    pub reused: i64,
    pub errors_count: i64,
    pub warnings_count: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminNodeView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: Option<String>,
    pub public_url: Option<String>,
    pub status: String,
    pub version: Option<String>,
    pub connection_mode: String,
    pub key_id: String,
    pub roots: Vec<String>,
    pub paired_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub libraries: Vec<NodeLibraryRef>,
    pub scans: Vec<NodeScanSummary>,
}

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

pub fn router(auth: AuthState, replay_window: Arc<ReplayWindow>) -> Router {
    let state = NodesState {
        auth,
        replay_window,
    };
    Router::new()
        .route("/api/v1/admin/nodes", get(list_nodes))
        .route("/internal/v1/core/music/tracks", post(receive_music_track))
        .route(
            "/api/v1/admin/nodes/enrollment-tokens",
            post(generate_enrollment_token_admin).get(list_enrollment_tokens_admin),
        )
        .route(
            "/api/v1/admin/nodes/enrollment-tokens/{id}",
            axum::routing::delete(delete_enrollment_token_admin),
        )
        .route("/api/v1/nodes/enroll", post(enroll_node))
        .route("/api/v1/admin/nodes/pair", post(pair_node_admin))
        .route("/api/v1/admin/nodes/{id}/reconnect", post(reconnect_node))
        .route(
            "/api/v1/admin/nodes/{id}/rotate-secret",
            post(rotate_node_secret),
        )
        .route("/api/v1/admin/nodes/{id}/revoke", post(revoke_node_admin))
        .route("/internal/v1/core/heartbeat", post(receive_node_heartbeat))
        .route("/internal/v1/core/nodes/{id}/channel", post(node_channel))
        .route(
            "/internal/v1/core/nodes/{id}/stream-uplink/{stream_id}",
            post(receive_stream_uplink),
        )
        .route(
            "/internal/v1/core/playback/authorize",
            post(authorize_playback),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/batches",
            post(receive_scan_batch),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/complete",
            post(receive_scan_complete),
        )
        .route(
            "/internal/v1/core/scans/{scan_id}/fail",
            post(receive_scan_fail),
        )
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
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

fn validate_http_url(raw: &str) -> Result<String, StatusCode> {
    let parsed = media_node::transport::validate_url(raw).map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

fn validate_public_url(raw: &str) -> Result<String, StatusCode> {
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

pub async fn load_node(
    store: &IdentityStore,
    node_id: &str,
) -> Result<Option<MediaNodeRecord>, sqlx::Error> {
    crate::database::query_as::<_, MediaNodeRecord>(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode FROM media_nodes WHERE id=?",
    )
    .bind(node_id)
    .fetch_optional(&store.pool)
    .await
}

pub async fn load_admin_nodes(store: &IdentityStore) -> Result<Vec<AdminNodeView>, sqlx::Error> {
    let records: Vec<MediaNodeRecord> = crate::database::query_as(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode FROM media_nodes ORDER BY CASE WHEN id='local' THEN 0 ELSE 1 END, name, id",
    )
    .fetch_all(&store.pool)
    .await?;

    let libs = crate::database::query(
        "SELECT id, name, kind, node_id, root, enabled FROM libraries ORDER BY name, id",
    )
    .fetch_all(&store.pool)
    .await?;

    let scans = crate::database::query(
        "SELECT j.library_id, j.status, j.discovered, j.probed, j.reused, j.errors_count, j.warnings_count, l.node_id FROM scan_jobs j JOIN libraries l ON l.id = j.library_id",
    )
    .fetch_all(&store.pool)
    .await?;

    let mut views = Vec::with_capacity(records.len());
    for rec in records {
        let roots: Vec<String> = serde_json::from_str(&rec.roots_json).unwrap_or_default();
        let node_libs = libs
            .iter()
            .filter(|r| r.get::<String, _>("node_id") == rec.id)
            .map(|r| NodeLibraryRef {
                id: r.get("id"),
                name: r.get("name"),
                kind: r.get("kind"),
                root: r.get("root"),
                enabled: r.get::<i64, _>("enabled") != 0,
            })
            .collect();
        let node_scans = scans
            .iter()
            .filter(|r| r.get::<String, _>("node_id") == rec.id)
            .map(|r| NodeScanSummary {
                library_id: r.get("library_id"),
                status: r.get("status"),
                discovered: r.get("discovered"),
                probed: r.get("probed"),
                reused: r.get("reused"),
                errors_count: r.get("errors_count"),
                warnings_count: r.get("warnings_count"),
            })
            .collect();

        let node_version = if rec.id == "local" {
            Some(media_contracts::version::current_version().to_string())
        } else {
            rec.version.clone()
        };

        let conn_mode = if rec.id == "local" {
            "local".to_owned()
        } else if rec.public_url.is_some() {
            "direct".to_owned()
        } else if rec.connection_mode.is_empty() {
            if rec.base_url.is_some() {
                "inbound".to_owned()
            } else {
                "outbound".to_owned()
            }
        } else {
            rec.connection_mode.clone()
        };

        views.push(AdminNodeView {
            id: rec.id,
            name: rec.name,
            kind: rec.kind,
            base_url: rec.base_url,
            public_url: rec.public_url,
            status: rec.status,
            version: node_version,
            connection_mode: conn_mode,
            key_id: rec.key_id,
            roots,
            paired_at: rec.paired_at,
            last_seen_at: rec.last_seen_at,
            revoked_at: rec.revoked_at,
            libraries: node_libs,
            scans: node_scans,
        });
    }
    Ok(views)
}

async fn list_nodes(
    State(state): State<NodesState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_admin(&state, &headers, false).await?;
    let nodes = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({ "nodes": nodes })))
}

async fn pair_node_admin(
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

    let body = serde_json::to_vec(&pair_req).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (status, res_headers, res_bytes) =
        send_plain_json_post(&base_url, "/internal/v1/node/pair", Bytes::from(body))
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
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

async fn generate_enrollment_token_admin(
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

async fn list_enrollment_tokens_admin(
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

async fn delete_enrollment_token_admin(
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

async fn enroll_node(
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

async fn node_channel(
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
    } else {
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(now)
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await;
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

async fn receive_stream_uplink(
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

async fn reconnect_node(
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
    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(&id);
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

        let views = load_admin_nodes(&state.auth.store)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return views
            .into_iter()
            .find(|n| n.id == id)
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND);
    };

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

    let now = unix_now();
    match hb_res {
        Ok((_, _, bytes)) => {
            if let Ok(hb) = serde_json::from_slice::<HeartbeatPayload>(&bytes) {
                let roots_json =
                    serde_json::to_string(&hb.roots).unwrap_or_else(|_| rec.roots_json.clone());
                let pub_url = hb
                    .public_url
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
            } else {
                crate::database::query(
                    "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=?",
                )
                .bind(now)
                .bind(&id)
                .execute(&state.auth.store.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

async fn rotate_node_secret(
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

    let views = load_admin_nodes(&state.auth.store)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    views
        .into_iter()
        .find(|n| n.id == id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn revoke_node_admin(
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

#[allow(clippy::result_large_err)]
async fn verify_signed_node_call(
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

    Ok((rec, nonce))
}

fn signed_core_response<T: Serialize>(
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

async fn receive_node_heartbeat(
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
        let roots_json =
            serde_json::to_string(&hb.roots).unwrap_or_else(|_| rec.roots_json.clone());
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', roots_json=?, version=COALESCE(?, version), last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(roots_json)
        .bind(hb.version.as_deref())
        .bind(unix_now())
        .bind(&rec.id)
        .execute(&state.auth.store.pool)
        .await;
    }
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "ok" }),
    )
}

async fn receive_music_track(
    State(state): State<NodesState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let submission: media_node::music::Submission = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_music_track"),
    };
    let valid:i64=crate::database::query_scalar("SELECT COUNT(*) FROM libraries l JOIN music_scan_runs run ON run.library_id=l.id WHERE l.id=? AND l.node_id=? AND l.kind='music' AND l.enabled=1 AND l.root=? AND run.scan_id=?").bind(&submission.library_id).bind(&rec.id).bind(&submission.root_snapshot).bind(&submission.scan_id).fetch_one(&state.auth.store.pool).await.unwrap_or(0);
    let path = std::path::Path::new(&submission.track.relative_path);
    if valid != 1
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || submission.track.size > i64::MAX as u64
        || !submission.track.mime.starts_with("audio/")
    {
        return error(StatusCode::FORBIDDEN, "music_track_denied");
    }
    if crate::music::publish_track(
        &state.auth.store,
        &submission.library_id,
        &submission.root_snapshot,
        &submission.scan_id,
        submission.track,
    )
    .await
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "music_track_failed");
    }
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({"accepted":true}),
    )
}
async fn receive_scan_batch(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let sub: RemoteBatchSubmission = match serde_json::from_slice(&body) {
        Ok(s) => s,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_batch_payload"),
    };
    if sub.node_id.as_str() != rec.id || sub.batch.scan_id.as_str() != scan_id {
        return error(StatusCode::FORBIDDEN, "scan_identity_mismatch");
    }

    // Verify library belongs to this Node
    let owns_lib: i64 = crate::database::query_scalar(
        "SELECT CAST(EXISTS(SELECT 1 FROM libraries WHERE id=? AND node_id=? AND enabled=1) AS INTEGER)",
    )
    .bind(sub.library_id.as_str())
    .bind(&rec.id)
    .fetch_one(&state.auth.store.pool)
    .await
    .unwrap_or(0);
    if owns_lib == 0 {
        return error(StatusCode::FORBIDDEN, "library_not_owned_by_node");
    }

    if inventory::stage_batch(
        &state.auth.store,
        &scan_id,
        sub.library_id.as_str(),
        &sub.batch,
    )
    .await
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "stage_batch_failed");
    }

    let ack = BatchAck {
        scan_id: ScanId::new(scan_id),
        sequence: sub.batch.sequence,
    };
    signed_core_response(&rec, &nonce, StatusCode::OK, &ack)
}

async fn receive_scan_complete(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let comp: RemoteScanCompletion = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_completion_payload"),
    };
    if comp.node_id.as_str() != rec.id || comp.finished.scan_id.as_str() != scan_id {
        return error(StatusCode::FORBIDDEN, "scan_identity_mismatch");
    }
    let lib_id = comp.library_id.as_str();

    // Update library root to canonical root_snapshot reported by the Node if needed
    let lib_row: Option<(String,)> = crate::database::query_as(
        "SELECT root FROM libraries WHERE id=? AND node_id=? AND enabled=1",
    )
    .bind(lib_id)
    .bind(&rec.id)
    .fetch_optional(&state.auth.store.pool)
    .await
    .ok()
    .flatten();
    if lib_row.is_none() {
        let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
        return error(StatusCode::FORBIDDEN, "library_not_owned_by_node");
    }

    if !comp.root_snapshot.is_empty() {
        let _ = crate::database::query("UPDATE libraries SET root=? WHERE id=? AND node_id=?")
            .bind(&comp.root_snapshot)
            .bind(lib_id)
            .bind(&rec.id)
            .execute(&state.auth.store.pool)
            .await;
    }

    let library_kind: Option<String> =
        crate::database::query_scalar("SELECT kind FROM libraries WHERE id=?")
            .bind(lib_id)
            .fetch_optional(&state.auth.store.pool)
            .await
            .ok()
            .flatten();
    if library_kind.as_deref() == Some("music") {
        if comp.finished.complete {
            let _=crate::database::query("UPDATE music_tracks SET available=0 WHERE library_id=? AND scan_id!=? AND EXISTS(SELECT 1 FROM music_scan_runs WHERE library_id=? AND scan_id=?)").bind(lib_id).bind(&scan_id).bind(lib_id).bind(&scan_id).execute(&state.auth.store.pool).await;
        }
    } else {
        let root_path = std::path::Path::new(&comp.root_snapshot);
        if let Err(err) = inventory::publish_staged_scan(
            &state.auth.store,
            &scan_id,
            lib_id,
            root_path,
            comp.finished.complete,
        )
        .await
        {
            eprintln!("[REMOTE SCAN PUBLISH ERROR in {lib_id}]: {err:?}");
            let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
            let _ = crate::database::query(
            "UPDATE scan_jobs SET status='error', errors_json='[\"Échec de publication du scan distant\"]', errors_count=1 WHERE library_id=?",
        )
        .bind(lib_id)
        .execute(&state.auth.store.pool)
        .await;
            return error(StatusCode::INTERNAL_SERVER_ERROR, "publish_scan_failed");
        }
    }
    let conflicts = inventory::editorial_warnings(&state.auth.store, lib_id)
        .await
        .unwrap_or_default();
    let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;

    let errors_json = serde_json::to_string(&conflicts).unwrap_or_else(|_| "[]".to_owned());
    let _ = crate::database::query(
        "UPDATE scan_jobs SET status=?, discovered=?, probed=?, reused=?, errors_json=?, errors_count=?, warnings_count=0 WHERE library_id=?",
    )
    .bind(if comp.finished.complete { "completed" } else { "error" })
    .bind(comp.finished.discovered as i64)
    .bind(comp.finished.probed as i64)
    .bind(comp.finished.reused as i64)
    .bind(errors_json)
    .bind(conflicts.len() as i64)
    .bind(lib_id)
    .execute(&state.auth.store.pool)
    .await;

    let inventory_sync = build_library_inventory_sync(&state.auth.store, lib_id)
        .await
        .unwrap_or_default();
    let resp = RemoteScanCompleteResponse {
        ack: BatchAck {
            scan_id: ScanId::new(scan_id),
            sequence: comp.finished.last_sequence,
        },
        inventory_sync,
    };
    signed_core_response(&rec, &nonce, StatusCode::OK, &resp)
}

async fn receive_scan_fail(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let fail: RemoteScanFailure = match serde_json::from_slice(&body) {
        Ok(f) => f,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_failure_payload"),
    };
    let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
    let err_json = serde_json::to_string(&vec![format!("Scan Node échoué: {}", fail.message)])
        .unwrap_or_else(|_| "[\"Scan Node échoué\"]".to_owned());
    let _ = crate::database::query(
        "UPDATE scan_jobs SET status='error', errors_json=?, errors_count=1 WHERE library_id=?",
    )
    .bind(err_json)
    .bind(fail.library_id.as_str())
    .execute(&state.auth.store.pool)
    .await;

    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "recorded" }),
    )
}

pub async fn build_library_inventory_sync(
    store: &IdentityStore,
    library_id: &str,
) -> Result<LibraryInventorySync, sqlx::Error> {
    let root: String = crate::database::query_scalar("SELECT root FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_optional(&store.pool)
        .await?
        .unwrap_or_default();

    let asset_rows = crate::database::query(
        "SELECT a.id, a.library_id, s.root_snapshot, a.relative_path, a.mime, a.size, a.modified_ns, au.audios_json
         FROM inventory_assets a
         LEFT JOIN inventory_audio au ON au.asset_id=a.id
         JOIN inventory_sources s ON s.id = a.source_id
         WHERE a.library_id=? AND a.available=1 UNION ALL SELECT id,library_id,root_snapshot,relative_path,mime,size,modified_ns,NULL AS audios_json FROM music_tracks WHERE library_id=? AND available=1",
    )
    .bind(library_id)
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;

    let mut assets = Vec::with_capacity(asset_rows.len());
    for r in asset_rows {
        let audios: Vec<media_contracts::inventory::ExternalAudio> = serde_json::from_str(
            r.get::<Option<String>, _>("audios_json")
                .as_deref()
                .unwrap_or("[]"),
        )
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        for audio in audios {
            assets.push(NodeInventoryAssetEntry {
                asset_id: AssetId::new(crate::inventory::audio_operational_id(
                    &r.get::<String, _>("id"),
                    &audio,
                )),
                library_id: LibraryId::new(r.get::<String, _>("library_id")),
                root_snapshot: r.get("root_snapshot"),
                relative_path: audio.relative_path,
                mime: "audio/ogg".into(),
                size: audio.size,
                modified_ns: audio.modified_ns,
            });
        }
        assets.push(NodeInventoryAssetEntry {
            asset_id: AssetId::new(r.get::<String, _>("id")),
            library_id: LibraryId::new(r.get::<String, _>("library_id")),
            root_snapshot: r.get("root_snapshot"),
            relative_path: r.get("relative_path"),
            mime: r.get("mime"),
            size: r.get::<i64, _>("size") as u64,
            modified_ns: r.get("modified_ns"),
        });
    }

    let image_rows = crate::database::query(
        "SELECT id, library_id, root_snapshot, relative_path, mime, size, modified_ns
         FROM inventory_images
         WHERE library_id=? AND available=1",
    )
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;

    let mut images = Vec::with_capacity(image_rows.len());
    for r in image_rows {
        images.push(NodeInventoryImageEntry {
            image_id: ImageId::new(r.get::<String, _>("id")),
            library_id: LibraryId::new(r.get::<String, _>("library_id")),
            root_snapshot: r.get("root_snapshot"),
            relative_path: r.get("relative_path"),
            mime: r.get("mime"),
            size: r.get::<i64, _>("size") as u64,
            modified_ns: r.get("modified_ns"),
        });
    }

    Ok(LibraryInventorySync {
        library_id: LibraryId::new(library_id),
        root_snapshot: root,
        assets,
        images,
    })
}

pub async fn sync_all_libraries_for_node(
    store: &IdentityStore,
    node_id: &str,
) -> Result<(), io::Error> {
    let Some(rec) = load_node(store, node_id).await.map_err(io::Error::other)? else {
        return Ok(());
    };
    if rec.status != NodeStatus::Connected.as_str() {
        return Ok(());
    }
    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(node_id);
    if !is_channel_active && rec.base_url.is_none() {
        return Ok(());
    }
    let lib_ids: Vec<String> =
        crate::database::query_scalar("SELECT id FROM libraries WHERE node_id=? AND enabled=1")
            .bind(node_id)
            .fetch_all(&store.pool)
            .await
            .map_err(io::Error::other)?;
    for lib_id in lib_ids {
        let sync = build_library_inventory_sync(store, &lib_id)
            .await
            .map_err(io::Error::other)?;
        if sync.assets.is_empty() && sync.images.is_empty() {
            continue;
        }

        if is_channel_active {
            let cmd = NodeCommandEnvelope {
                command_id: format!("sync-{}", sync.library_id),
                command_type: NodeCommandType::SyncInventory,
                payload: serde_json::to_value(&sync).unwrap_or_default(),
            };
            let channels = ACTIVE_CHANNELS.read().await;
            if let Some(sender) = channels.get(node_id) {
                let _ = sender.send(cmd).await;
                continue;
            }
        }

        if let Some(base_url) = rec.base_url.as_deref() {
            let body = serde_json::to_vec(&sync).map_err(io::Error::other)?;
            let _ = media_node::server::send_signed_http_request(
                base_url,
                "core-to-node",
                Method::POST,
                "/internal/v1/node/inventory-sync",
                &NodeId::new(&rec.id),
                &rec.key_id,
                &rec.shared_secret,
                None,
                Bytes::from(body),
            )
            .await;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_remote_scan(
    store: &IdentityStore,
    core_origin: &str,
    scan_id: &str,
    library_id: &str,
    node_id: &str,
    kind: &str,
    root: &str,
    force_refresh: bool,
    excluded_patterns: Vec<String>,
) -> Result<(), String> {
    let rec = load_node(store, node_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Node introuvable".to_owned())?;
    if rec.status == NodeStatus::Revoked.as_str() {
        return Err("Ce Node est révoqué".to_owned());
    }

    let core_internal_url = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| core_origin.to_owned());

    let cached_images: Vec<media_contracts::media::LocalImage> = if force_refresh {
        Vec::new()
    } else {
        inventory::load_image_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_values()
            .collect()
    };

    let cached_files: Vec<media_contracts::protocol::CachedFileMeta> = if force_refresh {
        Vec::new()
    } else {
        inventory::load_scan_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(path, cache)| media_contracts::protocol::CachedFileMeta {
                relative_path: path,
                size: cache.size,
                modified_ns: cache.modified_ns,
            })
            .collect()
    };

    if kind == "music" {
        crate::database::query("INSERT INTO music_scan_runs(library_id,scan_id) VALUES(?,?) ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id").bind(library_id).bind(scan_id).execute(&store.pool).await.map_err(|e|e.to_string())?;
    }
    let start_req = RemoteScanStartRequest {
        scan_id: ScanId::new(scan_id),
        node_id: NodeId::new(node_id),
        library_id: LibraryId::new(library_id),
        kind: kind.to_owned(),
        root: root.to_owned(),
        config_revision: format!("{library_id}:{root}"),
        core_base_url: core_internal_url,
        batch_size: 32,
        cached_files,
        cached_images,
        excluded_patterns,
    };

    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(node_id);
    if is_channel_active {
        let cmd = NodeCommandEnvelope {
            command_id: format!("scan-{scan_id}"),
            command_type: NodeCommandType::Scan,
            payload: serde_json::to_value(&start_req).unwrap_or_default(),
        };
        let sent = {
            let channels = ACTIVE_CHANNELS.read().await;
            if let Some(sender) = channels.get(node_id) {
                sender.send(cmd).await.is_ok()
            } else {
                false
            }
        };
        if sent {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
            )
            .bind(unix_now())
            .bind(node_id)
            .execute(&store.pool)
            .await;
            return Ok(());
        }
    }

    let Some(base_url) = rec.base_url.as_deref() else {
        return Err("Media Node hors ligne (en attente de connexion sortante)".to_owned());
    };

    let body = serde_json::to_vec(&start_req).map_err(|e| e.to_string())?;
    match media_node::server::send_signed_http_request(
        base_url,
        "core-to-node",
        Method::POST,
        "/internal/v1/node/scans",
        &NodeId::new(&rec.id),
        &rec.key_id,
        &rec.shared_secret,
        None,
        Bytes::from(body),
    )
    .await
    {
        Ok(_) => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
            )
            .bind(unix_now())
            .bind(node_id)
            .execute(&store.pool)
            .await;
            Ok(())
        }
        Err(err) => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(node_id)
            .execute(&store.pool)
            .await;
            Err(format!("Node injoignable: {err}"))
        }
    }
}

pub fn build_direct_playback_url(
    rec: &MediaNodeRecord,
    library_id: &str,
    target_kind: &str,
    target_id: &str,
    action: &str,
    session_hash: &str,
) -> Option<(String, i64)> {
    if rec.kind != "remote" || rec.status != NodeStatus::Connected.as_str() {
        return None;
    }
    let checked_public_url = validate_public_url(rec.public_url.as_deref()?).ok()?;
    let public_url = checked_public_url.trim_end_matches('/');
    if public_url.is_empty() || rec.shared_secret.is_empty() {
        return None;
    }
    let now = unix_now();
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

    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(&rec.id);
    if is_channel_active || rec.base_url.is_none() {
        let is_image = path_and_query.contains("/image");
        let prepared = path_and_query
            .split("/prepared/")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .map(|s| format!("prepared:{s}"));
        let target_kind = prepared
            .as_deref()
            .unwrap_or(if is_image { "image" } else { "asset" });
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
        PENDING_STREAMS.lock().await.insert(stream_id.clone(), tx);

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
                    payload: serde_json::to_value(StreamRelayRequest {
                        head: request.method() == Method::HEAD,
                        stream_id: stream_id.clone(),
                        target_kind: target_kind.to_owned(),
                        target_id: target_id.to_owned(),
                        range: range.clone(),
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
            match tokio::time::timeout(Duration::from_secs(15), rx).await {
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
                    ] {
                        if let Some(val) = stream_res.headers.get(&header_name) {
                            out.headers_mut().insert(header_name, val.clone());
                        }
                    }
                    if head && out.status().is_success() {
                        if let Some(range) = out
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
                    }
                    return out;
                }
                _ => {
                    PENDING_STREAMS.lock().await.remove(&stream_id);
                    if rec.base_url.is_none() {
                        return error(StatusCode::GATEWAY_TIMEOUT, "stream_timeout");
                    }
                }
            }
        } else {
            PENDING_STREAMS.lock().await.remove(&stream_id);
            if rec.base_url.is_none() {
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

    let stream = match media_node::transport::connect(base_url).await {
        Ok(s) => s,
        _ => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&rec.id)
            .execute(&store.pool)
            .await;
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        }
    };

    let io = TokioIo::new(stream);
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(ok) => ok,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable"),
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

    let upstream_req = match builder.body(Full::new(Bytes::new())) {
        Ok(r) => r,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "stream_error"),
    };

    let upstream_res = match media_node::transport::send_request(&mut sender, upstream_req).await {
        Ok(r) => r,
        Err(_) => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&rec.id)
            .execute(&store.pool)
            .await;
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        }
    };

    let _ = crate::database::query(
        "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
    )
    .bind(now)
    .bind(&rec.id)
    .execute(&store.pool)
    .await;

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
    ] {
        if let Some(val) = parts.headers.get(&header_name) {
            out.headers_mut().insert(header_name, val.clone());
        }
    }
    out
}

async fn send_plain_json_post(
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

async fn send_signed_rotation_request(
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

/// Online, fail-closed authorization for every new direct GET/HEAD/Range request.
async fn authorize_playback(
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
         WHERE s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-1800 \
         AND u.disabled_at IS NULL AND l.enabled=1 AND l.node_id=? \
         AND (u.role='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=u.id))")
        .bind(claims.library_id.as_str()).bind(&claims.session_hash).bind(&rec.id)
        .fetch_one(&state.auth.store.pool).await.unwrap_or(0) == 1;
    let valid_session = state
        .auth
        .session_from_hash(&claims.session_hash)
        .await
        .is_ok();
    let parental = match state.auth.session_from_hash(&claims.session_hash).await {
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

#[cfg(test)]
mod transport_policy_tests {
    use super::*;
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
}
