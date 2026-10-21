//! Versioned internal protocol (`v1`), mutual authentication, replay protection,
//! secret rotation, inventory synchronization, and short-lived playback capabilities
//! between Core and Media Nodes.
//!
//! Contains zero SQL, HTTP server, user session, or filesystem I/O dependencies.

use crate::{
    ids::{AssetId, ImageId, LibraryId, NodeId, ScanId},
    inventory::{BatchAck, InventoryBatch, ScanFailed, ScanFinished},
    media::LocalImage,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROTOCOL_VERSION: &str = "v1";
pub const MAX_CLOCK_SKEW_SECS: i64 = 300;
pub const DEFAULT_PLAYBACK_TOKEN_TTL_SECS: i64 = 900;
pub const MAX_REPLAY_ENTRIES: usize = 8192;

pub const HDR_PROTOCOL: &str = "x-media-protocol";
pub const HDR_NODE_ID: &str = "x-media-node-id";
pub const HDR_KEY_ID: &str = "x-media-key-id";
pub const HDR_TIMESTAMP: &str = "x-media-timestamp";
pub const HDR_NONCE: &str = "x-media-nonce";
pub const HDR_BODY_SHA256: &str = "x-media-content-sha256";
pub const HDR_SIGNATURE: &str = "x-media-signature";

pub const HDR_RES_TIMESTAMP: &str = "x-media-response-timestamp";
pub const HDR_RES_NONCE: &str = "x-media-response-nonce";
pub const HDR_RES_SIGNATURE: &str = "x-media-response-signature";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Connected,
    Disconnected,
    Revoked,
}

impl NodeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "connected" => Self::Connected,
            "revoked" => Self::Revoked,
            _ => Self::Disconnected,
        }
    }
}

/// Core -> Node pairing handshake request (`POST /internal/v1/node/pair`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairNodeRequest {
    pub protocol_version: String,
    pub core_id: String,
    pub key_id: String,
    pub core_secret_half: String,
    pub core_origin: String,
    pub timestamp: i64,
    pub nonce: String,
    /// HMAC-SHA256(pairing_token, "pair-req\n{protocol_version}\n{core_id}\n{key_id}\n{core_secret_half}\n{core_origin}\n{timestamp}\n{nonce}")
    pub pairing_proof: String,
}

/// Node -> Core pairing handshake response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairNodeResponse {
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub node_id: NodeId,
    pub roots: Vec<String>,
    pub public_url: Option<String>,
    pub node_secret_half: String,
    pub timestamp: i64,
    pub nonce: String,
    /// HMAC-SHA256(derived_secret, "pair-res\n{req_nonce}\n{node_id}\n{key_id}\n{node_secret_half}\n{timestamp}\n{nonce}")
    pub mutual_proof: String,
}

/// Node -> Core self-enrollment request (`POST /api/v1/nodes/enroll`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollNodeRequest {
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub enrollment_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    pub name: String,
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    pub node_secret_half: String,
    pub timestamp: i64,
    pub nonce: String,
}

/// Core -> Node self-enrollment response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollNodeResponse {
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub node_id: NodeId,
    pub key_id: String,
    pub core_secret_half: String,
    pub core_origin: String,
    pub timestamp: i64,
    pub nonce: String,
    /// HMAC-SHA256(derived_secret, "enroll-res\n{req_nonce}\n{node_id}\n{key_id}\n{core_secret_half}\n{timestamp}\n{nonce}")
    pub mutual_proof: String,
}

/// Core -> Node command types sent over outbound channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCommandType {
    Ping,
    Scan,
    SyncInventory,
    RotateSecret,
    Revoke,
    StreamRelay,
}

/// Core -> Node command envelope sent as response to channel poll.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCommandEnvelope {
    pub command_id: String,
    pub command_type: NodeCommandType,
    pub payload: serde_json::Value,
}

