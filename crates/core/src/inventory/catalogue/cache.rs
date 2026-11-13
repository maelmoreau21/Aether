//! Bounded catalogue response cache and generation invalidation.

use serde_json::Value;

pub struct BoundedCatalogueCache {
    max_bytes: usize,
    current_bytes: usize,
    entries: std::collections::HashMap<String, CatalogueCacheEntry>,
    access_seq: u64,
}

struct CatalogueCacheEntry {
    gen_fp: String,
    value: Value,
    size_bytes: usize,
    last_accessed: u64,
}

impl BoundedCatalogueCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes: max_bytes.max(1024 * 1024),
            current_bytes: 0,
            entries: std::collections::HashMap::new(),
            access_seq: 0,
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
        let fp_str = current_fp.to_string();
        if let Some(entry) = self.entries.get_mut(key) {
            if entry.gen_fp == fp_str {
                self.access_seq += 1;
                entry.last_accessed = self.access_seq;
                return Some(entry.value.clone());
            } else if let Some(removed) = self.entries.remove(key) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
            }
        }
        None
    }

    pub fn insert(&mut self, key: String, current_fp: i64, value: Value) {
        let entry_size = key.len() + 64 + estimate_json_bytes(&value);
        if entry_size > self.max_bytes {
            return;
        }

        while self.current_bytes.saturating_add(entry_size) > self.max_bytes
            && !self.entries.is_empty()
        {
            if let Some((oldest_key, _)) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_accessed)
                .map(|(k, e)| (k.clone(), e.size_bytes))
            {
                if let Some(removed) = self.entries.remove(&oldest_key) {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
                }
            } else {
                break;
            }
        }

        self.access_seq += 1;
        let entry = CatalogueCacheEntry {
            gen_fp: current_fp.to_string(),
            value,
            size_bytes: entry_size,
            last_accessed: self.access_seq,
        };

        if let Some(old) = self.entries.insert(key, entry) {
            self.current_bytes = self.current_bytes.saturating_sub(old.size_bytes);
        }
        self.current_bytes = self.current_bytes.saturating_add(entry_size);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_bytes = 0;
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (self.current_bytes, self.max_bytes, self.entries.len())
    }
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

pub(super) static CATALOGUE_CACHE: std::sync::OnceLock<std::sync::Mutex<BoundedCatalogueCache>> =
    std::sync::OnceLock::new();

pub fn invalidate_catalogue_cache() {
    if let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
    {
        guard.clear();
    }
}

pub fn catalogue_cache_stats() -> (usize, usize, usize) {
    if let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(guard) = lock.lock()
    {
        guard.stats()
    } else {
        (0, 0, 0)
    }
}

/// One computation per cache key at a time: when a page is missing (start-up, after a scan
/// changed the inventory), concurrent viewers wait for the first one instead of all running
/// the same catalogue queries at once.
static IN_FLIGHT: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>>,
> = std::sync::OnceLock::new();

pub(super) fn flight_lock(key: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    let map = IN_FLIGHT.get_or_init(Default::default);
    let mut map = map
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(lock) = map.get(key).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    if map.len() >= 1024 {
        map.retain(|_, lock| lock.strong_count() > 0);
    }
    let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    map.insert(key.to_owned(), std::sync::Arc::downgrade(&lock));
    lock
}
