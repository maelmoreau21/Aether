//! Standalone HTTP server for a distributed Media Node (`aether-node`).
//!
//! Responsibilities:
//! - Persistent Node identity (`node_identity.json`) and operational asset/image lookup table
//!   (`node_inventory.json`) in `MEDIA_NODE_DATA_DIR`, strictly outside read-only media roots.
//! - Mutual HMAC-SHA256 authenticated internal endpoints (`/internal/v1/...`) with replay window.
//! - Asynchronous bounded batch scanning (`LocalNode::scan_library_streaming`) pushing signed
//!   `RemoteBatchSubmission`, `RemoteScanCompletion`, and `RemoteScanFailure` to Core.
//! - Direct browser → Node streaming (`/stream/v1/assets/{id}/content` and `/stream/v1/assets/{id}/image`)
//!   verified via short-lived Core-signed `PlaybackToken`, plus Core-relayed streaming
//!   (`/internal/v1/node/assets/{id}/content` and `/internal/v1/node/assets/{id}/image`).

use crate::{BoxStreamBody, LocalNode};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use media_contracts::{
    errors::NodeErrorCode,
    ids::{AssetId, ImageId, NodeId},
    inventory::{CachedFile, InventoryBatch, ReadGrant, ReadTarget, ScanFailed, ScanFinished},
    media::LocalImage,
    protocol::{
        HDR_BODY_SHA256, HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE, HDR_PROTOCOL, HDR_RES_NONCE,
        HDR_RES_SIGNATURE, HDR_RES_TIMESTAMP, HDR_SIGNATURE, HDR_TIMESTAMP, HeartbeatPayload,
        LibraryInventorySync, NodeInventoryAssetEntry, NodeInventoryImageEntry, PROTOCOL_VERSION,
        PairNodeRequest, PairNodeResponse, RemoteBatchSubmission, RemoteScanCompleteResponse,
        RemoteScanCompletion, RemoteScanFailure, RemoteScanStartRequest, ReplayWindow,
        RotateSecretRequest, RotateSecretResponse, compute_pairing_mutual_proof,
        compute_pairing_proof, compute_rotation_mutual_proof, constant_time_eq,
        derive_paired_secret, random_nonce_hex, random_secret_hex, sha256_hex,
        sign_internal_request, sign_internal_response, unix_now, verify_internal_request,
        verify_internal_response, verify_playback_token,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone)]
pub struct NodeServerConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub bootstrap_token: Option<String>,
    pub configured_node_id: Option<NodeId>,
    pub configured_name: Option<String>,
    pub public_url: Option<String>,
    pub batch_size: usize,
}

