//! Bounded catalogue response, decorated-work, locale-chain and metadata caches.

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::{
    Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak,
    atomic::{AtomicU64, Ordering},
};

const DECORATED_WORK_MAX_ENTRIES: usize = 16_384;
const LOCALE_CHAIN_MAX_ENTRIES: usize = 4_096;
const VALID_LIBS_MAX_ENTRIES: usize = 2_048;
const META_FLAGS_MAX_ENTRIES: usize = 4_096;
const FTS_SCOPES_MAX_ENTRIES: usize = 256;

struct BucketEntry<V> {
    state_fp: i64,
    value: V,
    size_bytes: usize,
    last_accessed: u64,
}

/// Byte- and count-bounded LRU cache keyed by an arbitrary string and an exact state fingerprint.
struct LruBucket<V> {
    max_bytes: usize,
    max_entries: usize,
    current_bytes: usize,
    entries: HashMap<String, BucketEntry<V>>,
    lru: VecDeque<(String, u64)>,
    access_seq: u64,
}

impl<V> LruBucket<V> {
    fn new(max_bytes: usize, max_entries: usize) -> Self {
        Self {
            max_bytes,
            max_entries: max_entries.max(1),
            current_bytes: 0,
            entries: HashMap::new(),
            lru: VecDeque::new(),
            access_seq: 0,
        }
    }

    fn get_ref(&mut self, key: &str, current_fp: i64) -> Option<&V> {
        let matched = self
            .entries
            .get(key)
            .is_some_and(|entry| entry.state_fp == current_fp);
        if matched {
            self.access_seq += 1;
            let seq = self.access_seq;
            if let Some(entry) = self.entries.get_mut(key) {
                entry.last_accessed = seq;
            }
            if self.lru.len() < self.entries.len().saturating_mul(2).max(64) {
                self.lru.push_back((key.to_owned(), seq));
            }
            return self.entries.get(key).map(|entry| &entry.value);
        }
        if let Some(removed) = self.entries.remove(key) {
            self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
        }
        None
    }

