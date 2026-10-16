//! Read-only local film posters. Paths never form part of the public contract.
use image::{ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Cursor, Read},
    path::Path,
    time::UNIX_EPOCH,
};
use tokio::sync::Semaphore;

pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
static VALIDATION: Semaphore = Semaphore::const_new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalImage {
    pub path: String,
    pub size_bytes: u64,
    pub modified_ns: String,
    pub mime: String,
    pub priority: u8,
}
#[derive(Debug)]
pub struct ValidatedImage {
    pub mime: &'static str,
    pub size_bytes: u64,
    pub modified_ns: String,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Caller must first apply confined_file. One blocking decoder at a time; compressed
/// input <=10 MiB, strict dimensions <=4096 each and decoder allocation budget32MiB.
/// Allocation budget is decoder best effort, not a process RSS guarantee.
pub async fn validate(path: &Path) -> io::Result<ValidatedImage> {
    let permit = VALIDATION
        .acquire()
        .await
        .map_err(|_| io::Error::other("image validation stopped"))?;
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut file = std::fs::File::open(&path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES {
            return Err(invalid("image must be a regular file of at most 10 MiB"));
        }
        let mut bytes = Vec::new();
        (&mut file).take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("image changed during validation"));
        }
        let after = file.metadata()?;
        if after.len() != metadata.len() || after.modified()? != metadata.modified()? {
            return Err(invalid("image changed during validation"));
        }
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let mime = match reader.format() {
            Some(ImageFormat::Jpeg) => "image/jpeg",
            Some(ImageFormat::Png) => "image/png",
            Some(ImageFormat::WebP) => "image/webp",
            _ => return Err(invalid("unsupported image content")),
        };
        let extension = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let expected = match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => return Err(invalid("unsupported image extension")),
        };
        if expected != mime {
            return Err(invalid("poster extension does not match image content"));
        }
        let mut limits = Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let decoder = reader
            .into_decoder()
            .map_err(|_| invalid("invalid image header"))?;
        if decoder.total_bytes() > 32 * 1024 * 1024 {
            return Err(invalid("image decoded pixels exceed 32 MiB"));
        }
        image::DynamicImage::from_decoder(decoder)
            .map_err(|_| invalid("corrupt image or image decoding limit exceeded"))?;
        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid("image modification time unavailable"))?
            .as_nanos()
            .to_string();
        Ok(ValidatedImage {
            mime,
            size_bytes: metadata.len(),
            modified_ns,
        })
    })
    .await
    .map_err(|_| io::Error::other("image validation failed"))?
}

