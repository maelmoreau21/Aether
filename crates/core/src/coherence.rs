//! Process-local caches when several Cores share one database (multi-Core roadmap §4, step 6).
//!
//! Every cache keeps its single-Core fast path. What changes with other live Cores:
//!
//! | Cache | Class | Coherence across Cores |
//! |---|---|---|
//! | sessions, disabled users (`auth::state`) | security | none needed: read live on every request; `touch` only defers `last_seen_at` |
//! | `PERMISSIONS_CACHE` (`permissions`) | security | entries keyed by a live fingerprint of every permission table, read on every request |
//! | `CATALOGUE_CACHE` (`inventory::catalogue::cache`) | correctness | keyed by a live fingerprint that includes `inventory_cache_generation`, read on every request; PostgreSQL writers move that generation ([`crate::inventory::catalogue_changed`], [`crate::inventory::bump_shared_generation`]) |
//! | `IMAGE_META_CACHE` (`inventory::resources`) | correctness | same shared generation, read on every request |
//! | `ARTWORK_SHA_CACHE` (`artwork`) | correctness | bypassed while other Cores are live |
//! | `IdentityStore::cors_origins` | security | re-read at most every [`SHARED_REFRESH`] while other Cores are live |
//! | `NodeRuntimeRegistry` records (status, keys, `last_seen_at`) | security | re-read at most every [`SHARED_REFRESH`] while other Cores are live; signed Node calls read keys and status live |
//! | `NodeRuntimeRegistry` telemetry | best effort | `node_health` snapshot written by the Core receiving the heartbeats, read with the records |
//!
//! "Other live Cores" is [`Coherence::shared`]: refreshed by every heartbeat
//! ([`crate::core_instances::heartbeat`], every 10 s, PostgreSQL only). Alone (SQLite always,
//! PostgreSQL with one live Core) nothing above costs a database read.
//!
//! Only the rows marked "while other Cores are live" depend on that mode; sessions, users,
//! permissions, the catalogue and signed Node calls are validated live on every request in both
//! modes. Transition: a starting Core counts the running ones at its first heartbeat, and they
//! count it at their next one, within [`crate::core_instances::HEARTBEAT_INTERVAL`]. A Node
//! record, a CORS origin or an artwork the new Core changes before then reaches them within
//! that interval plus [`SHARED_REFRESH`] (12 s) instead of [`SHARED_REFRESH`].
use std::{
    sync::{
        LazyLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Longest a shared-mode cache (node records and telemetry, CORS origins) trusts its copy.
pub const SHARED_REFRESH: Duration = Duration::from_secs(2);

/// Multi-Core state of one opened database, shared by every clone of the store.
#[derive(Default)]
pub struct Coherence {
    shared: AtomicBool,
    catalogue_epoch: AtomicU64,
    /// Gates the re-read of `server_settings.cors_hosts` while other Cores are live.
    pub(crate) cors_refresh: RefreshGate,
}

impl Coherence {
    pub fn new() -> Self {
        Self {
            catalogue_epoch: AtomicU64::new(1),
            ..Self::default()
        }
    }

    /// Whether another live Core shares this database (as of the last heartbeat).
    pub fn shared(&self) -> bool {
        self.shared.load(Ordering::Relaxed)
    }

    pub(crate) fn set_shared(&self, shared: bool) {
        self.shared.store(shared, Ordering::Relaxed);
    }

    /// In-process catalogue epoch of this database, mixed into its cache fingerprints.
    pub(crate) fn catalogue_epoch(&self) -> u64 {
        self.catalogue_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn bump_catalogue_epoch(&self) {
        self.catalogue_epoch.fetch_add(1, Ordering::AcqRel);
    }
}

/// The other Cores are gone. The copies this Core kept or refreshed while they were live may
/// predate their last writes, and alone nothing re-reads them: re-read the Node registry and the
/// CORS origins, forget cached artwork. Runs once, in the heartbeat, never on a request.
pub(crate) async fn left_shared_mode(store: &crate::identity::IdentityStore) {
    crate::artwork::forget_artwork_scope(store.cache_scope);
    store.node_registry.forget_health_publications();
    crate::server_settings::reload_cors_origins(store).await;
    if let Err(error) = crate::nodes::reload_shared_view(store).await {
        // Signed Node calls still verify keys and status live.
        eprintln!("Node registry not re-read after the other Cores left ({error})");
    }
}

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Elects one caller per interval to refresh a cache; the others keep the current copy and
/// never wait.
#[derive(Default)]
pub struct RefreshGate {
    /// Milliseconds since [`START`] of the last election, plus one (0: never).
    last_ms: AtomicU64,
}

impl RefreshGate {
    /// `true` for exactly one caller once `interval` has elapsed since the previous election.
    pub fn due(&self, interval: Duration) -> bool {
        let now = START.elapsed().as_millis() as u64 + 1;
        let last = self.last_ms.load(Ordering::Acquire);
        if last != 0 && now.saturating_sub(last) < interval.as_millis() as u64 {
            return false;
        }
        self.last_ms
            .compare_exchange(last, now, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The next [`RefreshGate::due`] elects its caller whatever the interval.
    pub fn expire(&self) {
        self.last_ms.store(0, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "coherence_tests.rs"]
mod cross_core_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gate_elects_one_caller_per_interval() {
        let gate = RefreshGate::default();
        assert!(gate.due(Duration::from_secs(60)), "never refreshed: due");
        assert!(!gate.due(Duration::from_secs(60)), "just refreshed");
        assert!(gate.due(Duration::ZERO), "interval elapsed");
        gate.expire();
        assert!(gate.due(Duration::from_secs(60)), "expired: due again");
    }

    #[test]
    fn a_new_store_state_is_alone() {
        let coherence = Coherence::new();
        assert!(!coherence.shared());
        let epoch = coherence.catalogue_epoch();
        coherence.bump_catalogue_epoch();
        assert_eq!(coherence.catalogue_epoch(), epoch + 1);
    }
}