impl Default for NodeServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 3101)),
            data_dir: PathBuf::from("./data/node"),
            allowed_roots: Vec::new(),
            bootstrap_token: None,
            configured_node_id: None,
            configured_name: None,
            public_url: None,
            batch_size: 32,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeIdentityState {
    pub node_id: NodeId,
    pub name: String,
    pub public_url: Option<String>,
    pub core_internal_url: Option<String>,
    pub shared_secret: Option<String>,
    pub key_id: String,
    pub revoked: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NodeOperationalInventory {
    pub libraries: HashMap<String, LibraryInventorySync>,
    pub assets: HashMap<String, NodeInventoryAssetEntry>,
    pub images: HashMap<String, NodeInventoryImageEntry>,
}

impl NodeOperationalInventory {
    pub fn apply_library_sync(&mut self, sync: LibraryInventorySync) {
        let lib_key = sync.library_id.as_str().to_owned();
        self.assets
            .retain(|_, entry| entry.library_id != sync.library_id);
        self.images
            .retain(|_, entry| entry.library_id != sync.library_id);
        for asset in &sync.assets {
            self.assets
                .insert(asset.asset_id.as_str().to_owned(), asset.clone());
        }
        for image in &sync.images {
            self.images
                .insert(image.image_id.as_str().to_owned(), image.clone());
        }
        self.libraries.insert(lib_key, sync);
    }

    pub fn grant_for_asset(&self, node_id: &NodeId, asset_id: &str) -> Option<ReadGrant> {
        let entry = self.assets.get(asset_id)?;
        Some(ReadGrant {
            target: ReadTarget::Asset(AssetId::new(asset_id)),
            library_id: entry.library_id.clone(),
            node_id: node_id.clone(),
            root_snapshot: entry.root_snapshot.clone(),
            relative_path: entry.relative_path.clone(),
            mime: entry.mime.clone(),
        })
    }

    pub fn grant_for_image(&self, node_id: &NodeId, image_id: &str) -> Option<ReadGrant> {
        let entry = self.images.get(image_id)?;
        Some(ReadGrant {
            target: ReadTarget::Image {
                id: ImageId::new(image_id),
                expected_size: entry.size,
                expected_modified_ns: entry.modified_ns.clone(),
            },
            library_id: entry.library_id.clone(),
            node_id: node_id.clone(),
            root_snapshot: entry.root_snapshot.clone(),
            relative_path: entry.relative_path.clone(),
            mime: entry.mime.clone(),
        })
    }
}

#[derive(Clone)]
pub struct NodeServerState {
    pub config: NodeServerConfig,
    pub identity: Arc<RwLock<NodeIdentityState>>,
    identity_changes: Arc<tokio::sync::Mutex<()>>,
    pub inventory: Arc<RwLock<NodeOperationalInventory>>,
    pub replay_window: Arc<ReplayWindow>,
    pub active_scans: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    pub local_node: LocalNode,
}

impl NodeServerState {
    pub async fn initialize(config: NodeServerConfig) -> io::Result<Self> {
        tokio::fs::create_dir_all(&config.data_dir).await?;
        let canonical_data = tokio::fs::canonicalize(&config.data_dir).await?;
        let mut canonical_roots = Vec::with_capacity(config.allowed_roots.len());
        for root in &config.allowed_roots {
            let c_root = tokio::fs::canonicalize(root).await?;
            if canonical_data.starts_with(&c_root) || c_root.starts_with(&canonical_data) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "MEDIA_NODE_DATA_DIR must be outside read-only media roots",
                ));
            }
            canonical_roots.push(c_root);
        }

        let identity_path = canonical_data.join("node_identity.json");
        let identity = if let Ok(bytes) = tokio::fs::read(&identity_path).await {
            let mut loaded: NodeIdentityState = serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if let Some(name) = &config.configured_name {
                loaded.name = name.clone();
            }
            if config.public_url.is_some() {
                loaded.public_url = config.public_url.clone();
            }
            loaded
        } else {
            let node_id = config
                .configured_node_id
                .clone()
                .unwrap_or_else(|| NodeId::new(format!("node-{}", uuid::Uuid::new_v4())));
            let name = config
                .configured_name
                .clone()
                .unwrap_or_else(|| node_id.as_str().to_owned());
            let initial = NodeIdentityState {
                node_id,
                name,
                public_url: config.public_url.clone(),
                core_internal_url: None,
                shared_secret: None,
                key_id: "unpaired".to_owned(),
                revoked: false,
            };
            let serialized = serde_json::to_vec_pretty(&initial)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            tokio::fs::write(&identity_path, serialized).await?;
            initial
        };

        let inventory_path = canonical_data.join("node_inventory.json");
        let inventory = if let Ok(bytes) = tokio::fs::read(&inventory_path).await {
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            NodeOperationalInventory::default()
        };

        let mut updated_config = config;
        updated_config.data_dir = canonical_data;
        updated_config.allowed_roots = canonical_roots;

        Ok(Self {
            config: updated_config,
            identity: Arc::new(RwLock::new(identity)),
            identity_changes: Arc::new(tokio::sync::Mutex::new(())),
            inventory: Arc::new(RwLock::new(inventory)),
            replay_window: Arc::new(ReplayWindow::new()),
            active_scans: Arc::new(Mutex::new(HashMap::new())),
            local_node: LocalNode,
        })
    }

    pub async fn persist_identity(&self) -> io::Result<()> {
        let snapshot = self
            .identity
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let bytes = serde_json::to_vec_pretty(&snapshot)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = self.config.data_dir.join("node_identity.json");
        let temporary = self.config.data_dir.join("node_identity.pending");
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(temporary, path).await?;
        #[cfg(unix)]
        {
            let directory = self.config.data_dir.clone();
            tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all())
                .await
                .map_err(io::Error::other)??;
        }
        Ok(())
    }

    pub async fn persist_inventory(&self) -> io::Result<()> {
        let snapshot = self
            .inventory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let bytes = serde_json::to_vec(&snapshot)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let path = self.config.data_dir.join("node_inventory.json");
        tokio::fs::write(path, bytes).await
    }

    pub fn node_id(&self) -> NodeId {
        self.identity
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .node_id
            .clone()
    }
}

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
        .route(
            "/internal/v1/node/assets/{id}/content",
            get(internal_stream_asset).head(internal_stream_asset),
        )
        .route(
            "/internal/v1/node/assets/{id}/image",
            get(internal_stream_image).head(internal_stream_image),
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

#[allow(clippy::result_large_err)]
fn verify_signed_core_call(
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

fn signed_json_response<T: Serialize>(
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
    let payload = HeartbeatPayload {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        node_id: identity.node_id.clone(),
        roots,
        public_url: identity.public_url.clone(),
        active_scans,
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

async fn start_remote_scan(
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

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut scans = state.active_scans.lock().unwrap_or_else(|e| e.into_inner());
        scans.insert(req.scan_id.as_str().to_owned(), cancel.clone());
    }

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

async fn run_remote_scan_task(
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

    let result = state
        .local_node
        .scan_library_streaming(
            &canonical_root,
            &req.kind,
            &scan_cache,
            &image_cache,
            &cancel,
            batch_size,
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
                    let path = format!("/internal/v1/core/scans/{}/batches", scan_id.as_str());
                    post_signed_to_core(&state_ref, &path, &submission).await
                }
            },
        )
        .await;

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

async fn internal_stream_asset(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_asset(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownAsset, false),
            &identity,
            &nonce,
        );
    };
    if id.starts_with("sidecar-") {
        let expected = {
            let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
            inv.assets
                .get(&id)
                .map(|entry| (entry.size, entry.modified_ns.clone()))
        };
        let opened = state
            .local_node
            .open_media(
                Path::new(&grant.root_snapshot),
                Path::new(&grant.relative_path),
            )
            .await;
        let response = match (opened, expected) {
            (Ok(opened), Some((size, modified)))
                if opened.size() == size && opened.modified_ns() == modified =>
            {
                match crate::serve_opened(opened, &req, "audio/ogg").await {
                    Ok(response) => convert_stream_response(response, false),
                    Err(_) => node_error_response(NodeErrorCode::StreamError, false),
                }
            }
            _ => node_error_response(NodeErrorCode::UnavailableAsset, false),
        };
        return sign_stream_response(response, &identity, &nonce);
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => sign_stream_response(convert_stream_response(res, false), &identity, &nonce),
        Err(code) => sign_stream_response(node_error_response(code, false), &identity, &nonce),
    }
}

async fn internal_stream_image(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_image(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownImage, false),
            &identity,
            &nonce,
        );
    };
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => sign_stream_response(convert_stream_response(res, false), &identity, &nonce),
        Err(code) => sign_stream_response(node_error_response(code, false), &identity, &nonce),
    }
}

