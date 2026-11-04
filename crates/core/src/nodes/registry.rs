//! Node records, warm runtime telemetry, selection scores and registry projections.
use crate::identity::IdentityStore;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::unix_now;
use serde::Serialize;
use sqlx::Row;
use std::collections::HashMap;

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
}

impl NodeRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            entries: std::sync::RwLock::new(HashMap::new()),
            populated: std::sync::atomic::AtomicBool::new(false),
        }
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

    let stale_limit = std::env::var("MEDIA_NODE_STALE_SECS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let is_stale = stale_limit > 0
        && rec.id != "local"
        && rec.kind == "remote"
        && rec
            .last_seen_at
            .is_some_and(|seen| now.saturating_sub(seen) > stale_limit);

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

pub async fn ensure_node_registry_warm(store: &IdentityStore) -> Result<(), sqlx::Error> {
    if store.node_registry.is_populated() {
        return Ok(());
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
    if store.node_registry.is_populated()
        && let Some(entry) = store.node_registry.get(node_id)
    {
        return Ok(Some(entry.record));
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
        });
    }
    Ok(views)
}
