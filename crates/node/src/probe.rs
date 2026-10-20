//! Bounded ffprobe adapter for Media Node discovery. Never launched by a playback request.
//!
//! # Security Architecture & Sandbox Limits
//!
//! Input files are passed strictly as open OS file descriptors / handles (`std::fs::File`)
//! inherited via stdin (`Stdio::from(file)`), coupled with `-protocol_whitelist fd,pipe`.
//!
//! **Important Boundary Note**: An FFmpeg protocol whitelist (`fd,pipe`) prevents libavformat
//! from resolving arbitrary file paths (`file:`), network URLs (`http:`, `https:`), or nested
//! format references (`concat:`, `subfile:`, MOV `dref` atoms). However, **it is not a full OS sandbox**:
//! 1. FFmpeg protocol filters operate at application/library level, not the OS kernel level. They do
//!    not replace OS-level isolation (such as Docker non-root user `10001:10001`, `cap_drop: ALL`,
//!    `no-new-privileges: true`, or seccomp profiles).
//! 2. Parsers for complex media demuxers run inside the external `ffprobe` process. Defense-in-depth
//!    relies on:
//!    - Strictly bounded memory allocations (`-max_alloc 67108864`).
//!    - Bounded analysis windows (`-probesize 5000000`, `-analyzeduration 5000000`).
//!    - Hard process execution timeouts (`10 seconds`) with `kill_on_drop(true)`.
//!    - Maximum output stdout capture limit (`take(1_048_577)` / 1 MiB).
//!    - File handle identity verification before and after execution (`OpenedFile::verify_unchanged_sync`).
pub use media_contracts::media::{Probe, Track};
use std::{io, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

use crate::files::OpenedFile;

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("probe I/O: {0}")]
    Io(#[from] io::Error),
    #[error("probe timeout (10 seconds)")]
    Timeout,
    #[error("probe output exceeds 1 MiB")]
    TooLarge,
    #[error("invalid or unsupported media (ffprobe rejected input)")]
    Rejected,
    #[error("invalid probe JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub async fn probe(path: &Path) -> Result<Probe, ProbeError> {
    let file = std::fs::File::open(path)?;
    probe_file(file).await
}

pub async fn probe_file(mut file: std::fs::File) -> Result<Probe, ProbeError> {
    use std::io::Seek;
    file.rewind()?;
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "fd,pipe",
            "-format_whitelist",
            "matroska,webm,mov,mp4,ogg,mp3,flac,wav,aac",
            "-max_alloc",
            "67108864",
            "-probesize",
            "5000000",
            "-analyzeduration",
            "5000000",
            "-show_streams",
            "-show_format",
            "-show_chapters",
            "-show_data",
            "-of",
            "json",
            "-",
        ])
        .stdin(Stdio::from(file))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(ProbeError::Rejected)?
        .take(1_048_577);
    let mut bytes = Vec::new();
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        stdout.read_to_end(&mut bytes).await?;
        if bytes.len() > 1_048_576 {
            return Err(ProbeError::TooLarge);
        }
        if !child.wait().await?.success() {
            return Err(ProbeError::Rejected);
        }
        Ok(())
    })
    .await
    .map_err(|_| ProbeError::Timeout)?;
    outcome?;
    let mut result: Probe = serde_json::from_slice(&bytes)?;
    // The public diagnostic API must never disclose a local absolute path.
    result.format.remove("filename");
    if result.streams.is_empty() {
        return Err(ProbeError::Rejected);
    }
    Ok(result)
}

