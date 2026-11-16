//! Native trickplay generation and persistent LRU cache manager for Media Nodes.
//!
//! Media files are always accessed in read-only mode (`ro`). Video is never transcoded
//! for playback: FFmpeg is only used to extract low-resolution preview thumbnails.
//! Active streaming takes absolute priority over trickplay background jobs.

use crate::files::OpenedFile;
use image::ImageDecoder;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::{process::Command, sync::Semaphore};

pub const DEFAULT_COLUMNS: u32 = 10;
pub const DEFAULT_ROWS: u32 = 10;
pub const DEFAULT_WIDTH: u32 = 320;
pub const DEFAULT_QUALITY: u8 = 7;
pub const DEFAULT_INTERVAL_MS: u64 = 10_000;
pub const MAX_SHEETS: usize = 256;
pub const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_RUNTIME: Duration = Duration::from_secs(1800);
pub const FAILURE_COOLDOWN: Duration = Duration::from_secs(300);
pub const DEFAULT_MAX_CACHE_BYTES: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB default

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheManifest {
    pub version: u8,
    pub fingerprint: String,
    pub asset_id: String,
    pub sheet_count: usize,
    pub thumbnail_count: usize,
    pub interval_ms: u64,
    pub width: u32,
    pub height: u32,
    pub columns: u32,
    pub rows: u32,
    pub total_bytes: u64,
    pub completed: bool,
    #[serde(default = "jpeg_mime")]
    pub mime: String,
}

