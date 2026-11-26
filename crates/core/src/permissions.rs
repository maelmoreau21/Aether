//! Live, fail-closed library permissions before every media request.
use crate::{auth::AuthState, database::query, error, identity::IdentityStore, identity::Role};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sqlx::Row;
use std::{
    collections::{HashMap, VecDeque},
    hash::Hash,
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;

const VISIBLE_LIBS_CAP: usize = 2_048;
const RESOURCE_AUTH_CAP: usize = 32_768;

static NEXT_FLIGHT_ID: AtomicU64 = AtomicU64::new(1);
static PERMISSIONS_EPOCH: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
pub(crate) static CACHE_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PermissionsCacheStats {
    pub visible_entries: usize,
    pub resource_entries: usize,
    pub fingerprint_queries: u64,
    pub visible_queries: u64,
    pub resource_queries: u64,
    pub visible_hits: u64,
    pub resource_hits: u64,
    pub coalesced_waiters: u64,
}

#[derive(Clone)]
pub struct MediaAccess {
    pub auth: AuthState,
    pub root: PathBuf,
}
#[derive(Clone)]
pub struct VisibleLibraries(pub Vec<String>);
// Retained for callers describing the role default; actual rights also require an active library.
pub fn can_read_fixture_library(role: Role) -> bool {
    matches!(role, Role::Administrator)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct VisibleCacheKey {
    cache_scope: u64,
    root: String,
    user_id: String,
    is_admin: bool,
}

#[derive(Clone)]
struct CachedVisible {
    perm_fp: i64,
    visible: Arc<Vec<String>>,
    has_parental: bool,
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum RouteTarget {
    CatalogRoot,
    CatalogWork(String),
    Media(String),
    Asset {
        kind: &'static str,
        lib_id: String,
        parental_id: String,
    },
    DemoFixture {
        require_series_or_films: bool,
        parental_kind: &'static str,
        parental_id: String,
    },
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ResourceCacheKey {
    cache_scope: u64,
    root: String,
    user_id: String,
    is_admin: bool,
    target: RouteTarget,
}

#[derive(Clone, Copy)]
struct CachedDecision {
    perm_fp: i64,
    allowed: bool,
    parental_allowed: bool,
}

#[derive(Clone, Default)]
struct ScopeMetrics {
    fingerprint_queries: u64,
    visible_queries: u64,
    resource_queries: u64,
    visible_hits: u64,
    resource_hits: u64,
    coalesced_waiters: u64,
}

struct LruEntry<V> {
    seq: u64,
    value: V,
}

struct PermLru<K, V> {
    entries: HashMap<K, LruEntry<V>>,
    order: VecDeque<(K, u64)>,
    next_seq: u64,
    cap: usize,
}

impl<K: Clone + Eq + Hash, V: Clone> PermLru<K, V> {
    fn new(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            next_seq: 1,
            cap,
        }
    }

    fn bump_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1).max(1);
        seq
    }

    fn compact_if_needed(&mut self) {
        if self.order.len() <= self.entries.len().saturating_mul(4).saturating_add(64) {
            return;
        }
        let mut rebuilt = VecDeque::with_capacity(self.entries.len());
        let mut items: Vec<(u64, K)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.seq, key.clone()))
            .collect();
        items.sort_unstable_by_key(|(seq, _)| *seq);
        for (seq, key) in items {
            rebuilt.push_back((key, seq));
        }
        self.order = rebuilt;
    }

    fn evict_one(&mut self) -> bool {
        while let Some((candidate, seq)) = self.order.pop_front() {
            let matches = self
                .entries
                .get(&candidate)
                .is_some_and(|entry| entry.seq == seq);
            if matches {
                self.entries.remove(&candidate);
                return true;
            }
        }
        if let Some(fallback) = self.entries.keys().next().cloned() {
            self.entries.remove(&fallback);
            return true;
        }
        false
    }

    fn get(&mut self, key: &K) -> Option<V> {
        if !self.entries.contains_key(key) {
            return None;
        }
        let seq = self.bump_seq();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.seq = seq;
        }
        self.order.push_back((key.clone(), seq));
        self.compact_if_needed();
        self.entries.get(key).map(|entry| entry.value.clone())
    }

    fn insert(&mut self, key: K, value: V) {
        let seq = self.bump_seq();
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= self.cap && self.evict_one() {}
        }
        self.entries.insert(key.clone(), LruEntry { seq, value });
        self.order.push_back((key, seq));
        self.compact_if_needed();
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

struct PermissionsCache {
    visible_libs: PermLru<VisibleCacheKey, CachedVisible>,
    resource_auth: PermLru<ResourceCacheKey, CachedDecision>,
    metrics: HashMap<u64, ScopeMetrics>,
    totals: ScopeMetrics,
}

impl PermissionsCache {
    fn new() -> Self {
        Self {
            visible_libs: PermLru::new(VISIBLE_LIBS_CAP),
            resource_auth: PermLru::new(RESOURCE_AUTH_CAP),
            metrics: HashMap::new(),
            totals: ScopeMetrics::default(),
        }
    }

    fn record(&mut self, cache_scope: u64, mut update: impl FnMut(&mut ScopeMetrics)) {
        update(&mut self.totals);
        update(self.metrics.entry(cache_scope).or_default());
    }
}

static PERMISSIONS_CACHE: LazyLock<Mutex<PermissionsCache>> =
    LazyLock::new(|| Mutex::new(PermissionsCache::new()));

enum FlightSignal<T, E> {
    Ready(T),
    Err(E),
    Cancelled,
}

struct ActiveFlight<T, E> {
    id: u64,
    waiters: Vec<oneshot::Sender<FlightSignal<T, E>>>,
}

