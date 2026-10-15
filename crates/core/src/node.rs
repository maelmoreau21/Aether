use crate::{
    error,
    media::{self, Probe},
};
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tower::ServiceExt;
use tower_http::services::ServeFile;

// Explicit synthetic inventory for this first bench. These are stable fixture IDs,
// not work/source identities or a persistent production catalogue.
pub const FIXTURES: [(&str, &str, &str); 4] = [
    ("fixture-mkv", "av1-opus.mkv", "video/x-matroska"),
    ("fixture-mp4", "av1-opus.mp4", "video/mp4"),
    ("fixture-webm", "av1-opus.webm", "video/webm"),
    ("fixture-fmp4", "av1-opus-fragmented.mp4", "video/mp4"),
];

pub const REMUX_FIXTURES: [(&str, &str, &str); 4] = [
    (
        "fixture-mkv-fr",
        "language-remux/av1-opus-mkv-fr.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-mkv-en",
        "language-remux/av1-opus-mkv-en.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-mp4-fr",
        "language-remux/av1-opus-mp4-fr.mp4",
        "video/mp4",
    ),
    (
        "fixture-mp4-en",
        "language-remux/av1-opus-mp4-en.mp4",
        "video/mp4",
    ),
];

#[derive(Clone, Serialize)]
pub struct LanguageRemux {
    pub source_asset: &'static str,
    pub language: &'static str,
    pub source_audio_stream: u32,
}

pub const REPRESENTATIVE_FIXTURES: [(&str, &str, &str); 4] = [
    (
        "fixture-representative-mkv",
        "representative/representative.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-representative-mp4",
        "representative/representative.mp4",
        "video/mp4",
    ),
    (
        "fixture-representative-mkv-en",
        "representative/representative-en.mkv",
        "video/x-matroska",
    ),
    (
        "fixture-representative-mp4-en",
        "representative/representative-en.mp4",
        "video/mp4",
    ),
];

#[derive(Clone, Serialize)]
pub struct RepresentativeCase {
    pub expected_audio_language: &'static str,
    pub expected_audio_channels: u32,
    pub method: &'static str,
}

#[derive(Clone, Serialize)]
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

struct Asset {
    descriptor: Descriptor,
    file: PathBuf,
}
pub struct Inventory {
    root: PathBuf,
    assets: Vec<Asset>,
}

pub async fn shaka_content(
    State(s): State<Arc<Inventory>>,
    Path(name): Path<String>,
    req: Request,
) -> Response {
    let (relative, mime) = match name.as_str() {
        "shaka-player.compiled.js" => (
            "vendor/shaka-5.2.13/shaka-player.compiled.js".to_owned(),
            "text/javascript; charset=utf-8",
        ),
        "manifest.mpd" => (format!("dash/{name}"), "application/dash+xml"),
        "video.mp4" => (format!("dash/{name}"), "video/mp4"),
        "audio-fr.mp4" | "audio-en.mp4" => (format!("dash/{name}"), "audio/mp4"),
        "fr-full.vtt" | "fr-forced.vtt" | "en-full.vtt" => {
            (format!("dash/{name}"), "text/vtt; charset=utf-8")
        }
        _ => return error(StatusCode::NOT_FOUND, "unknown_experiment_asset"),
    };
    let file = match tokio::fs::canonicalize(s.root.join(relative)).await {
        Ok(file) if file.starts_with(&s.root) && file.is_file() => file,
        _ => return error(StatusCode::NOT_FOUND, "experiment_asset_unavailable"),
    };
    match ServeFile::new(file)
        .with_buf_chunk_size(64 * 1024)
        .oneshot(req)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static(mime),
            );
            response
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "experiment_read_failed"),
    }
}

// Separate, fixed experimental inventory: never accept a browser-supplied path.
pub async fn mse_content(
    State(s): State<Arc<Inventory>>,
    Path(name): Path<String>,
    req: Request,
) -> Response {
    let mime = match name.as_str() {
        "video.mp4" => "video/mp4",
        "audio-fr.mp4" | "audio-en.mp4" => "audio/mp4",
        "fr-full.vtt" | "fr-forced.vtt" | "en-full.vtt" => "text/vtt; charset=utf-8",
        _ => return error(StatusCode::NOT_FOUND, "unknown_experiment_asset"),
    };
    let file = match tokio::fs::canonicalize(s.root.join("mse").join("verified").join(&name)).await
    {
        Ok(file) if file.starts_with(&s.root) && file.is_file() => file,
        _ => return error(StatusCode::NOT_FOUND, "experiment_asset_unavailable"),
    };
    match ServeFile::new(file)
        .with_buf_chunk_size(64 * 1024)
        .oneshot(req)
        .await
    {
        Ok(response) => {
            let mut response = response.into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                HeaderValue::from_static(mime),
            );
            response
        }
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "experiment_read_failed"),
    }
}

