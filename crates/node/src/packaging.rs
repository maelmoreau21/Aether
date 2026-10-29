//! Optional, bounded packet-copy preparation. Sources are inherited read-only handles.
//! No network/file input protocols, video encoding, or writes beside original media.
use crate::files::OpenedFile;
use std::{
    collections::HashMap,
    io,
    path::Path,
    process::Stdio,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::{Mutex, Semaphore},
};

pub const MAX_PREPARATION_SECONDS: u64 = 3600;
struct Limits {
    cache: u64,
    video: u64,
    audio: u64,
    seconds: u64,
}
impl Limits {
    fn from_env() -> io::Result<Self> {
        fn bounded(name: &str, fallback: u64, min: u64, max: u64) -> io::Result<u64> {
            let invalid = || io::Error::new(io::ErrorKind::InvalidInput, format!("invalid {name}"));
            match std::env::var(name) {
                Ok(value) => value
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(invalid),
                Err(std::env::VarError::NotPresent) => Ok(fallback),
                Err(_) => Err(invalid()),
            }
        }
        let limits = Self {
            cache: bounded("MEDIA_PREPARATION_CACHE_GIB", 96, 1, 2048)? * 1024 * 1024 * 1024,
            video: bounded("MEDIA_PREPARATION_VIDEO_GIB", 64, 1, 1024)? * 1024 * 1024 * 1024,
            audio: bounded("MEDIA_PREPARATION_AUDIO_MIB", 4096, 64, 16384)? * 1024 * 1024,
            seconds: bounded(
                "MEDIA_PREPARATION_SECONDS",
                900,
                30,
                MAX_PREPARATION_SECONDS,
            )?,
        };
        if limits.video > limits.cache || limits.audio > limits.cache {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "component limit exceeds preparation cache",
            ));
        }
        Ok(limits)
    }
}

pub fn preparation_error_code(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::WouldBlock => "preparation_busy",
        io::ErrorKind::StorageFull => "preparation_cache_limit_reached",
        io::ErrorKind::TimedOut => "preparation_timeout",
        io::ErrorKind::NotFound => "preparation_tool_or_source_unavailable",
        io::ErrorKind::Unsupported => "unsupported_preparation",
        io::ErrorKind::InvalidInput => "preparation_configuration_invalid",
        _ => "preparation_failed",
    }
}
struct Entry {
    directory: Arc<crate::cache_dirs::CacheDirectory>,
    size: u64,
    used: Instant,
    chunk: bool,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    bytes: u64,
}
impl Cache {
    fn evict_idle(&mut self) -> bool {
        let victim = self
            .entries
            .iter()
            .filter(|(_, entry)| Arc::strong_count(&entry.directory) == 1)
            .min_by_key(|(_, entry)| (!entry.chunk, entry.used))
            .map(|(key, _)| key.clone());
        let Some(victim) = victim else {
            return false;
        };
        if let Some(entry) = self.entries.remove(&victim) {
            self.bytes -= entry.size;
        }
        true
    }
    fn hit(&mut self, key: &str) -> Option<Arc<crate::cache_dirs::CacheDirectory>> {
        let entry = self.entries.get_mut(key)?;
        entry.used = Instant::now();
        // Pin the directory before releasing the lock: eviction may run while
        // the confined output handle is opening on the blocking I/O pool.
        Some(entry.directory.clone())
    }
}
pub struct Preparations {
    cache: Mutex<Cache>,
    worker: Semaphore,
    waiters: Semaphore,
    limits: io::Result<Limits>,
    native_audio: crate::mp4_audio::NativeAudio,
    video_streams: crate::video_stream::VideoStreams,
    in_flight: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    joiners: Semaphore,
}
impl Default for Preparations {
    fn default() -> Self {
        // Recover orphaned entries at startup as well as before each new preparation.
        if let Err(error) = crate::cache_dirs::CacheDirectory::new() {
            eprintln!("[media cache] recovery deferred: {error}");
        }
        Self {
            cache: Mutex::new(Cache::default()),
            worker: Semaphore::new(1),
            waiters: Semaphore::new(8),
            limits: Limits::from_env(),
            native_audio: crate::mp4_audio::NativeAudio::default(),
            video_streams: crate::video_stream::VideoStreams::default(),
            in_flight: Mutex::new(HashMap::new()),
            joiners: Semaphore::new(64),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Component {
    Video,
    Audio(u32),
    AudioChunk(u32, u32),
    OpusChunk(u32, u32),
    Subtitle(u32),
}
impl Component {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Video => "video/mp4",
            Self::Audio(_) => "audio/ogg",
            Self::AudioChunk(_, _) | Self::OpusChunk(_, _) => "audio/mp4",
            Self::Subtitle(_) => "text/vtt; charset=utf-8",
        }
    }
}

pub const AUDIO_CHUNK_SECONDS: u32 = 10;
impl Component {
    pub fn opus_chunk(raw: &str) -> Option<Self> {
        let (track, chunk) = raw.strip_prefix("opus-")?.split_once("-chunk-")?;
        if track.is_empty()
            || chunk.is_empty()
            || !track
                .bytes()
                .chain(chunk.bytes())
                .all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let track = track.parse::<u32>().ok()?;
        let chunk = chunk.parse::<u32>().ok()?;
        (track > 0 && chunk < 8640).then_some(Self::OpusChunk(track, chunk))
    }