type FlightMap<K, T, E> = Mutex<HashMap<K, ActiveFlight<T, E>>>;
type FlightErr = (StatusCode, &'static str);
type VisibleFlightKey = (VisibleCacheKey, i64);
type ResourceFlightKey = (ResourceCacheKey, i64);

static FP_FLIGHTS: LazyLock<FlightMap<u64, i64, StatusCode>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static VIS_FLIGHTS: LazyLock<FlightMap<VisibleFlightKey, CachedVisible, FlightErr>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RES_FLIGHTS: LazyLock<FlightMap<ResourceFlightKey, CachedDecision, FlightErr>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct FlightGuard<'a, K: Eq + Hash, T, E> {
    map: &'a FlightMap<K, T, E>,
    key: Option<K>,
    id: u64,
}

impl<K: Eq + Hash, T, E> Drop for FlightGuard<'_, K, T, E> {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else {
            return;
        };
        let waiters = {
            let mut flights = self.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = flights.get(&key)
                && entry.id == self.id
            {
                flights.remove(&key).map(|entry| entry.waiters)
            } else {
                None
            }
        };
        if let Some(waiters) = waiters {
            for tx in waiters {
                let _ = tx.send(FlightSignal::Cancelled);
            }
        }
    }
}

fn record_coalesced_waiter(cache_scope: u64) {
    let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.record(cache_scope, |m| m.coalesced_waiters += 1);
}

async fn coalesced<K, T, E, F, Fut>(
    cache_scope: u64,
    map: &'static FlightMap<K, T, E>,
    key: K,
    mut compute: F,
) -> Result<T, E>
where
    K: Eq + Hash + Clone,
    T: Clone,
    E: Clone,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    for _ in 0..8 {
        enum FlightRole<T, E> {
            Leader(u64),
            Waiter(oneshot::Receiver<FlightSignal<T, E>>),
        }
        let role = {
            let mut flights = map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(flight) = flights.get_mut(&key) {
                let (tx, rx) = oneshot::channel();
                flight.waiters.push(tx);
                FlightRole::Waiter(rx)
            } else {
                let id = NEXT_FLIGHT_ID.fetch_add(1, Ordering::Relaxed);
                flights.insert(
                    key.clone(),
                    ActiveFlight {
                        id,
                        waiters: Vec::new(),
                    },
                );
                FlightRole::Leader(id)
            }
        };
        match role {
            FlightRole::Waiter(rx) => {
                record_coalesced_waiter(cache_scope);
                match rx.await {
                    Ok(FlightSignal::Ready(value)) => return Ok(value),
                    Ok(FlightSignal::Err(err)) => return Err(err),
                    Ok(FlightSignal::Cancelled) | Err(_) => continue,
                }
            }
            FlightRole::Leader(id) => {
                let mut guard = FlightGuard {
                    map,
                    key: Some(key.clone()),
                    id,
                };
                let result = compute().await;
                guard.key = None;
                let waiters = {
                    let mut flights = map.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = flights.get(&key)
                        && entry.id == id
                    {
                        flights.remove(&key).map(|entry| entry.waiters)
                    } else {
                        None
                    }
                };
                match result {
                    Ok(value) => {
                        if let Some(waiters) = waiters {
                            for tx in waiters {
                                let _ = tx.send(FlightSignal::Ready(value.clone()));
                            }
                        }
                        return Ok(value);
                    }
                    Err(err) => {
                        if let Some(waiters) = waiters {
                            for tx in waiters {
                                let _ = tx.send(FlightSignal::Err(err.clone()));
                            }
                        }
                        return Err(err);
                    }
                }
            }
        }
    }
    compute().await
}

fn cancel_flights<K: Eq + Hash, T, E>(map: &FlightMap<K, T, E>) {
    let drained: Vec<Vec<oneshot::Sender<FlightSignal<T, E>>>> = {
        let mut flights = map.lock().unwrap_or_else(|e| e.into_inner());
        flights.drain().map(|(_, flight)| flight.waiters).collect()
    };
    for waiters in drained {
        for tx in waiters {
            let _ = tx.send(FlightSignal::Cancelled);
        }
    }
}

pub fn invalidate_permissions_cache() {
    PERMISSIONS_EPOCH.fetch_add(1, Ordering::AcqRel);
    {
        let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.visible_libs.clear();
        cache.resource_auth.clear();
    }
    cancel_flights(&FP_FLIGHTS);
    cancel_flights(&VIS_FLIGHTS);
    cancel_flights(&RES_FLIGHTS);
}

pub fn permissions_cache_stats() -> PermissionsCacheStats {
    let cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    PermissionsCacheStats {
        visible_entries: cache.visible_libs.entries.len(),
        resource_entries: cache.resource_auth.entries.len(),
        fingerprint_queries: cache.totals.fingerprint_queries,
        visible_queries: cache.totals.visible_queries,
        resource_queries: cache.totals.resource_queries,
        visible_hits: cache.totals.visible_hits,
        resource_hits: cache.totals.resource_hits,
        coalesced_waiters: cache.totals.coalesced_waiters,
    }
}

#[cfg(test)]
fn scope_permissions_cache_stats(cache_scope: u64) -> PermissionsCacheStats {
    let cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let m = cache.metrics.get(&cache_scope).cloned().unwrap_or_default();
    PermissionsCacheStats {
        visible_entries: cache
            .visible_libs
            .entries
            .keys()
            .filter(|k| k.cache_scope == cache_scope)
            .count(),
        resource_entries: cache
            .resource_auth
            .entries
            .keys()
            .filter(|k| k.cache_scope == cache_scope)
            .count(),
        fingerprint_queries: m.fingerprint_queries,
        visible_queries: m.visible_queries,
        resource_queries: m.resource_queries,
        visible_hits: m.visible_hits,
        resource_hits: m.resource_hits,
        coalesced_waiters: m.coalesced_waiters,
    }
}