fn jpeg_mime() -> String {
    "image/jpeg".into()
}
fn sheet_extension(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct TrickplayRequest {
    pub asset_id: String,
    pub media_root: PathBuf,
    pub relative_path: PathBuf,
    pub duration: f64,
    pub size: u64,
    pub modified_ns: String,
    pub interval_ms: Option<u64>,
    pub width: Option<u32>,
    pub quality: Option<u8>,
    pub columns: Option<u32>,
    pub rows: Option<u32>,
    /// A real chapter boundary: extract one image rather than a periodic grid.
    pub chapter_time: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueStatus {
    Queued,
    Busy,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, Copy)]
enum JobState {
    Running,
    Failed(Instant),
}

type GridMap = HashMap<String, Vec<String>>;

#[derive(Clone)]
pub struct TrickplayManager {
    cache_root: PathBuf,
    /// Shared by every clone so an administrator change reaches running jobs.
    max_cache_bytes: Arc<AtomicU64>,
    /// `MEDIA_TRICKPLAY_MAX_CACHE_MB` is a deployment override: it wins over the server setting.
    cache_limit_from_env: bool,
    ffmpeg_binary: PathBuf,
    enabled: bool,
    active_streams: Arc<Mutex<Option<Arc<AtomicU32>>>>,
    jobs: Arc<Mutex<HashMap<String, JobState>>>,
    worker: Arc<Semaphore>,
    /// The first cache sweep of this process also removes stale crash leftovers.
    startup_swept: Arc<AtomicBool>,
    /// Media roots already verified not to contain `cache_root`.
    verified_media_roots: Arc<Mutex<HashSet<PathBuf>>>,
    /// Epoch milliseconds of the last `enforce_quota` sweep (`0` = never swept yet).
    last_quota_sweep_ms: Arc<AtomicU64>,
    /// Published bytes accumulated since the last `enforce_quota` sweep.
    bytes_since_quota_sweep: Arc<AtomicU64>,
    /// Cached `asset_id -> grid keys` index so `remove_superseded` avoids O(N^2) scans.
    grid_index: Arc<tokio::sync::Mutex<Option<GridMap>>>,
}

static GLOBAL_MANAGER: LazyLock<Arc<TrickplayManager>> =
    LazyLock::new(|| Arc::new(TrickplayManager::from_env()));
static WEBP_SUPPORT: LazyLock<tokio::sync::RwLock<HashMap<PathBuf, bool>>> =
    LazyLock::new(|| tokio::sync::RwLock::new(HashMap::new()));

pub fn global_manager() -> Arc<TrickplayManager> {
    GLOBAL_MANAGER.clone()
}

impl Default for TrickplayManager {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Parameters come only from authenticated Core requests, and remain bounded on
/// both direct HTTP and outbound-channel transports before looking up a cache.
pub fn parameters(query: &str) -> Option<(f64, u64, u32, u8)> {
    let values: HashMap<_, _> = query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .collect();
    let duration = values.get("duration")?.parse::<f64>().ok()?;
    let interval = values.get("intervalMs")?.parse::<u64>().ok()?;
    let width = values.get("width")?.parse::<u32>().ok()?;
    let quality = values.get("quality")?.parse::<u8>().ok()?;
    (duration.is_finite()
        && duration > 0.0
        && duration <= 86400.0
        && (1000..=60000).contains(&interval)
        && (160..=1920).contains(&width)
        && (1..=31).contains(&quality))
    .then_some((duration, interval, width, quality))
}

/// Optional sprite-sheet grid dimensions (`columns` × `rows`, default `10×10`).
pub fn grid_parameters(query: &str) -> Option<(u32, u32)> {
    let values: HashMap<_, _> = query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .collect();
    let cols = match values.get("columns") {
        Some(v) => v.parse::<u32>().ok()?,
        None => DEFAULT_COLUMNS,
    };
    let rows = match values.get("rows") {
        Some(v) => v.parse::<u32>().ok()?,
        None => DEFAULT_ROWS,
    };
    ((1..=20).contains(&cols) && (1..=20).contains(&rows)).then_some((cols, rows))
}

impl TrickplayManager {
    pub fn new(
        cache_root: PathBuf,
        max_cache_bytes: u64,
        ffmpeg_binary: PathBuf,
        enabled: bool,
        active_streams: Option<Arc<AtomicU32>>,
    ) -> Self {
        Self {
            cache_root,
            max_cache_bytes: Arc::new(AtomicU64::new(max_cache_bytes)),
            cache_limit_from_env: false,
            ffmpeg_binary,
            enabled,
            active_streams: Arc::new(Mutex::new(active_streams)),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            worker: Arc::new(Semaphore::const_new(4)),
            startup_swept: Arc::new(AtomicBool::new(false)),
            verified_media_roots: Arc::new(Mutex::new(HashSet::new())),
            last_quota_sweep_ms: Arc::new(AtomicU64::new(0)),
            bytes_since_quota_sweep: Arc::new(AtomicU64::new(0)),
            grid_index: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn from_env() -> Self {
        // Relative to the deployment's data directory, never the (possibly read-only) CWD.
        let cache_root = crate::data_subdir("MEDIA_TRICKPLAY_CACHE_DIR", "trickplay");

        let env_limit = std::env::var("MEDIA_TRICKPLAY_MAX_CACHE_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1024 * 1024));
        let max_cache_bytes = env_limit.unwrap_or(DEFAULT_MAX_CACHE_BYTES);

        let ffmpeg_binary = std::env::var_os("MEDIA_TRICKPLAY_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));

        let enabled = !matches!(
            std::env::var("MEDIA_TRICKPLAY_GENERATE").as_deref(),
            Ok("0" | "false" | "off")
        );

        let mut manager = Self::new(cache_root, max_cache_bytes, ffmpeg_binary, enabled, None);
        manager.cache_limit_from_env = env_limit.is_some();
        manager
    }

    /// Applies the administrator's cache limit (server setting `trickplayMaxCacheMb`),
    /// unless the deployment pinned one through `MEDIA_TRICKPLAY_MAX_CACHE_MB`.
    pub fn set_max_cache_mb(&self, mb: u32) {
        if self.cache_limit_from_env || mb == 0 {
            return;
        }
        self.max_cache_bytes
            .store(u64::from(mb) * 1024 * 1024, Ordering::Relaxed);
    }

    pub fn max_cache_bytes(&self) -> u64 {
        self.max_cache_bytes.load(Ordering::Relaxed)
    }

    pub fn with_active_streams(self, counter: Arc<AtomicU32>) -> Self {
        self.set_active_streams(counter);
        self
    }

    pub fn set_active_streams(&self, counter: Arc<AtomicU32>) {
        *self
            .active_streams
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(counter);
    }

    pub fn active_streams(&self) -> usize {
        self.active_streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |counter| {
                let count = counter.load(Ordering::Relaxed);
                usize::try_from(count).unwrap_or(usize::MAX)
            })
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Compute reproducible fingerprint for an asset version and trickplay parameters.
    pub fn fingerprint(&self, request: &TrickplayRequest) -> String {
        let interval = request.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS);
        let width = request.width.unwrap_or(DEFAULT_WIDTH);
        let quality = request.quality.unwrap_or(DEFAULT_QUALITY);

        let mut data = Vec::with_capacity(256);
        data.extend_from_slice(request.asset_id.as_bytes());
        data.push(b':');
        data.extend_from_slice(request.relative_path.to_string_lossy().as_bytes());
        data.push(b':');
        data.extend_from_slice(request.modified_ns.as_bytes());
        data.push(b':');
        data.extend_from_slice(&request.size.to_le_bytes());
        data.push(b':');
        data.extend_from_slice(&request.duration.to_bits().to_le_bytes());
        data.push(b':');
        data.extend_from_slice(&interval.to_le_bytes());
        data.push(b':');
        data.extend_from_slice(&width.to_le_bytes());
        data.push(b':');
        data.push(quality);
        if let Some(time) = request.chapter_time {
            data.extend_from_slice(b":chapter:");
            data.extend_from_slice(&time.to_bits().to_le_bytes());
        } else {
            let cols = request.columns.unwrap_or(DEFAULT_COLUMNS);
            let rows = request.rows.unwrap_or(DEFAULT_ROWS);
            if cols != DEFAULT_COLUMNS || rows != DEFAULT_ROWS {
                data.extend_from_slice(b":grid:");
                data.extend_from_slice(&cols.to_le_bytes());
                data.push(b'x');
                data.extend_from_slice(&rows.to_le_bytes());
            }
        }

        media_contracts::protocol::sha256_hex(&data)
    }

    /// Delete the prepared cache of one request (sheets of a file, or one chapter image) so
    /// the next warm-up rebuilds it. The folder name is a SHA-256 of the request, never a
    /// client path, and only a folder directly under the cache root is removed. Returns
    /// whether a prepared folder existed.
    pub async fn purge(&self, request: &TrickplayRequest) -> bool {
        let key = self.fingerprint(request);
        if !cache_key(&key) {
            return false;
        }
        let target = self.cache_root.join(&key);
        let existed = tokio::fs::metadata(&target)
            .await
            .is_ok_and(|meta| meta.is_dir());
        if existed {
            remove_dir_tolerant(&target).await;
        }
        existed
    }

    /// Check for an existing, completed native Aether trickplay cache.
    pub async fn check_native_cache(&self, request: &TrickplayRequest) -> Option<CacheManifest> {
        let key = self.fingerprint(request);
        let target_dir = self.cache_root.join(&key);
        let manifest_bytes = tokio::fs::read(target_dir.join("manifest.json"))
            .await
            .ok()?;
        let meta: CacheManifest = serde_json::from_slice(&manifest_bytes).ok()?;

        if !matches!(meta.version, 1 | 2)
            || sheet_extension(&meta.mime).is_none()
            || !meta.completed
            || meta.fingerprint != key
            || meta.sheet_count == 0
            || meta.sheet_count > MAX_SHEETS
            || meta.columns == 0
            || meta.rows == 0
            || meta.width == 0
            || meta.height == 0
        {
            return None;
        }
        // Legacy JPEG sheets are regenerated as WebP once this Node's FFmpeg can encode it.
        if meta.mime == "image/jpeg" && self.enabled && self.supports_libwebp().await {
            return None;
        }

        // Validate that the first and last sheets actually exist on disk and are non-empty.
        let extension = sheet_extension(&meta.mime)?;
        let first_file = target_dir.join(format!("0.{extension}"));
        let last_file = target_dir.join(format!("{}.{extension}", meta.sheet_count - 1));
        if !first_file.is_file() || !last_file.is_file() {
            return None;
        }

        let first_meta = tokio::fs::metadata(&first_file).await.ok()?;
        let last_meta = tokio::fs::metadata(&last_file).await.ok()?;
        if first_meta.len() == 0
            || first_meta.len() > MAX_IMAGE_BYTES
            || last_meta.len() == 0
            || last_meta.len() > MAX_IMAGE_BYTES
        {
            return None;
        }

        Some(meta)
    }

    /// Enqueue trickplay generation on the local Media Node.
    /// Resumes any partially generated sheets without re-decoding from the beginning.
    pub fn enqueue(&self, request: TrickplayRequest) -> QueueStatus {
        if !self.enabled {
            return QueueStatus::Disabled;
        }

        let key = self.fingerprint(&request);
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            match jobs.get(&key) {
                Some(JobState::Running) => return QueueStatus::Queued,
                Some(JobState::Failed(when)) if when.elapsed() < FAILURE_COOLDOWN => {
                    return QueueStatus::Failed;
                }
                _ => {}
            }
            jobs.retain(|_, state| match state {
                JobState::Running => true,
                JobState::Failed(when) => when.elapsed() < FAILURE_COOLDOWN,
            });
            if jobs.len() >= 8 {
                return QueueStatus::Busy;
            }
            jobs.insert(key.clone(), JobState::Running);
        }

        let permits = if request.chapter_time.is_some() { 1 } else { 2 };
        let this = Arc::new(self.clone());
        tokio::spawn(async move {
            let outcome = match this.worker.acquire_many(permits).await {
                Ok(_permit) => {
                    // Respect playback priority: yield if an active playback stream is running
                    let counter = this
                        .active_streams
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    if let Some(counter) = counter
                        && counter.load(Ordering::Relaxed) > 0
                    {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    this.execute_generation(&request, &key).await
                }
                Err(_) => Err("worker_closed"),
            };

            let mut jobs = this.jobs.lock().unwrap_or_else(|e| e.into_inner());
            match outcome {
                Ok(_) => {
                    jobs.remove(&key);
                }
                Err(err) => {
                    eprintln!(
                        "trickplay generation failed for {}: {}",
                        request.asset_id, err
                    );
                    jobs.insert(key, JobState::Failed(Instant::now()));
                }
            }
        });

        QueueStatus::Queued
    }

    async fn supports_libwebp(&self) -> bool {
        if let Some(supported) = WEBP_SUPPORT.read().await.get(&self.ffmpeg_binary) {
            return *supported;
        }
        let supported = match Command::new(&self.ffmpeg_binary)
            .args(["-hide_banner", "-encoders"])
            .output()
            .await
        {
            Ok(output) => String::from_utf8_lossy(&output.stdout).contains("libwebp"),
            Err(_) => false,
        };
        WEBP_SUPPORT
            .write()
            .await
            .insert(self.ffmpeg_binary.clone(), supported);
        supported
    }

    /// Durable task work shares the same worker pool and operator kill switch
    /// as previews requested by a player. Full grids serialize against each other
    /// and against chapters; up to two single-frame chapter previews may share it.
    /// The Core/Node task runner handles cancellation and playback preemption
    /// while this future waits or decodes.
    pub async fn generate_background(
        &self,
        request: &TrickplayRequest,
        key: &str,
    ) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("generation_disabled");
        }
        if self.check_native_cache(request).await.is_some() {
            return Ok(());
        }
        let permits = if request.chapter_time.is_some() { 1 } else { 2 };
        let _permit = self
            .worker
            .acquire_many(permits)
            .await
            .map_err(|_| "worker_closed")?;
        self.execute_generation(request, key).await
    }

    async fn ensure_cache_outside_media(&self, media_root: &Path) -> bool {
        if self
            .verified_media_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(media_root)
        {
            return true;
        }
        if !cache_outside_media(&self.cache_root, media_root).await {
            return false;
        }
        let mut guard = self
            .verified_media_roots
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.len() < 128 {
            guard.insert(media_root.to_path_buf());
        }
        true
    }

    /// Execute the FFmpeg generation, resuming partial work if present.
    pub async fn execute_generation(
        &self,
        request: &TrickplayRequest,
        key: &str,
    ) -> Result<(), &'static str> {
        if key != self.fingerprint(request) {
            return Err("invalid_fingerprint");
        }
        if !self.ensure_cache_outside_media(&request.media_root).await {
            return Err("cache_inside_media_denied");
        }
        let opened = OpenedFile::open_in_root(&request.media_root, &request.relative_path)
            .await
            .map_err(|_| "source_missing")?;

        if opened.size() != request.size || opened.modified_ns() != request.modified_ns {
            return Err("source_changed");
        }
        if !request.duration.is_finite() || request.duration <= 0.0 || request.duration > 86_400.0 {
            return Err("invalid_duration");
        }

        let interval_ms = request.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS);
        let width = request.width.unwrap_or(DEFAULT_WIDTH);
        let quality = request.quality.unwrap_or(DEFAULT_QUALITY);
        let req_cols = request.columns.unwrap_or(DEFAULT_COLUMNS);
        let req_rows = request.rows.unwrap_or(DEFAULT_ROWS);
        if !(160..=1920).contains(&width)
            || !(1..=31).contains(&quality)
            || !(1..=20).contains(&req_cols)
            || !(1..=20).contains(&req_rows)
            || interval_ms == 0
            || request
                .chapter_time
                .is_some_and(|time| !time.is_finite() || time < 0.0 || time >= request.duration)
        {
            return Err("invalid_generation_parameters");
        }
        let cols = if request.chapter_time.is_some() {
            1
        } else {
            req_cols.min((8192 / width).max(1))
        };
        let rows = if request.chapter_time.is_some() {
            1
        } else {
            req_rows.min((8192 / (width * 2)).max(1))
        };

        let frame_count = if request.chapter_time.is_some() {
            1
        } else {
            (request.duration * 1000.0 / interval_ms as f64).ceil() as usize
        };
        let sheet_limit = frame_count.div_ceil((cols * rows) as usize).min(MAX_SHEETS);
        if sheet_limit == 0 {
            return Err("invalid_frame_count");
        }

        tokio::fs::create_dir_all(&self.cache_root)
            .await
            .map_err(|_| "cache_permission")?;

        let target = self.cache_root.join(key);
        if self.check_native_cache(request).await.is_some() {
            return Ok(());
        }

        let use_webp = self.supports_libwebp().await;
        let ext = if use_webp { "webp" } else { "jpg" };
        let mime = if use_webp { "image/webp" } else { "image/jpeg" };
        let version = if use_webp { 2 } else { 1 };

        // Use a deterministic in-progress folder to enable resumable generation across restarts
        let work = self.cache_root.join(format!(".{key}.{ext}.in_progress"));
        tokio::fs::create_dir_all(&work)
            .await
            .map_err(|_| "cache_permission")?;

        // Determine already completed sheets to resume from. Checking metadata on earlier
        // sheets and decoding only the last committed sheet avoids decoding every prior
        // sprite sheet into RAM on resume.
        let mut completed_sheets = 0usize;
        while completed_sheets < sheet_limit {
            let sheet_path = work.join(format!("{completed_sheets}.{ext}"));
            let Ok(meta) = tokio::fs::metadata(&sheet_path).await else {
                break;
            };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
                if meta.is_file() {
                    let _ = tokio::fs::remove_file(&sheet_path).await;
                }
                break;
            }
            completed_sheets += 1;
        }
        if completed_sheets > 0 {
            let last_idx = completed_sheets - 1;
            let last_name = format!("{last_idx}.{ext}");
            let last_path = work.join(&last_name);
            let opened_last = OpenedFile::open_in_root(&work, Path::new(&last_name))
                .await
                .map_err(|_| "generated_image_invalid")?;
            if !validate_sheet(opened_last).await {
                // An interrupted encoder can leave a non-empty truncated final sheet.
                // Only this uncommitted work file is discarded before resuming it.
                tokio::fs::remove_file(&last_path)
                    .await
                    .map_err(|_| "cache_permission")?;
                completed_sheets = last_idx;
            }
        }

        if completed_sheets < sheet_limit {
            let use_threads = if self.active_streams() == 0 { "4" } else { "2" };
            self.run_ffmpeg_resumable(
                request,
                &opened,
                &work,
                completed_sheets,
                sheet_limit,
                interval_ms,
                width,
                quality,
                cols,
                rows,
                use_webp,
                use_threads,
            )
            .await?;
        }

        // Verify and calculate final sheet count and total bytes
        let mut count = 0usize;
        let mut total_bytes = 0u64;
        let mut first_dims = None;

        while count < sheet_limit {
            let sheet_path = work.join(format!("{count}.{ext}"));
            let Ok(meta) = tokio::fs::metadata(&sheet_path).await else {
                break;
            };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
                return Err("generated_image_invalid");
            }
            if first_dims.is_none()
                && let Ok(opened) =
                    OpenedFile::open_in_root(&work, Path::new(&format!("{count}.{ext}"))).await
            {
                first_dims = decode_opened_image_dimensions(opened).await;
            }
            total_bytes = total_bytes.saturating_add(meta.len());
            count += 1;
        }

        if count == 0 {
            return Err("no_frames_generated");
        }

        let (tot_w, tot_h) = first_dims.ok_or("generated_image_invalid")?;
        let (thumb_w, thumb_h) = (tot_w / cols, tot_h / rows);

        let thumbnails = frame_count.min(count * (cols * rows) as usize);

        let manifest = CacheManifest {
            version,
            mime: mime.into(),
            fingerprint: key.to_owned(),
            asset_id: request.asset_id.clone(),
            sheet_count: count,
            thumbnail_count: thumbnails,
            interval_ms,
            width: thumb_w,
            height: thumb_h,
            columns: cols,
            rows,
            total_bytes,
            completed: true,
        };

        let encoded = serde_json::to_vec(&manifest).map_err(|_| "manifest_encode")?;
        tokio::fs::write(work.join("manifest.json"), encoded)
            .await
            .map_err(|_| "manifest_write")?;

        match publish_work_dir(&work, &target).await {
            Ok(_) => {
                if request.chapter_time.is_none() {
                    self.remove_superseded(&request.asset_id, key).await;
                }
                self.enforce_quota_if_due(total_bytes).await;
                Ok(())
            }
            Err(_) if self.check_native_cache(request).await.is_some() => {
                let _ = tokio::fs::remove_dir_all(&work).await;
                self.enforce_quota_if_due(0).await;
                Ok(())
            }
            Err(_) => {
                let _ = tokio::fs::remove_dir_all(&work).await;
                Err("cache_publish_failed")
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_ffmpeg_resumable(
        &self,
        request: &TrickplayRequest,
        opened: &OpenedFile,
        output: &Path,
        start_sheet: usize,
        sheet_limit: usize,
        interval_ms: u64,
        width: u32,
        quality: u8,
        cols: u32,
        rows: u32,
        use_webp: bool,
        use_threads: &str,
    ) -> Result<(), &'static str> {
        let source_path = request.media_root.join(&request.relative_path);
        let remaining_sheets = sheet_limit - start_sheet;
        let sheet_capacity = (cols * rows) as f64;
        let start_time_secs = request
            .chapter_time
            .unwrap_or((start_sheet as f64 * sheet_capacity) * (interval_ms as f64 / 1000.0));

        // FFmpeg filterchain:
        // format=yuv420p guarantees seamless compatibility with AV1 10-bit, HEVC 10-bit, VP9, etc.
        let filter = if request.chapter_time.is_some() {
            format!("scale={width}:-2:flags=fast_bilinear,format=yuv420p")
        } else {
            format!(
                "fps=1000/{interval_ms},scale={width}:-2:flags=fast_bilinear,format=yuv420p,tile={cols}x{rows}"
            )
        };
        let skip_nokey =
            request.chapter_time.is_none() && interval_ms >= 2_000 && request.duration >= 30.0;
        let active_counter = self
            .active_streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let runtime = if request.chapter_time.is_some() {
            Duration::from_secs(60)
        } else {
            MAX_RUNTIME
        };
        let ext = if use_webp { "webp" } else { "jpg" };
        let first_sheet = output.join(format!("{start_sheet}.{ext}"));

        if skip_nokey {
            let piped = self
                .run_ffmpeg_bsf_pipe(
                    &source_path,
                    output,
                    start_time_secs,
                    start_sheet,
                    remaining_sheets,
                    &filter,
                    quality,
                    use_webp,
                    use_threads,
                    active_counter.clone(),
                    runtime,
                )
                .await;
            match piped {
                Ok(()) => {
                    let produced = tokio::fs::metadata(&first_sheet)
                        .await
                        .is_ok_and(|meta| meta.is_file() && meta.len() > 0);
                    if produced {
                        opened
                            .verify_unchanged_sync()
                            .map_err(|_| "source_changed")?;
                        return Ok(());
                    }
                }
                Err(err @ ("streaming_priority_yield" | "generation_timeout")) => return Err(err),
                Err(_) => {}
            }
            for idx in start_sheet..sheet_limit {
                let partial = output.join(format!("{idx}.{ext}"));
                if tokio::fs::metadata(&partial).await.is_ok() {
                    let _ = tokio::fs::remove_file(&partial).await;
                } else {
                    break;
                }
            }
        }

        let mut cmd = Command::new(&self.ffmpeg_binary);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-thread_type",
            "slice",
            "-threads",
            use_threads,
            "-filter_threads",
            "1",
            "-flags2",
            "fast",
        ]);
        if skip_nokey {
            cmd.args(["-discard", "nokey", "-skip_frame", "nokey"]);
        }
        if start_time_secs > 0.0 {
            cmd.arg("-ss").arg(format!("{:.3}", start_time_secs));
        }
        cmd.arg("-n")
            .arg("-i")
            .arg(&source_path)
            .args(["-map", "0:v:0", "-an", "-sn", "-dn"]);
        append_ffmpeg_output_args(
            &mut cmd,
            start_sheet,
            remaining_sheets,
            &filter,
            quality,
            use_webp,
            output,
        );
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let mut child = cmd.spawn().map_err(|_| "ffmpeg_unavailable")?;
        wait_ffmpeg_with_priority(active_counter, runtime, &mut child, None).await?;

        opened
            .verify_unchanged_sync()
            .map_err(|_| "source_changed")?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_ffmpeg_bsf_pipe(
        &self,
        source_path: &Path,
        output: &Path,
        start_time_secs: f64,
        start_sheet: usize,
        remaining_sheets: usize,
        filter: &str,
        quality: u8,
        use_webp: bool,
        use_threads: &str,
        active_counter: Option<Arc<AtomicU32>>,
        runtime: Duration,
    ) -> Result<(), &'static str> {
        let mut feeder_cmd = Command::new(&self.ffmpeg_binary);
        feeder_cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if start_time_secs > 0.0 {
            feeder_cmd.arg("-ss").arg(format!("{:.3}", start_time_secs));
        }
        feeder_cmd
            .arg("-i")
            .arg(source_path)
            .args([
                "-map",
                "0:v:0",
                "-an",
                "-sn",
                "-dn",
                "-c:v",
                "copy",
                "-bsf:v",
                "noise=drop=not(key)",
                "-f",
                "matroska",
                "-",
            ])
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        let mut feeder = feeder_cmd.spawn().map_err(|_| "ffmpeg_unavailable")?;
        let Some(feeder_stdout) = feeder.stdout.take() else {
            let _ = feeder.kill().await;
            let _ = feeder.wait().await;
            return Err("generation_process_error");
        };
        let pipe_in: std::process::Stdio = match feeder_stdout.try_into() {
            Ok(stdio) => stdio,
            Err(_) => {
                let _ = feeder.kill().await;
                let _ = feeder.wait().await;
                return Err("generation_process_error");
            }
        };

        let mut decoder_cmd = Command::new(&self.ffmpeg_binary);
        decoder_cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-thread_type",
            "slice",
            "-threads",
            use_threads,
            "-filter_threads",
            "1",
            "-flags2",
            "fast",
            "-discard",
            "nokey",
            "-skip_frame",
            "nokey",
            "-f",
            "matroska",
            "-i",
            "-",
            "-n",
        ]);
        append_ffmpeg_output_args(
            &mut decoder_cmd,
            start_sheet,
            remaining_sheets,
            filter,
            quality,
            use_webp,
            output,
        );
        decoder_cmd
            .stdin(pipe_in)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let mut decoder = match decoder_cmd.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = feeder.kill().await;
                let _ = feeder.wait().await;
                return Err("ffmpeg_unavailable");
            }
        };

        wait_ffmpeg_with_priority(active_counter, runtime, &mut decoder, Some(&mut feeder)).await
    }

    /// A modified source or a new profile publishes under a new fingerprint: earlier
    /// preview grids of the same asset are obsolete and removed (chapter images stay).
    async fn remove_superseded(&self, asset_id: &str, keep: &str) {
        let mut guard = self.grid_index.lock().await;
        if guard.is_none() {
            let mut built: HashMap<String, Vec<String>> = HashMap::new();
            if let Ok(mut read_dir) = tokio::fs::read_dir(&self.cache_root).await {
                while let Ok(Some(entry)) = read_dir.next_entry().await {
                    if !entry.file_type().await.is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !cache_key(&name) {
                        continue;
                    }
                    let Ok(bytes) = tokio::fs::read(entry.path().join("manifest.json")).await
                    else {
                        continue;
                    };
                    let Ok(meta) = serde_json::from_slice::<CacheManifest>(&bytes) else {
                        continue;
                    };
                    if meta.columns > 1 || meta.rows > 1 {
                        built.entry(meta.asset_id).or_default().push(name);
                    }
                }
            }
            *guard = Some(built);
        }
        let Some(index) = guard.as_mut() else {
            return;
        };
        let keys = index.entry(asset_id.to_owned()).or_default();
        let mut obsolete = Vec::new();
        keys.retain(|existing| {
            if existing.as_str() == keep {
                true
            } else {
                obsolete.push(existing.clone());
                false
            }
        });
        if !keys.iter().any(|existing| existing.as_str() == keep) {
            keys.push(keep.to_owned());
        }
        drop(guard);

        for old_key in obsolete {
            remove_dir_tolerant(&self.cache_root.join(old_key)).await;
        }
    }

    async fn enforce_quota_if_due(&self, added_bytes: u64) {
        let prev = self
            .bytes_since_quota_sweep
            .fetch_add(added_bytes, Ordering::Relaxed);
        let threshold = (self.max_cache_bytes() / 8).max(1);
        let last_ms = self.last_quota_sweep_ms.load(Ordering::Acquire);
        let now_ms = now_epoch_ms();
        if last_ms == 0
            || prev.saturating_add(added_bytes) >= threshold
            || now_ms.saturating_sub(last_ms) > 60_000
        {
            self.enforce_quota().await;
        }
    }

    /// Enforce global cache size quota using LRU eviction.
    pub async fn enforce_quota(&self) {
        self.bytes_since_quota_sweep.store(0, Ordering::Release);
        self.last_quota_sweep_ms
            .store(now_epoch_ms(), Ordering::Release);
        let Ok(mut read_dir) = tokio::fs::read_dir(&self.cache_root).await else {
            return;
        };
        let mut entries = Vec::new();
        let mut total_size = 0u64;
        // At startup no generation of this process is running yet: in-progress folders older
        // than an hour (or already published) are crash leftovers. Younger ones stay resumable.
        let startup = !self.startup_swept.swap(true, Ordering::AcqRel);
        let stale_after = if startup {
            Duration::from_secs(3600)
        } else {
            Duration::from_secs(86400)
        };

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let Ok(meta) = tokio::fs::symlink_metadata(entry.path()).await else {
                continue;
            };
            if crate::files::is_linked(&meta) || !meta.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();

            // Clean orphaned crash directories older than 1 hour
            let partial = name
                .strip_prefix('.')
                .and_then(|s| s.strip_suffix(".in_progress"))
                .map(|s| {
                    s.strip_suffix(".webp")
                        .or_else(|| s.strip_suffix(".jpg"))
                        .unwrap_or(s)
                });
            if partial.is_some_and(cache_key) {
                let active = self
                    .jobs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(partial.unwrap_or(""))
                    .is_some_and(|s| matches!(s, JobState::Running));
                let published = partial.is_some_and(|key| self.cache_root.join(key).is_dir());
                let expired = meta
                    .modified()
                    .ok()
                    .and_then(|mtime| mtime.elapsed().ok())
                    .is_some_and(|age| age > stale_after);
                if !active && (expired || (startup && published)) {
                    remove_dir_tolerant(&entry.path()).await;
                }
                continue;
            }

            if cache_key(&name) {
                let dir_size = dir_total_bytes(&entry.path()).await;
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                total_size = total_size.saturating_add(dir_size);
                entries.push((entry.path(), mtime, dir_size));
            }
        }

        let max_cache_bytes = self.max_cache_bytes();
        if total_size <= max_cache_bytes {
            return;
        }

        // Sort by mtime ascending (oldest first)
        entries.sort_by_key(|(_, mtime, _)| *mtime);
        let target_size = (max_cache_bytes * 8) / 10; // Evict to 80% watermark
        let mut evicted = HashSet::new();

        for (path, _, size) in entries {
            if total_size <= target_size {
                break;
            }
            if tokio::fs::remove_dir_all(&path).await.is_ok() {
                total_size = total_size.saturating_sub(size);
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    evicted.insert(name.to_owned());
                }
            }
        }

        if !evicted.is_empty() {
            let mut guard = self.grid_index.lock().await;
            if let Some(index) = guard.as_mut() {
                index.retain(|_, keys| {
                    keys.retain(|k| !evicted.contains(k));
                    !keys.is_empty()
                });
            }
        }
    }

    /// Open a single sheet image from the native Aether cache.
    pub async fn open_sheet(
        &self,
        request: &TrickplayRequest,
        index: usize,
    ) -> io::Result<OpenedFile> {
        self.open_sheet_with_mime(request, index)
            .await
            .map(|(file, _)| file)
    }
    pub async fn open_sheet_with_mime(
        &self,
        request: &TrickplayRequest,
        index: usize,
    ) -> io::Result<(OpenedFile, String)> {
        if index >= MAX_SHEETS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sheet_index_out_of_bounds",
            ));
        }

        // Check Aether native cache
        let key = self.fingerprint(request);
        let target_dir = self.cache_root.join(&key);
        if let Some(meta) = self.check_native_cache(request).await
            && index < meta.sheet_count
            && let Some(extension) = sheet_extension(&meta.mime)
            && let Ok(opened) =
                OpenedFile::open_in_root(&target_dir, Path::new(&format!("{index}.{extension}")))
                    .await
            && opened.size() > 0
            && opened.size() <= MAX_IMAGE_BYTES
        {
            return Ok((opened, meta.mime));
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "trickplay_sheet_not_found",
        ))
    }
}