async fn shared_allowed(root: &Path, video: &Path) -> io::Result<bool> {
    let parent = video.parent().unwrap_or(Path::new(""));
    if parent.as_os_str().is_empty() {
        return Ok(false);
    }
    let key = super::scanner::identity(video).3;
    let folder_key = super::scanner::identity(&parent.join("__unqualified_media__.mp4")).3;
    if !folder_key.starts_with("file:") && folder_key != key {
        return Ok(false);
    }
    let mut entries = tokio::fs::read_dir(root.join(parent)).await?;
    while let Some(entry) = entries.next_entry().await? {
        let p = entry.path();
        if p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("mkv") || s.eq_ignore_ascii_case("mp4"))
        {
            let relative = p
                .strip_prefix(root)
                .map_err(|_| invalid("image outside library"))?;
            if super::scanner::identity(relative).3 != key {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Name priority poster, folder, exact-video-poster, exact-video; within each:
/// jpg, jpeg, png, webp. Names follow filesystem case rules. Invalid first candidate
/// produces a warning/placeholder, never a silent switch to a different poster.
pub async fn discover(root: &Path, video: &Path) -> (Option<LocalImage>, Option<String>) {
    discover_cached(root, video, &std::collections::HashMap::new()).await
}

pub async fn discover_cached(
    root: &Path,
    video: &Path,
    cache: &std::collections::HashMap<String, LocalImage>,
) -> (Option<LocalImage>, Option<String>) {
    let parent = video.parent().unwrap_or(Path::new(""));
    let stem = video.file_stem().unwrap_or_default().to_string_lossy();
    let shared = match shared_allowed(root, video).await {
        Ok(value) => value,
        Err(_) => return (None, Some("poster directory unreadable".into())),
    };
    // Two files with the same stem can be separate Works under existing grouping.
    // A stem sidecar cannot choose between them, so only share it within one identity.
    let mut exact_allowed = true;
    if let Ok(mut entries) = tokio::fs::read_dir(root.join(parent)).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let p = entry.path();
            if p.file_stem() == video.file_stem()
                && p.extension()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.eq_ignore_ascii_case("mp4") || v.eq_ignore_ascii_case("mkv"))
                && p.strip_prefix(root).is_ok_and(|p| {
                    super::scanner::identity(p).3 != super::scanner::identity(video).3
                })
            {
                exact_allowed = false;
            }
        }
    } else {
        exact_allowed = false;
    }
    for (name_index, name) in [
        "poster".to_owned(),
        "folder".to_owned(),
        format!("{stem}-poster"),
        stem.into_owned(),
    ]
    .iter()
    .enumerate()
    {
        if (name_index < 2 && !shared) || (name_index >= 2 && !exact_allowed) {
            continue;
        }
        for (format_index, extension) in ["jpg", "jpeg", "png", "webp"].iter().enumerate() {
            let relative = parent.join(format!("{name}.{extension}"));
            if tokio::fs::symlink_metadata(root.join(&relative))
                .await
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                continue;
            }
            let result = async {
                let file = super::scanner::confined_file(root, &relative)
                    .await
                    .map_err(|_| invalid("poster unavailable or unconfined"))?;
                let metadata = tokio::fs::metadata(&file).await?;
                let key = relative.to_string_lossy().replace('\\', "/");
                let modified = metadata
                    .modified()?
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| invalid("image modification time unavailable"))?
                    .as_nanos()
                    .to_string();
                if let Some(cached) = cache.get(&key).filter(|c| {
                    c.size_bytes == metadata.len()
                        && c.modified_ns == modified
                        && c.size_bytes <= MAX_BYTES
                }) {
                    let mut reused = cached.clone();
                    reused.priority = (name_index * 4 + format_index) as u8;
                    return Ok(reused);
                }
                let validated = validate(&file).await?;
                let expected = match *extension {
                    "jpg" | "jpeg" => "image/jpeg",
                    "png" => "image/png",
                    _ => "image/webp",
                };
                if validated.mime != expected {
                    return Err(invalid("poster extension does not match image content"));
                }
                Ok(LocalImage {
                    path: relative.to_string_lossy().replace('\\', "/"),
                    size_bytes: validated.size_bytes,
                    modified_ns: validated.modified_ns,
                    mime: validated.mime.into(),
                    priority: (name_index * 4 + format_index) as u8,
                })
            }
            .await;
            return match result {
                Ok(image) => (Some(image), None),
                Err(error) => (None, Some(error.to_string())),
            };
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image_bytes(format: ImageFormat) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(2, 3);
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }
    async fn setup() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(temp.path().join("Film (2024)"))
            .await
            .unwrap();
        tokio::fs::write(temp.path().join("Film (2024)/Film.mp4"), b"synthetic")
            .await
            .unwrap();
        temp
    }
    #[tokio::test]
    async fn deterministic_names_formats_missing_and_original_bytes() {
        let root = setup().await;
        let video = Path::new("Film (2024)/Film.mp4");
        assert!(discover(root.path(), video).await.0.is_none());
        let folder = root.path().join("Film (2024)");
        let jpg = image_bytes(ImageFormat::Jpeg);
        tokio::fs::write(folder.join("folder.jpg"), &jpg)
            .await
            .unwrap();
        assert_eq!(discover(root.path(), video).await.0.unwrap().priority, 4);
        tokio::fs::write(folder.join("poster.png"), image_bytes(ImageFormat::Png))
            .await
            .unwrap();
        assert_eq!(discover(root.path(), video).await.0.unwrap().priority, 2);
        tokio::fs::write(folder.join("poster.jpeg"), &jpg)
            .await
            .unwrap();
        tokio::fs::write(folder.join("poster.jpg"), &jpg)
            .await
            .unwrap();
        let selected = discover(root.path(), video).await.0.unwrap();
        assert_eq!(selected.priority, 0);
        assert_eq!(selected.mime, "image/jpeg");
        assert_eq!(
            tokio::fs::read(folder.join("poster.jpg")).await.unwrap(),
            jpg
        );
    }
    #[tokio::test]
    async fn rejects_corruption_misleading_extension_and_oversize() {
        let root = setup().await;
        let path = root.path().join("Film (2024)/poster.jpg");
        for bytes in [
            b"not an image".to_vec(),
            image_bytes(ImageFormat::Png),
            vec![0; MAX_BYTES as usize + 1],
        ] {
            tokio::fs::write(&path, bytes).await.unwrap();
            let (poster, warning) = discover(root.path(), Path::new("Film (2024)/Film.mp4")).await;
            assert!(poster.is_none());
            assert!(warning.is_some());
        }
        for format in [ImageFormat::Jpeg, ImageFormat::Png, ImageFormat::WebP] {
            let actual = path.with_extension(match format {
                ImageFormat::Jpeg => "jpg",
                ImageFormat::Png => "png",
                _ => "webp",
            });
            tokio::fs::write(&actual, image_bytes(format))
                .await
                .unwrap();
            assert!(validate(&actual).await.is_ok());
        }
        let mut truncated = image_bytes(ImageFormat::Png);
        truncated.truncate(truncated.len() / 2);
        tokio::fs::write(&path, truncated).await.unwrap();
        assert!(validate(&path).await.is_err());
    }
    #[tokio::test]
    async fn shared_posters_never_cross_distinct_film_identities() {
        let root = setup().await;
        let folder = root.path().join("Film (2024)");
        tokio::fs::write(folder.join("poster.jpg"), image_bytes(ImageFormat::Jpeg))
            .await
            .unwrap();
        tokio::fs::write(folder.join("Other (2025).mp4"), b"synthetic")
            .await
            .unwrap();
        assert!(
            discover(root.path(), Path::new("Film (2024)/Film.mp4"))
                .await
                .0
                .is_none()
        );
        tokio::fs::write(
            folder.join("Film-poster.webp"),
            image_bytes(ImageFormat::WebP),
        )
        .await
        .unwrap();
        assert_eq!(
            discover(root.path(), Path::new("Film (2024)/Film.mp4"))
                .await
                .0
                .unwrap()
                .priority,
            11
        );
        assert!(
            discover(root.path(), Path::new("Film (2024)/Other (2025).mp4"))
                .await
                .0
                .is_none()
        );
        tokio::fs::write(
            root.path().join("poster.jpg"),
            image_bytes(ImageFormat::Jpeg),
        )
        .await
        .unwrap();
        assert!(
            discover(root.path(), Path::new("Loose.mp4"))
                .await
                .0
                .is_none()
        );
    }
    #[tokio::test]
    async fn junction_or_directory_link_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = setup().await;
        let target = outside.path().join("Film (2024)");
        tokio::fs::write(target.join("poster.jpg"), image_bytes(ImageFormat::Jpeg))
            .await
            .unwrap();
        let link = root.path().join("Film (2024)");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
        let (poster, warning) = discover(root.path(), Path::new("Film (2024)/Film.mp4")).await;
        assert!(poster.is_none());
        assert!(warning.unwrap().contains("unconfined"));
    }
}