impl Inventory {
    pub async fn load(root: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tokio::fs::canonicalize(root).await?;
        let mut assets = Vec::new();
        let mut fixtures = FIXTURES.to_vec();
        if tokio::fs::try_exists(root.join("language-remux")).await? {
            fixtures.extend(REMUX_FIXTURES);
        }
        if tokio::fs::try_exists(root.join("representative")).await? {
            fixtures.extend(REPRESENTATIVE_FIXTURES);
        }
        for (id, name, mime) in fixtures {
            let file = root.join(name);
            // Reject links outside the selected root before probing, too.
            let canonical = tokio::fs::canonicalize(&file).await?;
            if !canonical.starts_with(&root) || !tokio::fs::metadata(&canonical).await?.is_file() {
                return Err("fixture outside root or not a regular file".into());
            }
            let probe = media::probe(&canonical).await?;
            let language_remux = match id {
                "fixture-mkv-fr" => Some(LanguageRemux {
                    source_asset: "fixture-mkv",
                    language: "fr",
                    source_audio_stream: 1,
                }),
                "fixture-mkv-en" => Some(LanguageRemux {
                    source_asset: "fixture-mkv",
                    language: "en",
                    source_audio_stream: 2,
                }),
                "fixture-mp4-fr" => Some(LanguageRemux {
                    source_asset: "fixture-mp4",
                    language: "fr",
                    source_audio_stream: 1,
                }),
                "fixture-mp4-en" => Some(LanguageRemux {
                    source_asset: "fixture-mp4",
                    language: "en",
                    source_audio_stream: 2,
                }),
                _ => None,
            };
            if let Some(remux) = &language_remux {
                let expected_language = if remux.language == "fr" { "fra" } else { "eng" };
                if probe.streams.len() != 2
                    || probe.streams[0].codec_name.as_deref() != Some("av1")
                    || probe.streams[1].codec_name.as_deref() != Some("opus")
                    || probe.streams[1]
                        .tags
                        .get("language")
                        .and_then(|v| v.as_str())
                        != Some(expected_language)
                {
                    return Err("language remux does not match its declared tracks".into());
                }
            }
            let representative = if id.starts_with("fixture-representative-") {
                let english = id.ends_with("-en");
                let audio_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("audio"))
                    .collect();
                let video_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("video"))
                    .collect();
                let subtitle_tracks: Vec<_> = probe
                    .streams
                    .iter()
                    .filter(|t| t.codec_type.as_deref() == Some("subtitle"))
                    .collect();
                if video_tracks.len() != 1
                    || subtitle_tracks.len() != 3
                    || audio_tracks.len() != if english { 1 } else { 2 }
                    || video_tracks[0].codec_name.as_deref() != Some("av1")
                    || video_tracks[0]
                        .details
                        .get("pix_fmt")
                        .and_then(|v| v.as_str())
                        != Some("yuv420p10le")
                    || audio_tracks
                        .iter()
                        .any(|t| t.codec_name.as_deref() != Some("opus"))
                    || audio_tracks[0]
                        .details
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        != Some(if english { 2 } else { 6 })
                    || audio_tracks[0]
                        .tags
                        .get("language")
                        .and_then(|v| v.as_str())
                        != Some(if english { "eng" } else { "fra" })
                {
                    return Err("representative fixture does not match declared AV1 10-bit/Opus/subtitle profile".into());
                }
                if !english
                    && (audio_tracks[1]
                        .details
                        .get("channels")
                        .and_then(|v| v.as_u64())
                        != Some(2)
                        || audio_tracks[1]
                            .tags
                            .get("language")
                            .and_then(|v| v.as_str())
                            != Some("eng"))
                {
                    return Err(
                        "representative EN track does not match declared stereo profile".into(),
                    );
                }
                Some(RepresentativeCase {
                    expected_audio_language: if english { "en" } else { "fr" },
                    expected_audio_channels: if english { 2 } else { 6 },
                    method: if english {
                        "prepared-en-remux-native-url"
                    } else {
                        "multi-track-native-url"
                    },
                })
            } else {
                None
            };
            assets.push(Asset {
                file,
                descriptor: Descriptor {
                    id,
                    mime,
                    probe,
                    playback_status: "unverified-on-client",
                    language_remux,
                    representative,
                },
            });
        }
        Ok(Self { root, assets })
    }

    pub fn descriptors(&self) -> Vec<Descriptor> {
        self.assets.iter().map(|a| a.descriptor.clone()).collect()
    }
}

pub async fn content(
    State(s): State<Arc<Inventory>>,
    Path(id): Path<String>,
    req: Request,
) -> Response {
    let Some(asset) = s.assets.iter().find(|a| a.descriptor.id == id) else {
        return error(StatusCode::NOT_FOUND, "unknown_asset");
    };
    let Ok(file) = tokio::fs::canonicalize(&asset.file).await else {
        return error(StatusCode::NOT_FOUND, "asset_unavailable");
    };
    if !file.starts_with(&s.root) {
        return error(StatusCode::FORBIDDEN, "asset_outside_fixture_root");
    }
    // ServeFile streams with a bounded 64 KiB buffer; dropping the body cancels I/O.
    let service = ServeFile::new(file).with_buf_chunk_size(65_536);
    match service.oneshot(req).await {
        Ok(res) => {
            let (mut parts, body) = res.into_parts();
            parts.headers.insert(
                "content-type",
                HeaderValue::from_static(asset.descriptor.mime),
            );
            Response::from_parts(parts, Body::new(body))
        }
        Err(e) => match e {},
    }
    .into_response()
}
