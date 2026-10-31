//! Native trickplay generation, Jellyfin-compatible sprite-sheet inspection,
//! and persistent LRU cache manager for Media Nodes.
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
        atomic::{AtomicU32, Ordering},
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
pub struct TrickplayManifest {
    pub status: &'static str,
    pub source: &'static str,
    pub interval_ms: u64,
    pub width: u32,
    pub height: u32,
    pub columns: u32,
    pub rows: u32,
    pub sheet_count: usize,
    pub thumbnail_count: usize,
    pub url_prefix: String,
}

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

pub struct TrickplayManager {
    cache_root: PathBuf,
    max_cache_bytes: u64,
    ffmpeg_binary: PathBuf,
    enabled: bool,
    active_streams: Option<Arc<AtomicU32>>,
    jobs: Mutex<HashMap<String, JobState>>,
    worker: Semaphore,
}

static GLOBAL_MANAGER: LazyLock<Arc<TrickplayManager>> =
    LazyLock::new(|| Arc::new(TrickplayManager::from_env()));

pub fn global_manager() -> Arc<TrickplayManager> {
    GLOBAL_MANAGER.clone()
}

impl Default for TrickplayManager {
    fn default() -> Self {
        Self::from_env()
    }
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
            max_cache_bytes,
            ffmpeg_binary,
            enabled,
            active_streams,
            jobs: Mutex::new(HashMap::new()),
            worker: Semaphore::const_new(1),
        }
    }

    pub fn from_env() -> Self {
        let cache_root = std::env::var_os("MEDIA_TRICKPLAY_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/trickplay"));

        let max_cache_bytes = std::env::var("MEDIA_TRICKPLAY_MAX_CACHE_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb * 1024 * 1024)
            .unwrap_or(DEFAULT_MAX_CACHE_BYTES);

        let ffmpeg_binary = std::env::var_os("MEDIA_TRICKPLAY_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ffmpeg"));

        let enabled = !matches!(
            std::env::var("MEDIA_TRICKPLAY_GENERATE").as_deref(),
            Ok("0" | "false" | "off")
        );

        Self::new(cache_root, max_cache_bytes, ffmpeg_binary, enabled, None)
    }

    pub fn with_active_streams(mut self, counter: Arc<AtomicU32>) -> Self {
        self.active_streams = Some(counter);
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
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

        media_contracts::protocol::sha256_hex(&data)
    }

    /// Check for an existing, completed native Aether trickplay cache.
    pub async fn check_native_cache(&self, request: &TrickplayRequest) -> Option<CacheManifest> {
        let key = self.fingerprint(request);
        let target_dir = self.cache_root.join(&key);
        let manifest_bytes = tokio::fs::read(target_dir.join("manifest.json"))
            .await
            .ok()?;
        let meta: CacheManifest = serde_json::from_slice(&manifest_bytes).ok()?;

        if meta.version != 1
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

        // Validate that the first and last sheets actually exist on disk and are non-empty.
        let first_file = target_dir.join("0.jpg");
        let last_file = target_dir.join(format!("{}.jpg", meta.sheet_count - 1));
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

    /// Detect existing Jellyfin-style trickplay sheets beside an inventoried video file.
    /// Supports dynamic resolutions (e.g. 320, 240), grids (e.g. 10x10, 5x5) and intervals.
    pub async fn check_jellyfin(&self, request: &TrickplayRequest) -> Option<TrickplayManifest> {
        let video = &request.relative_path;
        let root = &request.media_root;

        // Try both folder conventions:
        // 1. "Video.mp4.trickplay"
        // 2. "Video.trickplay"
        let parent = video.parent().unwrap_or_else(|| Path::new(""));
        let file_name = video.file_name()?.to_string_lossy();
        let stem = video.file_stem()?.to_string_lossy();

        let candidates = [
            parent.join(format!("{file_name}.trickplay")),
            parent.join(format!("{stem}.trickplay")),
        ];

        for folder in candidates {
            if let Some(manifest) = self.probe_jellyfin_folder(root, &folder, request).await {
                return Some(manifest);
            }
        }
        None
    }

    async fn probe_jellyfin_folder(
        &self,
        root: &Path,
        trickplay_rel: &Path,
        request: &TrickplayRequest,
    ) -> Option<TrickplayManifest> {
        let full_dir = root.join(trickplay_rel);
        let mut read_dir = tokio::fs::read_dir(&full_dir).await.ok()?;

        let mut candidate_subdirs = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let file_type = entry.file_type().await.ok()?;
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                candidate_subdirs.push(name);
            }
        }

        // Check if there is an explicit manifest/metadata JSON in the trickplay folder
        let meta_json = self.read_json_metadata(root, trickplay_rel).await;

        for subdir_name in candidate_subdirs {
            let sheet_rel = trickplay_rel.join(&subdir_name);
            let parsed_grid = parse_grid_folder_name(&subdir_name);

            // Open 0.jpg securely via OpenedFile to ensure confinement
            let sheet_0_rel = sheet_rel.join("0.jpg");
            let opened_0 = OpenedFile::open_in_root(root, &sheet_0_rel).await.ok()?;
            if opened_0.size() == 0 || opened_0.size() > MAX_IMAGE_BYTES {
                continue;
            }

            // Decode image dimensions
            let dims = decode_opened_jpeg_dimensions(opened_0).await?;
            let (total_w, total_h) = dims;

            let (cols, rows, width, height) = if let Some((w, c, r)) = parsed_grid {
                if total_w % c == 0 && total_h % r == 0 {
                    (c, r, total_w / c, total_h / r)
                } else {
                    (c, r, w, (w * 9) / 16)
                }
            } else if let Some(ref meta) = meta_json {
                (meta.columns, meta.rows, meta.width, meta.height)
            } else if total_w % DEFAULT_COLUMNS == 0 && total_h % DEFAULT_ROWS == 0 {
                (
                    DEFAULT_COLUMNS,
                    DEFAULT_ROWS,
                    total_w / DEFAULT_COLUMNS,
                    total_h / DEFAULT_ROWS,
                )
            } else {
                continue;
            };

            if cols == 0 || rows == 0 || width == 0 || height == 0 {
                continue;
            }

            // Count contiguous sheets
            let sheet_count = count_contiguous_sheets(root, &sheet_rel).await;
            if sheet_count == 0 {
                continue;
            }

            let interval_ms = meta_json
                .as_ref()
                .map(|m| m.interval_ms)
                .or(request.interval_ms)
                .unwrap_or(DEFAULT_INTERVAL_MS);

            let total_capacity = sheet_count * (cols * rows) as usize;
            let thumbnail_count = if request.duration > 0.0 {
                let needed = (request.duration * 1000.0 / interval_ms as f64).ceil() as usize;
                needed.min(total_capacity)
            } else {
                total_capacity
            };

            if thumbnail_count == 0 {
                continue;
            }

            return Some(TrickplayManifest {
                status: "ready",
                source: "jellyfin",
                interval_ms,
                width,
                height,
                columns: cols,
                rows,
                sheet_count,
                thumbnail_count,
                url_prefix: format!("/api/v1/assets/{}/trickplay/", request.asset_id),
            });
        }

        None
    }

    async fn read_json_metadata(&self, root: &Path, rel: &Path) -> Option<JellyfinJsonMeta> {
        for file_name in ["manifest.json", "metadata.json", "trickplay.json"] {
            let path = rel.join(file_name);
            if let Ok(opened) = OpenedFile::open_in_root(root, &path).await
                && opened.size() > 0
                && opened.size() <= 64 * 1024
            {
                let full_path = root.join(&path);
                if let Ok(bytes) = tokio::fs::read(full_path).await
                    && let Ok(meta) = serde_json::from_slice::<JellyfinJsonMeta>(&bytes)
                {
                    return Some(meta);
                }
            }
        }
        None
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

        let this = Arc::new(self.clone_internal());
        tokio::spawn(async move {
            let outcome = match this.worker.acquire().await {
                Ok(_permit) => {
                    // Respect playback priority: yield if an active playback stream is running
                    if let Some(ref counter) = this.active_streams
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

    fn clone_internal(&self) -> Self {
        Self {
            cache_root: self.cache_root.clone(),
            max_cache_bytes: self.max_cache_bytes,
            ffmpeg_binary: self.ffmpeg_binary.clone(),
            enabled: self.enabled,
            active_streams: self.active_streams.clone(),
            jobs: Mutex::new(HashMap::new()),
            worker: Semaphore::const_new(1),
        }
    }

    /// Execute the FFmpeg generation, resuming partial work if present.
    pub async fn execute_generation(
        &self,
        request: &TrickplayRequest,
        key: &str,
    ) -> Result<(), &'static str> {
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
        let cols = DEFAULT_COLUMNS;
        let rows = DEFAULT_ROWS;

        let frame_count = (request.duration * 1000.0 / interval_ms as f64).ceil() as usize;
        let sheet_limit = frame_count.div_ceil((cols * rows) as usize).min(MAX_SHEETS);
        if sheet_limit == 0 {
            return Err("invalid_frame_count");
        }

        tokio::fs::create_dir_all(&self.cache_root)
            .await
            .map_err(|_| "cache_permission")?;

        // Evict LRU entries if quota is exceeded
        self.enforce_quota().await;

        let target = self.cache_root.join(key);
        if self.check_native_cache(request).await.is_some() {
            return Ok(());
        }

        // Use a deterministic in-progress folder to enable resumable generation across restarts
        let work = self.cache_root.join(format!(".{key}.in_progress"));
        tokio::fs::create_dir_all(&work)
            .await
            .map_err(|_| "cache_permission")?;

        // Determine already completed sheets to resume from
        let mut completed_sheets = 0usize;
        while completed_sheets < sheet_limit {
            let sheet_path = work.join(format!("{completed_sheets}.jpg"));
            let Ok(meta) = tokio::fs::metadata(&sheet_path).await else {
                break;
            };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
                break;
            }
            completed_sheets += 1;
        }

        if completed_sheets < sheet_limit {
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
            )
            .await?;
        }

        // Verify and calculate final sheet count and total bytes
        let mut count = 0usize;
        let mut total_bytes = 0u64;
        let mut first_dims = None;

        while count < sheet_limit {
            let sheet_path = work.join(format!("{count}.jpg"));
            let Ok(meta) = tokio::fs::metadata(&sheet_path).await else {
                break;
            };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_IMAGE_BYTES {
                return Err("generated_image_invalid");
            }
            if first_dims.is_none()
                && let Ok(opened) =
                    OpenedFile::open_in_root(&work, Path::new(&format!("{count}.jpg"))).await
            {
                first_dims = decode_opened_jpeg_dimensions(opened).await;
            }
            total_bytes = total_bytes.saturating_add(meta.len());
            count += 1;
        }

        if count == 0 {
            return Err("no_frames_generated");
        }

        let (thumb_w, thumb_h) = if let Some((tot_w, tot_h)) = first_dims {
            (tot_w / cols, tot_h / rows)
        } else {
            (width, (width * 9) / 16)
        };

        let thumbnails = frame_count.min(count * (cols * rows) as usize);

        let manifest = CacheManifest {
            version: 1,
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

        match tokio::fs::rename(&work, &target).await {
            Ok(_) => {
                self.enforce_quota().await;
                Ok(())
            }
            Err(_) if self.check_native_cache(request).await.is_some() => {
                let _ = tokio::fs::remove_dir_all(&work).await;
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
    ) -> Result<(), &'static str> {
        let source_path = request.media_root.join(&request.relative_path);
        let remaining_sheets = sheet_limit - start_sheet;
        let sheet_capacity = (cols * rows) as f64;
        let start_time_secs = (start_sheet as f64 * sheet_capacity) * (interval_ms as f64 / 1000.0);

        // FFmpeg filterchain:
        // format=yuvj420p guarantees seamless compatibility with AV1 10-bit, HEVC 10-bit, VP9, etc.
        let filter = format!(
            "fps=1000/{interval_ms},scale={width}:-2:flags=fast_bilinear,format=yuvj420p,tile={cols}x{rows}"
        );

        let mut cmd = Command::new(&self.ffmpeg_binary);
        cmd.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-threads",
            "1",
            "-filter_threads",
            "1",
        ]);

        if start_time_secs > 0.0 {
            cmd.arg("-ss").arg(format!("{:.3}", start_time_secs));
        }

        cmd.arg("-n")
            .arg("-i")
            .arg(&source_path)
            .args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf"])
            .arg(filter)
            .args([
                "-frames:v",
                &remaining_sheets.to_string(),
                "-q:v",
                &quality.to_string(),
                "-start_number",
                &start_sheet.to_string(),
                "-f",
                "image2",
            ])
            .arg(output.join("%d.jpg"))
            .kill_on_drop(true);

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let mut child = cmd.spawn().map_err(|_| "ffmpeg_unavailable")?;

        // Monitor active streaming and yield if priority streaming starts
        let active_counter = self.active_streams.clone();
        let wait_result = if let Some(counter) = active_counter {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            let start = Instant::now();
            loop {
                tokio::select! {
                    status = child.wait() => {
                        break status.map_err(|_| "generation_process_error");
                    }
                    _ = interval.tick() => {
                        if counter.load(Ordering::Relaxed) > 0 {
                            // Yield to streaming: terminate child gracefully, keeping partial progress
                            let _ = child.kill().await;
                            return Err("streaming_priority_yield");
                        }
                        if start.elapsed() > MAX_RUNTIME {
                            let _ = child.kill().await;
                            return Err("generation_timeout");
                        }
                    }
                }
            }
        } else {
            tokio::time::timeout(MAX_RUNTIME, child.wait())
                .await
                .map_err(|_| "generation_timeout")?
                .map_err(|_| "generation_process_error")
        };

        let status = wait_result?;
        if !status.success() {
            return Err("generation_failed");
        }

        opened
            .verify_unchanged_sync()
            .map_err(|_| "source_changed")?;
        Ok(())
    }

    /// Enforce global cache size quota using LRU eviction.
    pub async fn enforce_quota(&self) {
        let Ok(mut read_dir) = tokio::fs::read_dir(&self.cache_root).await else {
            return;
        };
        let mut entries = Vec::new();
        let mut total_size = 0u64;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();

            // Clean orphaned crash directories older than 1 hour
            if name.starts_with('.') && name.ends_with(".tmp") {
                if let Ok(mtime) = meta.modified()
                    && mtime
                        .elapsed()
                        .map(|d| d > Duration::from_secs(3600))
                        .unwrap_or(false)
                {
                    let _ = tokio::fs::remove_dir_all(entry.path()).await;
                }
                continue;
            }

            if meta.is_dir() && !name.starts_with('.') {
                let dir_size = dir_total_bytes(&entry.path()).await;
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                total_size = total_size.saturating_add(dir_size);
                entries.push((entry.path(), mtime, dir_size));
            }
        }

        if total_size <= self.max_cache_bytes {
            return;
        }

        // Sort by mtime ascending (oldest first)
        entries.sort_by_key(|(_, mtime, _)| *mtime);
        let target_size = (self.max_cache_bytes * 8) / 10; // Evict to 80% watermark

        for (path, _, size) in entries {
            if total_size <= target_size {
                break;
            }
            if tokio::fs::remove_dir_all(&path).await.is_ok() {
                total_size = total_size.saturating_sub(size);
            }
        }
    }

    /// Open a single sheet JPEG from either native cache or Jellyfin read-only directory.
    pub async fn open_sheet(
        &self,
        request: &TrickplayRequest,
        index: usize,
    ) -> io::Result<OpenedFile> {
        if index >= MAX_SHEETS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sheet_index_out_of_bounds",
            ));
        }

        // 1. Check Aether native cache first
        let key = self.fingerprint(request);
        let target_dir = self.cache_root.join(&key);
        let sheet_name = format!("{index}.jpg");
        if let Ok(opened) = OpenedFile::open_in_root(&target_dir, Path::new(&sheet_name)).await
            && opened.size() > 0
            && opened.size() <= MAX_IMAGE_BYTES
        {
            return Ok(opened);
        }

        // 2. Fall back to Jellyfin read-only sheet
        if let Some(manifest) = self.check_jellyfin(request).await
            && index < manifest.sheet_count
        {
            let video = &request.relative_path;
            let root = &request.media_root;
            let parent = video.parent().unwrap_or_else(|| Path::new(""));
            let file_name = video.file_name().unwrap_or_default().to_string_lossy();
            let stem = video.file_stem().unwrap_or_default().to_string_lossy();

            let candidates = [
                parent.join(format!("{file_name}.trickplay")),
                parent.join(format!("{stem}.trickplay")),
            ];

            for folder in candidates {
                let mut read_dir = match tokio::fs::read_dir(root.join(&folder)).await {
                    Ok(rd) => rd,
                    Err(_) => continue,
                };
                while let Ok(Some(entry)) = read_dir.next_entry().await {
                    if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                        let sheet_rel = folder.join(entry.file_name()).join(format!("{index}.jpg"));
                        if let Ok(opened) = OpenedFile::open_in_root(root, &sheet_rel).await
                            && opened.size() > 0
                            && opened.size() <= MAX_IMAGE_BYTES
                        {
                            return Ok(opened);
                        }
                    }
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "trickplay_sheet_not_found",
        ))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JellyfinJsonMeta {
    #[serde(default = "default_10k")]
    interval_ms: u64,
    #[serde(default = "default_320")]
    width: u32,
    #[serde(default = "default_180")]
    height: u32,
    #[serde(default = "default_10")]
    columns: u32,
    #[serde(default = "default_10")]
    rows: u32,
}
fn default_10k() -> u64 {
    10_000
}
fn default_320() -> u32 {
    320
}
fn default_180() -> u32 {
    180
}
fn default_10() -> u32 {
    10
}

