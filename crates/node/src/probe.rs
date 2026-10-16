//! Bounded ffprobe adapter for Media Node discovery. Never launched by a playback request.
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
            "matroska,webm,mov",
            "-max_alloc",
            "67108864",
            "-probesize",
            "5000000",
            "-analyzeduration",
            "5000000",
            "-show_streams",
            "-show_format",
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
}