/// Core -> Node request to stream asset/image chunk via outbound uplink.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRelayRequest {
    #[serde(default)]
    pub head: bool,
    pub stream_id: String,
    pub target_kind: String,
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_none_match: Option<String>,
}

/// Core -> Node or Node -> Core heartbeat payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatPayload {
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub node_id: NodeId,
    pub roots: Vec<String>,
    pub public_url: Option<String>,
    pub active_scans: u32,
    pub indexed_assets: usize,
    pub timestamp: i64,
}

/// Core -> Node secret rotation request (`POST /internal/v1/node/rotate-secret`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateSecretRequest {
    pub protocol_version: String,
    pub node_id: NodeId,
    pub new_key_id: String,
    pub new_secret: String,
    pub timestamp: i64,
    pub nonce: String,
}

/// Node -> Core secret rotation acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateSecretResponse {
    pub protocol_version: String,
    pub node_id: NodeId,
    pub new_key_id: String,
    pub timestamp: i64,
    pub nonce: String,
    /// HMAC-SHA256(new_secret, "rotate-ack\n{req_nonce}\n{node_id}\n{new_key_id}\n{timestamp}\n{nonce}")
    pub mutual_proof: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedFileMeta {
    pub relative_path: String,
    pub size: u64,
    pub modified_ns: String,
}

/// Core -> Node scan initiation command (`POST /internal/v1/node/scans`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScanStartRequest {
    pub scan_id: ScanId,
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub kind: String,
    pub root: String,
    pub config_revision: String,
    pub core_base_url: String,
    pub batch_size: usize,
    pub cached_files: Vec<CachedFileMeta>,
    pub cached_images: Vec<LocalImage>,
    #[serde(default)]
    pub excluded_patterns: Vec<String>,
}

/// Node -> Core batch submission (`POST /internal/v1/core/scans/{scan_id}/batches`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBatchSubmission {
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub batch: InventoryBatch,
}

/// Node -> Core scan completion (`POST /internal/v1/core/scans/{scan_id}/complete`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScanCompletion {
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub root_snapshot: String,
    pub config_revision: String,
    pub finished: ScanFinished,
}

/// Node -> Core scan failure (`POST /internal/v1/core/scans/{scan_id}/fail`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScanFailure {
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub failed: ScanFailed,
    pub message: String,
}

/// Asset mapping synced from Core to Node so the Node resolves `AssetId` from its own inventory
/// without ever accepting a filesystem path from a client or playback token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInventoryAssetEntry {
    pub asset_id: AssetId,
    pub library_id: LibraryId,
    pub root_snapshot: String,
    pub relative_path: String,
    pub mime: String,
    pub size: u64,
    pub modified_ns: String,
}

/// Image mapping synced from Core to Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInventoryImageEntry {
    pub image_id: ImageId,
    pub library_id: LibraryId,
    pub root_snapshot: String,
    pub relative_path: String,
    pub mime: String,
    pub size: u64,
    pub modified_ns: String,
}

/// Core -> Node operational inventory synchronization for a library.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInventorySync {
    pub library_id: LibraryId,
    pub root_snapshot: String,
    pub assets: Vec<NodeInventoryAssetEntry>,
    pub images: Vec<NodeInventoryImageEntry>,
}

/// Response from Core to Node on `POST /internal/v1/core/scans/{scan_id}/complete`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteScanCompleteResponse {
    pub ack: BatchAck,
    pub inventory_sync: LibraryInventorySync,
}

/// Short-lived, node-bound, asset-bound, action-bound playback authorization token claims.
/// Contains zero user session identifiers, zero cookies, and zero host filesystem paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTokenClaims {
    pub v: String,
    pub kid: String,
    pub node_id: NodeId,
    pub target_kind: String,
    pub target_id: String,
    pub library_id: LibraryId,
    pub action: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    #[serde(default)]
    pub session_hash: String,
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn random_nonce_hex() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn random_secret_hex() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// RFC 2104 HMAC-SHA256 implementation using `sha2::Sha256`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let outer_digest = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer_digest);
    out
}

