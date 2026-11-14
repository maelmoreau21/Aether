//! Small authorized image representations. No writes to media or unbounded image cache.
use crate::{
    files::OpenedFile,
    images,
    stream::{BoxStreamBody, empty_body, parse_single_range},
};
use bytes::Bytes;
use http::{HeaderMap, Method, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{self, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, Weak},
};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
/// On-disk preview cache quota (`MEDIA_IMAGE_CACHE_MAX_BYTES` overrides it); entries are rebuildable.
const DISK_CACHE_BYTES: u64 = 512 * 1024 * 1024;
static DISK_WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const CACHE_ENTRIES: usize = 2048;
const MAX_ENCODED: usize = 2 * 1024 * 1024;
type Key = (PathBuf, u64, String, u32);
#[derive(Clone)]
pub(crate) struct Preview {
    pub bytes: Bytes,
    pub mime: &'static str,
    pub source_mime: &'static str,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Preview>,
    order: VecDeque<Key>,
    bytes: usize,
}
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));
type InFlightSlot = Arc<tokio::sync::Mutex<Option<Result<Preview, io::ErrorKind>>>>;
type WeakInFlightSlot = Weak<tokio::sync::Mutex<Option<Result<Preview, io::ErrorKind>>>>;
static IN_FLIGHT: LazyLock<Mutex<HashMap<Key, WeakInFlightSlot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn acquire_in_flight(key: &Key) -> InFlightSlot {
    let mut map = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = map.get(key).and_then(Weak::upgrade) {
        return existing;
    }
    if map.len() >= 64 {
        map.retain(|_, weak| weak.strong_count() > 0);
    }
    let slot = Arc::new(tokio::sync::Mutex::new(None));
    map.insert(key.clone(), Arc::downgrade(&slot));
    slot
}

fn clear_in_flight(key: &Key, slot: &InFlightSlot) {
    let mut map = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = map.get(key)
        && std::ptr::eq(existing.as_ptr(), Arc::as_ptr(slot))
    {
        map.remove(key);
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid image preview")
}

/// Only three sizes can enter the cache, even through direct Node requests.
pub fn width(query: Option<&str>) -> io::Result<Option<u32>> {
    let mut widths = url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
        .filter(|(name, _)| name == "width");
    let Some((_, value)) = widths.next() else {
        return Ok(None);
    };
    let width = value.parse::<u32>().map_err(|_| invalid())?;
    if widths.next().is_some() || ![320, 640, 1280].contains(&width) {
        return Err(invalid());
    }
    Ok(Some(width))
}
fn cached(key: &Key) -> Option<Preview> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let value = cache.entries.get(key)?.clone();
    cache.order.retain(|item| item != key);
    cache.order.push_back(key.clone());
    Some(value)
}
fn remember(key: Key, value: Preview) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.entries.contains_key(&key) {
        return;
    }
    while cache.bytes + value.bytes.len() > CACHE_BYTES || cache.entries.len() >= CACHE_ENTRIES {
        let Some(oldest) = cache.order.pop_front() else {
            break;
        };
        if let Some(old) = cache.entries.remove(&oldest) {
            cache.bytes -= old.bytes.len();
        }
    }
    cache.bytes += value.bytes.len();
    cache.order.push_back(key.clone());
    cache.entries.insert(key, value);
}
fn encode(image: &image::DynamicImage) -> io::Result<(Vec<u8>, &'static str)> {
    let encoded =
        crate::image_encoding::encode(image, image.color().has_alpha()).map_err(|_| invalid())?;
    Ok((encoded, "image/webp"))
}

static VERIFIED_CACHE_DIRS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn dir_writable(base: &std::path::Path, cache_dir: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(base) else {
        return false;
    };
    if !meta.is_dir() || meta.permissions().readonly() {
        return false;
    }
    if VERIFIED_CACHE_DIRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(cache_dir)
    {
        return true;
    }
    if std::fs::create_dir_all(cache_dir).is_err() {
        return false;
    }
    // Probe actual effective-UID write access once (mode bits alone miss root-owned 0755 dirs).
    let probe = cache_dir.join(format!(".write-probe-{}", uuid::Uuid::new_v4().simple()));
    let writable = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    };
    if writable {
        VERIFIED_CACHE_DIRS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cache_dir.to_path_buf());
    }
    writable
}

