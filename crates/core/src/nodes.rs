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
    response::Response,
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
        DEFAULT_PLAYBACK_TOKEN_TTL_SECS, HDR_BODY_SHA256, HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE,
        HDR_PROTOCOL, HDR_RES_NONCE, HDR_RES_SIGNATURE, HDR_RES_TIMESTAMP, HDR_SIGNATURE,
        HDR_TIMESTAMP, HeartbeatPayload, LibraryInventorySync, NodeInventoryAssetEntry,
        NodeInventoryImageEntry, NodeStatus, PROTOCOL_VERSION, PairNodeRequest, PairNodeResponse,
        PlaybackTokenClaims, RemoteBatchSubmission, RemoteScanCompleteResponse,
        RemoteScanCompletion, RemoteScanFailure, RemoteScanStartRequest, ReplayWindow,
        RotateSecretRequest, RotateSecretResponse, compute_pairing_mutual_proof,
        compute_pairing_proof, compute_rotation_mutual_proof, constant_time_eq,
        derive_paired_secret, issue_playback_token, random_nonce_hex, random_secret_hex,
        sha256_hex, sign_internal_request, sign_internal_response, unix_now,
        verify_internal_request, verify_internal_response,
    },
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{io, sync::Arc, time::Duration};
use tokio::net::TcpStream;

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

pub fn router(auth: AuthState, replay_window: Arc<ReplayWindow>) -> Router {
    let state = NodesState {
        auth,
        replay_window,
    };
    Router::new()
        .route("/api/v1/admin/nodes", get(list_nodes))
        .route("/api/v1/admin/nodes/pair", post(pair_node_admin))
        .route("/api/v1/admin/nodes/{id}/reconnect", post(reconnect_node))
        .route(
            "/api/v1/admin/nodes/{id}/rotate-secret",
            post(rotate_node_secret),
        )
        .route("/api/v1/admin/nodes/{id}/revoke", post(revoke_node_admin))
        .route("/internal/v1/core/heartbeat", post(receive_node_heartbeat))
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
    let trimmed = raw.trim().trim_end_matches('/');
    let parsed = url::Url::parse(trimmed).map_err(|_| StatusCode::BAD_REQUEST)?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(trimmed.to_owned())
}

pub async fn load_node(
    store: &IdentityStore,
    node_id: &str,
) -> Result<Option<MediaNodeRecord>, sqlx::Error> {
    sqlx::query_as::<_, MediaNodeRecord>(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at FROM media_nodes WHERE id=?",
    )
    .bind(node_id)
    .fetch_optional(&store.pool)
    .await
}