fn append_ffmpeg_output_args(
    cmd: &mut Command,
    start_sheet: usize,
    remaining_sheets: usize,
    filter: &str,
    quality: u8,
    use_webp: bool,
    output: &Path,
) {
    let ext = if use_webp { "webp" } else { "jpg" };
    cmd.args(["-fps_mode", "vfr", "-vf", filter])
        .arg("-frames:v")
        .arg(remaining_sheets.to_string());
    if use_webp {
        // JPEG quality scale 1..=31 (default 7) maps inverse-linearly to libwebp 20..=95 (7 -> 75).
        let webp_q = 99u32
            .saturating_sub(u32::from(quality.saturating_sub(1)) * 4)
            .clamp(20, 95);
        cmd.args([
            "-c:v",
            "libwebp",
            "-preset",
            "picture",
            "-compression_level",
            "0",
        ])
        .arg("-q:v")
        .arg(webp_q.to_string());
    } else {
        cmd.arg("-q:v").arg(quality.to_string());
    }
    cmd.arg("-start_number")
        .arg(start_sheet.to_string())
        .args(["-f", "image2"])
        .arg(output.join(format!("%d.{ext}")))
        .kill_on_drop(true);
}

async fn kill_and_reap(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn wait_ffmpeg_with_priority(
    active_counter: Option<Arc<AtomicU32>>,
    runtime: Duration,
    child: &mut tokio::process::Child,
    mut upstream: Option<&mut tokio::process::Child>,
) -> Result<(), &'static str> {
    let deadline = tokio::time::Instant::now() + runtime;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Some(ref counter) = active_counter
                    && counter.load(Ordering::Relaxed) > 0
                {
                    kill_and_reap(child).await;
                    if let Some(feeder) = upstream.as_deref_mut() {
                        kill_and_reap(feeder).await;
                    }
                    return Err("streaming_priority_yield");
                }
                if tokio::time::Instant::now() >= deadline {
                    kill_and_reap(child).await;
                    if let Some(feeder) = upstream.as_deref_mut() {
                        kill_and_reap(feeder).await;
                    }
                    return Err("generation_timeout");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(_) => {
                kill_and_reap(child).await;
                if let Some(feeder) = upstream.as_deref_mut() {
                    kill_and_reap(feeder).await;
                }
                return Err("generation_process_error");
            }
        }
    };

    if let Some(feeder) = upstream
        && !matches!(feeder.try_wait(), Ok(Some(_)))
    {
        kill_and_reap(feeder).await;
    }
    if !status.success() {
        return Err("ffmpeg_failed");
    }
    Ok(())
}