fn disk_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEDIA_IMAGE_CACHE_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(media_data) = std::env::var_os("MEDIA_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        let dir = media_data.join("cache/images");
        if dir_writable(&media_data, &dir) {
            return dir;
        }
    }
    let default_data = crate::default_data_dir();
    let dir = crate::data_subdir("MEDIA_IMAGE_CACHE_DIR", "cache/images");
    if dir_writable(&default_data, &dir) {
        return dir;
    }
    std::env::temp_dir().join("media-image-cache")
}

fn disk_cache_path(key: &Key) -> PathBuf {
    let payload = format!(
        "webp-v1:{}:{}:{}:{}",
        key.0.to_string_lossy(),
        key.1,
        key.2,
        key.3
    );
    let hash = media_contracts::protocol::sha256_hex(payload.as_bytes());
    disk_cache_dir().join(format!("{hash}.img"))
}

fn read_disk_cache(key: &Key) -> Option<Preview> {
    let path = disk_cache_path(key);
    let mut file = std::fs::File::open(&path).ok()?;
    if file.metadata().ok()?.len() > (MAX_ENCODED + 2) as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_ENCODED + 3) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_ENCODED + 2 {
        return None;
    }
    if bytes.len() < 2 {
        return None;
    }
    let mime = match bytes[0] {
        1 => "image/jpeg",
        2 => "image/png",
        3 => "image/webp",
        _ => return None,
    };
    let source_mime = match bytes[1] {
        1 => "image/jpeg",
        2 => "image/png",
        3 => "image/webp",
        _ => return None,
    };
    Some(Preview {
        bytes: Bytes::from(bytes[2..].to_vec()),
        mime,
        source_mime,
    })
}

fn write_disk_cache(key: &Key, preview: &Preview) {
    let dir = disk_cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let target = disk_cache_path(key);
    let temp = dir.join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let mime_byte = match preview.mime {
        "image/png" => 2u8,
        "image/webp" => 3u8,
        _ => 1u8,
    };
    let source_mime_byte = match preview.source_mime {
        "image/png" => 2u8,
        "image/webp" => 3u8,
        _ => 1u8,
    };
    let mut file_bytes = Vec::with_capacity(2 + preview.bytes.len());
    file_bytes.push(mime_byte);
    file_bytes.push(source_mime_byte);
    file_bytes.extend_from_slice(&preview.bytes);
    if std::fs::write(&temp, file_bytes).is_ok() {
        let _ = std::fs::rename(temp, target);
    }
    // First write after a start, then every 64th: bounded size and crash leftovers.
    if DISK_WRITES
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .is_multiple_of(64)
    {
        let _ = crate::disk_cache::enforce_quota(
            &dir,
            "img",
            crate::disk_cache::budget_from_env("MEDIA_IMAGE_CACHE_MAX_BYTES", DISK_CACHE_BYTES),
            std::time::Duration::from_secs(3600),
        );
    }
}

async fn disk_cached(key: &Key) -> Option<Preview> {
    let key = key.clone();
    tokio::task::spawn_blocking(move || read_disk_cache(&key))
        .await
        .ok()
        .flatten()
}

