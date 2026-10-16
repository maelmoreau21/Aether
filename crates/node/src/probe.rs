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
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file",
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
        ])
        .arg(path)
        .stdin(Stdio::null())
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
    let result = probe(opened.canonical_path()).await?;
    opened
        .verify_unchanged_sync()
        .map_err(|_| ProbeError::Rejected)?;
    Ok(result)
}
