//! Aether-owned, opt-in-by-demand trickplay cache. Only background jobs decode video;
//! media files remain read-only and the player never invokes FFmpeg.
use media_node::OpenedFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio::{process::Command, sync::Semaphore};

pub const COLUMNS: u32 = 10;
pub const ROWS: u32 = 10;
pub const MAX_SHEETS: usize = 128;
const WIDTH: u32 = 240;
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RUNTIME: Duration = Duration::from_secs(1800);
const FAILURE_COOLDOWN: Duration = Duration::from_secs(300);
const MAX_QUEUED: usize = 8;

#[derive(Clone)]
pub struct Request {
    pub asset_id: String,
    pub media_root: PathBuf,
    pub video: PathBuf,
    pub duration: f64,
    pub size: u64,
    pub modified_ns: String,
}

#[derive(Clone)]
pub struct Cached {
    pub root: PathBuf,
    pub count: usize,
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum Job {
    Running,
    Failed(Instant),
}
static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static WORKER: Semaphore = Semaphore::const_new(1);

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheManifest {
    version: u8,
    fingerprint: String,
    sheet_count: usize,
    interval_ms: u64,
    width: u32,
    columns: u32,
    rows: u32,
}

pub fn interval_ms() -> u64 {
    std::env::var("MEDIA_TRICKPLAY_INTERVAL_MS")
        .ok().and_then(|s| s.parse::<u64>().ok())
        .filter(|s| (1_000..=60_000).contains(s))
        .unwrap_or(10_000)
}

pub fn enabled() -> bool {
    !matches!(
        std::env::var("MEDIA_TRICKPLAY_GENERATE").as_deref(),
        Ok("0" | "false" | "off")
    )
}

fn cache_root() -> PathBuf {
    std::env::var_os("MEDIA_TRICKPLAY_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/trickplay"))
}

pub fn fingerprint(request: &Request) -> String {
    let mut h = Sha256::new();
    for part in [
        request.asset_id.as_str(),
        request.media_root.to_str().unwrap_or(""),
        request.video.to_str().unwrap_or(""),
        request.modified_ns.as_str(),
    ] {
        h.update((part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    }
    h.update(request.size.to_le_bytes());
    h.update(request.duration.to_bits().to_le_bytes());
    h.update(interval_ms().to_le_bytes());
    h.update(WIDTH.to_le_bytes());
    format!("{:x}", h.finalize())
}

pub async fn cached(request: &Request) -> Option<Cached> {
    let key = fingerprint(request);
    let root = cache_root().join(&key);
    let bytes = tokio::fs::read(root.join("manifest.json")).await.ok()?;
    let metadata: CacheManifest = serde_json::from_slice(&bytes).ok()?;
    if metadata.version != 1 || metadata.fingerprint != key
        || metadata.sheet_count == 0 || metadata.sheet_count > MAX_SHEETS
        || metadata.columns != COLUMNS || metadata.rows != ROWS
        || metadata.width != WIDTH || metadata.interval_ms != interval_ms()
    {
        return None;
    }
    // Ensure sheets were fully published and remain confined.
    let last = root.join(format!("{}.jpg", metadata.sheet_count - 1));
    let opened = OpenedFile::open_in_root(&root, Path::new(&format!("{}.jpg", metadata.sheet_count - 1))).await.ok()?;
    if opened.size() == 0 || opened.size() > 32 * 1024 * 1024 || !last.is_file() { return None; }
    Some(Cached { root, count: metadata.sheet_count, interval_ms: metadata.interval_ms })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueStatus { Queued, Busy, Failed, Disabled }

pub fn enqueue(request: Request) -> QueueStatus {
    if !enabled() { return QueueStatus::Disabled; }
    let key = fingerprint(&request);
    {
        let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        match jobs.get(&key) {
            Some(Job::Running) => return QueueStatus::Queued,
            Some(Job::Failed(when)) if when.elapsed() < FAILURE_COOLDOWN => return QueueStatus::Failed,
            _ => {}
        }
        jobs.retain(|_, state| match state {
            Job::Running => true,
            Job::Failed(when) => when.elapsed() < FAILURE_COOLDOWN,
        });
        if jobs.len() >= MAX_QUEUED { return QueueStatus::Busy; }
        jobs.insert(key.clone(), Job::Running);
    }
    tokio::spawn(async move {
        let outcome = match WORKER.acquire().await {
            Ok(_permit) => {
                // Give interactive requests a chance to finish before beginning CPU-intensive work.
                if crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                generate(&request, &key).await
            }
            Err(_) => Err("worker_closed"),
        };
        let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        if outcome.is_ok() {
            jobs.remove(&key);
        } else {
            eprintln!("trickplay generation failed: {}", outcome.unwrap_err());
            jobs.insert(key, Job::Failed(Instant::now()));
        }
    });
    QueueStatus::Queued
}

async fn generate(request: &Request, key: &str) -> Result<(), &'static str> {
    let opened = OpenedFile::open_in_root(&request.media_root, &request.video)
        .await.map_err(|_| "source_missing")?;
    if opened.size() != request.size || opened.modified_ns() != request.modified_ns {
        return Err("source_changed");
    }
    if !request.duration.is_finite() || request.duration <= 0.0 || request.duration > 86_400.0 {
        return Err("invalid_duration");
    }
    let interval = interval_ms();
    let frame_count = (request.duration * 1000.0 / interval as f64).ceil() as usize;
    let sheet_limit = frame_count.div_ceil((COLUMNS * ROWS) as usize).min(MAX_SHEETS);
    if sheet_limit == 0 { return Err("invalid_frame_count"); }

    let root = cache_root();
    tokio::fs::create_dir_all(&root).await.map_err(|_| "cache_permission")?;
    let target = root.join(key);
    if cached(request).await.is_some() { return Ok(()); }
    let work = root.join(format!(".{key}.{}.tmp", uuid::Uuid::new_v4().simple()));
    tokio::fs::create_dir(&work).await.map_err(|_| "cache_permission")?;
    let result = generate_in_dir(request, &opened, &work, sheet_limit, interval, key).await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&work).await;
        return result;
    }
    match tokio::fs::rename(&work, &target).await {
        Ok(_) => Ok(()),
        Err(_) if cached(request).await.is_some() => {
            let _ = tokio::fs::remove_dir_all(&work).await;
            Ok(())
        }
        Err(_) => {
            let _ = tokio::fs::remove_dir_all(&work).await;
            Err("cache_publish_failed")
        }
    }
}

