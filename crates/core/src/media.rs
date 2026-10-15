//! Temporary ffprobe adapter for P0. Never launched by a playback request.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("probe I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("probe timeout (10 seconds)")]
    Timeout,
    #[error("probe output exceeds 1 MiB")]
    TooLarge,
    #[error("invalid or unsupported media (ffprobe rejected input)")]
    Rejected,
    #[error("invalid probe JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub index: u32,
    pub codec_type: Option<String>,
    pub codec_name: Option<String>,
    pub time_base: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, Value>,
    // Keep all available profiles, dimensions, channels, dispositions, init data,
    // start/duration and side data without fabricating unavailable values.
    #[serde(flatten)]
    pub details: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Probe {
    pub streams: Vec<Track>,
    pub format: BTreeMap<String, Value>,
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