pub(crate) async fn generate(opened: &OpenedFile, width: u32) -> io::Result<Preview> {
    if ![320, 640, 1280].contains(&width) {
        return Err(invalid());
    }
    opened.verify_unchanged_sync()?;
    let size = opened.size();
    if size == 0 || size > images::MAX_BYTES {
        return Err(invalid());
    }
    let key = (
        opened.canonical_path().to_owned(),
        size,
        opened.modified_ns().to_owned(),
        width,
    );
    if let Some(value) = cached(&key) {
        return Ok(value);
    }
    let slot = acquire_in_flight(&key);
    let mut guard = slot.lock().await;
    opened.verify_unchanged_sync()?;
    if let Some(cached_outcome) = guard.as_ref() {
        return match cached_outcome {
            Ok(preview) => Ok(preview.clone()),
            Err(kind) => Err(io::Error::new(*kind, "invalid image preview")),
        };
    }
    if let Some(value) = cached(&key) {
        clear_in_flight(&key, &slot);
        *guard = Some(Ok(value.clone()));
        return Ok(value);
    }
    if let Some(disk_value) = disk_cached(&key).await {
        remember(key.clone(), disk_value.clone());
        clear_in_flight(&key, &slot);
        *guard = Some(Ok(disk_value.clone()));
        return Ok(disk_value);
    }
    // Share the decoder budget with scans and original-image validation. The
    // permit is held by the blocking job even if its HTTP caller is cancelled.
    let permit = crate::image_encoding::DECODERS
        .acquire()
        .await
        .map_err(|_| invalid())?;
    opened.verify_unchanged_sync()?;
    let mut source = opened.try_clone()?;
    let blocking_result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        source.verify_unchanged_sync()?;
        let extension = source
            .canonical_path()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let file = source.std_file_mut();
        file.seek(SeekFrom::Start(0))?;
        let mut original = Vec::with_capacity(size as usize);
        file.take(images::MAX_BYTES + 1)
            .read_to_end(&mut original)?;
        source.std_file_mut().seek(SeekFrom::Start(0))?;
        if original.len() as u64 != size {
            return Err(invalid());
        }
        source.verify_unchanged_sync()?;
        let (source_mime, image) = images::decode_image(&original, &extension)?;
        let resized = image.width() > width || image.height() > width * 2;
        let mut image = if resized {
            crate::image_encoding::resize_bounded(&image, width, width * 2)
        } else {
            image
        };
        let (mut encoded, mut mime) = if !resized
            && source_mime == "image/webp"
            && crate::image_encoding::reusable_webp(&original, image.color().has_alpha())
        {
            (original, "image/webp")
        } else {
            encode(&image)?
        };
        while encoded.len() > MAX_ENCODED && image.width() > 320 {
            let smaller = (image.width() / 2).max(320);
            image = crate::image_encoding::resize_bounded(&image, smaller, smaller * 2);
            (encoded, mime) = encode(&image)?;
        }
        if encoded.len() > MAX_ENCODED {
            return Err(invalid());
        }
        source.verify_unchanged_sync()?;
        Ok(Preview {
            bytes: Bytes::from(encoded),
            mime,
            source_mime,
        })
    })
    .await
    .map_err(|_| invalid())?;
    let result = match blocking_result {
        Ok(preview) => preview,
        Err(err) => {
            clear_in_flight(&key, &slot);
            *guard = Some(Err(err.kind()));
            return Err(err);
        }
    };
    images::remember_validated_image(key.0.clone(), key.1, key.2.clone(), result.source_mime);
    remember(key.clone(), result.clone());
    clear_in_flight(&key, &slot);
    *guard = Some(Ok(result.clone()));
    let disk_key = key;
    let disk_result = result.clone();
    drop(tokio::task::spawn_blocking(move || {
        write_disk_cache(&disk_key, &disk_result)
    }));
    Ok(result)
}

