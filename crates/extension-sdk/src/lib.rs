//! Extension API v1 for Aether plugins.
//! Designed for sandboxed WebAssembly execution (target wasm32-unknown-unknown)
//! with host-brokered capabilities (secure HTTPS access, configuration, metadata contracts).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod storage;

pub const API_VERSION: u32 = 1;
pub const MAX_MODULE: usize = 262_144;
pub const MAX_JSON: usize = 65_536;

/// Generic ABI envelope. Host adapters authorize operations before invocation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRequest {
    pub api_version: u32,
    pub operation: String,
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionResponse {
    pub api_version: u32,
    pub result: serde_json::Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOption {
    pub label: String,
    pub value: String,
}

impl From<&str> for ConfigOption {
    fn from(s: &str) -> Self {
        Self {
            label: s.to_string(),
            value: s.to_string(),
        }
    }
}

impl From<String> for ConfigOption {
    fn from(s: String) -> Self {
        Self {
            label: s.clone(),
            value: s,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub field_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ConfigOption>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub repository: String,
    pub permissions: Vec<String>,
    /// Exact HTTPS hosts requested by this signed package; empty means no network access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_abi: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<Vec<ConfigField>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Package {
    pub manifest: Manifest,
    pub publisher_id: String,
    pub wasm: String,
    pub sha256: String,
    pub signature: String,
}

impl Package {
    /// Exact signing input: compact serde JSON tuple, in this field order.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&(
            "media-extension-package-v1",
            &self.manifest,
            &self.publisher_id,
            &self.sha256,
        ))
    }
}

// =========================================================================
// Rich Metadata Contracts
// =========================================================================

/// Supported media types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Movie,
    TvShow,
    Episode,
    Anime,
    /// Music release (album/single) or track.
    Music,
}

/// Capabilities exposed by a plugin
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    Search,
    Details,
    Images,
    Avatars,
    /// Plain and time-synced lyrics (lyrics.get operation).
    Lyrics,
    /// Intro and credits segment detection.
    Markers,
}

/// Image category
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageType {
    Poster,
    Backdrop,
    Banner,
    Logo,
    ClearArt,
    Thumb,
    DiscArt,
}

/// A media image (poster, backdrop, logo, etc.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaImage {
    pub url: String,
    pub image_type: ImageType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vote_count: Option<u64>,
    #[serde(default)]
    pub is_primary: bool,
}

/// An actor or crew credit
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCredit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

/// A rating score from a specific provider
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRating {
    pub provider: String,
    pub value: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes: Option<u64>,
}

/// Full structured metadata returned by a provider
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredMetadata {
    pub provider: String,
    pub provider_id: String,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_minutes: Option<u32>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub directors: Vec<String>,
    #[serde(default)]
    pub writers: Vec<String>,
    #[serde(default)]
    pub actors: Vec<ActorCredit>,
    #[serde(default)]
    pub ratings: Vec<ProviderRating>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub posters: Vec<MediaImage>,
    #[serde(default)]
    pub backdrops: Vec<MediaImage>,
    #[serde(default)]
    pub images: Vec<MediaImage>,
    #[serde(default)]
    pub seasons: Vec<SeasonMetadata>,
    #[serde(default)]
    pub translations: BTreeMap<String, TranslationItem>,
}

/// Localized translation for title, overview and tagline
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
}

/// Metadata for an episode within a season
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeMetadata {
    pub episode_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub still_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
}

/// Metadata for a TV season
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonMetadata {
    pub season_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(default)]
    pub episodes: Vec<EpisodeMetadata>,
}

/// Search query payload sent to the extension
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

fn default_media_type() -> MediaType {
    MediaType::Movie
}

/// Individual item in search results
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub id: String,
    pub title: String,
    /// Exact credited artist for music search; absent for non-music or unknown results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    pub media_type: MediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Details request payload
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailsRequest {
    pub id: String,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

/// Images request payload
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesRequest {
    pub id: String,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

// =========================================================================
// Lyrics Contracts (`lyrics.get` operation)
// =========================================================================

/// One time-synced lyric line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

/// Lyrics of a track returned by a lyrics provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub id: u64,
    pub track_name: String,
    pub artist_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    pub instrumental: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plain: Option<String>,
    /// Raw LRC text (`[mm:ss.xx] line`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced: Option<String>,
    #[serde(default)]
    pub synced_lines: Vec<LyricLine>,
}

/// Request payload of the `lyrics.get` operation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsRequest {
    /// Provider identifier; when set, other fields are ignored.
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub track: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<u32>,
}

// =========================================================================
// Timeline Markers Contracts (markers.get / intro.detect operations)
// =========================================================================

/// Marker indicating an intro, credits or recap interval.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentMarker {
    pub start: f64,
    pub end: f64,
    #[serde(default = "default_marker_type")]
    pub marker_type: String,
    #[serde(default = "default_marker_confidence")]
    pub confidence: f64,
}