pub fn hmac_sha256_hex(key: &str, message: &str) -> String {
    hex::encode(hmac_sha256(key.as_bytes(), message.as_bytes()))
}

/// Constant-time byte comparison to prevent timing side-channel attacks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Derive a mutually contributed 256-bit shared secret from Core and Node secret halves
/// bound to the initial pairing token.
pub fn derive_paired_secret(pairing_token: &str, core_half: &str, node_half: &str) -> String {
    let context = format!("aether-p2-pair-v1\n{core_half}\n{node_half}");
    hmac_sha256_hex(pairing_token, &context)
}

pub fn compute_pairing_proof(
    pairing_token: &str,
    core_id: &str,
    key_id: &str,
    core_secret_half: &str,
    core_origin: &str,
    timestamp: i64,
    nonce: &str,
) -> String {
    let msg = format!(
        "pair-req\n{PROTOCOL_VERSION}\n{core_id}\n{key_id}\n{core_secret_half}\n{core_origin}\n{timestamp}\n{nonce}"
    );
    hmac_sha256_hex(pairing_token, &msg)
}

pub fn compute_pairing_mutual_proof(
    derived_secret: &str,
    req_nonce: &str,
    node_id: &str,
    key_id: &str,
    node_secret_half: &str,
    timestamp: i64,
    nonce: &str,
) -> String {
    let msg = format!(
        "pair-res\n{req_nonce}\n{node_id}\n{key_id}\n{node_secret_half}\n{timestamp}\n{nonce}"
    );
    hmac_sha256_hex(derived_secret, &msg)
}

pub fn compute_enrollment_mutual_proof(
    derived_secret: &str,
    req_nonce: &str,
    node_id: &str,
    key_id: &str,
    core_secret_half: &str,
    timestamp: i64,
    nonce: &str,
) -> String {
    let msg = format!(
        "enroll-res\n{req_nonce}\n{node_id}\n{key_id}\n{core_secret_half}\n{timestamp}\n{nonce}"
    );
    hmac_sha256_hex(derived_secret, &msg)
}

pub fn generate_enrollment_token() -> (String, String) {
    let raw = format!(
        "mnet_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let hash = sha256_hex(raw.as_bytes());
    (raw, hash)
}

pub fn compute_rotation_mutual_proof(
    new_secret: &str,
    req_nonce: &str,
    node_id: &str,
    new_key_id: &str,
    timestamp: i64,
    nonce: &str,
) -> String {
    let msg = format!("rotate-ack\n{req_nonce}\n{node_id}\n{new_key_id}\n{timestamp}\n{nonce}");
    hmac_sha256_hex(new_secret, &msg)
}

#[allow(clippy::too_many_arguments)]
pub fn sign_internal_request(
    secret: &str,
    direction: &str,
    method: &str,
    path_and_query: &str,
    node_id: &str,
    key_id: &str,
    timestamp: i64,
    nonce: &str,
    body_sha256: &str,
) -> String {
    let canonical = format!(
        "{PROTOCOL_VERSION}\n{direction}\n{method}\n{path_and_query}\n{node_id}\n{key_id}\n{timestamp}\n{nonce}\n{body_sha256}"
    );
    hmac_sha256_hex(secret, &canonical)
}

#[allow(clippy::too_many_arguments)]
pub fn verify_internal_request(
    secret: &str,
    direction: &str,
    method: &str,
    path_and_query: &str,
    node_id: &str,
    key_id: &str,
    timestamp: i64,
    nonce: &str,
    body_sha256: &str,
    signature: &str,
    now: i64,
) -> Result<(), &'static str> {
    if (now - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err("timestamp_out_of_range");
    }
    if nonce.len() < 16 || nonce.len() > 128 {
        return Err("invalid_nonce");
    }
    let expected = sign_internal_request(
        secret,
        direction,
        method,
        path_and_query,
        node_id,
        key_id,
        timestamp,
        nonce,
        body_sha256,
    );
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err("invalid_signature");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn sign_internal_response(
    secret: &str,
    req_nonce: &str,
    status: u16,
    node_id: &str,
    key_id: &str,
    timestamp: i64,
    res_nonce: &str,
    body_sha256: &str,
) -> String {
    let canonical = format!(
        "{PROTOCOL_VERSION}-res\n{req_nonce}\n{status}\n{node_id}\n{key_id}\n{timestamp}\n{res_nonce}\n{body_sha256}"
    );
    hmac_sha256_hex(secret, &canonical)
}

#[allow(clippy::too_many_arguments)]
pub fn verify_internal_response(
    secret: &str,
    req_nonce: &str,
    status: u16,
    node_id: &str,
    key_id: &str,
    timestamp: i64,
    res_nonce: &str,
    signature: &str,
    body_sha256: &str,
    now: i64,
) -> Result<(), &'static str> {
    if (now - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
        return Err("response_timestamp_out_of_range");
    }
    if res_nonce.len() < 16 || res_nonce.len() > 128 {
        return Err("invalid_response_nonce");
    }
    let expected = sign_internal_response(
        secret,
        req_nonce,
        status,
        node_id,
        key_id,
        timestamp,
        res_nonce,
        body_sha256,
    );
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err("invalid_response_signature");
    }
    Ok(())
}