pub(crate) fn response(
    preview: Preview,
    method: &Method,
    headers: &HeaderMap,
    etag: &str,
) -> io::Result<Response<BoxStreamBody>> {
    let size = preview.bytes.len();
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, preview.mime)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .header(header::ETAG, etag)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    let range = headers.get(header::RANGE);
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| {
            s.split(',')
                .any(|v| v.trim() == "*" || v.trim().strip_prefix("W/").unwrap_or(v.trim()) == etag)
        });
    if range.is_none() && unchanged {
        return builder
            .status(StatusCode::NOT_MODIFIED)
            .body(empty_body())
            .map_err(|_| invalid());
    }
    let bytes = if let Some(range) = range {
        let Some(range) = range
            .to_str()
            .ok()
            .and_then(|v| parse_single_range(v, size as u64))
        else {
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .header(header::CONTENT_LENGTH, "0")
                .body(empty_body())
                .map_err(|_| invalid());
        };
        builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{size}", range.start, range.end),
        );
        preview
            .bytes
            .slice(range.start as usize..=range.end as usize)
    } else {
        preview.bytes
    };
    builder = builder.header(header::CONTENT_LENGTH, bytes.len().to_string());
    let body = if *method == Method::HEAD {
        empty_body()
    } else {
        Full::new(bytes).map_err(|never| match never {}).boxed()
    };
    builder.body(body).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounded_preview_is_small_cached_and_changes_with_source() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("poster.jpg");
        // Deterministic detailed image, rather than a two-pixel fixture.
        image::RgbImage::from_fn(1800, 2700, |x, y| {
            image::Rgb([
                (x.wrapping_mul(37) ^ y) as u8,
                (y.wrapping_mul(17) ^ x) as u8,
                (x * y) as u8,
            ])
        })
        .save(&path)
        .unwrap();
        let opened = OpenedFile::open_in_root(root.path(), std::path::Path::new("poster.jpg"))
            .await
            .unwrap();
        let start = std::time::Instant::now();
        let preview = generate(&opened, 320).await.unwrap();
        let cold = start.elapsed();
        assert_eq!(preview.mime, "image/webp");
        assert!(preview.bytes.len() < opened.size() as usize / 5);
        let decoded = image::load_from_memory(&preview.bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 480));
        let warm = std::time::Instant::now();
        let again = generate(&opened, 320).await.unwrap();
        eprintln!(
            "image-preview: original={} bytes, preview={} bytes, cold={:?}, cached={:?}",
            opened.size(),
            preview.bytes.len(),
            cold,
            warm.elapsed()
        );
        assert_eq!(preview.bytes.as_ptr(), again.bytes.as_ptr());
        image::RgbImage::from_pixel(80, 120, image::Rgb([0, 100, 20]))
            .save(&path)
            .unwrap();
        assert!(generate(&opened, 320).await.is_err());
        let changed = OpenedFile::open_in_root(root.path(), std::path::Path::new("poster.jpg"))
            .await
            .unwrap();
        assert_ne!(generate(&changed, 320).await.unwrap().bytes, preview.bytes);
    }
    #[tokio::test]
    async fn transparency_and_conditional_range_head_contracts() {
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(12, 18, image::Rgba([8, 20, 90, 100]))
            .save(root.path().join("logo.png"))
            .unwrap();
        let opened = OpenedFile::open_in_root(root.path(), std::path::Path::new("logo.png"))
            .await
            .unwrap();
        let preview = generate(&opened, 320).await.unwrap();
        assert_eq!(preview.mime, "image/webp");
        assert_eq!(
            image::load_from_memory(&preview.bytes)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0[3],
            100
        );
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, "\"preview\"".parse().unwrap());
        assert_eq!(
            response(preview.clone(), &Method::GET, &headers, "\"preview\"")
                .unwrap()
                .status(),
            304
        );
        headers.insert(header::RANGE, "bytes=0-3".parse().unwrap());
        let res = response(preview.clone(), &Method::HEAD, &headers, "\"preview\"").unwrap();
        assert_eq!(res.status(), 206);
        assert_eq!(res.headers()[header::CONTENT_LENGTH], "4");
        assert!(
            res.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        headers.insert(header::RANGE, "bytes=999999-".parse().unwrap());
        assert_eq!(
            response(preview, &Method::GET, &headers, "\"preview\"")
                .unwrap()
                .status(),
            416
        );
        assert!(width(Some("width=99999")).is_err());
        assert!(width(Some("width=320&width=640")).is_err());
    }

    #[tokio::test]
    async fn singleflight_coalesces_concurrent_generations_and_writes_disk_async() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("concurrent.jpg");
        image::RgbImage::from_fn(900, 1350, |x, y| {
            image::Rgb([(x ^ y) as u8, (x * 3) as u8, (y * 5) as u8])
        })
        .save(&path)
        .unwrap();
        let opened = OpenedFile::open_in_root(root.path(), std::path::Path::new("concurrent.jpg"))
            .await
            .unwrap();
        let key = (
            opened.canonical_path().to_owned(),
            opened.size(),
            opened.modified_ns().to_owned(),
            640,
        );
        let mut set = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let clone = opened.try_clone().unwrap();
            set.spawn(async move { generate(&clone, 640).await.unwrap() });
        }
        let mut ptrs: Vec<usize> = Vec::new();
        while let Some(res) = set.join_next().await {
            let preview = res.unwrap();
            ptrs.push(preview.bytes.as_ptr() as usize);
        }
        assert!(ptrs.windows(2).all(|w| w[0] == w[1]));
        // Detached disk cache task finishes asynchronously without stalling HTTP callers.
        let mut persisted = false;
        for _ in 0..40 {
            if disk_cached(&key).await.is_some() {
                persisted = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert!(persisted);
    }
}
