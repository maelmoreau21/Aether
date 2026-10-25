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
    collections::{HashMap, VecDeque},
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
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
    let mut encoded = Vec::new();
    let mime = if image.color().has_alpha() {
        image
            .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
            .map_err(|_| invalid())?;
        "image/png"
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 78)
            .encode_image(image)
            .map_err(|_| invalid())?;
        "image/jpeg"
    };
    Ok((encoded, mime))
}

fn disk_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEDIA_IMAGE_CACHE_DIR") {
        PathBuf::from(dir)
    } else {
        std::env::temp_dir().join("media-image-cache")
    }
}

fn disk_cache_path(key: &Key) -> PathBuf {
    let payload = format!("{}:{}:{}:{}", key.0.to_string_lossy(), key.1, key.2, key.3);
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
    if let Some(disk_value) = disk_cached(&key).await {
        remember(key, disk_value.clone());
        return Ok(disk_value);
    }
    // Share the decoder budget with scans and original-image validation. The
    // permit is held by the blocking job even if its HTTP caller is cancelled.
    let permit = images::VALIDATION.acquire().await.map_err(|_| invalid())?;
    opened.verify_unchanged_sync()?;
    if let Some(value) = cached(&key) {
        return Ok(value);
    }
    if let Some(disk_value) = disk_cached(&key).await {
        remember(key, disk_value.clone());
        return Ok(disk_value);
    }
    let mut source = opened.try_clone()?;
    let result = tokio::task::spawn_blocking(move || {
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
            image.thumbnail(width, width * 2)
        } else {
            image
        };
        let (encoded, mime) = encode(&image)?;
        // Small originals can be smaller than a JPEG header; never inflate them.
        let (mut encoded, mut mime) = if !resized && original.len() <= encoded.len() {
            (original, source_mime)
        } else {
            (encoded, mime)
        };
        while encoded.len() > MAX_ENCODED && image.width() > 320 {
            let smaller = (image.width() / 2).max(320);
            image = image.thumbnail(smaller, smaller * 2);
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
    .map_err(|_| invalid())??;
    images::remember_validated_image(key.0.clone(), key.1, key.2.clone(), result.source_mime);
    let disk_key = key.clone();
    let disk_result = result.clone();
    let _ = tokio::task::spawn_blocking(move || write_disk_cache(&disk_key, &disk_result)).await;
    remember(key, result.clone());
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
        assert_eq!(preview.mime, "image/jpeg");
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
        assert_eq!(preview.mime, "image/png");
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
}