async fn generate_in_dir(
    request: &Request, opened: &OpenedFile, output: &Path,
    sheet_limit: usize, interval: u64, key: &str,
) -> Result<(), &'static str> {
    let source_path = request.media_root.join(&request.video);
    // Input comes exclusively from an inventoried, confined source, not an HTTP path.
    // Read-only mounts prevent an untrusted local writer replacing the file mid-command.
    let filter = format!(
        "fps=1000/{interval},scale={WIDTH}:-2:flags=fast_bilinear,tile={COLUMNS}x{ROWS}"
    );
    let binary = std::env::var_os("MEDIA_TRICKPLAY_FFMPEG")
        .unwrap_or_else(|| std::ffi::OsString::from("ffmpeg"));
    let mut cmd = Command::new(binary);
    cmd.args(["-hide_banner","-loglevel","error","-nostdin","-threads","1","-filter_threads","1","-n","-i"])
        .arg(&source_path)
        .args(["-map","0:v:0","-an","-sn","-dn","-vf"])
        .arg(filter)
        .args(["-frames:v", &sheet_limit.to_string(), "-q:v","7","-start_number","0","-f","image2"])
        .arg(output.join("%d.jpg"))
        .kill_on_drop(true);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().map_err(|_| "ffmpeg_unavailable")?;
    let status = tokio::time::timeout(MAX_RUNTIME, child.wait())
        .await.map_err(|_| "generation_timeout")?
        .map_err(|_| "generation_process_error")?;
    if !status.success() { return Err("generation_failed"); }
    opened.verify_unchanged_sync().map_err(|_| "source_changed")?;

    let mut count = 0usize;
    let mut total_bytes = 0u64;
    while count < sheet_limit {
        let path = output.join(format!("{count}.jpg"));
        let Ok(meta) = tokio::fs::metadata(&path).await else { break };
        if !meta.is_file() || meta.len() == 0 || meta.len() > 32 * 1024 * 1024 {
            return Err("generated_image_invalid");
        }
        total_bytes = total_bytes.saturating_add(meta.len());
        if total_bytes > MAX_CACHE_BYTES { return Err("cache_size_exceeded"); }
        count += 1;
    }
    if count == 0 { return Err("no_frames"); }
    let manifest = CacheManifest {
        version: 1, fingerprint: key.to_owned(), sheet_count: count,
        interval_ms: interval, width: WIDTH, columns: COLUMNS, rows: ROWS,
    };
    let encoded = serde_json::to_vec(&manifest).map_err(|_| "manifest_encode")?;
    tokio::fs::write(output.join("manifest.json"), encoded)
        .await.map_err(|_| "manifest_write")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_is_versioned_by_source() {
        let mut request = Request {
            asset_id: "asset".into(), media_root: "/media".into(), video: "film.mp4".into(),
            duration: 90.0, size: 100, modified_ns: "123".into(),
        };
        let original = fingerprint(&request);
        request.modified_ns = "124".into();
        assert_ne!(fingerprint(&request), original);
    }
    #[test]
    fn sheet_bounds_never_exceed_capacity() {
        assert_eq!((7200usize.div_ceil(10)).div_ceil(100), 8);
        assert_eq!(MAX_SHEETS, 128);
    }
}
