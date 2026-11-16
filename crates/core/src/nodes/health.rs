//! Node telemetry shared between Cores (multi-Core roadmap §4.1).
//!
//! Only the Core that receives a Node's heartbeats (the owner of its command channel, or the Core
//! probing an inbound Node) knows its load. While other Cores are live it writes a snapshot to
//! `node_health`: at most once per [`HEALTH_MIN_INTERVAL`] per Node, and only when a value that
//! ranking or dispatching reads has changed. The other Cores read it together with the Node
//! records when they refresh their registry ([`super::registry`], at most every
//! [`crate::coherence::SHARED_REFRESH`]), never on the hot path.
//!
//! Alone, a Core writes nothing: a Core taking a Node over gets its first heartbeat within one
//! heartbeat period, and nothing else reads the snapshot.
use super::registry::NodeRuntimeTelemetry;
use crate::identity::IdentityStore;
use media_contracts::protocol::ResourcePressure;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Shortest interval between two snapshots of one Node by one Core (Nodes beat every 15 s).
pub const HEALTH_MIN_INTERVAL: Duration = Duration::from_secs(14);

/// Telemetry of one Node as written to `node_health.telemetry_json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcePressure>,
    #[serde(default)]
    pub active_scan_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    pub active_scans: u32,
    #[serde(default)]
    pub active_streams: u32,
    #[serde(default)]
    pub indexed_assets: usize,
    #[serde(default)]
    pub consecutive_failures: u32,
}

impl HealthSnapshot {
    pub(crate) fn of(telemetry: &NodeRuntimeTelemetry) -> Self {
        Self {
            resources: telemetry.resources.clone(),
            active_scan_ids: telemetry.active_scan_ids.clone(),
            latency_ms: telemetry.latency_ms,
            active_scans: telemetry.reported_active_scans,
            active_streams: telemetry.reported_active_streams,
            indexed_assets: telemetry.indexed_assets,
            consecutive_failures: telemetry.consecutive_failures,
        }
    }

    /// Copy into `telemetry`, sampled at `sampled_at` (the relay count stays this Core's own).
    pub(crate) fn apply(self, telemetry: &mut NodeRuntimeTelemetry, sampled_at: i64) {
        telemetry.resources = self.resources;
        telemetry.active_scan_ids = self.active_scan_ids;
        telemetry.latency_ms = self.latency_ms;
        telemetry.reported_active_scans = self.active_scans;
        telemetry.reported_active_streams = self.active_streams;
        telemetry.indexed_assets = self.indexed_assets;
        telemetry.consecutive_failures = self.consecutive_failures;
        telemetry.sampled_at = sampled_at;
    }

    fn pressure_blocks(&self) -> bool {
        self.resources
            .as_ref()
            .is_some_and(|r| crate::scheduler::resource_pressure_blocks(r, r.sampled_at))
    }

    /// Whether another Core would rank or dispatch differently with `next` than with `self`.
    /// A blocking resource pressure always counts: dispatchers ignore samples older than 30 s.
    pub(crate) fn significant_change(&self, next: &Self) -> bool {
        let latency_moved = match (self.latency_ms, next.latency_ms) {
            (Some(a), Some(b)) => a.abs_diff(b) > 10 && a.abs_diff(b) * 4 > a.max(b),
            (a, b) => a.is_some() != b.is_some(),
        };
        self.active_scans != next.active_scans
            || self.active_streams != next.active_streams
            || self.active_scan_ids != next.active_scan_ids
            || self.indexed_assets != next.indexed_assets
            || (self.consecutive_failures == 0) != (next.consecutive_failures == 0)
            || latency_moved
            || self.pressure_blocks() != next.pressure_blocks()
            || next.pressure_blocks()
    }
}

/// After this Core recorded a heartbeat of `node_id`: share its telemetry with the other Cores
/// when they exist and it changed significantly. Best effort; errors are logged.
pub(crate) async fn publish(store: &IdentityStore, node_id: &str, now: i64) {
    if !store.shared_with_other_cores() || node_id == "local" {
        return;
    }
    let Some(entry) = store.node_registry.get(node_id) else {
        return;
    };
    let snapshot = HealthSnapshot::of(&entry.telemetry);
    if !store
        .node_registry
        .claim_health_publication(node_id, &snapshot)
    {
        return;
    }
    let Ok(json) = serde_json::to_string(&snapshot) else {
        return;
    };
    // A snapshot of an older channel owner never replaces a fresh one of the current owner.
    let written = crate::database::query(
        "INSERT INTO node_health(node_id,core_id,epoch,sampled_at,telemetry_json) \
         VALUES(?,?,COALESCE((SELECT o.epoch FROM node_channel_owner o WHERE o.node_id=? AND o.core_id=?),0),?,?) \
         ON CONFLICT(node_id) DO UPDATE SET core_id=excluded.core_id,epoch=excluded.epoch,\
         sampled_at=excluded.sampled_at,telemetry_json=excluded.telemetry_json \
         WHERE excluded.epoch>=node_health.epoch OR node_health.sampled_at<excluded.sampled_at-60",
    )
    .bind(node_id)
    .bind(store.instance_id())
    .bind(node_id)
    .bind(store.instance_id())
    .bind(now)
    .bind(json)
    .execute(store.pool())
    .await;
    if let Err(error) = written {
        // Retried at the next heartbeat.
        store.node_registry.forget_health_publication(node_id);
        eprintln!("Node {node_id} health not shared with the other Cores ({error})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> HealthSnapshot {
        HealthSnapshot {
            latency_ms: Some(40),
            active_streams: 1,
            ..HealthSnapshot::default()
        }
    }

    #[test]
    fn only_changes_that_affect_ranking_or_dispatch_are_significant() {
        let base = snapshot();
        assert!(!base.significant_change(&base.clone()));
        let jitter = HealthSnapshot {
            latency_ms: Some(46),
            ..snapshot()
        };
        assert!(!base.significant_change(&jitter), "latency jitter");
        let slower = HealthSnapshot {
            latency_ms: Some(90),
            ..snapshot()
        };
        assert!(base.significant_change(&slower));
        let busier = HealthSnapshot {
            active_streams: 2,
            ..snapshot()
        };
        assert!(base.significant_change(&busier));
        let failing = HealthSnapshot {
            consecutive_failures: 1,
            ..snapshot()
        };
        assert!(base.significant_change(&failing));
        let overloaded = HealthSnapshot {
            resources: Some(ResourcePressure {
                sampled_at: 100,
                cpu_usage_percent: Some(95),
                ..ResourcePressure::default()
            }),
            ..snapshot()
        };
        assert!(base.significant_change(&overloaded));
        assert!(
            overloaded.significant_change(&overloaded.clone()),
            "a blocking pressure is kept fresh"
        );
    }

    #[test]
    fn a_snapshot_round_trips_through_its_json() {
        let original = HealthSnapshot {
            active_scan_ids: vec!["scan:1".into()],
            indexed_assets: 12,
            ..snapshot()
        };
        let mut telemetry = NodeRuntimeTelemetry {
            core_active_relays: 3,
            ..NodeRuntimeTelemetry::default()
        };
        let decoded: HealthSnapshot =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        decoded.apply(&mut telemetry, 77);
        assert_eq!(HealthSnapshot::of(&telemetry), original);
        assert_eq!(telemetry.sampled_at, 77);
        assert_eq!(telemetry.core_active_relays, 3, "relays stay local");
    }
}