    pub fn audio_chunk(raw: &str) -> Option<Self> {
        let (index, chunk) = raw.strip_prefix("audio-")?.split_once("-chunk-")?;
        let index = index.parse::<u32>().ok()?;
        let chunk = chunk.parse::<u32>().ok()?;
        (chunk < 8640).then_some(Self::AudioChunk(index, chunk))
    }
}

// Dropping a request/timeout records cancellation too. No paths, URLs or tokens.
struct PreparationLog {
    id: String,
    component: Component,
    started: Instant,
    complete: bool,
}
impl Drop for PreparationLog {
    fn drop(&mut self) {
        if !self.complete {
            eprintln!(
                "{}",
                serde_json::json!({"event":"preparation_cancelled_or_failed","attempt":self.id,"component":format!("{:?}",self.component),"elapsed_ms":self.started.elapsed().as_millis()})
            );
        }
    }
}

/// Call only with an inventory-resolved source and stream index verified by the caller.
/// Cache is process-local, removed at shutdown, capped at 64 entries and configured bytes.
/// Saturation fails explicitly: no unbounded queue, no eviction of playing files.
impl Preparations {
    pub fn native_audio(&self) -> &crate::mp4_audio::NativeAudio {
        &self.native_audio
    }
    pub fn video_streams(&self) -> &crate::video_stream::VideoStreams {
        &self.video_streams
    }

