//! Read-only audio discovery; metadata comes from bounded probes, never Core filesystem access.
use crate::{files::OpenedFile, probe};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTrack {
    pub relative_path: String,
    pub size: u64,
    pub modified_ns: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub duration: Option<f64>,
    pub codec: String,
    pub mime: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub cover_path: Option<String>,
    #[serde(default)]
    pub cover_size: Option<u64>,
    #[serde(default)]
    pub cover_modified_ns: Option<String>,
    #[serde(default)]
    pub cover_mime: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub library_id: String,
    pub scan_id: String,
    pub root_snapshot: String,
    pub track: MusicTrack,
}
pub async fn scan<F, Fut>(root: &Path, cancel: &AtomicBool, mut publish: F) -> io::Result<usize>
where
    F: FnMut(MusicTrack) -> Fut,
    Fut: std::future::Future<Output = io::Result<()>>,
{
    scan_with_exclusions(root, cancel, &[], &mut publish).await
}
pub async fn scan_with_exclusions<F, Fut>(
    root: &Path,
    cancel: &AtomicBool,
    excluded: &[String],
    mut publish: F,
) -> io::Result<usize>
where
    F: FnMut(MusicTrack) -> Fut,
    Fut: std::future::Future<Output = io::Result<()>>,
{
    let root = root.canonicalize()?;
    let mut directories = vec![PathBuf::new()];
    let mut cover_cache: std::collections::HashMap<PathBuf, Option<crate::images::LocalImage>> =
        std::collections::HashMap::new();
    let mut count = 0;
    while let Some(relative) = directories.pop() {
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::other("scan cancelled"));
        }
        let directory = crate::files::validate_library_root(&root.join(&relative), &root).await?;
        let mut entries = tokio::fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::other("scan cancelled"));
            }
            let kind = entry.file_type().await?;
            if kind.is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if tokio::fs::symlink_metadata(entry.path())
                    .await?
                    .file_attributes()
                    & 0x400
                    != 0
                {
                    continue;
                }
            }
            let path = relative.join(entry.file_name());
            if crate::scanner::is_excluded(
                &path.to_string_lossy(),
                &entry.file_name().to_string_lossy(),
                excluded,
            ) {
                continue;
            }
            if kind.is_dir() {
                if crate::scanner::directory_is_ignored(&root.join(&path)) {
                    continue;
                }
                if directories.len() >= 10000 {
                    return Err(io::Error::other("too many directories"));
                }
                directories.push(path);
                continue;
            }
            let extension = path
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let mime = match extension.as_str() {
                "opus" | "ogg" => "audio/ogg",
                "mp3" => "audio/mpeg",
                "flac" => "audio/flac",
                "wav" => "audio/wav",
                "m4a" | "mp4" => "audio/mp4",
                "aac" => "audio/aac",
                _ => continue,
            };
            if !kind.is_file() {
                continue;
            }
            let opened = OpenedFile::open_in_root(&root, &path).await?;
            let probe = probe::probe_opened(&opened)
                .await
                .map_err(io::Error::other)?;
            let Some(audio) = probe
                .streams
                .iter()
                .find(|s| s.codec_type.as_deref() == Some("audio"))
            else {
                continue;
            };
            let tags = probe.format.get("tags").and_then(|v| v.as_object());
            let tag = |key: &str| -> Option<String> {
                tags.and_then(|t| t.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)))
                    .or_else(|| audio.tags.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)))
                    .and_then(|(_, v)| v.as_str())
                    .map(|v| v.chars().filter(|c| !c.is_control()).take(512).collect())
            };
            let artist = tag("artist").unwrap_or_else(|| "Artiste inconnu".into());
            let mut lyrics = tags
                .and_then(|tags| {
                    tags.iter().find(|(key, _)| {
                        key.eq_ignore_ascii_case("lyrics")
                            || key.eq_ignore_ascii_case("unsyncedlyrics")
                    })
                })
                .and_then(|(_, value)| value.as_str())
                .unwrap_or("")
                .to_owned();
            // The Node alone resolves same-stem sidecars through confined handles.
            // No browser-provided path or network lyrics provider is involved.
            for extension in ["lrc", "txt"] {
                if let Ok(sidecar) =
                    OpenedFile::open_in_root(&root, &path.with_extension(extension)).await
                    && sidecar.size() <= 32768
                {
                    use tokio::io::AsyncReadExt;
                    let mut text = String::new();
                    let read = tokio::fs::File::from_std(sidecar.try_clone_std()?)
                        .take(32769)
                        .read_to_string(&mut text)
                        .await;
                    if read.is_ok()
                        && text.len() <= 32768
                        && sidecar.verify_unchanged_sync().is_ok()
                    {
                        lyrics = text;
                        break;
                    }
                }
            }
            if lyrics.len() > 32768 {
                lyrics.clear();
            }
            let parent_dir = relative.clone();
            let cover = if let Some(cached) = cover_cache.get(&parent_dir) {
                cached.clone()
            } else {
                let (discovered, _) = crate::images::discover_named(
                    &root,
                    &parent_dir,
                    &std::collections::HashMap::new(),
                    &["folder", "cover", "album", "poster", "front"],
                )
                .await;
                cover_cache.insert(parent_dir, discovered.clone());
                discovered
            };
            let track = MusicTrack {
                relative_path: path.to_string_lossy().into_owned(),
                size: opened.size(),
                modified_ns: opened.modified_ns().into(),
                title: tag("title").unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                }),
                album: tag("album").unwrap_or_else(|| {
                    path.parent()
                        .and_then(|p| p.file_name())
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                }),
                album_artist: tag("album_artist")
                    .or_else(|| tag("albumartist"))
                    .unwrap_or_else(|| artist.clone()),
                artist,
                track_number: tag("track").and_then(|v| v.split('/').next()?.parse().ok()),
                disc_number: tag("disc").and_then(|v| v.split('/').next()?.parse().ok()),
                duration: probe
                    .format
                    .get("duration")
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.),
                codec: audio.codec_name.clone().unwrap_or_default(),
                mime: mime.into(),
                lyrics,
                cover_path: cover.as_ref().map(|c| c.path.clone()),
                cover_size: cover.as_ref().map(|c| c.size_bytes),
                cover_modified_ns: cover.as_ref().map(|c| c.modified_ns.clone()),
                cover_mime: cover.as_ref().map(|c| c.mime.clone()),
            };
            opened.verify_unchanged_sync()?;
            publish(track).await?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn music_scan_respects_directory_ignore_markers() {
        let root = tempfile::tempdir().unwrap();
        for (index, marker) in [".nomedia", ".ignore", ".aetherignore"].iter().enumerate() {
            let directory = root.path().join(format!("excluded-{index}"));
            tokio::fs::create_dir(&directory).await.unwrap();
            tokio::fs::write(directory.join(marker), b"").await.unwrap();
            // Probing this invalid audio would fail the scan if the marker were ignored.
            tokio::fs::write(directory.join("broken.mp3"), b"not audio")
                .await
                .unwrap();
        }
        let count = scan(root.path(), &AtomicBool::new(false), |_| async { Ok(()) })
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
