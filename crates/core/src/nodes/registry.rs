//! Node records, warm runtime telemetry, selection scores and registry projections.
use crate::identity::IdentityStore;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::unix_now;
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use super::health::{HEALTH_MIN_INTERVAL, HealthSnapshot};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    pub active_scans: u32,
    pub active_streams: u32,
    pub indexed_assets: usize,
    pub selection_score: i64,
    pub libraries: Vec<NodeLibraryRef>,
    pub scans: Vec<NodeScanSummary>,
    /// Core instance owning the Node's command channel (`node_channel_owner`), when any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_owner: Option<String>,
    /// Ownership epoch of that channel, carried by every command the owner delivers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_epoch: Option<i64>,
    /// A secret rotation waits for the Node to use its new key (two-phase rotation); omitted
    /// otherwise. Neither the pending key id nor its secret is ever exposed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending_rotation: bool,
}

#[derive(Clone, Debug, Default)]
pub struct NodeRuntimeTelemetry {
    pub resources: Option<media_contracts::protocol::ResourcePressure>,
    pub active_scan_ids: Vec<String>,
    pub latency_ms: Option<u32>,
    pub reported_active_scans: u32,
    pub reported_active_streams: u32,
    pub core_active_relays: u32,
    pub indexed_assets: usize,
    pub consecutive_failures: u32,
    /// Unix time of this telemetry: the last heartbeat this Core received, or the `node_health`
    /// snapshot of the Core that received it (multi-Core). 0: none yet.
    pub sampled_at: i64,
}

#[derive(Clone, Debug)]
pub struct NodeRuntimeEntry {
    pub record: MediaNodeRecord,
    pub telemetry: NodeRuntimeTelemetry,
}

#[derive(Default)]
pub struct NodeRuntimeRegistry {
    entries: std::sync::RwLock<HashMap<String, NodeRuntimeEntry>>,
    populated: std::sync::atomic::AtomicBool,
    /// While other Cores are live: elects the request that re-reads records and shared telemetry.
    shared_refresh: crate::coherence::RefreshGate,
    /// Re-reads done for other Cores (multi-Core mode only).
    shared_refreshes: AtomicU64,
    /// Last `node_health` snapshot this Core wrote per Node, and when.
    health_published: std::sync::Mutex<HashMap<String, (Instant, HealthSnapshot)>>,
    /// Sequence of this Core's own record writes ([`Self::upsert_record`]), and the last one per
    /// Node: a shared re-read whose SELECT started before a local write (a revocation, a key
    /// rotation) never puts the older database row back over it.
    local_writes: AtomicU64,
    written_at: std::sync::Mutex<HashMap<String, u64>>,
}

