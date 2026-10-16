use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub streams: Vec<Track>,
    pub format: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilmMetadata {
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub plot: Option<String>,
    pub outline: Option<String>,
    pub year: Option<u16>,
    pub premiered: Option<String>,
    pub genres: Vec<String>,
    pub rating: Option<f64>,
    pub mpaa: Option<String>,
    pub external_ids: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NfoOutcome {
    Missing,
    Valid {
        metadata: FilmMetadata,
        path: String,
        priority: u8,
    },
    Invalid {
        path: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalImage {
    pub path: String,
    pub size_bytes: u64,
    pub modified_ns: String,
    pub mime: String,
    pub priority: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedImage {
    pub mime: &'static str,
    pub size_bytes: u64,
    pub modified_ns: String,
}