    fn insert(&mut self, key: String, current_fp: i64, value: V, value_bytes: usize) {
        let entry_size = key.len().saturating_add(64).saturating_add(value_bytes);
        if entry_size > self.max_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size_bytes);
        }

        if self.lru.len() > self.entries.len().saturating_mul(4).max(64) {
            self.compact_lru();
        }
        while (self.current_bytes.saturating_add(entry_size) > self.max_bytes
            || self.entries.len() >= self.max_entries)
            && !self.entries.is_empty()
        {
            if !self.evict_oldest() {
                break;
            }
        }

        self.access_seq += 1;
        let seq = self.access_seq;
        let entry = BucketEntry {
            state_fp: current_fp,
            value,
            size_bytes: entry_size,
            last_accessed: seq,
        };
        self.entries.insert(key.clone(), entry);
        self.lru.push_back((key, seq));
        self.current_bytes = self.current_bytes.saturating_add(entry_size);
    }

    fn evict_oldest(&mut self) -> bool {
        while let Some((candidate, seq)) = self.lru.pop_front() {
            let is_live = self
                .entries
                .get(&candidate)
                .is_some_and(|entry| entry.last_accessed == seq);
            if is_live && let Some(removed) = self.entries.remove(&candidate) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
                return true;
            }
        }
        if let Some(oldest_key) = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(k, _)| k.clone())
            && let Some(removed) = self.entries.remove(&oldest_key)
        {
            self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
            return true;
        }
        false
    }

    fn compact_lru(&mut self) {
        let mut live: Vec<(u64, String)> = self
            .entries
            .iter()
            .map(|(k, entry)| (entry.last_accessed, k.clone()))
            .collect();
        live.sort_unstable_by_key(|(seq, _)| *seq);
        self.lru.clear();
        for (seq, k) in live {
            self.lru.push_back((k, seq));
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.current_bytes = 0;
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

pub struct BoundedCatalogueCache {
    pages: LruBucket<Value>,
    decorated_works: LruBucket<Value>,
    locale_chains: LruBucket<Vec<String>>,
    valid_libs: LruBucket<(Vec<String>, bool)>,
    meta_flags: LruBucket<(bool, bool, bool)>,
    fts_tables: HashMap<u64, bool>,
}

impl BoundedCatalogueCache {
    pub fn new(max_bytes: usize) -> Self {
        let bounded = max_bytes.max(1024 * 1024);
        Self {
            pages: LruBucket::new(bounded, usize::MAX),
            decorated_works: LruBucket::new(bounded, DECORATED_WORK_MAX_ENTRIES),
            locale_chains: LruBucket::new(bounded.min(8 * 1024 * 1024), LOCALE_CHAIN_MAX_ENTRIES),
            valid_libs: LruBucket::new(bounded.min(4 * 1024 * 1024), VALID_LIBS_MAX_ENTRIES),
            meta_flags: LruBucket::new(bounded.min(2 * 1024 * 1024), META_FLAGS_MAX_ENTRIES),
            fts_tables: HashMap::new(),
        }
    }

    pub fn from_env() -> Self {
        let max_mb = std::env::var("MEDIA_CATALOGUE_CACHE_MAX_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(32)
            .clamp(1, 1024);
        Self::new(max_mb * 1024 * 1024)
    }

    pub fn get(&mut self, key: &str, current_fp: i64) -> Option<Value> {
        self.pages.get_ref(key, current_fp).cloned()
    }

    pub fn insert(&mut self, key: String, current_fp: i64, value: Value) {
        let size = estimate_json_bytes(&value);
        self.pages.insert(key, current_fp, value, size);
    }

    pub fn locale_chain_key(chain: &[String]) -> String {
        chain.join("\x1f")
    }

    pub fn get_decorated_work(
        &mut self,
        scope: u64,
        state_fp: i64,
        locale_key: &str,
        work_id: &str,
    ) -> Option<Value> {
        let mut key = String::with_capacity(32 + locale_key.len() + work_id.len());
        write_work_prefix(&mut key, scope, locale_key);
        key.push_str(work_id);
        self.decorated_works.get_ref(&key, state_fp).cloned()
    }

    pub fn insert_decorated_work(
        &mut self,
        scope: u64,
        state_fp: i64,
        locale_key: &str,
        work_id: &str,
        value: Value,
    ) {
        let mut key = String::with_capacity(32 + locale_key.len() + work_id.len());
        write_work_prefix(&mut key, scope, locale_key);
        key.push_str(work_id);
        let size = estimate_json_bytes(&value);
        self.decorated_works.insert(key, state_fp, value, size);
    }

    pub(super) fn get_decorated_works_batch(
        &mut self,
        scope: u64,
        state_fp: i64,
        locale_key: &str,
        ids: &[&String],
    ) -> Vec<Option<Value>> {
        let mut buf = String::with_capacity(64 + locale_key.len());
        write_work_prefix(&mut buf, scope, locale_key);
        let prefix_len = buf.len();
        ids.iter()
            .map(|id| {
                buf.truncate(prefix_len);
                buf.push_str(id);
                self.decorated_works.get_ref(&buf, state_fp).cloned()
            })
            .collect()
    }

    pub(super) fn insert_decorated_works_batch(
        &mut self,
        scope: u64,
        state_fp: i64,
        locale_key: &str,
        items: &[Value],
    ) {
        let mut prefix = String::with_capacity(32 + locale_key.len());
        write_work_prefix(&mut prefix, scope, locale_key);
        for item in items {
            let Some(work_id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut key = String::with_capacity(prefix.len() + work_id.len());
            key.push_str(&prefix);
            key.push_str(work_id);
            let size = estimate_json_bytes(item);
            self.decorated_works
                .insert(key, state_fp, item.clone(), size);
        }
    }

    pub fn get_locale_chain(
        &mut self,
        scope: u64,
        state_fp: i64,
        user_id: &str,
        requested: Option<&str>,
    ) -> Option<Vec<String>> {
        let key = locale_cache_key(scope, user_id, requested);
        self.locale_chains.get_ref(&key, state_fp).cloned()
    }

    pub fn insert_locale_chain(
        &mut self,
        scope: u64,
        state_fp: i64,
        user_id: &str,
        requested: Option<&str>,
        chain: Vec<String>,
    ) {
        let key = locale_cache_key(scope, user_id, requested);
        let size = estimate_strings_bytes(&chain);
        self.locale_chains.insert(key, state_fp, chain, size);
    }

    pub fn decorated_work_stats(&self) -> (usize, usize, usize) {
        (
            self.decorated_works.current_bytes(),
            self.decorated_works.max_bytes(),
            self.decorated_works.len(),
        )
    }

    pub fn locale_chain_len(&self) -> usize {
        self.locale_chains.len()
    }

    pub(super) fn get_valid_libs(
        &mut self,
        key: &str,
        state_fp: i64,
    ) -> Option<(Vec<String>, bool)> {
        self.valid_libs.get_ref(key, state_fp).cloned()
    }

    pub(super) fn insert_valid_libs(
        &mut self,
        key: String,
        state_fp: i64,
        valid_libs: Vec<String>,
        fast_avail: bool,
    ) {
        let size = estimate_strings_bytes(&valid_libs) + 16;
        self.valid_libs
            .insert(key, state_fp, (valid_libs, fast_avail), size);
    }

    pub(super) fn get_meta_flags(
        &mut self,
        key: &str,
        state_fp: i64,
    ) -> Option<(bool, bool, bool)> {
        self.meta_flags.get_ref(key, state_fp).copied()
    }

    pub(super) fn insert_meta_flags(
        &mut self,
        key: String,
        state_fp: i64,
        has_groups: bool,
        has_parental_policy: bool,
        sort_lang_exists: bool,
    ) {
        self.meta_flags.insert(
            key,
            state_fp,
            (has_groups, has_parental_policy, sort_lang_exists),
            32,
        );
    }

    pub(super) fn get_use_fts(&self, scope: u64) -> Option<bool> {
        self.fts_tables.get(&scope).copied()
    }

    pub(super) fn insert_use_fts(&mut self, scope: u64, use_fts: bool) {
        if self.fts_tables.len() >= FTS_SCOPES_MAX_ENTRIES {
            self.fts_tables.clear();
        }
        self.fts_tables.insert(scope, use_fts);
    }

    pub fn clear(&mut self) {
        self.pages.clear();
        self.decorated_works.clear();
        self.locale_chains.clear();
        self.valid_libs.clear();
        self.meta_flags.clear();
        self.fts_tables.clear();
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.pages.current_bytes(),
            self.pages.max_bytes(),
            self.pages.len(),
        )
    }
}

fn write_work_prefix(buf: &mut String, scope: u64, locale_key: &str) {
    let _ = write!(buf, "{scope}:{}:{locale_key}:", locale_key.len());
}

fn locale_cache_key(scope: u64, user_id: &str, requested: Option<&str>) -> String {
    match requested {
        Some(req) => format!("{scope}:{}:{user_id}:1:{req}", user_id.len()),
        None => format!("{scope}:{}:{user_id}:0:", user_id.len()),
    }
}

fn estimate_strings_bytes(values: &[String]) -> usize {
    24 + values
        .iter()
        .map(|s| 24 + s.capacity().max(s.len()))
        .sum::<usize>()
}

pub fn estimate_json_bytes(val: &Value) -> usize {
    match val {
        Value::Null | Value::Bool(_) | Value::Number(_) => 16,
        Value::String(s) => 24 + s.capacity().max(s.len()),
        Value::Array(arr) => {
            24 + arr.capacity() * std::mem::size_of::<Value>()
                + arr.iter().map(estimate_json_bytes).sum::<usize>()
        }
        Value::Object(map) => {
            48 + map
                .iter()
                .map(|(k, v)| 24 + k.capacity().max(k.len()) + estimate_json_bytes(v))
                .sum::<usize>()
        }
    }
}

static CATALOGUE_EPOCH: AtomicU64 = AtomicU64::new(1);

pub(super) static CATALOGUE_CACHE: OnceLock<Mutex<BoundedCatalogueCache>> = OnceLock::new();

pub(super) fn catalogue_epoch() -> u64 {
    CATALOGUE_EPOCH.load(Ordering::Acquire)
}

pub(super) fn lock_cache() -> MutexGuard<'static, BoundedCatalogueCache> {
    CATALOGUE_CACHE
        .get_or_init(|| Mutex::new(BoundedCatalogueCache::from_env()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub fn invalidate_catalogue_cache() {
    CATALOGUE_EPOCH.fetch_add(1, Ordering::AcqRel);
    if let Some(lock) = CATALOGUE_CACHE.get() {
        let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        guard.clear();
    }
    crate::permissions::invalidate_permissions_cache();
}

pub fn catalogue_cache_stats() -> (usize, usize, usize) {
    if let Some(lock) = CATALOGUE_CACHE.get() {
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        guard.stats()
    } else {
        (0, 0, 0)
    }
}

/// One computation per cache key at a time: when a page is missing (start-up, after a scan
/// changed the inventory), concurrent viewers wait for the first one instead of all running
/// the same catalogue queries at once.
static IN_FLIGHT: OnceLock<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>> = OnceLock::new();

pub(super) fn flight_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let map = IN_FLIGHT.get_or_init(Default::default);
    let mut map = map.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(lock) = map.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    if map.len() >= 1024 {
        map.retain(|_, lock| lock.strong_count() > 0);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    map.insert(key.to_owned(), Arc::downgrade(&lock));
    lock
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decorated_work_cache_separates_locales_and_invalidates_on_state_fp() {
        let mut cache = BoundedCatalogueCache::new(1024 * 1024);
        let en = BoundedCatalogueCache::locale_chain_key(&["en".to_owned()]);
        let fr = BoundedCatalogueCache::locale_chain_key(&["fr".to_owned(), "en".to_owned()]);

        cache.insert_decorated_work(1, 10, &en, "w1", json!({"id": "w1", "title": "English"}));
        cache.insert_decorated_work(1, 10, &fr, "w1", json!({"id": "w1", "title": "Français"}));

        assert_eq!(
            cache.get_decorated_work(1, 10, &en, "w1").unwrap()["title"],
            "English"
        );
        assert_eq!(
            cache.get_decorated_work(1, 10, &fr, "w1").unwrap()["title"],
            "Français"
        );
        assert!(cache.get_decorated_work(2, 10, &en, "w1").is_none());
        assert!(cache.get_decorated_work(1, 11, &en, "w1").is_none());
        // Mismatched state_fp evicts the stale entry cleanly.
        assert!(cache.get_decorated_work(1, 10, &en, "w1").is_none());
    }

    #[test]
    fn locale_chain_cache_keys_on_scope_state_user_and_requested() {
        let mut cache = BoundedCatalogueCache::new(1024 * 1024);
        cache.insert_locale_chain(1, 42, "u1", None, vec!["fr".to_owned(), "en".to_owned()]);
        cache.insert_locale_chain(
            1,
            42,
            "u1",
            Some("ja"),
            vec!["ja".to_owned(), "fr".to_owned()],
        );

        assert_eq!(
            cache.get_locale_chain(1, 42, "u1", None),
            Some(vec!["fr".to_owned(), "en".to_owned()])
        );
        assert_eq!(
            cache.get_locale_chain(1, 42, "u1", Some("ja")),
            Some(vec!["ja".to_owned(), "fr".to_owned()])
        );
        assert_eq!(cache.get_locale_chain(1, 43, "u1", None), None);
        assert_eq!(cache.get_locale_chain(1, 43, "u1", None), None);
    }

    #[test]
    fn clear_empties_all_buckets_and_preserves_page_stats_contract() {
        let mut cache = BoundedCatalogueCache::new(1024 * 1024);
        cache.insert("page1".to_owned(), 7, json!({"items": []}));
        cache.insert_decorated_work(1, 7, "en", "w1", json!({"id": "w1"}));
        cache.insert_locale_chain(1, 7, "u1", None, vec!["en".to_owned()]);
        cache.insert_valid_libs("vl".to_owned(), 7, vec!["lib1".to_owned()], true);
        cache.insert_meta_flags("mf".to_owned(), 7, true, false, true);
        cache.insert_use_fts(1, true);

        // `stats().2` tracks response-page count so store-scope checks remain exact.
        assert_eq!(cache.stats().2, 1);
        assert_eq!(cache.decorated_work_stats().2, 1);
        assert_eq!(cache.locale_chain_len(), 1);

        cache.clear();
        assert_eq!(cache.stats().2, 0);
        assert_eq!(cache.decorated_work_stats().2, 0);
        assert_eq!(cache.locale_chain_len(), 0);
        assert!(cache.get_use_fts(1).is_none());
    }

    #[test]
    fn lru_eviction_respects_byte_budget() {
        let mut bucket = LruBucket::new(1024, 100);
        for idx in 0..20 {
            let val = json!({"id": format!("w{idx}"), "pad": "x".repeat(120)});
            let bytes = estimate_json_bytes(&val);
            bucket.insert(format!("k{idx}"), 1, val, bytes);
        }
        assert!(bucket.current_bytes() <= 1024);
        assert!(bucket.len() < 20);
        assert!(bucket.get_ref("k0", 1).is_none());
        assert!(bucket.get_ref("k19", 1).is_some());
    }
}