/// Probe an already-confined `OpenedFile` and verify that the underlying handle
/// and path metadata remained unchanged throughout the external `ffprobe` execution.
pub async fn probe_opened(opened: &OpenedFile) -> Result<Probe, ProbeError> {
    opened
        .verify_unchanged_sync()
        .map_err(|_| ProbeError::Rejected)?;
    let file = opened.try_clone_std().map_err(ProbeError::Io)?;
    let result = probe_file(file).await?;
    opened
        .verify_unchanged_sync()
        .map_err(|_| ProbeError::Rejected)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated")
    }

    #[tokio::test]
    async fn probe_opened_consumes_handle_and_resists_path_substitution() {
        let root = tempfile::tempdir().unwrap();
        let fixtures = fixture_root();
        let original = root.path().join("film.mkv");
        let substitute = root.path().join("substitute.mp4");

        // Copy a 3-stream MKV as the original confined file
        tokio::fs::copy(fixtures.join("av1-opus.mkv"), &original)
            .await
            .unwrap();
        // Copy a 6-stream MP4 as the substitute
        tokio::fs::copy(
            fixtures.join("representative/representative.mp4"),
            &substitute,
        )
        .await
        .unwrap();

        // 1. Confine and open the original file
        let opened = OpenedFile::open_in_root(root.path(), Path::new("film.mkv"))
            .await
            .unwrap();

        // 2. Perform path substitution on disk while handle is held
        let backup = root.path().join("film_backup.mkv");
        tokio::fs::rename(&original, &backup).await.unwrap();
        tokio::fs::copy(&substitute, &original).await.unwrap();

        // A path-based probe on the disk path probes the substituted file (6 streams)
        let path_probe = probe(&original).await.unwrap();
        assert_eq!(path_probe.streams.len(), 6);

        // Directly probing the descriptor of the opened file probes the original file (3 streams)
        let handle_probe = probe_file(opened.try_clone_std().unwrap()).await.unwrap();
        assert_eq!(handle_probe.streams.len(), 3);
        assert_eq!(handle_probe.streams[0].codec_name.as_deref(), Some("av1"));

        // probe_opened detects that the path on disk was altered and rejects
        assert!(matches!(
            probe_opened(&opened).await,
            Err(ProbeError::Rejected)
        ));

        // 3. Transient substitution: restore original file path on disk before probe_opened
        tokio::fs::remove_file(&original).await.unwrap();
        tokio::fs::rename(&backup, &original).await.unwrap();

        // Now probe_opened succeeds, consuming the verified handle with identical 3 streams
        opened.verify_unchanged_sync().unwrap();
        let restored_probe = probe_opened(&opened).await.unwrap();
        assert_eq!(restored_probe.streams.len(), 3);
        assert_eq!(restored_probe.streams[0].codec_name.as_deref(), Some("av1"));
    }

    #[tokio::test]
    async fn probe_real_mkv_and_mp4_containers_and_fragmented() {
        let fixtures = fixture_root();

        for filename in ["av1-opus.mkv", "av1-opus.mp4", "av1-opus-fragmented.mp4"] {
            let path = fixtures.join(filename);
            let file = std::fs::File::open(&path).unwrap();
            let probe = probe_file(file).await.unwrap();

            assert_eq!(
                probe.streams.len(),
                3,
                "File {filename} must have 3 streams"
            );
            assert_eq!(probe.streams[0].codec_name.as_deref(), Some("av1"));
            assert_eq!(probe.streams[0].codec_type.as_deref(), Some("video"));
            assert_eq!(probe.streams[1].codec_name.as_deref(), Some("opus"));
            assert_eq!(probe.streams[1].codec_type.as_deref(), Some("audio"));
            assert_eq!(probe.streams[2].codec_name.as_deref(), Some("opus"));
            assert_eq!(probe.streams[2].codec_type.as_deref(), Some("audio"));

            assert_eq!(
                probe.streams[1]
                    .tags
                    .get("language")
                    .and_then(|v| v.as_str()),
                Some("fra")
            );
            assert_eq!(
                probe.streams[2]
                    .tags
                    .get("language")
                    .and_then(|v| v.as_str()),
                Some("eng")
            );

            assert!(
                !probe.format.contains_key("filename"),
                "Filename must never be disclosed"
            );
            assert!(probe.format.contains_key("duration") || filename.contains("fragmented"));
        }
    }

    #[tokio::test]
    async fn probe_representative_10bit_av1_and_multitrack_opus() {
        let fixtures = fixture_root().join("representative");

        for filename in ["representative.mkv", "representative.mp4"] {
            let path = fixtures.join(filename);
            let file = std::fs::File::open(&path).unwrap();
            let probe = probe_file(file).await.unwrap();

            assert_eq!(probe.streams.len(), 6, "{filename} must have 6 streams");

            // Video stream: AV1 10-bit
            let video = &probe.streams[0];
            assert_eq!(video.codec_name.as_deref(), Some("av1"));
            assert_eq!(video.codec_type.as_deref(), Some("video"));
            assert_eq!(
                video.details.get("pix_fmt").and_then(|v| v.as_str()),
                Some("yuv420p10le")
            );

            // Audio streams: Opus 5.1 and stereo
            let audio_tracks: Vec<_> = probe
                .streams
                .iter()
                .filter(|s| s.codec_type.as_deref() == Some("audio"))
                .collect();
            assert_eq!(audio_tracks.len(), 2);
            assert_eq!(audio_tracks[0].codec_name.as_deref(), Some("opus"));
            assert_eq!(
                audio_tracks[0]
                    .details
                    .get("channels")
                    .and_then(|v| v.as_i64()),
                Some(6)
            );
            assert_eq!(audio_tracks[1].codec_name.as_deref(), Some("opus"));
            assert_eq!(
                audio_tracks[1]
                    .details
                    .get("channels")
                    .and_then(|v| v.as_i64()),
                Some(2)
            );

            // Subtitle tracks: 3 tracks
            let sub_tracks: Vec<_> = probe
                .streams
                .iter()
                .filter(|s| s.codec_type.as_deref() == Some("subtitle"))
                .collect();
            assert_eq!(sub_tracks.len(), 3);

            let duration: f64 = probe.format["duration"].as_str().unwrap().parse().unwrap();
            assert!((duration - 12.0).abs() < 0.5, "Duration should be ~12.0s");
            assert!(!probe.format.contains_key("filename"));
        }
    }

    #[tokio::test]
    async fn probe_mp4_with_moov_at_end_validates_seek_via_file_descriptor() {
        let ffmpeg_check = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        if ffmpeg_check.is_err() {
            eprintln!("ffmpeg not found, skipping synthetic moov_at_end generation test");
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let moov_at_end_path = temp.path().join("moov_at_end.mp4");

        // Generate non-faststart MP4 where moov is placed after mdat at the end of the file
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x180:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1:frequency=440",
                "-c:v",
                "libaom-av1",
                "-crf",
                "50",
                "-b:v",
                "0",
                "-c:a",
                "libopus",
                moov_at_end_path.to_str().unwrap(),
            ])
            .output()
            .expect("ffmpeg failed to execute");
        assert!(
            output.status.success(),
            "ffmpeg must successfully create non-faststart MP4"
        );

        // Verify atom positions in raw bytes: mdat must precede moov
        let bytes = std::fs::read(&moov_at_end_path).unwrap();
        let mdat_pos = bytes
            .windows(4)
            .position(|w| w == b"mdat")
            .expect("mdat atom must exist");
        let moov_pos = bytes
            .windows(4)
            .position(|w| w == b"moov")
            .expect("moov atom must exist");
        assert!(
            mdat_pos < moov_pos,
            "moov ({moov_pos}) must be positioned after mdat ({mdat_pos}) to test seek"
        );

        // 1. Probing via open file descriptor MUST succeed because fd 0 supports lseek/SetFilePointer
        let file = std::fs::File::open(&moov_at_end_path).unwrap();
        let probe_res = probe_file(file)
            .await
            .expect("probe_file must succeed on moov-at-end MP4");
        assert_eq!(probe_res.streams.len(), 2);
        assert_eq!(probe_res.streams[0].codec_name.as_deref(), Some("av1"));
        assert_eq!(probe_res.streams[1].codec_name.as_deref(), Some("opus"));
        assert!(!probe_res.format.contains_key("filename"));

        // 2. Probing via OpenedFile in a root MUST also succeed and detect no modification
        let opened = OpenedFile::open_in_root(temp.path(), Path::new("moov_at_end.mp4"))
            .await
            .unwrap();
        let opened_probe = probe_opened(&opened)
            .await
            .expect("probe_opened must succeed on moov-at-end MP4");
        assert_eq!(opened_probe.streams.len(), 2);
    }

    #[tokio::test]
    async fn probe_codecs_matrix_hevc_h264_aac_ac3_eac3() {
        let ffmpeg_check = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        if ffmpeg_check.is_err() {
            eprintln!("ffmpeg not found, skipping synthetic codec matrix test");
            return;
        }

        let temp = tempfile::tempdir().unwrap();

        // 1. H.264 + AAC MP4
        let h264_path = temp.path().join("h264_aac.mp4");
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x180:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1:frequency=440",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
                h264_path.to_str().unwrap(),
            ])
            .output()
            .expect("ffmpeg failed to generate h264 clip");
        assert!(output.status.success());

        let probe_h264 = probe_file(std::fs::File::open(&h264_path).unwrap())
            .await
            .expect("probe_file must succeed on H.264 + AAC MP4");
        assert_eq!(probe_h264.streams.len(), 2);
        assert_eq!(probe_h264.streams[0].codec_name.as_deref(), Some("h264"));
        assert_eq!(probe_h264.streams[0].codec_type.as_deref(), Some("video"));
        assert_eq!(probe_h264.streams[1].codec_name.as_deref(), Some("aac"));
        assert_eq!(probe_h264.streams[1].codec_type.as_deref(), Some("audio"));

        // 2. HEVC + AC3 / EAC3 MKV with subtitles
        let hevc_path = temp.path().join("hevc_ac3.mkv");
        let sub_path = temp.path().join("sub.srt");
        std::fs::write(&sub_path, "1\n00:00:00,000 --> 00:00:01,000\nSub\n").unwrap();

        let output = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x180:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1:frequency=440",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1:frequency=880",
                "-i",
                sub_path.to_str().unwrap(),
                "-map",
                "0:v",
                "-map",
                "1:a",
                "-map",
                "2:a",
                "-map",
                "3:s",
                "-c:v",
                "libx265",
                "-pix_fmt",
                "yuv420p",
                "-c:a:0",
                "eac3",
                "-c:a:1",
                "ac3",
                "-c:s",
                "subrip",
                hevc_path.to_str().unwrap(),
            ])
            .output()
            .expect("ffmpeg failed to generate hevc clip");
        assert!(output.status.success());

        let probe_hevc = probe_file(std::fs::File::open(&hevc_path).unwrap())
            .await
            .expect("probe_file must succeed on HEVC + EAC3/AC3 MKV");
        assert_eq!(probe_hevc.streams.len(), 4);
        assert_eq!(probe_hevc.streams[0].codec_name.as_deref(), Some("hevc"));
        assert_eq!(probe_hevc.streams[1].codec_name.as_deref(), Some("eac3"));
        assert_eq!(probe_hevc.streams[2].codec_name.as_deref(), Some("ac3"));
        assert_eq!(probe_hevc.streams[3].codec_name.as_deref(), Some("subrip"));
    }
}