impl NodeRuntimeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-reads of the shared view done by this registry (none while the Core is alone).
    pub fn shared_refreshes(&self) -> u64 {
        self.shared_refreshes.load(Ordering::Relaxed)
    }

    /// The next registry read re-reads the shared view (tests, and after this Core wrote).
    pub fn expire_shared_view(&self) {
        self.shared_refresh.expire();
    }

    /// Whether this Core should write `snapshot` of `node_id` now (recorded as written if so):
    /// first snapshot, or a significant change at least [`HEALTH_MIN_INTERVAL`] after the last.
    pub(crate) fn claim_health_publication(
        &self,
        node_id: &str,
        snapshot: &HealthSnapshot,
    ) -> bool {
        let mut published = self
            .health_published
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A stable Node is still republished now and then: peers compare its `sampled_at` with
        // their own deadlines (an administrator's stop request, for instance).
        let due = published.get(node_id).is_none_or(|(at, previous)| {
            at.elapsed() >= HEALTH_MIN_INTERVAL
                && (previous.significant_change(snapshot)
                    || at.elapsed() >= 2 * HEALTH_MIN_INTERVAL)
        });
        if due {
            published.insert(node_id.to_owned(), (Instant::now(), snapshot.clone()));
        }
        due
    }

    /// Forget every published snapshot: the next heartbeat of each Node publishes again (multi-
    /// Core mode left, peers will need a fresh snapshot when it is entered again).
    pub(crate) fn forget_health_publications(&self) {
        self.health_published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Mark taken before a shared re-read: local record writes after it win over that read.
    pub(crate) fn write_mark(&self) -> u64 {
        self.local_writes.load(Ordering::Acquire)
    }

    pub(crate) fn forget_health_publication(&self, node_id: &str) {
        self.health_published
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(node_id);
    }

    /// Records and telemetry as stored by every Core: the record is the database's (status,
    /// keys), keeping the most recent `last_seen_at`; telemetry is replaced by a snapshot newer
    /// than this Core's own.
    fn merge_shared(&self, rows: Vec<SharedNodeRow>, since: u64) {
        let written_at = self
            .written_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let newer_locally = |id: &str| written_at.get(id).is_some_and(|seq| *seq > since);
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let mut seen = std::collections::HashSet::with_capacity(rows.len());
        for row in rows {
            let SharedNodeRow {
                mut record,
                health_sampled_at,
                health_json,
            } = row;
            seen.insert(record.id.clone());
            let entry = match guard.entry(record.id.clone()) {
                std::collections::hash_map::Entry::Occupied(slot) => {
                    let existing = slot.into_mut();
                    if !newer_locally(&record.id) {
                        record.last_seen_at = record.last_seen_at.max(existing.record.last_seen_at);
                        existing.record = record;
                    }
                    existing
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    let is_local = record.id == "local";
                    slot.insert(NodeRuntimeEntry {
                        record,
                        telemetry: NodeRuntimeTelemetry {
                            latency_ms: if is_local { Some(0) } else { None },
                            ..NodeRuntimeTelemetry::default()
                        },
                    })
                }
            };
            if let (Some(sampled_at), Some(json)) = (health_sampled_at, health_json)
                && sampled_at > entry.telemetry.sampled_at
                && let Ok(snapshot) = serde_json::from_str::<HealthSnapshot>(&json)
            {
                snapshot.apply(&mut entry.telemetry, sampled_at);
            }
        }
        guard.retain(|id, _| seen.contains(id) || newer_locally(id));
        self.populated
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn is_populated(&self) -> bool {
        self.populated.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn get(&self, node_id: &str) -> Option<NodeRuntimeEntry> {
        let guard = self.entries.read().unwrap_or_else(|e| e.into_inner());
        guard.get(node_id).cloned()
    }

    pub fn snapshot_all(&self) -> HashMap<String, NodeRuntimeEntry> {
        let guard = self.entries.read().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    }

    pub fn upsert_record(&self, record: MediaNodeRecord) {
        let seq = self.local_writes.fetch_add(1, Ordering::AcqRel) + 1;
        self.written_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(record.id.clone(), seq);
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        match guard.get_mut(&record.id) {
            Some(existing) => {
                existing.record = record;
            }
            None => {
                let is_local = record.id == "local";
                guard.insert(
                    record.id.clone(),
                    NodeRuntimeEntry {
                        record,
                        telemetry: NodeRuntimeTelemetry {
                            latency_ms: if is_local { Some(0) } else { None },
                            ..NodeRuntimeTelemetry::default()
                        },
                    },
                );
            }
        }
    }

    pub fn populate_records(&self, records: Vec<MediaNodeRecord>) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        let mut seen = std::collections::HashSet::with_capacity(records.len());
        for record in records {
            seen.insert(record.id.clone());
            match guard.get_mut(&record.id) {
                Some(existing) => {
                    existing.record = record;
                }
                None => {
                    let is_local = record.id == "local";
                    guard.insert(
                        record.id.clone(),
                        NodeRuntimeEntry {
                            record,
                            telemetry: NodeRuntimeTelemetry {
                                latency_ms: if is_local { Some(0) } else { None },
                                ..NodeRuntimeTelemetry::default()
                            },
                        },
                    );
                }
            }
        }
        guard.retain(|id, _| seen.contains(id));
        self.populated
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub fn record_heartbeat(
        &self,
        node_id: &str,
        hb: &HeartbeatPayload,
        rtt_ms: Option<u32>,
        now: i64,
    ) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            if entry.record.status != NodeStatus::Revoked.as_str() {
                entry.record.status = NodeStatus::Connected.as_str().to_owned();
                entry.record.last_seen_at = Some(now);
                if let Some(ref v) = hb.version {
                    entry.record.version = Some(v.clone());
                }
                if let Ok(roots_json) = serde_json::to_string(&hb.roots) {
                    entry.record.roots_json = roots_json;
                }
            }
            entry.telemetry.active_scan_ids = hb.active_scan_ids.iter().take(64).cloned().collect();
            entry.telemetry.resources = hb.resource_pressure.clone();
            entry.telemetry.reported_active_scans = hb.active_scans;
            entry.telemetry.reported_active_streams = hb.active_streams;
            entry.telemetry.indexed_assets = hb.indexed_assets;
            entry.telemetry.consecutive_failures = 0;
            entry.telemetry.sampled_at = now;
            if let Some(sample) = rtt_ms {
                entry.telemetry.latency_ms = Some(match entry.telemetry.latency_ms {
                    Some(prev) => ((prev as u64 * 3 + sample as u64) / 4) as u32,
                    None => sample,
                });
            }
        }
    }

    pub fn record_success(&self, node_id: &str, rtt_ms: Option<u32>, now: i64) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            if entry.record.status != NodeStatus::Revoked.as_str() {
                entry.record.status = NodeStatus::Connected.as_str().to_owned();
                entry.record.last_seen_at = Some(now);
            }
            entry.telemetry.consecutive_failures = 0;
            if let Some(sample) = rtt_ms {
                entry.telemetry.latency_ms = Some(match entry.telemetry.latency_ms {
                    Some(prev) => ((prev as u64 * 3 + sample as u64) / 4) as u32,
                    None => sample,
                });
            }
        }
    }

    pub fn record_failure(&self, node_id: &str, mark_disconnected: bool) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            entry.telemetry.consecutive_failures =
                entry.telemetry.consecutive_failures.saturating_add(1);
            if mark_disconnected && entry.record.status != NodeStatus::Revoked.as_str() {
                entry.record.status = NodeStatus::Disconnected.as_str().to_owned();
            }
        }
    }

    pub fn mark_disconnected(&self, node_id: &str) {
        self.record_failure(node_id, true);
    }

    pub fn set_telemetry(
        &self,
        node_id: &str,
        latency_ms: Option<u32>,
        active_scans: u32,
        active_streams: u32,
    ) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            entry.telemetry.latency_ms = latency_ms;
            entry.telemetry.reported_active_scans = active_scans;
            entry.telemetry.reported_active_streams = active_streams;
        }
    }

    pub fn increment_relay(&self, node_id: &str) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            entry.telemetry.core_active_relays =
                entry.telemetry.core_active_relays.saturating_add(1);
        }
    }

    pub fn decrement_relay(&self, node_id: &str) {
        let mut guard = self.entries.write().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(node_id) {
            entry.telemetry.core_active_relays =
                entry.telemetry.core_active_relays.saturating_sub(1);
        }
    }

    pub fn invalidate(&self) {
        self.populated
            .store(false, std::sync::atomic::Ordering::Release);
    }

    pub fn connected_public_origins(&self) -> Vec<String> {
        let guard = self.entries.read().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for entry in guard.values() {
            if entry.record.kind == "remote"
                && entry.record.status == NodeStatus::Connected.as_str()
                && let Some(ref u) = entry.record.public_url
                && !u.is_empty()
                && let Ok(parsed) = url::Url::parse(u)
                && let Some(h) = parsed.host_str()
            {
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    continue;
                }
                let origin_str = if let Some(p) = parsed.port() {
                    format!("{scheme}://{h}:{p}")
                } else {
                    format!("{scheme}://{h}")
                };
                if !out.contains(&origin_str) {
                    out.push(origin_str);
                }
            }
        }
        out
    }
}