#[inline]
fn mix_u64(state: &mut u64, value: u64) {
    let mut x = *state ^ value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    *state = x ^ (x >> 31);
}

#[inline]
fn mix_bytes(state: &mut u64, bytes: &[u8]) {
    mix_u64(state, bytes.len() as u64);
    let (chunks, rem) = bytes.as_chunks::<8>();
    for chunk in chunks {
        mix_u64(state, u64::from_le_bytes(*chunk));
    }
    if !rem.is_empty() {
        let mut buf = [0u8; 8];
        buf[..rem.len()].copy_from_slice(rem);
        mix_u64(state, u64::from_le_bytes(buf));
    }
}

/// Computes an exact live fingerprint of every database table influencing library
/// visibility, resource ownership or parental restrictions in a single join-free query.
pub async fn permission_fingerprint(store: &IdentityStore) -> Result<i64, sqlx::Error> {
    let epoch = PERMISSIONS_EPOCH.load(Ordering::Acquire);
    {
        let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        cache.record(store.cache_scope, |m| m.fingerprint_queries += 1);
    }
    const INT_COLS: usize = 34;
    const TOTAL_COLS: usize = 65;
    let row = query(
        "SELECT
           CAST(COALESCE((SELECT gen FROM inventory_cache_generation WHERE id = 1), 0) AS BIGINT),
           (SELECT COUNT(*) FROM libraries),
           CAST(COALESCE((SELECT SUM(enabled * 13 + length(id) * 3 + length(root) * 5 + length(node_id) * 7 + length(kind) * 11 + CASE WHEN COALESCE(options_json, '') LIKE '%adminAccess__false%' THEN 29 ELSE 0 END) FROM libraries), 0) AS BIGINT),
           (SELECT COUNT(*) FROM media_nodes),
           CAST(COALESCE((SELECT SUM((CASE status WHEN 'revoked' THEN 7 WHEN 'connected' THEN 1 ELSE 3 END) * (1 + length(id))) FROM media_nodes), 0) AS BIGINT),
           (SELECT COUNT(*) FROM library_folders),
           CAST(COALESCE((SELECT SUM(active * 17 + length(library_id) * 3 + length(parent_id) * 5) FROM library_folders), 0) AS BIGINT),
           (SELECT COUNT(*) FROM library_permissions),
           CAST(COALESCE((SELECT SUM(length(library_id) * 7 + length(user_id) * 13) FROM library_permissions), 0) AS BIGINT),
           (SELECT COUNT(*) FROM user_groups),
           CAST(COALESCE((SELECT SUM(is_default * 19 + length(id) * 5) FROM user_groups), 0) AS BIGINT),
           (SELECT COUNT(*) FROM user_group_members),
           CAST(COALESCE((SELECT SUM(length(group_id) * 11 + length(user_id) * 17) FROM user_group_members), 0) AS BIGINT),
           (SELECT COUNT(*) FROM user_group_libraries),
           CAST(COALESCE((SELECT SUM(length(group_id) * 23 + length(library_id) * 29) FROM user_group_libraries), 0) AS BIGINT),
           (SELECT COUNT(*) FROM account_profiles),
           CAST(COALESCE((SELECT SUM(length(user_id) * 31 + length(owner_id) * 37) FROM account_profiles), 0) AS BIGINT),
           (SELECT COUNT(*) FROM user_media_kind_denials),
           CAST(COALESCE((SELECT SUM(length(user_id) * 41 + length(kind) * 43) FROM user_media_kind_denials), 0) AS BIGINT),
           (SELECT COUNT(*) FROM group_media_kind_denials),
           CAST(COALESCE((SELECT SUM(length(group_id) * 47 + length(kind) * 53) FROM group_media_kind_denials), 0) AS BIGINT),
           (SELECT COUNT(*) FROM parental_policies),
           CAST(COALESCE((SELECT SUM(maximum_age * 7 + allow_unrated * 3 + length(user_id) * 59 + length(blocked_tags_json) * 19) FROM parental_policies), 0) AS BIGINT),
           (SELECT COUNT(*) FROM parental_blocked_tags),
           CAST(COALESCE((SELECT SUM(length(user_id) * 61 + length(tag_json) * 67) FROM parental_blocked_tags), 0) AS BIGINT),
           (SELECT COUNT(*) FROM users),
           CAST(COALESCE((SELECT SUM((CASE role WHEN 'administrator' THEN 2 ELSE 1 END) * (1 + length(id))) FROM users), 0) AS BIGINT),
           (SELECT COUNT(*) FROM inventory_works),
           (SELECT COUNT(*) FROM inventory_assets),
           (SELECT COUNT(*) FROM inventory_images),
           (SELECT COUNT(*) FROM inventory_sources),
           (SELECT COUNT(*) FROM media_group_members),
           CAST(COALESCE((SELECT SUM(length(root_snapshot) * 3 + length(library_id) * 5) FROM inventory_sources), 0) AS BIGINT),
           CAST(COALESCE((SELECT SUM(length(work_id) * 13 + length(group_id) * 17) FROM media_group_members), 0) AS BIGINT),
           COALESCE((SELECT MIN(id || ':' || CAST(enabled AS TEXT) || ':' || root || ':' || node_id || ':' || kind || ':' || COALESCE(options_json, '')) FROM libraries), ''),
           COALESCE((SELECT MAX(id || ':' || CAST(enabled AS TEXT) || ':' || root || ':' || node_id || ':' || kind || ':' || COALESCE(options_json, '')) FROM libraries), ''),
           COALESCE((SELECT MIN(id || ':' || status) FROM media_nodes), ''),
           COALESCE((SELECT MAX(id || ':' || status) FROM media_nodes), ''),
           COALESCE((SELECT MIN(library_id || ':' || parent_id || ':' || CAST(active AS TEXT)) FROM library_folders), ''),
           COALESCE((SELECT MAX(library_id || ':' || parent_id || ':' || CAST(active AS TEXT)) FROM library_folders), ''),
           COALESCE((SELECT MIN(library_id || ':' || user_id) FROM library_permissions), ''),
           COALESCE((SELECT MAX(library_id || ':' || user_id) FROM library_permissions), ''),
           COALESCE((SELECT MIN(id || ':' || CAST(is_default AS TEXT)) FROM user_groups), ''),
           COALESCE((SELECT MAX(id || ':' || CAST(is_default AS TEXT)) FROM user_groups), ''),
           COALESCE((SELECT MIN(group_id || ':' || user_id) FROM user_group_members), ''),
           COALESCE((SELECT MAX(group_id || ':' || user_id) FROM user_group_members), ''),
           COALESCE((SELECT MIN(group_id || ':' || library_id) FROM user_group_libraries), ''),
           COALESCE((SELECT MAX(group_id || ':' || library_id) FROM user_group_libraries), ''),
           COALESCE((SELECT MIN(user_id || ':' || owner_id) FROM account_profiles), ''),
           COALESCE((SELECT MAX(user_id || ':' || owner_id) FROM account_profiles), ''),
           COALESCE((SELECT MIN(user_id || ':' || kind) FROM user_media_kind_denials), ''),
           COALESCE((SELECT MAX(user_id || ':' || kind) FROM user_media_kind_denials), ''),
           COALESCE((SELECT MIN(group_id || ':' || kind) FROM group_media_kind_denials), ''),
           COALESCE((SELECT MAX(group_id || ':' || kind) FROM group_media_kind_denials), ''),
           COALESCE((SELECT MIN(user_id || ':' || CAST(maximum_age AS TEXT) || ':' || CAST(allow_unrated AS TEXT) || ':' || blocked_tags_json) FROM parental_policies), ''),
           COALESCE((SELECT MAX(user_id || ':' || CAST(maximum_age AS TEXT) || ':' || CAST(allow_unrated AS TEXT) || ':' || blocked_tags_json) FROM parental_policies), ''),
           COALESCE((SELECT MIN(user_id || ':' || tag_json) FROM parental_blocked_tags), ''),
           COALESCE((SELECT MAX(user_id || ':' || tag_json) FROM parental_blocked_tags), ''),
           COALESCE((SELECT MIN(id || ':' || role) FROM users), ''),
           COALESCE((SELECT MAX(id || ':' || role) FROM users), ''),
           COALESCE((SELECT MIN(work_id || ':' || group_id) FROM media_group_members), ''),
           COALESCE((SELECT MAX(work_id || ':' || group_id) FROM media_group_members), ''),
           COALESCE((SELECT MAX(id) FROM inventory_works), ''),
           COALESCE((SELECT MAX(id) FROM inventory_assets), ''),
           COALESCE((SELECT MAX(id) FROM inventory_images), '')",
    )
    .fetch_one(&store.pool)
    .await?;

    let mut hash: u64 = 0x517c_c1b7_2722_0a95;
    mix_u64(&mut hash, epoch);
    for idx in 0..INT_COLS {
        let value = row.try_get::<i64, usize>(idx)?;
        mix_u64(&mut hash, value as u64);
    }
    for idx in INT_COLS..TOTAL_COLS {
        let value = row.try_get::<String, usize>(idx)?;
        mix_bytes(&mut hash, value.as_bytes());
    }
    Ok(((hash & 0x7fff_ffff_ffff_ffff) | 1) as i64)
}

