//! Optional, bounded packet-copy preparation. Sources are inherited read-only handles.
//! No network/file input protocols, video encoding, or writes beside original media.
use crate::files::OpenedFile;
use std::{collections::HashMap, io, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::AsyncReadExt,
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
    directory: crate::cache_dirs::CacheDirectory,
}
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    bytes: u64,
}
pub struct Preparations {
    cache: Mutex<Cache>,
    worker: Semaphore,
    limits: io::Result<Limits>,
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
            limits: Limits::from_env(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Component {
    Video,
    Audio(u32),
    Subtitle(u32),
}
impl Component {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Video => "video/mp4",
            Self::Audio(_) => "audio/ogg",
            Self::Subtitle(_) => "text/vtt; charset=utf-8",
        }
    }
}

/// Call only with an inventory-resolved source and stream index verified by the caller.
/// Cache is process-local, removed at shutdown, capped at 64 entries and configured bytes.
/// Saturation fails explicitly: no unbounded queue, no eviction of playing files.
impl Preparations {
    pub async fn prepare(
        &self,
        root: &Path,
        relative: &Path,
        expected_size: u64,
        expected_mtime: &str,
        component: Component,
    ) -> io::Result<OpenedFile> {
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
        let cache = self.cache.lock().await;
        if let Some(entry) = cache.entries.get(&key) {
            return OpenedFile::open_in_root(entry.directory.path(), Path::new("content")).await;
        }
        if cache.entries.len() >= 64 || cache.bytes >= limits.cache {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "preparation cache full; restart to clear",
            ));
        }
        let _worker = self.worker.try_acquire().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "preparation busy; retry explicitly",
            )
        })?;
        let remaining = limits.cache - cache.bytes;
        drop(cache);
        let directory = crate::cache_dirs::CacheDirectory::new()?;
        let output = std::fs::File::create(directory.path().join("content"))?;
        let limit = remaining.min(match component {
            Component::Video => limits.video,
            Component::Audio(_) => limits.audio,
            Component::Subtitle(_) => 4 * 1024 * 1024,
        });
        let mut file = input.try_clone_std()?;
        std::io::Seek::rewind(&mut file)?;
        let mut command = Command::new("ffmpeg");
        command.args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "fd,pipe",
            "-format_whitelist",
            "matroska,webm,mov,mp4",
            "-max_alloc",
            "67108864",
            "-fd",
            "0",
            "-i",
            "fd:",
        ]);
        match component {
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
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("preparation output missing"))?;
        let mut bounded = stdout.take(limit + 1);
        let mut output = tokio::fs::File::from_std(output);
        let size = tokio::time::timeout(Duration::from_secs(limits.seconds), async {
            let size = tokio::io::copy(&mut bounded, &mut output).await?;
            if size > limit {
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
            input.verify_unchanged_sync()?;
            Ok::<_, io::Error>(size)
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "preparation timeout"))??;
        drop(output);
        let opened = OpenedFile::open_in_root(directory.path(), Path::new("content")).await?;
        let mut cache = self.cache.lock().await;
        cache.bytes += size;
        cache.entries.insert(key, Entry { directory });
        Ok(opened)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