pub fn effective_connection_mode(rec: &MediaNodeRecord) -> &'static str {
    if rec.id == "local" || rec.kind == "local" {
        "local"
    } else if rec.public_url.as_deref().is_some_and(|u| !u.is_empty())
        || rec.connection_mode == "direct"
    {
        "direct"
    } else if rec.connection_mode == "inbound" || rec.base_url.is_some() {
        "inbound"
    } else {
        "outbound"
    }
}

/// A paired Node heartbeats every 15 s (and long-polls the control channel every 25 s at most).
/// Five missed beats mean the Node is gone, even if no request ever failed to tell the Core:
/// a power cut or a network partition produces no TCP reset on the Core side.
pub const DEFAULT_NODE_STALE_SECS: i64 = 75;

/// Seconds without any sign of life after which a remote Node counts as disconnected.
/// `MEDIA_NODE_STALE_SECS=0` restores the former behaviour (never stale).
pub fn node_stale_limit_secs() -> i64 {
    std::env::var("MEDIA_NODE_STALE_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .map_or(DEFAULT_NODE_STALE_SECS, |v| v.max(0))
}

/// Pure staleness rule shared by Node scoring and by direct-URL issuance, so a route is never
/// handed to a browser for a Node that the ranking already considers gone.
pub fn node_is_stale(rec: &MediaNodeRecord, now: i64) -> bool {
    let limit = node_stale_limit_secs();
    limit > 0
        && rec.id != "local"
        && rec.kind == "remote"
        && rec
            .last_seen_at
            .is_some_and(|seen| now.saturating_sub(seen) > limit)
}

/// Pure O(1) warm-cache node & source scoring function.
/// Never performs network I/O. Computes a deterministic priority score:
/// - Connected status (+10_000) vs Disconnected (-100_000) vs Revoked (-1_000_000)
/// - Transport efficiency: Local (+2_000) > Direct Node URL (+1_500, zero Core relay cost) > Inbound Relay (+800) > Outbound Uplink Relay (+500)
/// - Load & Latency penalties: active streams (-120 each), active scans (-200 each), RTT latency (-1 per ms up to 1000ms), consecutive failures (-300 each)
/// - Format & Quality bonus: direct-playable MP4/WebM (+300) vs MKV (+150), plus resolution bonus (height / 10).
pub fn score_node_entry(
    entry: &NodeRuntimeEntry,
    mime: Option<&str>,
    height: Option<u32>,
    now: i64,
) -> (String, String, i64) {
    let rec = &entry.record;
    let tel = &entry.telemetry;
    let conn_mode = effective_connection_mode(rec);

    if rec.status == NodeStatus::Revoked.as_str() {
        return ("revoked".to_owned(), conn_mode.to_owned(), -1_000_000);
    }

    let is_stale = node_is_stale(rec, now);

    let effective_status = if rec.id == "local" {
        "connected"
    } else if rec.status != NodeStatus::Connected.as_str() || is_stale {
        "disconnected"
    } else {
        "connected"
    };

    if effective_status != "connected" {
        return (
            effective_status.to_owned(),
            conn_mode.to_owned(),
            -100_000 - (tel.consecutive_failures as i64 * 300),
        );
    }

    let mut score: i64 = 10_000;
    score += match conn_mode {
        "local" => 2_000,
        "direct" => 1_500,
        "inbound" => 800,
        _ => 500,
    };

    let active_streams = tel.reported_active_streams.max(tel.core_active_relays) as i64;
    let active_scans = tel.reported_active_scans as i64;
    let latency_penalty = match tel.latency_ms {
        Some(ms) => (ms as i64).min(1_000),
        None => {
            if conn_mode == "local" {
                0
            } else {
                15
            }
        }
    };

    score -= active_streams * 120;
    score -= active_scans * 200;
    score -= latency_penalty;
    score -= (tel.consecutive_failures as i64) * 300;

    if let Some(m) = mime {
        if m == "video/mp4" || m == "video/webm" {
            score += 300;
        } else if m == "video/x-matroska" {
            score += 150;
        }
    }
    if let Some(h) = height {
        score += (h.min(4320) / 10) as i64;
    }

    (effective_status.to_owned(), conn_mode.to_owned(), score)
}

/// A Node record with the shared telemetry snapshot of the Core that receives its heartbeats.
#[derive(sqlx::FromRow)]
struct SharedNodeRow {
    #[sqlx(flatten)]
    record: MediaNodeRecord,
    health_sampled_at: Option<i64>,
    health_json: Option<String>,
}

/// Re-read every Node record and its `node_health` snapshot: one query, run only while other
/// Cores are live, at most every [`crate::coherence::SHARED_REFRESH`] (multi-Core §4.1, §4.4).
async fn load_shared_view(store: &IdentityStore) -> Result<(), sqlx::Error> {
    store
        .node_registry
        .shared_refreshes
        .fetch_add(1, Ordering::Relaxed);
    let since = store.node_registry.write_mark();
    let rows: Vec<SharedNodeRow> = crate::database::query_as(
        "SELECT n.id, n.name, n.kind, n.base_url, n.public_url, n.status, n.key_id, n.shared_secret, n.roots_json, n.paired_at, n.last_seen_at, n.revoked_at, n.version, n.connection_mode, h.sampled_at AS health_sampled_at, h.telemetry_json AS health_json FROM media_nodes n LEFT JOIN node_health h ON h.node_id=n.id",
    )
    .fetch_all(&store.pool)
    .await?;
    store.node_registry.merge_shared(rows, since);
    Ok(())
}

/// Re-read records and shared telemetry now, whatever the mode (multi-Core mode just left).
pub(crate) async fn reload_shared_view(store: &IdentityStore) -> Result<(), sqlx::Error> {
    if !store.node_registry.is_populated() {
        return Ok(());
    }
    store
        .node_registry
        .shared_refresh
        .due(std::time::Duration::ZERO);
    load_shared_view(store).await
}

/// Make the registry usable. Alone, it is loaded once and then kept up to date by this Core's
/// own writes and heartbeats: no query on the hot path. While other Cores are live (they write
/// statuses, keys and telemetry this Core never sees), one request at most every
/// [`crate::coherence::SHARED_REFRESH`] re-reads it; the others use the current copy.
pub async fn ensure_node_registry_warm(store: &IdentityStore) -> Result<(), sqlx::Error> {
    let shared = store.shared_with_other_cores();
    if store.node_registry.is_populated() {
        if shared
            && store
                .node_registry
                .shared_refresh
                .due(crate::coherence::SHARED_REFRESH)
            && let Err(error) = load_shared_view(store).await
        {
            // The current copy stays; signed Node calls still verify keys and status live.
            eprintln!("Node registry not refreshed from the database ({error})");
        }
        return Ok(());
    }
    if shared {
        store
            .node_registry
            .shared_refresh
            .due(std::time::Duration::ZERO);
        return load_shared_view(store).await;
    }
    let records: Vec<MediaNodeRecord> = crate::database::query_as(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode FROM media_nodes",
    )
    .fetch_all(&store.pool)
    .await?;
    store.node_registry.populate_records(records);
    Ok(())
}

pub async fn refresh_node_in_registry(
    store: &IdentityStore,
    node_id: &str,
) -> Result<Option<MediaNodeRecord>, sqlx::Error> {
    let record = crate::database::query_as::<_, MediaNodeRecord>(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode FROM media_nodes WHERE id=?",
    )
    .bind(node_id)
    .fetch_optional(&store.pool)
    .await?;
    if let Some(ref rec) = record {
        store.node_registry.upsert_record(rec.clone());
    }
    Ok(record)
}

pub async fn load_node(
    store: &IdentityStore,
    node_id: &str,
) -> Result<Option<MediaNodeRecord>, sqlx::Error> {
    if store.node_registry.is_populated() {
        if store.shared_with_other_cores() {
            ensure_node_registry_warm(store).await?;
        }
        if let Some(entry) = store.node_registry.get(node_id) {
            return Ok(Some(entry.record));
        }
    }
    refresh_node_in_registry(store, node_id).await
}

pub async fn load_node_entry(
    store: &IdentityStore,
    node_id: &str,
) -> Result<Option<NodeRuntimeEntry>, sqlx::Error> {
    ensure_node_registry_warm(store).await?;
    if let Some(entry) = store.node_registry.get(node_id) {
        return Ok(Some(entry));
    }
    let _ = refresh_node_in_registry(store, node_id).await?;
    Ok(store.node_registry.get(node_id))
}

pub async fn load_admin_nodes(store: &IdentityStore) -> Result<Vec<AdminNodeView>, sqlx::Error> {
    let records: Vec<MediaNodeRecord> = crate::database::query_as(
        "SELECT id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at, version, connection_mode FROM media_nodes ORDER BY CASE WHEN id='local' THEN 0 ELSE 1 END, name, id",
    )
    .fetch_all(&store.pool)
    .await?;
    store.node_registry.populate_records(records.clone());

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

    let asset_counts: Vec<(String, i64)> = crate::database::query_as(
        "SELECT l.node_id, COUNT(a.id) FROM inventory_assets a JOIN libraries l ON l.id = a.library_id WHERE a.available=1 AND l.enabled=1 GROUP BY l.node_id",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap_or_default();
    let asset_count_map: HashMap<String, usize> = asset_counts
        .into_iter()
        .map(|(nid, cnt)| (nid, cnt.max(0) as usize))
        .collect();
    let channel_owners: HashMap<String, (String, i64)> =
        crate::database::query_as::<_, (String, String, i64)>(
            "SELECT node_id, core_id, epoch FROM node_channel_owner",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(node, core, epoch)| (node, (core, epoch)))
        .collect();
    let pending_rotations: std::collections::HashSet<String> =
        crate::database::query_scalar::<_, String>(
            "SELECT id FROM media_nodes WHERE pending_key_id IS NOT NULL",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();

    let now = unix_now();
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
        let node_scans: Vec<NodeScanSummary> = scans
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

        let running_db_scans = node_scans
            .iter()
            .filter(|s| s.status == "running" || s.status == "queued")
            .count() as u32;

        let node_version = if rec.id == "local" {
            Some(media_contracts::version::current_version().to_string())
        } else {
            rec.version.clone()
        };

        let entry = store
            .node_registry
            .get(&rec.id)
            .unwrap_or_else(|| NodeRuntimeEntry {
                record: rec.clone(),
                telemetry: NodeRuntimeTelemetry::default(),
            });
        let (effective_status, conn_mode, selection_score) =
            score_node_entry(&entry, Some("video/mp4"), Some(1080), now);
        let active_scans = entry.telemetry.reported_active_scans.max(running_db_scans);
        let active_streams = entry
            .telemetry
            .reported_active_streams
            .max(entry.telemetry.core_active_relays);
        let db_assets = asset_count_map.get(&rec.id).copied().unwrap_or(0);
        let indexed_assets = entry.telemetry.indexed_assets.max(db_assets);
        let (channel_owner, channel_epoch) = channel_owners
            .get(&rec.id)
            .map(|(core, epoch)| (Some(core.clone()), Some(*epoch)))
            .unwrap_or_default();
        let pending_rotation = pending_rotations.contains(&rec.id);

        views.push(AdminNodeView {
            id: rec.id,
            name: rec.name,
            kind: rec.kind,
            base_url: rec.base_url,
            public_url: rec.public_url,
            status: effective_status,
            version: node_version,
            connection_mode: conn_mode,
            key_id: rec.key_id,
            roots,
            paired_at: rec.paired_at,
            last_seen_at: rec.last_seen_at,
            revoked_at: rec.revoked_at,
            latency_ms: entry.telemetry.latency_ms,
            active_scans,
            active_streams,
            indexed_assets,
            selection_score,
            libraries: node_libs,
            scans: node_scans,
            channel_owner,
            channel_epoch,
            pending_rotation,
        });
    }
    Ok(views)
}

#[cfg(test)]
mod staleness_tests {
    use super::*;

    fn record(kind: &str, id: &str, last_seen_at: Option<i64>) -> MediaNodeRecord {
        MediaNodeRecord {
            id: id.into(),
            name: id.into(),
            kind: kind.into(),
            base_url: None,
            public_url: None,
            status: "connected".into(),
            version: None,
            connection_mode: "direct".into(),
            key_id: "k".into(),
            shared_secret: "s".into(),
            roots_json: "[]".into(),
            paired_at: 0,
            last_seen_at,
            revoked_at: None,
        }
    }

    #[test]
    fn a_silent_remote_node_is_stale_after_five_missed_heartbeats_by_default() {
        // The default must hold unless an operator overrides it in the environment.
        if std::env::var_os("MEDIA_NODE_STALE_SECS").is_some() {
            return;
        }
        assert_eq!(node_stale_limit_secs(), DEFAULT_NODE_STALE_SECS);
        let now = 10_000;
        let remote = |seen| record("remote", "node-a", Some(seen));
        assert!(!node_is_stale(&remote(now - DEFAULT_NODE_STALE_SECS), now));
        assert!(node_is_stale(
            &remote(now - DEFAULT_NODE_STALE_SECS - 1),
            now
        ));
    }

    #[test]
    fn local_nodes_and_nodes_without_any_history_are_never_stale() {
        let now = 10_000;
        assert!(!node_is_stale(&record("local", "local", Some(0)), now));
        assert!(!node_is_stale(&record("remote", "local", Some(0)), now));
        assert!(!node_is_stale(&record("remote", "node-b", None), now));
    }

    #[test]
    fn a_stale_node_scores_as_disconnected_and_is_never_handed_out() {
        if std::env::var_os("MEDIA_NODE_STALE_SECS").is_some() {
            return;
        }
        let now = 10_000;
        let entry = NodeRuntimeEntry {
            record: record("remote", "node-c", Some(now - 3_600)),
            telemetry: NodeRuntimeTelemetry::default(),
        };
        let (status, _, score) = score_node_entry(&entry, Some("video/mp4"), None, now);
        assert_eq!(status, "disconnected");
        assert!(score < 0);
        let fresh = NodeRuntimeEntry {
            record: record("remote", "node-c", Some(now - 5)),
            telemetry: NodeRuntimeTelemetry::default(),
        };
        assert_eq!(
            score_node_entry(&fresh, Some("video/mp4"), None, now).0,
            "connected"
        );
    }
}

#[cfg(test)]
mod shared_view_tests {
    use super::*;

    fn record(status: &str, key: &str) -> MediaNodeRecord {
        MediaNodeRecord {
            id: "nas".into(),
            name: "NAS".into(),
            kind: "remote".into(),
            base_url: None,
            public_url: None,
            status: status.into(),
            version: None,
            connection_mode: "outbound".into(),
            key_id: key.into(),
            shared_secret: "secret".into(),
            roots_json: "[]".into(),
            paired_at: 1,
            last_seen_at: Some(10),
            revoked_at: None,
        }
    }

    fn row(status: &str, key: &str) -> SharedNodeRow {
        SharedNodeRow {
            record: record(status, key),
            health_sampled_at: None,
            health_json: None,
        }
    }

    #[test]
    fn a_shared_reread_never_overwrites_a_newer_local_write() {
        let registry = NodeRuntimeRegistry::new();
        registry.upsert_record(record("connected", "k1"));
        // A re-read starts (its SELECT sees the database before the revocation)...
        let since = registry.write_mark();
        // ...this Core revokes the Node meanwhile...
        registry.upsert_record(record("revoked", "k1"));
        // ...then the older rows are merged: the revocation stays.
        registry.merge_shared(vec![row("connected", "k1")], since);
        assert_eq!(registry.get("nas").unwrap().record.status, "revoked");
        // A re-read that started after the local write applies the database's state.
        let since = registry.write_mark();
        registry.merge_shared(vec![row("connected", "k2")], since);
        let entry = registry.get("nas").unwrap();
        assert_eq!(
            (entry.record.status.as_str(), entry.record.key_id.as_str()),
            ("connected", "k2")
        );
        // A Node written locally after the read started is not dropped for missing from it.
        let since = registry.write_mark();
        let mut other = record("connected", "k1");
        other.id = "new".into();
        registry.upsert_record(other);
        registry.merge_shared(vec![row("connected", "k2")], since);
        assert!(registry.get("new").is_some());
    }
}