async fn permission_fingerprint_coalesced(store: &IdentityStore) -> Result<i64, StatusCode> {
    coalesced(
        store.cache_scope,
        &FP_FLIGHTS,
        store.cache_scope,
        || async {
            permission_fingerprint(store)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        },
    )
    .await
}

async fn resolve_visible_libraries(
    store: &IdentityStore,
    root: &str,
    user_id: &str,
    role: Role,
    perm_fp: i64,
) -> Result<CachedVisible, (StatusCode, &'static str)> {
    let is_admin = role == Role::Administrator;
    let key = VisibleCacheKey {
        cache_scope: store.cache_scope,
        root: root.to_owned(),
        user_id: user_id.to_owned(),
        is_admin,
    };
    {
        let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.visible_libs.get(&key)
            && entry.perm_fp == perm_fp
        {
            cache.record(store.cache_scope, |m| m.visible_hits += 1);
            return Ok(entry);
        }
    }

    let epoch = PERMISSIONS_EPOCH.load(Ordering::Acquire);
    coalesced(
        store.cache_scope,
        &VIS_FLIGHTS,
        (key.clone(), perm_fp),
        || async {
            {
                let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = cache.visible_libs.get(&key)
                    && entry.perm_fp == perm_fp
                {
                    cache.record(store.cache_scope, |m| m.visible_hits += 1);
                    return Ok(entry);
                }
                cache.record(store.cache_scope, |m| m.visible_queries += 1);
            }
            // Read permissions are live; fixture associations remain separate from indexed assets.
            // A library with `adminAccess: false` reaches administrators only through an explicit grant.
            let rows = crate::database::query_as::<_, (i64, String, String, String, i64)>(
                "SELECT CAST(0 AS BIGINT), CAST('' AS TEXT), CAST('' AS TEXT), CAST('' AS TEXT), \
                 CAST(CASE WHEN EXISTS(SELECT 1 FROM parental_policies WHERE user_id=?) THEN 1 ELSE 0 END AS BIGINT) \
                 UNION ALL \
                 SELECT CAST(1 AS BIGINT), id, root, kind, CAST(0 AS BIGINT) \
                 FROM libraries \
                 WHERE enabled=1 AND EXISTS(SELECT 1 FROM media_nodes n WHERE n.id=libraries.node_id \
                 AND n.status != 'revoked') AND ((?='administrator' AND NOT EXISTS(SELECT 1 \
                 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id \
                 AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id \
                 WHERE hx.id=libraries.id \
                 AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
                 OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=libraries.id \
                 AND p.user_id=?))",
            )
            .bind(user_id)
            .bind(role.as_str())
            .bind(user_id)
            .fetch_all(&store.pool)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;

            let mut has_parental = false;
            let mut visible = Vec::new();
            for (row_kind, id, r, kind, parental_flag) in rows {
                if row_kind == 0 {
                    has_parental = parental_flag != 0;
                    continue;
                }
                if !id.starts_with("fixture-")
                    || (r == root
                        && ((id == "fixture-films" && kind == "films")
                            || (id == "fixture-series" && kind == "series")))
                {
                    visible.push(id);
                }
            }
            let entry = CachedVisible {
                perm_fp,
                visible: Arc::new(visible),
                has_parental,
            };
            if PERMISSIONS_EPOCH.load(Ordering::Acquire) == epoch {
                let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                cache.visible_libs.insert(key.clone(), entry.clone());
            }
            Ok(entry)
        },
    )
    .await
}