pub async fn load_admin_nodes(store: &IdentityStore) -> Result<Vec<AdminNodeView>, sqlx::Error> {
    let records: Vec<MediaNodeRecord> = sqlx::query_as(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at FROM media_nodes ORDER BY CASE WHEN id='local' THEN 0 ELSE 1 END, name, id",
    )
    .fetch_all(&store.pool)
    .await?;

    let libs = sqlx::query(
        "SELECT id, name, kind, node_id, root, enabled FROM libraries ORDER BY name, id",
    )
    .fetch_all(&store.pool)
    .await?;

    let scans = sqlx::query(
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
                enabled: r.get("enabled"),
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

        views.push(AdminNodeView {
            id: rec.id,
            name: rec.name,
            kind: rec.kind,
            base_url: rec.base_url,
            public_url: rec.public_url,
            status: rec.status,
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
        Some(u) => Some(validate_http_url(u)?),
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
    let (status, _res_headers, res_bytes) =
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

    let final_public_url = explicit_public_url.or({
        match pair_res.public_url.as_deref() {
            Some(u) => validate_http_url(u).ok(),
            None => None,
        }
    });
    let roots_json = serde_json::to_string(&pair_res.roots).unwrap_or_else(|_| "[]".to_owned());
    let now = unix_now();

    sqlx::query(
        "INSERT INTO media_nodes(id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at)
         VALUES(?, ?, 'remote', ?, ?, 'connected', ?, ?, ?, ?, ?, NULL)
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name,
            kind='remote',
            base_url=excluded.base_url,
            public_url=excluded.public_url,
            status='connected',
            key_id=excluded.key_id,
            shared_secret=excluded.shared_secret,
            roots_json=excluded.roots_json,
            last_seen_at=excluded.last_seen_at,
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
    let base_url = rec.base_url.as_deref().ok_or(StatusCode::BAD_REQUEST)?;

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
                let pub_url = hb.public_url.or(rec.public_url);
                sqlx::query(
                    "UPDATE media_nodes SET status='connected', roots_json=?, public_url=?, last_seen_at=? WHERE id=?",
                )
                .bind(roots_json)
                .bind(pub_url)
                .bind(now)
                .bind(&id)
                .execute(&state.auth.store.pool)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            } else {
                sqlx::query("UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=?")
                    .bind(now)
                    .bind(&id)
                    .execute(&state.auth.store.pool)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            let _ = sync_all_libraries_for_node(&state.auth.store, &id).await;
        }
        Err(_) => {
            sqlx::query(
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
    let base_url = rec.base_url.as_deref().ok_or(StatusCode::BAD_REQUEST)?;

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
    let body = serde_json::to_vec(&req_payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Note: Node signs its response with new_secret/new_key_id, so we use a custom call or accept the response proof
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
    sqlx::query(
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

    // Best-effort notify Node of revocation
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
    sqlx::query(
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
    sqlx::query(
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

    let _ = sqlx::query(
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
        let _ = sqlx::query(
            "UPDATE media_nodes SET status='connected', roots_json=?, last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(roots_json)
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
    let owns_lib: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM libraries WHERE id=? AND node_id=? AND enabled=1)",
    )
    .bind(sub.library_id.as_str())
    .bind(&rec.id)
    .fetch_one(&state.auth.store.pool)
    .await
    .unwrap_or(false);
    if !owns_lib {
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
    let lib_row: Option<(String,)> =
        sqlx::query_as("SELECT root FROM libraries WHERE id=? AND node_id=? AND enabled=1")
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
        let _ = sqlx::query("UPDATE libraries SET root=? WHERE id=? AND node_id=?")
            .bind(&comp.root_snapshot)
            .bind(lib_id)
            .bind(&rec.id)
            .execute(&state.auth.store.pool)
            .await;
    }

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
        let _ = sqlx::query(
            "UPDATE scan_jobs SET status='error', errors_json='[\"Échec de publication du scan distant\"]', errors_count=1 WHERE library_id=?",
        )
        .bind(lib_id)
        .execute(&state.auth.store.pool)
        .await;
        return error(StatusCode::INTERNAL_SERVER_ERROR, "publish_scan_failed");
    }

    let conflicts = inventory::editorial_warnings(&state.auth.store, lib_id)
        .await
        .unwrap_or_default();
    let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;

    let errors_json = serde_json::to_string(&conflicts).unwrap_or_else(|_| "[]".to_owned());
    let _ = sqlx::query(
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
    let _ = sqlx::query(
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
    let root: String = sqlx::query_scalar("SELECT root FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_optional(&store.pool)
        .await?
        .unwrap_or_default();

    let asset_rows = sqlx::query(
        "SELECT a.id, a.library_id, s.root_snapshot, a.relative_path, a.mime, a.size, a.modified_ns
         FROM inventory_assets a
         JOIN inventory_sources s ON s.id = a.source_id
         WHERE a.library_id=? AND a.available=1",
    )
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;

    let mut assets = Vec::with_capacity(asset_rows.len());
    for r in asset_rows {
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

    let image_rows = sqlx::query(
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
    let Some(base_url) = rec.base_url.as_deref() else {
        return Ok(());
    };
    let lib_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM libraries WHERE node_id=? AND enabled=1")
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
    Ok(())
}

pub async fn dispatch_remote_scan(
    store: &IdentityStore,
    core_origin: &str,
    scan_id: &str,
    library_id: &str,
    node_id: &str,
    kind: &str,
    root: &str,
) -> Result<(), String> {
    let rec = load_node(store, node_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Node introuvable".to_owned())?;
    if rec.status == NodeStatus::Revoked.as_str() {
        return Err("Ce Node est révoqué".to_owned());
    }
    let base_url = rec
        .base_url
        .as_deref()
        .ok_or_else(|| "URL interne du Node manquante".to_owned())?;

    let core_internal_url = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| core_origin.to_owned());

    let cached_images: Vec<media_contracts::media::LocalImage> =
        inventory::load_image_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_values()
            .collect();

    let cached_files: Vec<media_contracts::protocol::CachedFileMeta> =
        inventory::load_scan_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(path, cache)| media_contracts::protocol::CachedFileMeta {
                relative_path: path,
                size: cache.size,
                modified_ns: cache.modified_ns,
            })
            .collect();

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
            let _ = sqlx::query(
                "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
            )
            .bind(unix_now())
            .bind(node_id)
            .execute(&store.pool)
            .await;
            Ok(())
        }
        Err(err) => {
            let _ = sqlx::query(
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
) -> Option<(String, i64)> {
    if rec.kind != "remote" || rec.status != NodeStatus::Connected.as_str() {
        return None;
    }
    let public_url = rec.public_url.as_deref()?.trim_end_matches('/');
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
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr = format!("{host}:{port}");
    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let stream = match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await
    {
        Ok(Ok(s)) => s,
        _ => {
            let _ = sqlx::query(
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

    let upstream_res = match sender.send_request(upstream_req).await {
        Ok(r) => r,
        Err(_) => {
            let _ = sqlx::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(&rec.id)
            .execute(&store.pool)
            .await;
            return error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        }
    };

    let _ = sqlx::query(
        "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
    )
    .bind(now)
    .bind(&rec.id)
    .execute(&store.pool)
    .await;

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
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr = format!("{host}:{port}");
    let host_header = if let Some(p) = parsed.port() {
        format!("{host}:{p}")
    } else {
        host.clone()
    };

    let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connection timed out"))??;
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
    let res = sender.send_request(req).await.map_err(io::Error::other)?;
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res
        .into_body()
        .collect()
        .await
        .map_err(io::Error::other)?
        .to_bytes();
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
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addr = format!("{host}:{port}");
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

    let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connection timed out"))??;
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

    let res = sender.send_request(req).await.map_err(io::Error::other)?;
    let status = res.status();
    let headers = res.headers().clone();
    let collected = res
        .into_body()
        .collect()
        .await
        .map_err(io::Error::other)?
        .to_bytes();

    if !status.is_success() {
        return Err(io::Error::other("rotation rejected by node"));
    }

    if let Some(resp_sig) = headers.get(HDR_RES_SIGNATURE).and_then(|v| v.to_str().ok()) {
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