fn parse_grid_folder_name(name: &str) -> Option<(u32, u32, u32)> {
    // E.g. "320 - 10x10", "240 - 5x5", "320-10x10"
    let parts: Vec<&str> = name.split('-').map(str::trim).collect();
    if parts.len() == 2 {
        let width = parts[0].parse::<u32>().ok()?;
        let (c, r) = parts[1].split_once('x')?;
        let cols = c.trim().parse::<u32>().ok()?;
        let rows = r.trim().parse::<u32>().ok()?;
        if width > 0 && cols > 0 && rows > 0 {
            return Some((width, cols, rows));
        }
    }
    None
}

async fn count_contiguous_sheets(root: &Path, sheet_rel: &Path) -> usize {
    let Ok(mut dir) = tokio::fs::read_dir(root.join(sheet_rel)).await else {
        return 0;
    };
    let mut found = HashSet::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let Some(idx) = name_str
            .strip_suffix(".jpg")
            .and_then(|s| s.parse::<usize>().ok())
        else {
            continue;
        };
        if idx < MAX_SHEETS {
            found.insert(idx);
        }
    }
    let mut count = 0;
    while found.contains(&count) {
        count += 1;
    }
    count
}

async fn decode_opened_jpeg_dimensions(opened: OpenedFile) -> Option<(u32, u32)> {
    tokio::task::spawn_blocking(move || {
        let reader = std::io::BufReader::new(opened.std_file());
        let decoder = image::codecs::jpeg::JpegDecoder::new(reader).ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jellyfin_grid_folder_name() {
        assert_eq!(parse_grid_folder_name("320 - 10x10"), Some((320, 10, 10)));
        assert_eq!(parse_grid_folder_name("240 - 5x5"), Some((240, 5, 5)));
        assert_eq!(parse_grid_folder_name("160-4x4"), Some((160, 4, 4)));
        assert_eq!(parse_grid_folder_name("random"), None);
        assert_eq!(parse_grid_folder_name("320"), None);
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
        };
        let fp1 = mgr.fingerprint(&base);

        let mut mod_time = base.clone();
        mod_time.modified_ns = "101".into();
        assert_ne!(fp1, mgr.fingerprint(&mod_time));

        let mut mod_width = base.clone();
        mod_width.width = Some(240);
        assert_ne!(fp1, mgr.fingerprint(&mod_width));

        let mut mod_interval = base.clone();
        mod_interval.interval_ms = Some(5_000);
        assert_ne!(fp1, mgr.fingerprint(&mod_interval));
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
        let dir1 = cache_root.join("cache_entry_1");
        let dir2 = cache_root.join("cache_entry_2");
        let dir3 = cache_root.join("cache_entry_3");
        tokio::fs::create_dir(&dir1).await.unwrap();
        tokio::fs::create_dir(&dir2).await.unwrap();
        tokio::fs::create_dir(&dir3).await.unwrap();

        let dummy = vec![0u8; 1_000_000];
        tokio::fs::write(dir1.join("0.jpg"), &dummy).await.unwrap();
        tokio::fs::write(dir2.join("0.jpg"), &dummy).await.unwrap();
        tokio::fs::write(dir3.join("0.jpg"), &dummy).await.unwrap();

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
    }
}