/// The file a `/api/v1/media/{id}` request is served from, with its library: the id itself
/// for a file, the first playable file of a film, an episode or a series otherwise (the
/// descriptor handler resolves the same way). An unknown id comes back unchanged and without
/// a library, so it fails the library check.
async fn media_asset(
    store: &IdentityStore,
    id: &str,
) -> Result<(String, Option<String>), sqlx::Error> {
    if let Some(library) = crate::inventory::resource_library(store, "asset", id).await? {
        return Ok((id.to_owned(), Some(library)));
    }
    let Some(asset) = crate::inventory::playable_asset_for_work(store, id).await? else {
        return Ok((id.to_owned(), None));
    };
    let library = crate::inventory::resource_library(store, "asset", &asset).await?;
    Ok((asset, library))
}

/// `/api/v1/assets/{asset}/hls/{grant}/{component}`: (asset, grant). Such a URL
/// carries its own authorization, so a cast device can follow it with no cookie.
pub fn hls_grant(path: &str) -> Option<(&str, &str)> {
    let mut parts = path.strip_prefix("/api/v1/assets/")?.split('/');
    let (asset, kind, grant, component) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (kind == "hls" && !asset.is_empty() && !component.is_empty() && parts.next().is_none())
        .then_some((asset, grant))
}

fn asset_resource_id(rest: &str) -> &str {
    rest.strip_suffix("/content")
        .or_else(|| rest.strip_suffix("/download"))
        .or_else(|| rest.strip_suffix("/image"))
        .or_else(|| rest.strip_suffix("/trickplay"))
        .or_else(|| rest.strip_suffix("/chapter-images"))
        .or_else(|| rest.strip_suffix("/details"))
        .or_else(|| rest.split_once("/chapter-images/").map(|(id, _)| id))
        .or_else(|| rest.split_once("/trickplay/").map(|(id, _)| id))
        .or_else(|| rest.split_once("/subtitles/").map(|(id, _)| id))
        .or_else(|| rest.split_once("/audio/").map(|(id, _)| id))
        .or_else(|| rest.split_once("/prepared/").map(|(id, _)| id))
        .or_else(|| rest.strip_suffix("/hls"))
        .or_else(|| rest.split_once("/hls/").map(|(id, _)| id))
        .unwrap_or("")
}

fn fallback_parental_target(path: &str) -> (&'static str, String) {
    if let Some(id) = path.strip_prefix("/api/v1/assets/") {
        (
            if path.ends_with("/image") {
                "image"
            } else {
                "asset"
            },
            id.split('/').next().unwrap_or("").to_owned(),
        )
    } else {
        ("asset", String::new())
    }
}

fn classify_route(path: &str) -> RouteTarget {
    if path == "/api/v1/catalog" {
        RouteTarget::CatalogRoot
    } else if let Some(id) = path.strip_prefix("/api/v1/catalog/") {
        RouteTarget::CatalogWork(id.to_owned())
    } else if let Some(id) = path.strip_prefix("/api/v1/media/") {
        RouteTarget::Media(id.to_owned())
    } else if path.starts_with("/api/v1/assets/")
        && !path.starts_with("/api/v1/assets/demo-")
        && !path.starts_with("/api/v1/assets/fixture-")
    {
        let rest = path.strip_prefix("/api/v1/assets/").unwrap_or("");
        RouteTarget::Asset {
            kind: if path.ends_with("/image") {
                "image"
            } else {
                "asset"
            },
            lib_id: asset_resource_id(rest).to_owned(),
            parental_id: rest.split('/').next().unwrap_or("").to_owned(),
        }
    } else if path.starts_with("/api/v1/demo/") || path.starts_with("/api/v1/assets/demo-") {
        let (parental_kind, parental_id) = fallback_parental_target(path);
        RouteTarget::DemoFixture {
            require_series_or_films: true,
            parental_kind,
            parental_id,
        }
    } else {
        let (parental_kind, parental_id) = fallback_parental_target(path);
        RouteTarget::DemoFixture {
            require_series_or_films: false,
            parental_kind,
            parental_id,
        }
    }
}