    pub async fn prepare(
        &self,
        root: &Path,
        relative: &Path,
        expected_size: u64,
        expected_mtime: &str,
        component: Component,
    ) -> io::Result<OpenedFile> {
        let mut log = PreparationLog {
            id: uuid::Uuid::new_v4().to_string(),
            component,
            started: Instant::now(),
            complete: false,
        };
        eprintln!(
            "{}",
            serde_json::json!({"event":"preparation_requested","attempt":log.id,"component":format!("{component:?}"),"source_bytes":expected_size})
        );
        let limits = self
            .limits
            .as_ref()
            .map_err(|e| io::Error::new(e.kind(), e.to_string()))?;
        let input = OpenedFile::open_in_root(root, relative).await?;
        if input.size() != expected_size || input.modified_ns() != expected_mtime {
            return Err(io::Error::other("source changed; rescan required"));
        }
        let key = format!(
            "{:?}:{relative:?}:{expected_size}:{expected_mtime}:{component:?}",
            input.canonical_path()
        );
        let mut cache = self.cache.lock().await;
        if let Some(directory) = cache.hit(&key) {
            drop(cache);
            let opened = OpenedFile::open_in_root(directory.path(), Path::new("content")).await?;
            log.complete = true;
            eprintln!(
                "{}",
                serde_json::json!({"event":"preparation_cache_hit","attempt":log.id,"component":format!("{component:?}"),"elapsed_ms":log.started.elapsed().as_millis()})
            );
            return Ok(opened.with_cache_lease(directory));
        }
        let busy = || {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "preparation busy; retry explicitly",
            )
        };
        // Identical cold components share a key gate before seeking admission to
        // the single FFmpeg worker. Followers consume no worker queue slots.
        // Cancellation drops both guards; if the leader is cancelled, the next
        // interested request can take over without a detached background job.
        let (_joined, _key_guard) = {
            drop(cache);
            let joined = self.joiners.try_acquire().map_err(|_| busy())?;
            let gate = {
                let mut keys = self.in_flight.lock().await;
                keys.retain(|_, gate| gate.strong_count() > 0);
                if let Some(gate) = keys.get(&key).and_then(Weak::upgrade) {
                    gate
                } else {
                    if keys.len() >= 64 {
                        return Err(busy());
                    }
                    let gate = Arc::new(Mutex::new(()));
                    keys.insert(key.clone(), Arc::downgrade(&gate));
                    gate
                }
            };
            let wait_seconds = if matches!(component, Component::AudioChunk(..)) {
                limits.seconds.min(20) + 5
            } else {
                limits.seconds + 5
            };
            let guard = tokio::time::timeout(Duration::from_secs(wait_seconds), gate.lock_owned())
                .await
                .map_err(|_| busy())?;
            cache = self.cache.lock().await;
            if let Some(directory) = cache.hit(&key) {
                drop(cache);
                let opened =
                    OpenedFile::open_in_root(directory.path(), Path::new("content")).await?;
                log.complete = true;
                return Ok(opened.with_cache_lease(directory));
            }
            (joined, guard)
        };
        let _worker = match self.worker.try_acquire() {
            Ok(permit) => permit,
            Err(_) if matches!(component, Component::AudioChunk(..)) => {
                // Eight bounded waiters absorb cancelled seeks/prefetch and coalesce
                // identical windows. Never hold the cache lock while waiting.
                drop(cache);
                let _waiting = self.waiters.try_acquire().map_err(|_| busy())?;
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"preparation_waiting","attempt":log.id,"component":format!("{component:?}")})
                );
                let permit = tokio::time::timeout(Duration::from_secs(5), self.worker.acquire())
                    .await
                    .map_err(|_| busy())?
                    .map_err(|_| busy())?;
                cache = self.cache.lock().await;
                if let Some(directory) = cache.hit(&key) {
                    drop(cache);
                    drop(permit);
                    let opened =
                        OpenedFile::open_in_root(directory.path(), Path::new("content")).await?;
                    log.complete = true;
                    return Ok(opened.with_cache_lease(directory));
                }
                permit
            }
            Err(_) => return Err(busy()),
        };
        while cache.entries.len() >= 64 || cache.bytes >= limits.cache {
            if !cache.evict_idle() {
                break;
            }
        }
        if cache.entries.len() >= 64 || cache.bytes >= limits.cache {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "preparation cache full; restart to clear",
            ));
        }
        let remaining = limits.cache - cache.bytes;
        drop(cache);
        let directory = Arc::new(crate::cache_dirs::CacheDirectory::new()?);
        let output = std::fs::File::create(directory.path().join("content"))?;
        let limit = remaining.min(match component {
            Component::Video => limits.video,
            Component::Audio(_) => limits.audio,
            Component::AudioChunk(_, _) | Component::OpusChunk(_, _) => {
                limits.audio.min(16 * 1024 * 1024)
            }
            Component::Subtitle(_) => 4 * 1024 * 1024,
        });
        let mut file = input.try_clone_std()?;
        std::io::Seek::rewind(&mut file)?;
        let mut command = Command::new("ffmpeg");
        if let Component::AudioChunk(_, chunk) = component {
            // Seek in the original index, keeping one second of codec preroll.
            let start = (chunk * AUDIO_CHUNK_SECONDS).saturating_sub(1);
            command.args(["-ss", &start.to_string()]);
        }
        command.args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "fd,pipe",
            "-format_whitelist",
            "matroska,webm,mov,mp4,obu",
            "-max_alloc",
            "67108864",
            "-fd",
            "0",
            "-i",
            "fd:",
        ]);
        match component {
            Component::OpusChunk(..) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "native Opus chunks require the direct native audio service",
                ));
            }
            Component::Video => {
                command.args([
                    "-map",
                    "0:v:0",
                    "-an",
                    "-sn",
                    "-c:v",
                    "copy",
                    "-movflags",
                    "frag_keyframe+empty_moov+default_base_moof",
                    "-f",
                    "mp4",
                ]);
            }
            Component::Audio(index) => {
                command.args([
                    "-map",
                    &format!("0:{index}"),
                    "-vn",
                    "-sn",
                    "-c:a",
                    "copy",
                    "-f",
                    "ogg",
                ]);
            }
            Component::AudioChunk(index, _) => {
                command.args([
                    // Input seeking lands at a preceding video keyframe. Drop
                    // its negative audio preroll before rebasing this window.
                    "-ss",
                    "0",
                    "-map",
                    &format!("0:{index}"),
                    "-vn",
                    "-sn",
                    "-c:a",
                    "copy",
                    "-t",
                    "12",
                    "-avoid_negative_ts",
                    "make_zero",
                    "-movflags",
                    "frag_keyframe+empty_moov+default_base_moof",
                    "-frag_duration",
                    "1000000",
                    "-f",
                    "mp4",
                ]);
            }
            Component::Subtitle(index) => {
                command.args([
                    "-map",
                    &format!("0:{index}"),
                    "-vn",
                    "-an",
                    "-c:s",
                    "webvtt",
                    "-f",
                    "webvtt",
                ]);
            }
        }
        let mut child = command
            .arg("pipe:1")
            .stdin(Stdio::from(file))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("preparation output missing"))?;
        let mut bounded = stdout.take(limit + 1);
        let mut output = tokio::fs::File::from_std(output);
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("preparation diagnostics missing"))?;
        let diagnostics = async {
            let mut retained = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let n = stderr.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                retained.extend_from_slice(&buffer[..n]);
                if retained.len() > 16384 {
                    retained.drain(..retained.len() - 16384);
                }
            }
            Ok::<_, io::Error>(String::from_utf8_lossy(&retained).into_owned())
        };
        let job = async {
            let size = match tokio::io::copy(&mut bounded, &mut output).await {
                Ok(size) => size,
                Err(error) => {
                    let _ = child.kill().await;
                    return Err(error);
                }
            };
            if size > limit {
                let _ = child.kill().await;
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "preparation exceeds cache limit",
                ));
            }
            if !child.wait().await?.success() || size == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "unsupported packaging or stream",
                ));
            }
            // Tokio's File may still have a blocking write in flight after copy
            // reports the final bytes. Publish only after that write completes;
            // dropping the file alone does not wait for it or report its error.
            output.flush().await?;
            input.verify_unchanged_sync()?;
            Ok::<_, io::Error>(size)
        };
        let (result, diagnostics) = tokio::time::timeout(
            Duration::from_secs(if matches!(component, Component::AudioChunk(..)) {
                limits.seconds.min(20)
            } else {
                limits.seconds
            }),
            async { tokio::join!(job, diagnostics) },
        )
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "preparation timeout"))?;
        let size = match result {
            Ok(size) => size,
            Err(error) => {
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"preparation_ffmpeg_error","attempt":log.id,"component":format!("{component:?}"),"code":preparation_error_code(&error),"elapsed_ms":log.started.elapsed().as_millis(),"stderr":diagnostics.unwrap_or_default()})
                );
                return Err(error);
            }
        };
        drop(output);
        let opened = OpenedFile::open_in_root(directory.path(), Path::new("content")).await?;
        let mut cache = self.cache.lock().await;
        cache.bytes += size;
        cache.entries.insert(
            key,
            Entry {
                directory: directory.clone(),
                size,
                used: Instant::now(),
                chunk: matches!(component, Component::AudioChunk(..)),
            },
        );
        log.complete = true;
        eprintln!(
            "{}",
            serde_json::json!({"event":"preparation_ready","attempt":log.id,"component":format!("{component:?}"),"bytes":size,"elapsed_ms":log.started.elapsed().as_millis()})
        );
        Ok(opened.with_cache_lease(directory))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn idle_full_preparations_evict_but_playing_handles_remain_pinned() {
        let directory = Arc::new(crate::cache_dirs::CacheDirectory::new().unwrap());
        std::fs::write(directory.path().join("content"), b"prepared").unwrap();
        let playing = OpenedFile::open_in_root(directory.path(), Path::new("content"))
            .await
            .unwrap()
            .with_cache_lease(directory.clone());
        let mut cache = Cache::default();
        cache.entries.insert(
            "full".into(),
            Entry {
                directory: directory.clone(),
                size: 8,
                used: Instant::now(),
                chunk: false,
            },
        );
        cache.bytes = 8;
        drop(directory);
        assert!(
            !cache.evict_idle(),
            "an open playback handle must pin the full preparation"
        );
        drop(playing);
        assert!(
            cache.evict_idle(),
            "idle full preparations must not fill the cache forever"
        );
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn idle_chunks_evict_before_full_preparations() {
        let mut cache = Cache::default();
        for (name, chunk) in [("full", false), ("chunk", true)] {
            let directory = Arc::new(crate::cache_dirs::CacheDirectory::new().unwrap());
            cache.entries.insert(
                name.into(),
                Entry {
                    directory,
                    size: 1,
                    used: Instant::now(),
                    chunk,
                },
            );
            cache.bytes += 1;
        }
        assert!(cache.evict_idle());
        assert!(cache.entries.contains_key("full"));
        assert!(!cache.entries.contains_key("chunk"));
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn identical_cold_audio_windows_coalesce_without_consuming_worker_waiters() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        let source = OpenedFile::open_in_root(&root, Path::new("representative.mp4"))
            .await
            .unwrap();
        let preparations = Arc::new(Preparations::default());
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let preparations = preparations.clone();
            let root = root.clone();
            let size = source.size();
            let modified = source.modified_ns().to_owned();
            tasks.spawn(async move {
                preparations
                    .prepare(
                        &root,
                        Path::new("representative.mp4"),
                        size,
                        &modified,
                        Component::AudioChunk(1, 0),
                    )
                    .await
                    .unwrap()
            });
        }
        let mut sizes = Vec::new();
        while let Some(result) = tasks.join_next().await {
            sizes.push(result.unwrap().size());
        }
        assert_eq!(sizes.len(), 8);
        assert!(sizes.iter().all(|size| *size == sizes[0]));
        assert_eq!(preparations.cache.lock().await.entries.len(), 1);
        assert_eq!(preparations.worker.available_permits(), 1);
        assert_eq!(preparations.waiters.available_permits(), 8);
        assert_eq!(preparations.joiners.available_permits(), 64);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn identical_cold_full_components_share_one_published_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        for component in [
            Component::Video,
            Component::Audio(1),
            Component::Subtitle(4),
        ] {
            let source = OpenedFile::open_in_root(&root, Path::new("representative.mp4"))
                .await
                .unwrap();
            let preparations = Arc::new(Preparations::default());
            let mut tasks = tokio::task::JoinSet::new();
            let start = Arc::new(tokio::sync::Barrier::new(8));
            for _ in 0..8 {
                let preparations = preparations.clone();
                let root = root.clone();
                let start = start.clone();
                let size = source.size();
                let modified = source.modified_ns().to_owned();
                tasks.spawn(async move {
                    start.wait().await;
                    preparations
                        .prepare(
                            &root,
                            Path::new("representative.mp4"),
                            size,
                            &modified,
                            component,
                        )
                        .await
                });
            }
            let mut paths = Vec::new();
            let mut files = Vec::new();
            let mut failures = Vec::new();
            while let Some(result) = tasks.join_next().await {
                match result.unwrap() {
                    Ok(file) => {
                        assert!(file.size() > 0);
                        paths.push(file.canonical_path().to_owned());
                        files.push(file);
                    }
                    Err(error) => failures.push(error.to_string()),
                }
            }
            assert!(failures.is_empty(), "{component:?}: {failures:?}");
            assert_eq!(paths.len(), 8);
            assert!(paths.iter().all(|path| path == &paths[0]));
            assert_eq!(preparations.cache.lock().await.entries.len(), 1);
            assert_eq!(preparations.worker.available_permits(), 1);
            assert_eq!(preparations.joiners.available_permits(), 64);
        }
    }

    #[tokio::test]
    async fn cancelling_a_window_leader_allows_an_interested_follower_to_take_over() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        let source = OpenedFile::open_in_root(&root, Path::new("representative.mp4"))
            .await
            .unwrap();
        let preparations = Arc::new(Preparations::default());
        let held_worker = preparations.worker.acquire().await.unwrap();
        let start = || {
            let preparations = preparations.clone();
            let root = root.clone();
            let size = source.size();
            let modified = source.modified_ns().to_owned();
            tokio::spawn(async move {
                preparations
                    .prepare(
                        &root,
                        Path::new("representative.mp4"),
                        size,
                        &modified,
                        Component::AudioChunk(1, 0),
                    )
                    .await
            })
        };
        let leader = start();
        tokio::time::timeout(Duration::from_secs(2), async {
            while preparations.waiters.available_permits() == 8 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let follower = start();
        tokio::time::timeout(Duration::from_secs(2), async {
            while preparations.joiners.available_permits() > 62 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());
        drop(held_worker);
        assert!(follower.await.unwrap().unwrap().size() > 0);
        assert_eq!(preparations.worker.available_permits(), 1);
        assert_eq!(preparations.joiners.available_permits(), 64);
    }

    #[tokio::test]
    async fn cached_output_is_pinned_without_holding_cache_lock_during_open() {
        let directory = Arc::new(crate::cache_dirs::CacheDirectory::new().unwrap());
        let path = directory.path().to_owned();
        std::fs::write(path.join("content"), b"cached packets").unwrap();
        let mut entries = HashMap::new();
        entries.insert(
            "cached".to_owned(),
            Entry {
                directory: directory.clone(),
                size: 14,
                used: Instant::now(),
                chunk: true,
            },
        );
        let cache = Mutex::new(Cache { entries, bytes: 14 });
        drop(directory);

        let pinned = cache.lock().await.hit("cached").unwrap();
        // Another session can acquire the cache while this session opens its
        // file. Its eviction predicate must see the in-flight lease.
        let mut concurrent = cache.try_lock().expect("cache lock released before I/O");
        assert_eq!(
            Arc::strong_count(&concurrent.entries["cached"].directory),
            2
        );
        concurrent.entries.remove("cached");
        drop(concurrent);
        assert!(path.exists());
        let opened = OpenedFile::open_in_root(&path, Path::new("content"))
            .await
            .unwrap()
            .with_cache_lease(pinned);
        let request = http::Request::builder().body(()).unwrap();
        let response = crate::stream::serve_opened(opened, &request, "audio/mp4")
            .await
            .unwrap();
        assert!(path.exists());
        // Cancellation releases the last lease even without reading the body.
        drop(response);
        assert!(!path.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "local I/O microbenchmark; run explicitly with --ignored --nocapture"]
    async fn benchmark_preparation_cache_open_concurrency() {
        use http_body_util::BodyExt;

        let directory = Arc::new(crate::cache_dirs::CacheDirectory::new().unwrap());
        std::fs::write(directory.path().join("content"), vec![42; 256 * 1024]).unwrap();
        let mut entries = HashMap::new();
        entries.insert(
            "cached".to_owned(),
            Entry {
                directory,
                size: 256 * 1024,
                used: Instant::now(),
                chunk: true,
            },
        );
        let cache = Arc::new(Mutex::new(Cache {
            entries,
            bytes: 256 * 1024,
        }));
        let mut measurements = Vec::new();
        // Alternate order to reduce filesystem warming/order bias. Both paths
        // use the same confined handle opening and bounded HTTP body; only
        // the cache mutex lifetime differs. No decoder/first-frame claim.
        for baseline in [true, false, false, true, true, false] {
            let start = Instant::now();
            let mut sessions = tokio::task::JoinSet::new();
            for _ in 0..8 {
                let cache = cache.clone();
                sessions.spawn(async move {
                    for _ in 0..64 {
                        let mut locked = cache.lock().await;
                        let directory = locked.hit("cached").unwrap();
                        let opened = if baseline {
                            let opened =
                                OpenedFile::open_in_root(directory.path(), Path::new("content"))
                                    .await
                                    .unwrap();
                            drop(locked);
                            opened
                        } else {
                            drop(locked);
                            OpenedFile::open_in_root(directory.path(), Path::new("content"))
                                .await
                                .unwrap()
                        };
                        let request = http::Request::builder().body(()).unwrap();
                        let mut response = crate::stream::serve_opened(
                            opened.with_cache_lease(directory),
                            &request,
                            "video/mp4",
                        )
                        .await
                        .unwrap();
                        let first = response
                            .body_mut()
                            .frame()
                            .await
                            .unwrap()
                            .unwrap()
                            .into_data()
                            .unwrap();
                        assert_eq!(first.len(), crate::stream::STREAM_CHUNK_BYTES);
                        assert!(first.iter().all(|byte| *byte == 42));
                        // Drop cancels the unread remainder, as a browser seek does.
                    }
                });
            }
            while let Some(result) = sessions.join_next().await {
                result.unwrap();
            }
            measurements.push(serde_json::json!({
                "strategy": if baseline { "mutex_during_open" } else { "lease_before_open" },
                "sessions": 8,
                "requests": 512,
                "bytes_per_request": crate::stream::STREAM_CHUNK_BYTES,
                "elapsed_ms": start.elapsed().as_secs_f64() * 1000.0,
            }));
        }
        println!(
            "{}",
            serde_json::json!({ "cache_open_benchmark": measurements })
        );
    }

    #[tokio::test]
    async fn preserves_eight_opus_channels_and_packets_in_both_containers() {
        let temp = tempfile::tempdir().unwrap();
        let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative/representative.mp4");
        let preparations = Preparations::default();
        for extension in ["mkv", "mp4"] {
            let name = format!("eight.{extension}");
            let status = Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(&reference)
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=48000:cl=7.1",
                    "-map",
                    "0:v:0",
                    "-map",
                    "1:a:0",
                    "-c:v",
                    "copy",
                    "-c:a",
                    "libopus",
                    "-mapping_family",
                    "1",
                    "-t",
                    "1",
                ])
                .arg(temp.path().join(&name))
                .status()
                .await
                .unwrap();
            assert!(status.success());
            let original = OpenedFile::open_in_root(temp.path(), Path::new(&name))
                .await
                .unwrap();
            let prepared = preparations
                .prepare(
                    temp.path(),
                    Path::new(&name),
                    original.size(),
                    original.modified_ns(),
                    Component::Audio(1),
                )
                .await
                .unwrap();
            let probe = crate::probe::probe_opened(&prepared).await.unwrap();
            assert_eq!(probe.streams[0].details["channels"], serde_json::json!(8));
            assert_eq!(
                probe.streams[0].details["channel_layout"],
                serde_json::json!("7.1")
            );
            assert_eq!(hashes(&original, 1).await, hashes(&prepared, 0).await);
            let chunk = preparations
                .prepare(
                    temp.path(),
                    Path::new(&name),
                    original.size(),
                    original.modified_ns(),
                    Component::AudioChunk(1, 0),
                )
                .await
                .unwrap();
            let probe = crate::probe::probe_opened(&chunk).await.unwrap();
            assert_eq!(probe.streams[0].details["channels"], 8);
            assert_eq!(probe.streams[0].details["channel_layout"], "7.1");
            let all = hashes(&original, 1).await;
            let copied = hashes(&chunk, 0).await;
            assert!(!copied.is_empty());
            assert!(copied.iter().all(|packet| all.contains(packet)));
        }
    }
    async fn hashes(file: &OpenedFile, index: u32) -> Vec<String> {
        let mut input = file.try_clone_std().unwrap();
        std::io::Seek::rewind(&mut input).unwrap();
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "fd,pipe",
                "-show_packets",
                "-show_data_hash",
                "sha256",
                "-show_entries",
                "packet=data_hash",
                "-select_streams",
                &index.to_string(),
                "-of",
                "json",
                "-",
            ])
            .stdin(Stdio::from(input))
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value["packets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|packet| packet["data_hash"].as_str().unwrap().to_owned())
            .collect()
    }
    #[tokio::test]
    #[ignore = "run python scripts/generate-dual-opus-fixture.py first; requires FFmpeg"]
    async fn copies_dual_51_opus_mp4_without_changing_packets_or_channels() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/dual-opus");
        let name = Path::new("dual-opus.mp4");
        let original = OpenedFile::open_in_root(&root, name).await.unwrap();
        let preparations = Preparations::default();
        for index in 0..=2 {
            let component = if index == 0 {
                Component::Video
            } else {
                Component::Audio(index)
            };
            let file = preparations
                .prepare(
                    &root,
                    name,
                    original.size(),
                    original.modified_ns(),
                    component,
                )
                .await
                .unwrap();
            let probe = crate::probe::probe_opened(&file).await.unwrap();
            assert_eq!(probe.streams.len(), 1);
            let track = &probe.streams[0];
            if index == 0 {
                assert_eq!(track.codec_name.as_deref(), Some("av1"));
                assert_eq!(track.details["pix_fmt"], "yuv420p10le");
                assert_eq!(track.details["width"], 1920);
                assert_eq!(track.details["height"], 1080);
            } else {
                assert_eq!(track.codec_name.as_deref(), Some("opus"));
                assert_eq!(track.details["channels"], 6);
                assert_eq!(track.details["channel_layout"], "5.1");
            }
            assert_eq!(hashes(&original, index).await, hashes(&file, 0).await);
        }
        let (first, second) = tokio::join!(
            preparations.prepare(
                &root,
                name,
                original.size(),
                original.modified_ns(),
                Component::AudioChunk(2, 0)
            ),
            preparations.prepare(
                &root,
                name,
                original.size(),
                original.modified_ns(),
                Component::AudioChunk(2, 0)
            )
        );
        assert_eq!(first.unwrap().size(), second.unwrap().size());
        assert_ne!(hashes(&original, 1).await, hashes(&original, 2).await);
        for index in [1, 2] {
            let all = hashes(&original, index).await;
            for chunk in [0, 1] {
                let file = preparations
                    .prepare(
                        &root,
                        name,
                        original.size(),
                        original.modified_ns(),
                        Component::AudioChunk(index, chunk),
                    )
                    .await
                    .unwrap();
                let probe = crate::probe::probe_opened(&file).await.unwrap();
                assert_eq!(probe.streams[0].codec_name.as_deref(), Some("opus"));
                assert_eq!(probe.streams[0].details["channels"], 6);
                assert_eq!(probe.streams[0].details["channel_layout"], "5.1");
                let copied = hashes(&file, 0).await;
                assert!(!copied.is_empty());
                assert!(copied.len() < all.len());
                assert!(
                    copied.iter().all(|packet| all.contains(packet)),
                    "chunk packets must be unchanged"
                );
                // Synthetic Opus uses 20 ms packets with a 6.5 ms pre-skip.
                // A preceding AV1 keyframe must never shift a chunk's content
                // back by a second while its MSE clock still appears aligned.
                let wanted = f64::from((chunk * AUDIO_CHUNK_SECONDS).saturating_sub(1));
                // A steady test tone can produce identical packets at multiple
                // positions. Require a match at the intended timestamp, and
                // independently check the remaining duration near EOF.
                assert!(
                    all.iter()
                        .enumerate()
                        .any(|(position, packet)| packet == &copied[0]
                            && (position as f64 * 0.02 - 0.0065 - wanted).abs() < 0.021),
                    "chunk must start within one Opus packet of {wanted}"
                );
                if chunk == 1 {
                    let duration = probe.format["duration"]
                        .as_str()
                        .unwrap()
                        .parse::<f64>()
                        .unwrap();
                    assert!(
                        (duration - 7.0).abs() < 0.04,
                        "preceding keyframe must not leak into window: {duration}"
                    );
                }
            }
        }
        let failed = preparations
            .prepare(
                &root,
                name,
                original.size(),
                original.modified_ns(),
                Component::AudioChunk(999, 0),
            )
            .await
            .unwrap_err();
        assert_eq!(preparation_error_code(&failed), "unsupported_preparation");
    }

    #[tokio::test]
    async fn copies_av1_opus_tracks_and_reuses_cache_with_source_validation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        let preparations = Preparations::default();
        for extension in ["mkv", "mp4"] {
            let name = format!("representative.{extension}");
            let original = OpenedFile::open_in_root(&root, Path::new(&name))
                .await
                .unwrap();
            let mut video_identity = None;
            for component in [
                Component::Video,
                Component::Audio(1),
                Component::Audio(2),
                Component::Subtitle(4),
            ] {
                let file = preparations
                    .prepare(
                        &root,
                        Path::new(&name),
                        original.size(),
                        original.modified_ns(),
                        component,
                    )
                    .await
                    .unwrap();
                assert!(file.size() > 0);
                if matches!(component, Component::Video) {
                    video_identity = Some(file.size());
                }
                if !matches!(component, Component::Subtitle(_)) {
                    let probe = crate::probe::probe_opened(&file).await.unwrap();
                    assert_eq!(probe.streams.len(), 1);
                    assert_eq!(
                        probe.streams[0].codec_name.as_deref(),
                        Some(if matches!(component, Component::Video) {
                            "av1"
                        } else {
                            "opus"
                        })
                    );
                    let index = match component {
                        Component::Audio(index) => index,
                        _ => 0,
                    };
                    assert_eq!(
                        hashes(&original, index).await,
                        hashes(&file, 0).await,
                        "packet copy must preserve AV1/Opus payloads"
                    );
                }
            }
            let reused = preparations
                .prepare(
                    &root,
                    Path::new(&name),
                    original.size(),
                    original.modified_ns(),
                    Component::Video,
                )
                .await
                .unwrap();
            assert_eq!(Some(reused.size()), video_identity);
            assert!(
                preparations
                    .prepare(
                        &root,
                        Path::new(&name),
                        original.size() + 1,
                        original.modified_ns(),
                        Component::Video
                    )
                    .await
                    .is_err()
            );
            assert!(
                preparations
                    .prepare(&root, Path::new("../outside.mkv"), 1, "0", Component::Video)
                    .await
                    .is_err()
            );
        }
    }
}