#[derive(Deserialize)]
struct StreamQuery {
    token: Option<String>,
}

async fn stream_options() -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::NO_CONTENT;
    apply_cors_headers(res.headers_mut());
    res
}

async fn direct_stream_asset(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    req: Request,
) -> Response {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return cors_error_response(StatusCode::FORBIDDEN, "node_revoked");
    }
    let Some(secret) = identity.shared_secret.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "node_unpaired");
    };
    let Some(token) = query.token.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "missing_playback_token");
    };
    let claims = match verify_playback_token(
        secret,
        &identity.key_id,
        identity.node_id.as_str(),
        "asset",
        &id,
        "stream",
        token,
        unix_now(),
    ) {
        Ok(claims) => claims,
        Err(reason) => return cors_error_response(StatusCode::FORBIDDEN, reason),
    };

    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_asset(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return node_error_response(NodeErrorCode::UnknownAsset, true);
    };
    if claims.library_id != grant.library_id
        || post_signed_to_core(&state, "/internal/v1/core/playback/authorize", &claims)
            .await
            .is_err()
    {
        return cors_error_response(StatusCode::FORBIDDEN, "playback_authorization_denied");
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => convert_stream_response(res, true),
        Err(code) => node_error_response(code, true),
    }
}

async fn direct_stream_image(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    req: Request,
) -> Response {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return cors_error_response(StatusCode::FORBIDDEN, "node_revoked");
    }
    let Some(secret) = identity.shared_secret.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "node_unpaired");
    };
    let Some(token) = query.token.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "missing_playback_token");
    };
    let claims = match verify_playback_token(
        secret,
        &identity.key_id,
        identity.node_id.as_str(),
        "image",
        &id,
        "stream",
        token,
        unix_now(),
    ) {
        Ok(claims) => claims,
        Err(reason) => return cors_error_response(StatusCode::FORBIDDEN, reason),
    };

    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_image(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return node_error_response(NodeErrorCode::UnknownImage, true);
    };
    if claims.library_id != grant.library_id
        || post_signed_to_core(&state, "/internal/v1/core/playback/authorize", &claims)
            .await
            .is_err()
    {
        return cors_error_response(StatusCode::FORBIDDEN, "playback_authorization_denied");
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => convert_stream_response(res, true),
        Err(code) => node_error_response(code, true),
    }
}