/// Issue a short-lived signed playback capability token for direct Browser -> Node streaming.
pub fn issue_playback_token(
    secret: &str,
    claims: &PlaybackTokenClaims,
) -> Result<String, serde_json::Error> {
    let json_bytes = serde_json::to_vec(claims)?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&json_bytes);
    let sig = hmac_sha256(secret.as_bytes(), payload_b64.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(sig);
    Ok(format!("{payload_b64}.{sig_b64}"))
}

/// Verify a short-lived playback capability token on the Node.
#[allow(clippy::too_many_arguments)]
pub fn verify_playback_token(
    secret: &str,
    expected_kid: &str,
    expected_node_id: &str,
    expected_target_kind: &str,
    expected_target_id: &str,
    expected_action: &str,
    token: &str,
    now: i64,
) -> Result<PlaybackTokenClaims, &'static str> {
    let Some((payload_b64, sig_b64)) = token.split_once('.') else {
        return Err("malformed_playback_token");
    };
    let expected_sig = hmac_sha256(secret.as_bytes(), payload_b64.as_bytes());
    let expected_sig_b64 = URL_SAFE_NO_PAD.encode(expected_sig);
    if !constant_time_eq(expected_sig_b64.as_bytes(), sig_b64.as_bytes()) {
        return Err("invalid_playback_token_signature");
    }
    let json_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| "malformed_playback_token")?;
    let claims: PlaybackTokenClaims =
        serde_json::from_slice(&json_bytes).map_err(|_| "malformed_playback_token")?;
    if claims.v != PROTOCOL_VERSION {
        return Err("unsupported_token_version");
    }
    if !constant_time_eq(claims.kid.as_bytes(), expected_kid.as_bytes()) {
        return Err("revoked_or_rotated_key_id");
    }
    if claims.node_id.as_str() != expected_node_id {
        return Err("wrong_node_target");
    }
    if claims.target_kind != expected_target_kind || claims.target_id != expected_target_id {
        return Err("wrong_asset_target");
    }
    if claims.action != expected_action {
        return Err("wrong_token_action");
    }
    if now >= claims.exp
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > DEFAULT_PLAYBACK_TOKEN_TTL_SECS
    {
        return Err("expired_playback_token");
    }
    if claims.iat > now + MAX_CLOCK_SKEW_SECS {
        return Err("invalid_token_iat");
    }
    Ok(claims)
}

/// Bounded anti-replay window tracking seen `(node_id, nonce)` pairs within `MAX_CLOCK_SKEW_SECS`.
#[derive(Debug, Default)]
pub struct ReplayWindow {
    seen: Mutex<HashMap<(String, String), i64>>,
}