fn now_epoch_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
    .max(1)
}

fn cache_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

async fn cache_outside_media(cache: &Path, media: &Path) -> bool {
    let Ok(media) = tokio::fs::canonicalize(media).await else {
        return false;
    };
    let absolute = if cache.is_absolute() {
        cache.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(root) => root.join(cache),
            Err(_) => return false,
        }
    };
    if absolute
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            return false;
        };
        suffix.push(name.to_owned());
        let Some(parent) = ancestor.parent() else {
            return false;
        };
        ancestor = parent;
    }
    let Ok(mut resolved) = tokio::fs::canonicalize(ancestor).await else {
        return false;
    };
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    !resolved.starts_with(media)
}

async fn decode_opened_image_dimensions(opened: OpenedFile) -> Option<(u32, u32)> {
    tokio::task::spawn_blocking(move || {
        let reader = std::io::BufReader::new(opened.std_file());
        let decoder = image::ImageReader::new(reader)
            .with_guessed_format()
            .ok()?
            .into_decoder()
            .ok()?;
        let (w, h) = decoder.dimensions();
        if w > 0 && h > 0 && w <= 8192 && h <= 8192 {
            Some((w, h))
        } else {
            None
        }
    })
    .await
    .ok()
    .flatten()
}

async fn validate_sheet(opened: OpenedFile) -> bool {
    tokio::task::spawn_blocking(move || {
        let mut reader = image::ImageReader::new(std::io::BufReader::new(opened.std_file()))
            .with_guessed_format()
            .ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        reader.decode().ok().map(|_| ())
    })
    .await
    .ok()
    .flatten()
    .is_some()
}

