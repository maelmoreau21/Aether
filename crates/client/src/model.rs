//! Values exchanged with the Core, in the shape its JSON uses. Unknown fields
//! are ignored, so a newer Core keeps working with an older application.
use serde::{Deserialize, Serialize};

/// A device token. Store it in the platform keychain, never in plain files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access: String,
    /// Unix seconds; refresh well before (see `Client::refresh_if_due`).
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub login: String,
    /// `administrator` or `user`.
    pub role: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    /// Shown to the person, typed on the approving device if not scanned.
    pub code: String,
    /// Kept by this device only; it proves the request is ours when polling.
    pub secret: String,
    /// Opened by the approving phone (also encoded in the QR modules).
    pub url: String,
    pub expires_in: i64,
    pub qr_width: usize,
    /// Row-major, `qr_width × qr_width`, `true` for a dark module.
    pub qr_modules: Vec<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogQuery {
    /// `film` or `series`; `None` for both.
    pub kind: Option<String>,
    pub search: Option<String>,
    /// A library id from `Client::libraries`; `None` for every library.
    pub library: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub description: String,
    /// Server-relative and versioned (`/api/v1/assets/…/image?v=…`): cache it
    /// forever, fetch it with the token (`Client::url` makes it absolute).
    #[serde(default)]
    pub poster_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    /// The episode's own work id: play it with `Client::media`.
    pub id: String,
    #[serde(default)]
    pub number: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub id: String,
    #[serde(default)]
    pub number: Option<i64>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

/// A work with its seasons (empty for a film).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDetail {
    #[serde(flatten)]
    pub work: Work,
    #[serde(default)]
    pub seasons: Vec<Season>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub items: Vec<Work>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub total: Option<usize>,
}

/// A library the signed-in user may browse.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    /// `films`, `series`, `music`… as configured by the administrator.
    #[serde(default)]
    pub kind: String,
}

/// A saved position, most recent first in `Client::continue_watching`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    /// The film or episode to play.
    pub work_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: String,
    /// The series of an episode.
    #[serde(default)]
    pub series_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub watched: bool,
    /// Unix seconds.
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// The asset (file) id, used for `Client::playback_url`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackUrl {
    pub url: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub watched: bool,
    /// Pass it back as `ProgressUpdate::expected_revision`.
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressUpdate {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub expected_revision: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProgressWrite {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub expected_revision: i64,
}
