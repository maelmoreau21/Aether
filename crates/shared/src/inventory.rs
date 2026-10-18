use crate::{
    ids::{AssetId, ImageId, LibraryId, NodeId, ScanId},
    media::{LocalImage, NfoOutcome, Probe},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedFile {
    pub size: u64,
    pub modified_ns: String,
    pub probe: Probe,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScannedFile {
    #[serde(default)]
    pub audios: Vec<ExternalAudio>,
    /// Bounded text snapshots prepared by the Node; no public filesystem paths.
    #[serde(default)]
    pub subtitles: Vec<ExternalSubtitle>,
    pub poster: Option<LocalImage>,
    pub editorial: NfoOutcome,
    pub relative_path: String,
    pub size: u64,
    pub modified_ns: String,
    pub title: String,
    pub year: Option<u16>,
    /// Explicit provider identity or an exact local path, never fuzzy title matching.
    pub identity_key: String,
    pub external_ids: BTreeMap<String, String>,
    pub probe: Probe,
    pub mime: String,
    #[serde(default)]
    pub reused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_identity_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_external_ids: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_editorial: Option<NfoOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_poster: Option<LocalImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub season_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_number: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalSubtitle {
    pub language: String,
    pub label: String,
    pub forced: bool,
    pub sdh: bool,
    pub source_format: String,
    pub webvtt: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalAudio {
    pub relative_path: String,
    pub size: u64,
    pub modified_ns: String,
    pub language: String,
    pub probe: Probe,
    /// Timeline of this sidecar begins at video time zero. No filename offset guess.
    pub offset_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanIssue {
    pub relative_path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanReport {
    pub files: Vec<ScannedFile>,
    pub errors: Vec<ScanIssue>,
    pub warnings: Vec<ScanIssue>,
    pub errors_count: u64,
    pub warnings_count: u64,
    pub discovered: u64,
    pub probed: u64,
    pub reused: u64,
    /// False on interrupted/incomplete traversal: callers must not mark missing files unavailable.
    pub complete: bool,
}

/// Core -> Node scan initiation contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRequest {
    pub scan_id: ScanId,
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub config_revision: String,
}

/// Node -> Core scan acceptance acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanStarted {
    pub scan_id: ScanId,
    pub node_id: NodeId,
    pub library_id: LibraryId,
    pub config_revision: String,
}

/// Node -> Core bounded batch of discovered media observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InventoryBatch {
    pub scan_id: ScanId,
    pub sequence: u64,
    pub files: Vec<ScannedFile>,
    pub errors: Vec<ScanIssue>,
    pub warnings: Vec<ScanIssue>,
}

/// Core -> Node durable batch acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchAck {
    pub scan_id: ScanId,
    pub sequence: u64,
}

/// Node -> Core scan completion summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanFinished {
    pub scan_id: ScanId,
    pub last_sequence: u64,
    pub discovered: u64,
    pub probed: u64,
    pub reused: u64,
    pub complete: bool,
}

/// Node -> Core scan failure report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanFailed {
    pub scan_id: ScanId,
    pub code: String,
    pub retryable: bool,
    pub config_revision: String,
}

/// Core -> Node authorized read grant for an asset or image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadTarget {
    Asset(AssetId),
    Image {
        id: ImageId,
        expected_size: u64,
        expected_modified_ns: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadGrant {
    pub target: ReadTarget,
    pub library_id: LibraryId,
    pub node_id: NodeId,
    pub root_snapshot: String,
    pub relative_path: String,
    pub mime: String,
}

/// Synthetic bench contracts shared between Node and Core.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageRemux {
    pub source_asset: &'static str,
    pub language: &'static str,
    pub source_audio_stream: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepresentativeCase {
    pub expected_audio_language: &'static str,
    pub expected_audio_channels: u32,
    pub method: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    pub id: &'static str,
    pub mime: &'static str,
    pub probe: Probe,
    pub playback_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_remux: Option<LanguageRemux>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representative: Option<RepresentativeCase>,
}