impl ReplayWindow {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Record `(node_id, nonce)` at `now`, returning `Err("replay_detected")` if already used.
    pub fn check_and_record(
        &self,
        node_id: &str,
        nonce: &str,
        timestamp: i64,
        now: i64,
    ) -> Result<(), &'static str> {
        if (now - timestamp).abs() > MAX_CLOCK_SKEW_SECS {
            return Err("timestamp_out_of_range");
        }
        let mut guard = match self.seen.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let cutoff = now - (MAX_CLOCK_SKEW_SECS * 2);
        if guard.len() >= MAX_REPLAY_ENTRIES {
            guard.retain(|_, ts| *ts >= cutoff);
        }
        let key = (node_id.to_owned(), nonce.to_owned());
        if let Some(&existing_ts) = guard.get(&key)
            && existing_ts >= cutoff
        {
            return Err("replay_detected");
        }
        if guard.len() >= MAX_REPLAY_ENTRIES {
            // If still full after pruning expired entries, evict one oldest entry to stay strictly bounded.
            if let Some(oldest_key) = guard
                .iter()
                .min_by_key(|(_, ts)| **ts)
                .map(|(k, _)| k.clone())
            {
                guard.remove(&oldest_key);
            }
        }
        guard.insert(key, timestamp);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4231_hmac_sha256_test_vector_1() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac = hex::encode(hmac_sha256(&key, data));
        assert_eq!(
            mac,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn internal_request_signature_and_replay_window_reject_replays_and_tampering() {
        let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let now = 1_760_000_000;
        let nonce = "nonce-1234567890abcdef";
        let body_hash = sha256_hex(b"{\"hello\":\"world\"}");
        let sig = sign_internal_request(
            secret,
            "core-to-node",
            "POST",
            "/internal/v1/node/heartbeat",
            "node-films",
            "kid-1",
            now,
            nonce,
            &body_hash,
        );

        assert!(
            verify_internal_request(
                secret,
                "core-to-node",
                "POST",
                "/internal/v1/node/heartbeat",
                "node-films",
                "kid-1",
                now,
                nonce,
                &body_hash,
                &sig,
                now,
            )
            .is_ok()
        );

        // Wrong direction or wrong node_id must fail
        assert_eq!(
            verify_internal_request(
                secret,
                "node-to-core",
                "POST",
                "/internal/v1/node/heartbeat",
                "node-films",
                "kid-1",
                now,
                nonce,
                &body_hash,
                &sig,
                now,
            ),
            Err("invalid_signature")
        );

        // Expired timestamp must fail
        assert_eq!(
            verify_internal_request(
                secret,
                "core-to-node",
                "POST",
                "/internal/v1/node/heartbeat",
                "node-films",
                "kid-1",
                now - 600,
                nonce,
                &body_hash,
                &sig,
                now,
            ),
            Err("timestamp_out_of_range")
        );

        // Replay window accepts first use and rejects second use
        let window = ReplayWindow::new();
        assert!(
            window
                .check_and_record("node-films", nonce, now, now)
                .is_ok()
        );
        assert_eq!(
            window.check_and_record("node-films", nonce, now, now + 1),
            Err("replay_detected")
        );
        // Same nonce on a different node is isolated
        assert!(
            window
                .check_and_record("node-series", nonce, now, now + 1)
                .is_ok()
        );
    }

    #[test]
    fn playback_token_binds_node_asset_action_key_and_expiration() {
        let secret = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let now = 1_760_000_000;
        let claims = PlaybackTokenClaims {
            v: PROTOCOL_VERSION.to_owned(),
            kid: "kid-active".to_owned(),
            node_id: NodeId::from("node-films"),
            target_kind: "asset".to_owned(),
            target_id: "asset-123".to_owned(),
            library_id: LibraryId::from("lib-films"),
            action: "stream".to_owned(),
            iat: now,
            exp: now + 900,
            session_hash: String::new(),
            jti: "jti-1".to_owned(),
        };
        let token = issue_playback_token(secret, &claims).unwrap();

        let verified = verify_playback_token(
            secret,
            "kid-active",
            "node-films",
            "asset",
            "asset-123",
            "stream",
            &token,
            now + 10,
        )
        .unwrap();
        assert_eq!(verified, claims);

        // Wrong node rejected
        assert_eq!(
            verify_playback_token(
                secret,
                "kid-active",
                "node-series",
                "asset",
                "asset-123",
                "stream",
                &token,
                now + 10,
            ),
            Err("wrong_node_target")
        );

        // Wrong asset rejected
        assert_eq!(
            verify_playback_token(
                secret,
                "kid-active",
                "node-films",
                "asset",
                "asset-999",
                "stream",
                &token,
                now + 10,
            ),
            Err("wrong_asset_target")
        );

        // Rotated key_id rejected
        assert_eq!(
            verify_playback_token(
                secret,
                "kid-rotated",
                "node-films",
                "asset",
                "asset-123",
                "stream",
                &token,
                now + 10,
            ),
            Err("revoked_or_rotated_key_id")
        );

        // Expired token rejected
        assert_eq!(
            verify_playback_token(
                secret,
                "kid-active",
                "node-films",
                "asset",
                "asset-123",
                "stream",
                &token,
                now + 901,
            ),
            Err("expired_playback_token")
        );
    }

    #[test]
    fn node_payloads_support_optional_version_field_backward_compatibly() {
        // Without version field (legacy payload)
        let legacy_json = r#"{
            "protocolVersion": "v1",
            "nodeId": "test-node",
            "roots": ["/media"],
            "activeScans": 0,
            "indexedAssets": 5,
            "timestamp": 123456
        }"#;
        let hb: HeartbeatPayload = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(hb.version, None);

        // With version field
        let modern_json = r#"{
            "protocolVersion": "v1",
            "version": "0.1.0-beta",
            "nodeId": "test-node",
            "roots": ["/media"],
            "activeScans": 0,
            "indexedAssets": 5,
            "timestamp": 123456
        }"#;
        let hb2: HeartbeatPayload = serde_json::from_str(modern_json).unwrap();
        assert_eq!(hb2.version.as_deref(), Some("0.1.0-beta"));
    }

    #[test]
    fn enrollment_token_generation_and_mutual_proof_verification() {
        let (token, hash) = generate_enrollment_token();
        assert!(token.starts_with("mnet_"));
        assert_eq!(hash, sha256_hex(token.as_bytes()));

        let core_half = random_secret_hex();
        let node_half = random_secret_hex();
        let derived = derive_paired_secret(&token, &core_half, &node_half);

        let req_nonce = random_nonce_hex();
        let res_nonce = random_nonce_hex();
        let now = unix_now();
        let proof = compute_enrollment_mutual_proof(
            &derived, &req_nonce, "node-123", "key-456", &core_half, now, &res_nonce,
        );

        let expected = compute_enrollment_mutual_proof(
            &derived, &req_nonce, "node-123", "key-456", &core_half, now, &res_nonce,
        );
        assert!(constant_time_eq(proof.as_bytes(), expected.as_bytes()));

        let bad = compute_enrollment_mutual_proof(
            "bad-secret",
            &req_nonce,
            "node-123",
            "key-456",
            &core_half,
            now,
            &res_nonce,
        );
        assert!(!constant_time_eq(proof.as_bytes(), bad.as_bytes()));
    }

    #[test]
    fn command_envelope_serialization_round_trip() {
        let cmd = NodeCommandEnvelope {
            command_id: "cmd-1".to_owned(),
            command_type: NodeCommandType::StreamRelay,
            payload: serde_json::json!({
                "streamId": "str-99",
                "targetKind": "asset",
                "targetId": "asset-1"
            }),
        };
        let serialized = serde_json::to_string(&cmd).unwrap();
        let deserialized: NodeCommandEnvelope = serde_json::from_str(&serialized).unwrap();
        assert_eq!(cmd, deserialized);
    }
}