fn convert_stream_response(res: http::Response<BoxStreamBody>, with_cors: bool) -> Response {
    let (mut parts, body) = res.into_parts();
    if with_cors {
        apply_cors_headers(&mut parts.headers);
    }
    Response::from_parts(parts, Body::new(body))
}

fn node_error_response(code: NodeErrorCode, with_cors: bool) -> Response {
    let status = match code {
        NodeErrorCode::UnknownAsset | NodeErrorCode::UnknownImage => StatusCode::NOT_FOUND,
        NodeErrorCode::UnavailableAsset | NodeErrorCode::UnavailableImage => StatusCode::NOT_FOUND,
        NodeErrorCode::InvalidImage | NodeErrorCode::ImageChangedRescanRequired => {
            StatusCode::CONFLICT
        }
        NodeErrorCode::StreamError | NodeErrorCode::ImageStreamError => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::BAD_REQUEST,
    };
    let mut res = (status, Json(serde_json::json!({ "error": code.as_str() }))).into_response();
    if with_cors {
        apply_cors_headers(res.headers_mut());
    }
    res
}

fn cors_error_response(status: StatusCode, reason: &'static str) -> Response {
    let mut res = (status, Json(serde_json::json!({ "error": reason }))).into_response();
    apply_cors_headers(res.headers_mut());
    res
}

fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Range, Accept, Content-Type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("Accept-Ranges, Content-Length, Content-Range, Content-Type"),
    );
    headers.insert(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("cross-origin"),
    );
}

fn sign_stream_response(
    mut response: Response,
    identity: &NodeIdentityState,
    request_nonce: &str,
) -> Response {
    let now = unix_now();
    let nonce = random_nonce_hex();
    let digest = crate::transport::stream_metadata_digest(response.headers());
    let signature = sign_internal_response(
        identity.shared_secret.as_deref().unwrap_or(""),
        request_nonce,
        response.status().as_u16(),
        identity.node_id.as_str(),
        &identity.key_id,
        now,
        &nonce,
        &digest,
    );
    for (key, value) in [
        (HDR_RES_TIMESTAMP, now.to_string()),
        (HDR_RES_NONCE, nonce),
        (HDR_RES_SIGNATURE, signature),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().insert(key, value);
        }
    }
    response
}