async fn resolve_resource_decision(
    store: &IdentityStore,
    root: &str,
    user_id: &str,
    role: Role,
    visible: &CachedVisible,
    target: &RouteTarget,
) -> Result<CachedDecision, (StatusCode, &'static str)> {
    let is_admin = role == Role::Administrator;
    let key = ResourceCacheKey {
        cache_scope: store.cache_scope,
        root: root.to_owned(),
        user_id: user_id.to_owned(),
        is_admin,
        target: target.clone(),
    };
    {
        let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.resource_auth.get(&key)
            && entry.perm_fp == visible.perm_fp
        {
            cache.record(store.cache_scope, |m| m.resource_hits += 1);
            return Ok(entry);
        }
    }

    let epoch = PERMISSIONS_EPOCH.load(Ordering::Acquire);
    coalesced(
        store.cache_scope,
        &RES_FLIGHTS,
        (key.clone(), visible.perm_fp),
        || async {
            {
                let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = cache.resource_auth.get(&key)
                    && entry.perm_fp == visible.perm_fp
                {
                    cache.record(store.cache_scope, |m| m.resource_hits += 1);
                    return Ok(entry);
                }
                cache.record(store.cache_scope, |m| m.resource_queries += 1);
            }
            let (allowed, parental_kind, parental_id): (bool, &str, String) = match target {
                RouteTarget::CatalogRoot => {
                    return Ok(CachedDecision {
                        perm_fp: visible.perm_fp,
                        allowed: !visible.visible.is_empty() || is_admin,
                        parental_allowed: true,
                    });
                }
                RouteTarget::CatalogWork(id) => {
                    let fixture_lib = match id.as_str() {
                        "fixture-film" => "fixture-films",
                        "fixture-series" | "fixture-episode-1" => "fixture-series",
                        _ => "",
                    };
                    let ok = if fixture_lib.is_empty() {
                        let direct_ok =
                            match crate::inventory::resource_library(store, "work", id).await {
                                Ok(Some(library)) => visible.visible.iter().any(|v| v == &library),
                                Ok(None) => false,
                                Err(_) => {
                                    return Err((
                                        StatusCode::INTERNAL_SERVER_ERROR,
                                        "inventory_unavailable",
                                    ));
                                }
                            };
                        if direct_ok {
                            true
                        } else {
                            match crate::consolidation::authorized_group_members(
                                store,
                                id,
                                Some(user_id),
                                Some(role.as_str()),
                            )
                            .await
                            {
                                Ok(members) => !members.is_empty(),
                                Err(_) => false,
                            }
                        }
                    } else {
                        visible.visible.iter().any(|v| v == fixture_lib)
                    };
                    (ok, "work", id.clone())
                }
                RouteTarget::Media(id) => {
                    let err = (StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable");
                    let (asset, library) = media_asset(store, id).await.map_err(|_| err)?;
                    let ok = library
                        .as_ref()
                        .is_some_and(|lib| visible.visible.iter().any(|v| v == lib));
                    (ok, "asset", asset)
                }
                RouteTarget::Asset {
                    kind,
                    lib_id,
                    parental_id,
                } => {
                    let ok = match crate::inventory::resource_library(store, kind, lib_id).await {
                        Ok(Some(library)) => visible.visible.iter().any(|v| v == &library),
                        Ok(None) => false,
                        Err(_) => {
                            return Err((
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "inventory_unavailable",
                            ));
                        }
                    };
                    (ok, *kind, parental_id.clone())
                }
                RouteTarget::DemoFixture {
                    require_series_or_films,
                    parental_kind,
                    parental_id,
                } => {
                    let ok = if *require_series_or_films {
                        visible
                            .visible
                            .iter()
                            .any(|v| v == "fixture-films" || v == "fixture-series")
                    } else {
                        visible.visible.iter().any(|v| v == "fixture-films")
                    };
                    (ok, *parental_kind, parental_id.clone())
                }
            };

            let parental_allowed = if !allowed {
                false
            } else if !visible.has_parental {
                true
            } else {
                crate::product::target_allowed(store, user_id, parental_kind, &parental_id)
                    .await
                    .map_err(|status| (status, "parental_policy_unavailable"))?
            };

            let decision = CachedDecision {
                perm_fp: visible.perm_fp,
                allowed,
                parental_allowed,
            };
            if PERMISSIONS_EPOCH.load(Ordering::Acquire) == epoch {
                let mut cache = PERMISSIONS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
                cache.resource_auth.insert(key.clone(), decision);
            }
            Ok(decision)
        },
    )
    .await
}

/// Cached read authorization for a single work (RBAC + library visibility + parental policy).
pub(crate) async fn require_work_read(
    auth: &AuthState,
    headers: &axum::http::HeaderMap,
    work_id: &str,
) -> Result<String, StatusCode> {
    let identity = auth.authenticate(headers).await?;
    let perm_fp = permission_fingerprint_coalesced(&auth.store).await?;
    let visible = resolve_visible_libraries(
        &auth.store,
        "",
        &identity.user.id,
        identity.user.role,
        perm_fp,
    )
    .await
    .map_err(|(status, _)| status)?;
    let target = RouteTarget::CatalogWork(work_id.to_owned());
    let decision = resolve_resource_decision(
        &auth.store,
        "",
        &identity.user.id,
        identity.user.role,
        &visible,
        &target,
    )
    .await
    .map_err(|(status, _)| status)?;
    if !decision.allowed || !decision.parental_allowed {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(identity.user.id)
}

pub async fn require_media(
    State(state): State<MediaAccess>,
    mut request: Request,
    next: Next,
) -> Response {
    // A capability URL never falls back to a cookie, and a cookie never reaches it.
    let authenticated = match hls_grant(request.uri().path()) {
        Some((asset, grant)) => state.auth.authenticate_grant(grant, asset).await,
        None => state.auth.authenticate(request.headers()).await,
    };
    let identity = match authenticated {
        Ok(identity) => identity,
        Err(status) => return error(status, "authentication_required"),
    };
    let perm_fp = match permission_fingerprint_coalesced(&state.auth.store).await {
        Ok(fp) => fp,
        Err(status) => return error(status, "internal_error"),
    };
    let root = state.root.to_string_lossy().into_owned();
    let path = request.uri().path().to_owned();
    let target = classify_route(&path);

    let visible = match resolve_visible_libraries(
        &state.auth.store,
        &root,
        &identity.user.id,
        identity.user.role,
        perm_fp,
    )
    .await
    {
        Ok(visible) => visible,
        Err((status, code)) => return error(status, code),
    };

    if matches!(target, RouteTarget::CatalogRoot) {
        let allowed = !visible.visible.is_empty() || identity.user.role == Role::Administrator;
        if !allowed {
            return error(StatusCode::FORBIDDEN, "permission_denied");
        }
    } else {
        let decision = match resolve_resource_decision(
            &state.auth.store,
            &root,
            &identity.user.id,
            identity.user.role,
            &visible,
            &target,
        )
        .await
        {
            Ok(decision) => decision,
            Err((status, code)) => return error(status, code),
        };
        if !decision.allowed {
            return error(StatusCode::FORBIDDEN, "permission_denied");
        }
        if !decision.parental_allowed {
            return error(StatusCode::FORBIDDEN, "parental_restriction");
        }
    }

    let downloading = path.ends_with("/download");
    if downloading {
        // Originals are downloaded by the native applications only: they let the
        // person pick the version, language and tracks, which a browser cannot show.
        if !identity.bearer {
            return error(StatusCode::FORBIDDEN, "media_download_native_only");
        }
        match crate::user_media_policy::can_download(&state.auth.store, &identity.user.id).await {
            Ok(true) => {}
            Ok(false) => return error(StatusCode::FORBIDDEN, "media_download_disabled"),
            Err(status) => return error(status, "media_policy_unavailable"),
        }
    }
    request
        .extensions_mut()
        .insert(VisibleLibraries((*visible.visible).clone()));
    request.extensions_mut().insert(identity);
    let mut response = next.run(request).await;
    if downloading && response.status().is_success() {
        response.headers_mut().insert(
            axum::http::header::CONTENT_DISPOSITION,
            axum::http::HeaderValue::from_static("attachment"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::hash;
    use crate::database::query as db_query;
    use axum::{Router, body::Body, http::header, middleware, routing::get};
    use futures_util::future::join_all;
    use tower::ServiceExt;
    use zeroize::Zeroizing;

    const SESSION_COOKIE: &str = "aether_session";

    async fn setup_test_env(
        poster_count: usize,
    ) -> (
        tempfile::TempDir,
        IdentityStore,
        Router,
        String,
        String,
        String,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("perms.sqlite3"))
            .await
            .unwrap();
        let admin_id = store
            .bootstrap("admin", Zeroizing::new("strong-admin-password".into()))
            .await
            .unwrap();
        let user_id = "user-perm-1".to_owned();
        db_query("INSERT INTO users(id, role) VALUES (?, 'user')")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        let raw_token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        db_query(
            "INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at, device_label) \
             VALUES (?, ?, unixepoch(), unixepoch(), unixepoch() + 86400, 'test')",
        )
        .bind(hash(raw_token))
        .bind(&user_id)
        .execute(&store.pool)
        .await
        .unwrap();
        let cookie = format!("{SESSION_COOKIE}={raw_token}");

        db_query(
            "INSERT INTO libraries(id, name, kind, node_id, root, enabled) \
             VALUES ('lib-films', 'Films', 'films', 'local', '/private/media', 1)",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        db_query("INSERT INTO library_permissions(library_id, user_id) VALUES ('lib-films', ?)")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();

        for idx in 0..poster_count {
            let work_id = format!("work-{idx}");
            let img_id = format!("img-{idx}");
            db_query(
                "INSERT INTO inventory_works(id, library_id, identity_key, title, external_ids_json, kind, title_sort, minimum_age) \
                 VALUES (?, 'lib-films', ?, ?, '{}', 'film', ?, 12)",
            )
            .bind(&work_id)
            .bind(format!("key-{idx}"))
            .bind(format!("Film {idx}"))
            .bind(format!("film {idx:03}"))
            .execute(&store.pool)
            .await
            .unwrap();
            db_query(
                "INSERT INTO inventory_images(id, work_id, library_id, root_snapshot, relative_path, size, modified_ns, mime, priority, available, role) \
                 VALUES (?, ?, 'lib-films', '/private/media', ?, 1024, '100', 'image/jpeg', 1, 1, 'poster')",
            )
            .bind(&img_id)
            .bind(&work_id)
            .bind(format!("posters/{idx}.jpg"))
            .execute(&store.pool)
            .await
            .unwrap();
        }

        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let router = Router::new()
            .route("/api/v1/catalog", get(|| async { StatusCode::OK }))
            .route("/api/v1/catalog/{id}", get(|| async { StatusCode::OK }))
            .route(
                "/api/v1/assets/{id}/image",
                get(|| async { StatusCode::OK }),
            )
            .layer(middleware::from_fn_with_state(
                MediaAccess {
                    auth,
                    root: dir.path().to_owned(),
                },
                require_media,
            ));
        (dir, store, router, user_id, admin_id, cookie)
    }

    async fn get_status(router: &Router, path: &str, cookie: &str) -> StatusCode {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "127.0.0.1:3000")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn burst_coalesces_fingerprint_and_warm_posters_hit_zero_redundant_queries() {
        let _cache_guard = CACHE_TEST_LOCK.lock().await;
        let (_dir, store, router, _user_id, _admin_id, cookie) = setup_test_env(40).await;

        // Deterministic proof of singleflight coalescing on 40 concurrent fingerprint calls.
        let before = scope_permissions_cache_stats(store.cache_scope);
        let fp_results = join_all((0..40).map(|_| permission_fingerprint_coalesced(&store))).await;
        let fp0 = fp_results[0].unwrap();
        assert!(fp_results.iter().all(|r| *r == Ok(fp0)));
        let after_fp = scope_permissions_cache_stats(store.cache_scope);
        assert_eq!(
            after_fp.fingerprint_queries - before.fingerprint_queries,
            1,
            "40 concurrent fingerprint checks must coalesce into 1 query"
        );
        assert_eq!(after_fp.coalesced_waiters - before.coalesced_waiters, 39);

        // Concurrent cold burst of 40 distinct poster image requests.
        let mut paths = Vec::with_capacity(40);
        for i in 0..40 {
            paths.push(format!("/api/v1/assets/img-{i}/image"));
        }
        let cold_stats_before = scope_permissions_cache_stats(store.cache_scope);
        let cold_futs = paths.iter().map(|p| get_status(&router, p, &cookie));
        let cold_statuses = join_all(cold_futs).await;
        assert!(cold_statuses.into_iter().all(|s| s == StatusCode::OK));
        let cold_stats_after = scope_permissions_cache_stats(store.cache_scope);
        assert!(
            cold_stats_after.visible_queries - cold_stats_before.visible_queries <= 2,
            "visible libraries query should coalesce across 40 concurrent requests"
        );
        assert_eq!(cold_stats_after.resource_entries, 40);

        // Warm pass over all 40 posters: ZERO visible_libs or resource_auth SQL queries.
        let warm_before = scope_permissions_cache_stats(store.cache_scope);
        let warm_futs = paths.iter().map(|p| get_status(&router, p, &cookie));
        let warm_statuses = join_all(warm_futs).await;
        assert!(warm_statuses.into_iter().all(|s| s == StatusCode::OK));
        let warm_after = scope_permissions_cache_stats(store.cache_scope);
        assert_eq!(warm_after.visible_queries - warm_before.visible_queries, 0);
        assert_eq!(
            warm_after.resource_queries - warm_before.resource_queries,
            0
        );
        assert!(warm_after.visible_hits - warm_before.visible_hits >= 40);
        assert_eq!(warm_after.resource_hits - warm_before.resource_hits, 40);
    }

    #[tokio::test]
    async fn raw_sqlite_mutations_invalidate_permissions_and_parental_immediately() {
        let _cache_guard = CACHE_TEST_LOCK.lock().await;
        let (_dir, store, router, user_id, _admin_id, cookie) = setup_test_env(2).await;

        // Warm cache for poster img-0 and catalog work-0.
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );
        assert_eq!(
            get_status(&router, "/api/v1/catalog/work-0", &cookie).await,
            StatusCode::OK
        );
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );

        // Revoke library permission directly in SQL -> immediate 403 on next call (<1ms).
        db_query("DELETE FROM library_permissions WHERE library_id='lib-films' AND user_id=?")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get_status(&router, "/api/v1/catalog", &cookie).await,
            StatusCode::FORBIDDEN
        );

        // Re-grant permission directly in SQL -> immediate 200 again.
        db_query("INSERT INTO library_permissions(library_id, user_id) VALUES ('lib-films', ?)")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );

        // Restrict via parental policy directly in SQL (work-0 has minimum_age=12, set max_age=7) -> immediate 403.
        db_query(
            "INSERT INTO parental_policies(user_id, maximum_age, allow_unrated) VALUES (?, 7, 1)",
        )
        .bind(&user_id)
        .execute(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            get_status(&router, "/api/v1/catalog/work-0", &cookie).await,
            StatusCode::FORBIDDEN
        );

        // Relax parental limit directly in SQL -> immediate 200.
        db_query("UPDATE parental_policies SET maximum_age=16 WHERE user_id=?")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );

        // Deny 'films' kind via user_media_kind_denials -> immediate 403.
        db_query("INSERT INTO user_media_kind_denials(user_id, kind) VALUES (?, 'films')")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn role_promotion_and_library_admin_exclusion_invalidate_immediately() {
        let _cache_guard = CACHE_TEST_LOCK.lock().await;
        let (_dir, store, router, user_id, admin_id, cookie) = setup_test_env(1).await;

        // Deny direct permission and promote user directly to administrator.
        db_query("DELETE FROM library_permissions WHERE user_id=?")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::FORBIDDEN
        );
        db_query("UPDATE users SET role='administrator' WHERE id=?")
            .bind(&user_id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );

        // Toggle adminAccess: false on the library without an explicit grant -> immediate 403.
        let _ = admin_id;
        db_query(
            r#"UPDATE libraries SET options_json='{"adminAccess":false}' WHERE id='lib-films'"#,
        )
        .execute(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn invalidate_permissions_cache_and_catalogue_cache_bump_epoch_immediately() {
        let _cache_guard = CACHE_TEST_LOCK.lock().await;
        let (_dir, store, router, _user_id, _admin_id, cookie) = setup_test_env(2).await;

        assert_eq!(
            get_status(&router, "/api/v1/assets/img-0/image", &cookie).await,
            StatusCode::OK
        );
        let fp_before = permission_fingerprint(&store).await.unwrap();
        let before_stats = scope_permissions_cache_stats(store.cache_scope);
        assert!(before_stats.visible_entries >= 1);
        assert!(before_stats.resource_entries >= 1);

        // A catalogue change clears permissions caches and bumps PERMISSIONS_EPOCH even without
        // any database `inventory_cache_generation.gen` mutation.
        crate::inventory::catalogue_changed(&store).await;

        let after_invalidation = scope_permissions_cache_stats(store.cache_scope);
        assert_eq!(after_invalidation.visible_entries, 0);
        assert_eq!(after_invalidation.resource_entries, 0);

        let fp_after = permission_fingerprint(&store).await.unwrap();
        assert_ne!(fp_before, fp_after);
    }
}