async fn dir_total_bytes(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(mut rd) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            if let Ok(meta) = entry.metadata().await
                && meta.is_file()
            {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

/// Remove a directory tree; files held open by another process (Windows antivirus, indexers)
/// are retried briefly, then left for a later sweep instead of failing.
async fn remove_dir_tolerant(path: &Path) {
    for attempt in 0..3u32 {
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(_) if attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
            }
            Err(error) => {
                eprintln!("trickplay: stale work folder kept for a later sweep: {error}");
            }
        }
    }
}

/// Publish a finished work folder under its final name. Windows refuses to rename a folder
/// while one of its files is still held open (antivirus scan right after ffmpeg wrote it) or
/// onto an existing folder: retry with backoff, replace an existing incomplete target, and as a
/// last resort copy the (flat) folder before removing the work copy.
async fn publish_work_dir(work: &Path, target: &Path) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..5u32 {
        match tokio::fs::rename(work, target).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if attempt == 0
                    && tokio::fs::symlink_metadata(target)
                        .await
                        .is_ok_and(|meta| meta.is_dir() && !crate::files::is_linked(&meta))
                {
                    // A previous publication was interrupted: the work folder supersedes it.
                    let _ = tokio::fs::remove_dir_all(target).await;
                }
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(50 << attempt)).await;
            }
        }
    }
    let staging = target.with_extension("publishing");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    let copied = async {
        tokio::fs::create_dir(&staging).await?;
        let mut entries = tokio::fs::read_dir(work).await?;
        while let Some(entry) = entries.next_entry().await? {
            let meta = tokio::fs::symlink_metadata(entry.path()).await?;
            if meta.is_file() && !crate::files::is_linked(&meta) {
                tokio::fs::copy(entry.path(), staging.join(entry.file_name())).await?;
            }
        }
        tokio::fs::rename(&staging, target).await
    }
    .await;
    match copied {
        Ok(()) => {
            remove_dir_tolerant(work).await;
            Ok(())
        }
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            Err(last_error.unwrap_or(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn webp_native_legacy_jpeg_and_jellyfin_have_matching_mime_and_survive_restart() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(media.join("film.mkv"), b"synthetic video identity only").unwrap();
        let opened = OpenedFile::open_in_root(&media, Path::new("film.mkv"))
            .await
            .unwrap();
        let request = TrickplayRequest {
            asset_id: "fixture".into(),
            media_root: media.clone(),
            relative_path: "film.mkv".into(),
            duration: 10.0,
            size: opened.size(),
            modified_ns: opened.modified_ns().into(),
            interval_ms: None,
            width: None,
            quality: None,
            columns: None,
            rows: None,
            chapter_time: None,
        };
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        // No FFmpeg here: legacy JPEG caches stay valid when WebP cannot be produced.
        let manager = TrickplayManager::new(
            cache.clone(),
            16 * 1024 * 1024,
            "aether-missing-ffmpeg".into(),
            true,
            None,
        );
        let key = manager.fingerprint(&request);
        let image = image::DynamicImage::new_rgb8(320, 180);
        // Native Aether engine ignores external .trickplay folders placed in media directories.
        let external = media.join("film.mkv.trickplay").join("320 - 1x1");
        std::fs::create_dir_all(&external).unwrap();
        image
            .save_with_format(external.join("0.jpg"), image::ImageFormat::Jpeg)
            .unwrap();
        assert!(manager.check_native_cache(&request).await.is_none());
        assert!(manager.open_sheet_with_mime(&request, 0).await.is_err());
        for (version, mime, extension, format) in [
            (1, "image/jpeg", "jpg", image::ImageFormat::Jpeg),
            (2, "image/webp", "webp", image::ImageFormat::WebP),
        ] {
            let target = cache.join(&key);
            std::fs::create_dir_all(&target).unwrap();
            image
                .save_with_format(target.join(format!("0.{extension}")), format)
                .unwrap();
            let mut metadata = serde_json::json!({"version":version,"fingerprint":key,"assetId":"fixture","sheetCount":1,"thumbnailCount":1,"intervalMs":10000,"width":320,"height":180,"columns":1,"rows":1,"totalBytes":100,"completed":true});
            if version == 2 {
                metadata["mime"] = serde_json::json!(mime);
            }
            std::fs::write(
                target.join("manifest.json"),
                serde_json::to_vec(&metadata).unwrap(),
            )
            .unwrap();
            let restarted = TrickplayManager::new(
                cache.clone(),
                16 * 1024 * 1024,
                "aether-missing-ffmpeg".into(),
                true,
                None,
            );
            assert_eq!(
                restarted.check_native_cache(&request).await.unwrap().mime,
                mime
            );
            let (opened, actual) = restarted.open_sheet_with_mime(&request, 0).await.unwrap();
            assert_eq!(actual, mime);
            assert!(validate_sheet(opened).await);
        }
        assert_eq!(
            std::fs::read(media.join("film.mkv")).unwrap(),
            b"synthetic video identity only"
        );
        assert!(!media.join("0.webp").exists());
    }

    #[tokio::test]
    async fn publication_replaces_an_interrupted_target_and_stale_work_is_swept() {
        let temp = tempfile::tempdir().unwrap();
        let work = temp.path().join(".key.in_progress");
        let target = temp.path().join("key");
        tokio::fs::create_dir(&work).await.unwrap();
        tokio::fs::write(work.join("0.jpg"), b"sheet")
            .await
            .unwrap();
        tokio::fs::create_dir(&target).await.unwrap();
        tokio::fs::write(target.join("partial"), b"old")
            .await
            .unwrap();
        publish_work_dir(&work, &target).await.unwrap();
        assert!(!work.exists());
        assert!(target.join("0.jpg").exists());
        assert!(!target.join("partial").exists());
        remove_dir_tolerant(&temp.path().join("absent")).await;
    }

    #[test]
    fn fingerprint_differs_on_modified_source_or_params() {
        let mgr = TrickplayManager::from_env();
        let base = TrickplayRequest {
            asset_id: "asset-1".into(),
            media_root: "/media".into(),
            relative_path: "film.mp4".into(),
            duration: 120.0,
            size: 50_000,
            modified_ns: "100".into(),
            interval_ms: Some(10_000),
            width: Some(320),
            quality: Some(7),
            columns: None,
            rows: None,
            chapter_time: None,
        };
        let fp1 = mgr.fingerprint(&base);

        let mut mod_default_grid = base.clone();
        mod_default_grid.columns = Some(10);
        mod_default_grid.rows = Some(10);
        assert_eq!(fp1, mgr.fingerprint(&mod_default_grid));

        let mut mod_custom_grid = base.clone();
        mod_custom_grid.columns = Some(5);
        mod_custom_grid.rows = Some(5);
        assert_ne!(fp1, mgr.fingerprint(&mod_custom_grid));

        let mut mod_time = base.clone();
        mod_time.modified_ns = "101".into();
        assert_ne!(fp1, mgr.fingerprint(&mod_time));

        let mut mod_width = base.clone();
        mod_width.width = Some(240);
        assert_ne!(fp1, mgr.fingerprint(&mod_width));

        let mut mod_interval = base.clone();
        mod_interval.interval_ms = Some(5_000);
        assert_ne!(fp1, mgr.fingerprint(&mod_interval));
        let mut chapter = base.clone();
        chapter.chapter_time = Some(10.0);
        assert_ne!(fp1, mgr.fingerprint(&chapter));
        let first = mgr.fingerprint(&chapter);
        chapter.chapter_time = Some(20.0);
        assert_ne!(first, mgr.fingerprint(&chapter));
    }

    #[test]
    fn signed_trickplay_parameters_bound_both_transports() {
        assert_eq!(
            parameters("duration=120&intervalMs=5000&width=410&quality=12"),
            Some((120.0, 5000, 410, 12))
        );
        for query in [
            "duration=NaN&intervalMs=5000&width=320&quality=7",
            "duration=0&intervalMs=5000&width=320&quality=7",
            "duration=120&intervalMs=0&width=320&quality=7",
            "duration=120&intervalMs=5000&width=9999&quality=7",
            "duration=120&intervalMs=5000&width=320&quality=0",
            "",
        ] {
            assert!(parameters(query).is_none(), "{query}");
        }
    }

    #[tokio::test]
    async fn failed_background_jobs_update_the_shared_admission_state() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = TrickplayManager::new(
            tmp.path().join("cache"),
            1024 * 1024,
            "missing-ffmpeg".into(),
            true,
            None,
        );
        let request = TrickplayRequest {
            asset_id: "missing".into(),
            media_root: tmp.path().into(),
            relative_path: "missing.mp4".into(),
            duration: 12.0,
            size: 1,
            modified_ns: "0".into(),
            interval_ms: None,
            width: Some(320),
            quality: Some(7),
            columns: None,
            rows: None,
            chapter_time: Some(2.0),
        };
        assert_eq!(manager.enqueue(request.clone()), QueueStatus::Queued);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if manager.enqueue(request.clone()) == QueueStatus::Failed {
                    break;
                }
            }
        })
        .await
        .expect("the manager must observe the worker's failure, not retain Running forever");
    }

    #[tokio::test]
    #[ignore = "requires FFmpeg and generated legal representative fixture"]
    async fn chapter_image_extracts_one_frame_and_survives_manager_restart() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative/representative.mp4")
            .canonicalize()
            .unwrap();
        let root = source.parent().unwrap();
        let relative = PathBuf::from(source.file_name().unwrap());
        let opened = OpenedFile::open_in_root(root, &relative).await.unwrap();
        let before = tokio::fs::read(&source).await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let manager = TrickplayManager::new(
            tmp.path().join("cache"),
            16 * 1024 * 1024,
            "ffmpeg".into(),
            true,
            None,
        );
        let request = TrickplayRequest {
            asset_id: "chapter-fixture".into(),
            media_root: root.into(),
            relative_path: relative,
            duration: 12.0,
            size: opened.size(),
            modified_ns: opened.modified_ns().into(),
            interval_ms: None,
            width: Some(320),
            quality: Some(7),
            columns: None,
            rows: None,
            chapter_time: Some(2.0),
        };
        let key = manager.fingerprint(&request);
        manager.execute_generation(&request, &key).await.unwrap();
        let manifest = manager.check_native_cache(&request).await.unwrap();
        assert_eq!(
            (
                manifest.columns,
                manifest.rows,
                manifest.sheet_count,
                manifest.thumbnail_count
            ),
            (1, 1, 1, 1)
        );
        assert_eq!(manifest.width, 320);
        let image = manager.open_sheet(&request, 0).await.unwrap();
        assert_eq!(decode_opened_image_dimensions(image).await.unwrap().0, 320);
        let restarted = TrickplayManager::new(
            tmp.path().join("cache"),
            16 * 1024 * 1024,
            "ffmpeg".into(),
            true,
            None,
        );
        assert!(restarted.check_native_cache(&request).await.is_some());
        let mut changed = request;
        changed.modified_ns = "changed".into();
        assert!(restarted.check_native_cache(&changed).await.is_none());
        assert_eq!(tokio::fs::read(&source).await.unwrap(), before);
    }

    #[tokio::test]
    async fn enforce_quota_evicts_oldest_directories() {
        let temp = tempfile::tempdir().unwrap();
        let cache_root = temp.path().to_path_buf();
        // Set quota to 2.5 MB
        let mgr = TrickplayManager::new(
            cache_root.clone(),
            2_500_000,
            PathBuf::from("ffmpeg"),
            true,
            None,
        );

        // Create 3 mock cache directories of 1 MB each
        let dir1 = cache_root.join("1".repeat(64));
        let dir2 = cache_root.join("2".repeat(64));
        let dir3 = cache_root.join("3".repeat(64));
        tokio::fs::create_dir(&dir1).await.unwrap();
        tokio::fs::create_dir(&dir2).await.unwrap();
        tokio::fs::create_dir(&dir3).await.unwrap();

        let dummy = vec![0u8; 1_000_000];
        tokio::fs::write(dir1.join("0.jpg"), &dummy).await.unwrap();
        tokio::fs::write(dir2.join("0.jpg"), &dummy).await.unwrap();
        tokio::fs::write(dir3.join("0.jpg"), &dummy).await.unwrap();
        let unrelated = cache_root.join("personal-files");
        tokio::fs::create_dir(&unrelated).await.unwrap();
        tokio::fs::write(unrelated.join("keep.txt"), b"not a derivative")
            .await
            .unwrap();

        // Total is 3 MB > 2.5 MB quota
        mgr.enforce_quota().await;

        let mut remaining = 0;
        if dir1.exists() {
            remaining += 1;
        }
        if dir2.exists() {
            remaining += 1;
        }
        if dir3.exists() {
            remaining += 1;
        }
        assert_eq!(remaining, 2);
        assert!(unrelated.join("keep.txt").exists());
    }

    #[tokio::test]
    async fn purge_removes_only_the_prepared_folder_of_that_request() {
        let temp = tempfile::tempdir().unwrap();
        let manager = TrickplayManager::new(
            temp.path().to_path_buf(),
            1_000_000,
            PathBuf::from("ffmpeg"),
            true,
            None,
        );
        let request = |asset: &str| TrickplayRequest {
            asset_id: asset.into(),
            media_root: PathBuf::from("/media"),
            relative_path: PathBuf::from("film.mkv"),
            duration: 60.0,
            size: 1,
            modified_ns: "1".into(),
            interval_ms: Some(10_000),
            width: Some(320),
            quality: Some(7),
            columns: Some(10),
            rows: Some(10),
            chapter_time: None,
        };
        let (mine, other) = (request("mine"), request("other"));
        for r in [&mine, &other] {
            let dir = temp.path().join(manager.fingerprint(r));
            tokio::fs::create_dir(&dir).await.unwrap();
            tokio::fs::write(dir.join("0.webp"), b"x").await.unwrap();
        }
        assert!(manager.purge(&mine).await);
        assert!(!temp.path().join(manager.fingerprint(&mine)).exists());
        assert!(
            temp.path()
                .join(manager.fingerprint(&other))
                .join("0.webp")
                .exists()
        );
        // Nothing prepared: nothing to delete, and no error.
        assert!(!manager.purge(&mine).await);
    }

    #[tokio::test]
    async fn chapter_cache_cannot_write_beneath_the_media_root() {
        let root = tempfile::tempdir().unwrap();
        assert!(!cache_outside_media(&root.path().join("new-cache/nested"), root.path()).await);
        let outside = tempfile::tempdir().unwrap();
        assert!(cache_outside_media(&outside.path().join("new-cache"), root.path()).await);
    }

    #[tokio::test]
    async fn ensure_cache_outside_media_does_not_bypass_unsafe_roots() {
        let temp = tempfile::tempdir().unwrap();
        let safe_root = temp.path().join("safe-media");
        let unsafe_root = temp.path().join("parent");
        let cache_root = unsafe_root.join("cache");
        tokio::fs::create_dir_all(&safe_root).await.unwrap();
        tokio::fs::create_dir_all(&cache_root).await.unwrap();
        let streams = Arc::new(AtomicU32::new(0));
        let mgr = TrickplayManager::new(
            cache_root,
            1024 * 1024,
            PathBuf::from("ffmpeg"),
            true,
            Some(streams.clone()),
        );
        assert_eq!(mgr.active_streams(), 0);
        streams.store(2, Ordering::Relaxed);
        assert_eq!(mgr.active_streams(), 2);

        assert!(mgr.ensure_cache_outside_media(&safe_root).await);
        assert!(mgr.ensure_cache_outside_media(&safe_root).await);
        assert!(!mgr.ensure_cache_outside_media(&unsafe_root).await);
    }

    #[tokio::test]
    async fn enforce_quota_if_due_batches_small_writes_until_threshold() {
        let temp = tempfile::tempdir().unwrap();
        let cache_root = temp.path().to_path_buf();
        // Quota = 800 KB -> threshold = 100 KB
        let mgr = TrickplayManager::new(
            cache_root.clone(),
            800_000,
            PathBuf::from("ffmpeg"),
            true,
            None,
        );
        let dir1 = cache_root.join("a".repeat(64));
        let dir2 = cache_root.join("b".repeat(64));
        tokio::fs::create_dir(&dir1).await.unwrap();
        tokio::fs::write(dir1.join("0.webp"), vec![0u8; 500_000])
            .await
            .unwrap();

        // First call sweeps (last_quota_sweep_ms == 0) and initializes sweep state.
        mgr.enforce_quota_if_due(1_000).await;
        assert!(mgr.last_quota_sweep_ms.load(Ordering::Acquire) > 0);
        assert_eq!(mgr.bytes_since_quota_sweep.load(Ordering::Acquire), 0);

        // Add another 500 KB directory directly; a small 10 KB notification stays under
        // the 100 KB threshold and skips the directory scan.
        tokio::fs::create_dir(&dir2).await.unwrap();
        tokio::fs::write(dir2.join("0.webp"), vec![0u8; 500_000])
            .await
            .unwrap();
        mgr.enforce_quota_if_due(10_000).await;
        assert!(dir1.exists() && dir2.exists());

        // Crossing 100 KB triggers enforce_quota and evicts down to <= 80% of 800 KB.
        mgr.enforce_quota_if_due(95_000).await;
        assert_eq!(mgr.bytes_since_quota_sweep.load(Ordering::Acquire), 0);
        assert_ne!(dir1.exists(), dir2.exists());
    }

    #[tokio::test]
    async fn remove_superseded_uses_index_and_preserves_chapter_images() {
        let temp = tempfile::tempdir().unwrap();
        let cache_root = temp.path().to_path_buf();
        let mgr = TrickplayManager::new(
            cache_root.clone(),
            1024 * 1024,
            PathBuf::from("ffmpeg"),
            true,
            None,
        );
        let old_grid = "1".repeat(64);
        let chapter_key = "2".repeat(64);
        let other_grid = "3".repeat(64);
        let new_grid_a = "4".repeat(64);
        let new_grid_b = "5".repeat(64);

        for (key, asset, cols, rows) in [
            (&old_grid, "film-1", 10u32, 10u32),
            (&chapter_key, "film-1", 1, 1),
            (&other_grid, "film-2", 10, 10),
        ] {
            let dir = cache_root.join(key);
            tokio::fs::create_dir(&dir).await.unwrap();
            let manifest = serde_json::json!({
                "version": 2,
                "fingerprint": key,
                "assetId": asset,
                "sheetCount": 1,
                "thumbnailCount": 1,
                "intervalMs": 10000,
                "width": 320,
                "height": 180,
                "columns": cols,
                "rows": rows,
                "totalBytes": 100,
                "completed": true,
                "mime": "image/webp"
            });
            tokio::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .await
            .unwrap();
        }

        tokio::fs::create_dir(cache_root.join(&new_grid_a))
            .await
            .unwrap();
        mgr.remove_superseded("film-1", &new_grid_a).await;
        assert!(!cache_root.join(&old_grid).exists());
        assert!(cache_root.join(&chapter_key).exists());
        assert!(cache_root.join(&other_grid).exists());

        // Second supersede uses the in-memory index and deletes new_grid_a.
        tokio::fs::create_dir(cache_root.join(&new_grid_b))
            .await
            .unwrap();
        mgr.remove_superseded("film-1", &new_grid_b).await;
        assert!(!cache_root.join(&new_grid_a).exists());
        assert!(cache_root.join(&new_grid_b).exists());
    }

    #[tokio::test]
    async fn resumes_from_in_progress_sheets_and_drops_only_truncated_tail() {
        let temp = tempfile::tempdir().unwrap();
        let media_root = temp.path().join("media");
        let cache_root = temp.path().join("cache");
        tokio::fs::create_dir_all(&media_root).await.unwrap();
        let movie = media_root.join("movie.mkv");
        tokio::fs::write(&movie, b"video").await.unwrap();
        let opened = OpenedFile::open_in_root(&media_root, Path::new("movie.mkv"))
            .await
            .unwrap();

        let mgr = TrickplayManager::new(
            cache_root.clone(),
            16 * 1024 * 1024,
            PathBuf::from("nonexistent-ffmpeg-binary"),
            true,
            None,
        );
        let req = TrickplayRequest {
            asset_id: "asset-resume".into(),
            media_root,
            relative_path: PathBuf::from("movie.mkv"),
            duration: 20.0,
            size: opened.size(),
            modified_ns: opened.modified_ns().to_owned(),
            interval_ms: Some(10_000),
            width: Some(320),
            quality: Some(7),
            columns: Some(1),
            rows: Some(1),
            chapter_time: None,
        };
        let key = mgr.fingerprint(&req);
        let work = cache_root.join(format!(".{key}.jpg.in_progress"));
        tokio::fs::create_dir_all(&work).await.unwrap();

        let mut jpg = Vec::new();
        let img = image::DynamicImage::new_rgb8(320, 180);
        img.write_to(
            &mut std::io::Cursor::new(&mut jpg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();

        // Corrupt last sheet (1.jpg) leaves 0.jpg intact and attempts resume via ffmpeg at sheet 1.
        tokio::fs::write(work.join("0.jpg"), &jpg).await.unwrap();
        tokio::fs::write(work.join("1.jpg"), b"truncated-partial-jpeg")
            .await
            .unwrap();
        assert_eq!(
            mgr.execute_generation(&req, &key).await.unwrap_err(),
            "ffmpeg_unavailable"
        );
        assert!(work.join("0.jpg").exists(), "committed sheet 0 kept");
        assert!(!work.join("1.jpg").exists(), "truncated sheet 1 dropped");

        // Completing 1.jpg lets execute_generation publish cleanly without spawning ffmpeg.
        tokio::fs::write(work.join("1.jpg"), &jpg).await.unwrap();
        mgr.execute_generation(&req, &key).await.unwrap();
        let manifest = mgr.check_native_cache(&req).await.unwrap();
        assert_eq!(manifest.sheet_count, 2);
        assert_eq!(manifest.thumbnail_count, 2);
        assert!(!work.exists());
    }
}