fn default_marker_type() -> String {
    "intro".into()
}

fn default_marker_confidence() -> f64 {
    1.0
}

/// Media markers collection returned for a playable asset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMarkers {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<SegmentMarker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<SegmentMarker>,
    #[serde(default)]
    pub markers: Vec<SegmentMarker>,
}
/// Legacy / simple metadata proposal
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataInput {
    pub api_version: u32,
    pub work_id: String,
    pub library_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetadataProposal {
    pub title: String,
    pub summary: String,
}

/// Standard error structure for extension execution
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {}

impl PluginError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            code: "rate_limited".into(),
            message: format!("Provider rate limit exceeded. Retry after {retry_after_secs}s"),
            retry_after_secs: Some(retry_after_secs),
        }
    }

    pub fn not_found(id: &str) -> Self {
        Self {
            code: "not_found".into(),
            message: format!("Media '{id}' not found on provider"),
            retry_after_secs: None,
        }
    }

    pub fn missing_api_key(provider: &str) -> Self {
        Self {
            code: "missing_api_key".into(),
            message: format!("API key is required for provider '{provider}'"),
            retry_after_secs: None,
        }
    }
}

// =========================================================================
// Host HTTP Brokering Contracts
// =========================================================================

/// Host HTTP request emitted by an extension through the Aether broker.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostHttpRequest {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
}

fn default_http_method() -> String {
    "GET".into()
}

/// Host HTTP response returned by the Aether broker.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Host HTTP broker error.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostHttpError {
    pub code: String,
    pub message: String,
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "aether")]
unsafe extern "C" {
    fn aether_host_http(req_ptr: u32, req_len: u32) -> u64;
}

/// Call the Aether host HTTP broker to execute a secure HTTPS request.
/// On wasm32, invokes the host import `aether_host_http`.
/// Outside of wasm32 (unit tests/native), returns an explicit error or test mock.
pub fn host_http_fetch(request: &HostHttpRequest) -> Result<HostHttpResponse, HostHttpError> {
    let req_bytes = serde_json::to_vec(request).map_err(|e| HostHttpError {
        code: "invalid_request".into(),
        message: e.to_string(),
    })?;

    if req_bytes.len() > MAX_JSON {
        return Err(HostHttpError {
            code: "request_too_large".into(),
            message: format!(
                "Request size {} exceeds maximum {}",
                req_bytes.len(),
                MAX_JSON
            ),
        });
    }

    #[cfg(target_arch = "wasm32")]
    {
        let ptr = req_bytes.as_ptr() as u32;
        let packed = unsafe { aether_host_http(ptr, req_bytes.len() as u32) };
        if packed == 0 {
            return Err(HostHttpError {
                code: "host_call_failed".into(),
                message: "Host HTTP broker returned null pointer".into(),
            });
        }
        let resp_ptr = (packed >> 32) as usize;
        let resp_len = (packed & 0xffff_ffff) as usize;
        let resp_bytes = unsafe { std::slice::from_raw_parts(resp_ptr as *const u8, resp_len) };
        let response: Result<HostHttpResponse, HostHttpError> = serde_json::from_slice(resp_bytes)
            .map_err(|e| HostHttpError {
                code: "invalid_host_response".into(),
                message: e.to_string(),
            })?;
        response
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Native fallback for local tests and mocks
        Err(HostHttpError {
            code: "unsupported_platform".into(),
            message: "Host HTTP call requires wasm32 runtime or test mock".into(),
        })
    }
}

// =========================================================================
// Wasm ABI Runner & Macro
// =========================================================================

/// Implement the ABI without exposing raw pointers to extension authors.
/// Buffers live for one invocation; the host destroys the whole guest afterwards.
#[macro_export]
macro_rules! extension {
    ($handler:path) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn media_alloc(len: u32) -> u32 {
            if len as usize > $crate::MAX_JSON {
                return 0;
            }
            let buffer = vec![0u8; len as usize].into_boxed_slice();
            Box::into_raw(buffer) as *mut u8 as u32
        }
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn media_run(ptr: u32, len: u32) -> u64 {
            if len as usize > $crate::MAX_JSON {
                return 0;
            }
            // The ABI host supplies the allocation returned by media_alloc.
            let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let Ok(input) = $crate::serde_json::from_slice::<$crate::ExtensionRequest>(bytes)
            else {
                return 0;
            };
            let result: $crate::ExtensionResponse = $handler(input);
            let Ok(bytes) = $crate::serde_json::to_vec(&result) else {
                return 0;
            };
            if bytes.len() > $crate::MAX_JSON {
                return 0;
            }
            let len = bytes.len() as u64;
            let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as u32;
            ((ptr as u64) << 32) | len
        }
    };
}
#[doc(hidden)]
pub use serde_json;

pub mod objects;
