//! Local/remote media delivery after common route authorization.

use super::decode;
use super::playback::{audio_operational_id, native_opus_track_id, playback_library_options};
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{
    LazyLock, Mutex,
    atomic::{AtomicU64, Ordering},
};

const IMAGE_META_CACHE_CAP: usize = 4096;
static IMAGE_META_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct CachedImageMeta {
    state_fp: (i64, i64, i64, u64),
    node_id: String,
    library_id: String,
    root_snapshot: String,
    relative_path: String,
    mime: String,
    size: u64,
    modified_ns: String,
}

#[derive(Default)]
struct ImageMetaCache {
    entries: HashMap<(u64, String), CachedImageMeta>,
    order: VecDeque<(u64, String)>,
}

static IMAGE_META_CACHE: LazyLock<Mutex<ImageMetaCache>> =
    LazyLock::new(|| Mutex::new(ImageMetaCache::default()));

async fn image_state_fingerprint(store: &IdentityStore) -> Option<(i64, i64, i64, u64)> {
    let row: (i64, i64, i64) = query_as(
        "SELECT \
         CAST(COALESCE((SELECT gen FROM inventory_cache_generation WHERE id = 1), 0) AS BIGINT), \
         (SELECT COUNT(*) FROM media_nodes), \
         (SELECT COUNT(CASE WHEN status != 'revoked' THEN 1 END) FROM media_nodes)",
    )
    .fetch_one(&store.pool)
    .await
    .ok()?;
    let epoch = IMAGE_META_EPOCH.load(Ordering::Relaxed);
    Some((row.0, row.1, row.2, epoch))
}

fn cached_image_meta(
    cache_scope: u64,
    image_id: &str,
    state_fp: (i64, i64, i64, u64),
) -> Option<CachedImageMeta> {
    let key = (cache_scope, image_id.to_owned());
    let cache = IMAGE_META_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let entry = cache.entries.get(&key)?;
    (entry.state_fp == state_fp).then(|| entry.clone())
}

fn store_image_meta(cache_scope: u64, image_id: &str, meta: CachedImageMeta) {
    let key = (cache_scope, image_id.to_owned());
    let mut cache = IMAGE_META_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if !cache.entries.contains_key(&key) {
        while cache.entries.len() >= IMAGE_META_CACHE_CAP {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            cache.entries.remove(&oldest);
        }
        cache.order.push_back(key.clone());
    }
    cache.entries.insert(key, meta);
}

fn invalidate_image_meta(cache_scope: u64, image_id: &str) {
    IMAGE_META_EPOCH.fetch_add(1, Ordering::Relaxed);
    let key = (cache_scope, image_id.to_owned());
    let mut cache = IMAGE_META_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.entries.remove(&key);
}

fn expected_image_etag(modified_ns: &str, size: u64, preview_width: Option<u32>) -> String {
    match preview_width {
        Some(width) => format!("\"preview-v1-{modified_ns}-{size:x}-{width}\""),
        None => format!("\"{modified_ns}-{size:x}\""),
    }
}

fn if_none_match_matches(headers: &axum::http::HeaderMap, etag: &str) -> bool {
    headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|condition| {
            condition.split(',').any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
            })
        })
}

/// Returns the `Cache-Control` header value for a sidecar refreshed inline after
/// `ImageChangedRescanRequired`: preserve 1-year `immutable` caching only if `?v=`
/// already matches the updated `(modified_ns, size_bytes)`, otherwise force revalidation.
fn refreshed_image_cache_control(
    version_token: Option<&str>,
    modified_ns: &str,
    size_bytes: u64,
) -> Option<&'static str> {
    let requested = version_token?;
    let current = format!("{modified_ns}-{size_bytes}");
    Some(if requested == current {
        "private, max-age=31536000, immutable"
    } else {
        "private, no-cache"
    })
}

pub async fn prepared(
    store: &IdentityStore,
    preparations: &std::sync::Arc<media_node::packaging::Preparations>,
    asset_id: &str,
    component: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_node::packaging::Component;
    let row = query(
        "SELECT a.relative_path,a.size,a.modified_ns,a.probe_json,a.library_id,l.root,l.node_id \
            FROM inventory_assets a \
            JOIN inventory_sources s ON s.id=a.source_id \
            JOIN libraries l ON l.id=a.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root \
            AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "asset_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let remote_id = row.get::<String, _>("node_id");
    let request_started = std::time::Instant::now();
    eprintln!(
        "{}",
        json!({"event":"prepared_http_start","asset":asset_id,"node":remote_id,"component":component,"method":request.method().as_str(),"range":request.headers().contains_key("range")})
    );
    let probe = match decode(row.get("probe_json")) {
        Ok(probe) => probe,
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "probe_invalid"),
    };
    let transcode = media_node::transcode::request(component);
    let hls = media_node::hls::request(component);
    // A burnt subtitle obeys the same library policy as an extracted one.
    let subtitle_index = component
        .strip_prefix("subtitle-ass-")
        .or_else(|| component.strip_prefix("subtitle-pgs-"))
        .or_else(|| component.strip_prefix("subtitle-"))
        .and_then(|s| s.parse::<u32>().ok())
        .or_else(|| transcode.and_then(|request| request.subtitle));
    if let Some(index) = subtitle_index {
        let options = match playback_library_options(
            store,
            row.get::<String, _>("library_id").as_str(),
        )
        .await
        {
            Ok(options) => options,
            Err(_) => {
                return crate::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "library_options_unavailable",
                );
            }
        };
        if probe.streams.iter().any(|track| {
            track.index == index
                && track.codec_type.as_deref() == Some("subtitle")
                && !options.embedded_subtitle_allowed(track.codec_name.as_deref())
        }) {
            return crate::error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "embedded_subtitle_disabled",
            );
        }
    }
    let stream_position = media_node::video_stream::position(component);
    // Audio and video encodes follow their own server switch; copied streams need neither.
    let (encodes_audio, encodes_video) = match (&transcode, &hls) {
        (Some(request), _) => (
            request.audio.is_some() && !request.has(media_node::transcode::FLAG_COPY_AUDIO),
            !request.copies_video(),
        ),
        (None, Some(request)) => (request.convert, request.encode.is_some()),
        (None, None) => (false, false),
    };
    if encodes_audio || encodes_video {
        let settings = match crate::server_settings::load(store).await {
            Ok(settings) => settings,
            Err(status) => return crate::error(status, "settings_unavailable"),
        };
        if let Some(code) = encode_refusal(
            &settings,
            (encodes_audio, encodes_video),
            transcode.as_ref(),
            hls.as_ref(),
        ) {
            let mut response = crate::error(StatusCode::UNPROCESSABLE_ENTITY, code);
            response.headers_mut().insert(
                "x-media-preparation-error",
                axum::http::HeaderValue::from_static(code),
            );
            return response;
        }
    }
    let selected = if let Some(request) = hls {
        super::hls::allowed(&probe, &request).then_some(Component::Video)
    } else if let Some(request) = transcode {
        transcode_allowed(&probe, &request).then_some(Component::Video)
    } else if let Some((index, position)) = stream_position {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .any(|track| {
                track.index == index
                    && track.codec_type.as_deref() == Some("video")
                    && track.codec_name.as_deref() == Some("av1")
            })
            .then_some(Component::Video)
            .filter(|_| {
                duration.is_some_and(|duration| {
                    duration.is_finite() && f64::from(position) / 1000.0 < duration
                })
            })
    } else if let Some(Component::OpusChunk(id, chunk)) = Component::opus_chunk(component) {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .any(|track| native_opus_track_id(&probe, track) == Some(id))
            .then_some(Component::OpusChunk(id, chunk))
            .filter(|_| {
                duration.is_some_and(|duration| {
                    duration.is_finite()
                        && f64::from(chunk * media_node::packaging::AUDIO_CHUNK_SECONDS) < duration
                })
            })
    } else if let Some(Component::AudioChunk(index, chunk)) = Component::audio_chunk(component) {
        let duration = probe
            .format
            .get("duration")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<f64>().ok());
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("audio")
                    && t.codec_name.as_deref() == Some("opus")
            })
            .filter(|_| {
                duration.is_some_and(|d| {
                    d.is_finite()
                        && f64::from(chunk * media_node::packaging::AUDIO_CHUNK_SECONDS) < d
                })
            })
            .map(|_| Component::AudioChunk(index, chunk))
    } else if component == "video"
        && probe.streams.iter().any(|t| {
            t.codec_type.as_deref() == Some("video") && t.codec_name.as_deref() == Some("av1")
        })
    {
        Some(Component::Video)
    } else if let Some(index) = component
        .strip_prefix("audio-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("audio")
                    && t.codec_name.as_deref() == Some("opus")
            })
            .map(|_| Component::Audio(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-ass-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("subtitle")
                    && matches!(t.codec_name.as_deref(), Some("ass" | "ssa"))
            })
            .map(|_| Component::AssSubtitle(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-pgs-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("subtitle")
                    && t.codec_name.as_deref() == Some("hdmv_pgs_subtitle")
            })
            .map(|_| Component::PgsSubtitle(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        probe
            .streams
            .iter()
            .find(|t| {
                t.index == index
                    && t.codec_type.as_deref() == Some("subtitle")
                    && matches!(
                        t.codec_name.as_deref(),
                        Some("subrip" | "webvtt" | "ass" | "ssa" | "mov_text")
                    )
            })
            .map(|_| Component::Subtitle(index))
    } else {
        None
    };
    let Some(selected) = selected else {
        return crate::error(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_track");
    };
    if remote_id != "local" {
        return match crate::nodes::load_node(store, &remote_id).await {
            Ok(Some(node)) => {
                let mut response = crate::nodes::relay_from_remote_node(
                    store,
                    &node,
                    &format!("/internal/v1/node/assets/{asset_id}/prepared/{component}"),
                    request,
                )
                .await;
                if !response.status().is_success()
                    && !response.headers().contains_key("x-media-preparation-error")
                {
                    let code = if response.status() == StatusCode::SERVICE_UNAVAILABLE {
                        "node_unavailable"
                    } else if response.status() == StatusCode::GATEWAY_TIMEOUT {
                        "preparation_timeout"
                    } else {
                        "preparation_failed"
                    };
                    response.headers_mut().insert(
                        "x-media-preparation-error",
                        axum::http::HeaderValue::from_static(code),
                    );
                }
                eprintln!(
                    "{}",
                    json!({"event":"prepared_remote_response","asset":asset_id,"node":remote_id,"component":component,"status":response.status().as_u16(),"elapsed_ms":request_started.elapsed().as_millis()})
                );
                response
            }
            _ => {
                let mut response =
                    crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static("node_unavailable"),
                );
                response
            }
        };
    }
    if let Component::Audio(index) | Component::AudioChunk(index, _) = selected
        && let Some(track) = probe.streams.iter().find(|track| track.index == index)
    {
        eprintln!(
            "{}",
            json!({"event":"prepared_audio_track","asset":asset_id,"index":index,"language":track.tags.get("language"),"channels":track.details.get("channels"),"codec":track.codec_name})
        );
    }
    if matches!(selected, Component::OpusChunk(..))
        || stream_position.is_some()
        || transcode.is_some()
        || hls.is_some()
    {
        let opened = match media_node::OpenedFile::open_in_root(
            Path::new(&row.get::<String, _>("root")),
            Path::new(&row.get::<String, _>("relative_path")),
        )
        .await
        {
            Ok(opened)
                if opened.size() == row.get::<i64, _>("size") as u64
                    && opened.modified_ns() == row.get::<String, _>("modified_ns") =>
            {
                opened
            }
            _ => {
                let mut response = crate::error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "asset_changed_rescan_required",
                );
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static("asset_changed_rescan_required"),
                );
                return response;
            }
        };
        let streamed = if let Component::OpusChunk(id, chunk) = selected {
            preparations
                .native_audio()
                .serve(opened, id, chunk, &request)
                .await
        } else if let Some(transcode) = transcode {
            preparations
                .transcodes()
                .serve(opened, transcode, &request)
                .await
        } else if let Some(hls) = hls {
            // The Node's keyframe index cache follows this exact version of the file.
            let source = format!(
                "{asset_id}:{}:{}",
                row.get::<i64, _>("size"),
                row.get::<String, _>("modified_ns")
            );
            preparations
                .serve_hls(opened, source, hls, request.method().clone())
                .await
        } else {
            preparations
                .video_streams()
                .serve(
                    opened,
                    stream_position.map_or(0, |(index, _)| index),
                    stream_position.map_or(0, |(_, position)| position),
                    &request,
                )
                .await
        };
        return match streamed {
            Ok(response) => {
                let (parts, body) = response.into_parts();
                Response::from_parts(parts, Body::new(body))
            }
            Err(error) => {
                let code = media_node::packaging::preparation_error_code(&error);
                let status = if matches!(
                    error.kind(),
                    std::io::ErrorKind::Unsupported
                        | std::io::ErrorKind::InvalidData
                        | std::io::ErrorKind::InvalidInput
                ) {
                    StatusCode::UNPROCESSABLE_ENTITY
                } else {
                    StatusCode::SERVICE_UNAVAILABLE
                };
                let mut response = crate::error(status, code);
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static(code),
                );
                response
            }
        };
    }
    let opened = preparations
        .prepare(
            std::path::Path::new(&row.get::<String, _>("root")),
            std::path::Path::new(&row.get::<String, _>("relative_path")),
            row.get::<i64, _>("size") as u64,
            &row.get::<String, _>("modified_ns"),
            selected,
        )
        .await;
    let opened = match opened {
        Ok(file) => file,
        Err(error) => {
            let code = media_node::packaging::preparation_error_code(&error);
            eprintln!(
                "{}",
                json!({"event":"prepared_http_error","asset":asset_id,"component":component,"code":code,"elapsed_ms":request_started.elapsed().as_millis()})
            );
            let mut response = crate::error(StatusCode::SERVICE_UNAVAILABLE, code);
            response.headers_mut().insert(
                "x-media-preparation-error",
                axum::http::HeaderValue::from_static(code),
            );
            return response;
        }
    };
    eprintln!(
        "{}",
        json!({"event":"prepared_http_ready","asset":asset_id,"component":component,"elapsed_ms":request_started.elapsed().as_millis(),"bytes":opened.size()})
    );
    match media_node::serve_opened(opened, &request, selected.mime()).await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "prepared_stream_failed"),
    }
}

pub async fn audio(
    store: &IdentityStore,
    asset_id: &str,
    key: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    let row = query(
        "SELECT au.audios_json,l.root,l.node_id \
            FROM inventory_audio au \
            JOIN inventory_assets a ON a.id=au.asset_id \
            JOIN inventory_sources s ON s.id=a.source_id \
            JOIN libraries l ON l.id=a.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root \
            AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "audio_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let tracks: Vec<media_contracts::inventory::ExternalAudio> =
        match serde_json::from_str(&row.get::<String, _>("audios_json")) {
            Ok(tracks) => tracks,
            Err(_) => {
                return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "audio_inventory_invalid");
            }
        };
    let Some(track) = tracks
        .iter()
        .find(|track| audio_operational_id(asset_id, track) == key)
    else {
        return crate::error(StatusCode::NOT_FOUND, "unknown_audio");
    };
    let node_id: String = row.get("node_id");
    if node_id != "local" {
        let Some(node) = crate::nodes::load_node(store, &node_id)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let path = format!("/internal/v1/node/assets/{key}/content");
        return crate::nodes::relay_from_remote_node(store, &node, &path, request).await;
    }
    let root: String = row.get("root");
    let opened = match crate::node::LocalNode
        .open_media(Path::new(&root), Path::new(&track.relative_path))
        .await
    {
        Ok(opened) if opened.size() == track.size && opened.modified_ns() == track.modified_ns => {
            opened
        }
        _ => return crate::error(StatusCode::NOT_FOUND, "audio_changed_rescan_required"),
    };
    match media_node::serve_opened(opened, &request, "audio/ogg").await {
        Ok(response) => {
            let (parts, body) = response.into_parts();
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "audio_stream_failed"),
    }
}

/// Session and live library permissions run in the common media middleware.
pub async fn subtitle(
    store: &IdentityStore,
    asset_id: &str,
    index: usize,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{
        body::Body,
        http::{StatusCode, header},
        response::Response,
    };
    let value: Result<Option<String>, _> = query_scalar(
        "SELECT t.subtitles_json \
            FROM inventory_subtitles t \
            JOIN inventory_assets a ON a.id=t.asset_id \
            JOIN inventory_sources s ON s.id=a.source_id \
            JOIN libraries l ON l.id=a.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE a.id=? AND a.available=1 AND l.enabled=1 AND l.root=s.root_snapshot \
            AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await;
    let tracks: Vec<media_contracts::inventory::ExternalSubtitle> = match value {
        Ok(Some(text)) => match serde_json::from_str(&text) {
            Ok(tracks) => tracks,
            Err(_) => {
                return crate::error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "subtitle_inventory_invalid",
                );
            }
        },
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "subtitle_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let Some(track) = tracks.get(index) else {
        return crate::error(StatusCode::NOT_FOUND, "unknown_subtitle");
    };
    let original = request
        .uri()
        .query()
        .is_some_and(|query| query == "format=ass");
    let content = if original {
        let Some(ass) = track.ass.as_deref() else {
            return crate::error(StatusCode::NOT_FOUND, "original_subtitle_unavailable");
        };
        ass
    } else {
        &track.webvtt
    };
    let bytes = content.as_bytes();
    let mut response = Response::new(Body::empty());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static(if original {
            "text/plain; charset=utf-8"
        } else {
            "text/vtt; charset=utf-8"
        }),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        axum::http::HeaderValue::from_static("bytes"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    let range = request.headers().get(header::RANGE);
    let selected = match range {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|s| media_node::parse_single_range(s, bytes.len() as u64))
        {
            Some(range) => {
                *response.status_mut() = StatusCode::PARTIAL_CONTENT;
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", range.start, range.end, bytes.len())
                        .parse()
                        .unwrap_or_else(|_| header::HeaderValue::from_static("bytes */*")),
                );
                &bytes[range.start as usize..=range.end as usize]
            }
            None => {
                *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes */{}", bytes.len())
                        .parse()
                        .unwrap_or_else(|_| header::HeaderValue::from_static("bytes */*")),
                );
                return response;
            }
        },
        None => bytes,
    };
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, selected.len().into());
    if request.method() != axum::http::Method::HEAD {
        *response.body_mut() = Body::from(selected.to_vec());
    }
    response
}

/// Authorization is enforced by the common route middleware before this helper.
/// Resolves an authorized `ReadGrant` from SQLite and delegates confined handle streaming to `LocalNode`
/// or relays in 64 KiB chunks from a paired remote Node.
pub async fn content(
    store: &IdentityStore,
    asset_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_contracts::{
        errors::NodeErrorCode,
        ids::{AssetId, LibraryId, NodeId},
        inventory::{ReadGrant, ReadTarget},
    };
    let row = query(
        "SELECT a.library_id,a.relative_path,a.mime,l.root,l.node_id \
         FROM inventory_assets a \
         JOIN libraries l ON l.id=a.library_id \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
    )
    .bind(asset_id)
    .fetch_optional(&store.pool)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return crate::error(StatusCode::NOT_FOUND, NodeErrorCode::UnknownAsset.as_str());
        }
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
    };
    let node_id_str: String = row.get("node_id");
    let started = std::time::Instant::now();
    let method = request.method().as_str().to_owned();
    let has_range = request.headers().contains_key("range");
    eprintln!(
        "{}",
        json!({"event":"media_http_start","asset":asset_id,"node":node_id_str,"method":method,"range":has_range})
    );
    // Headers only: never log signed URLs, query strings, cookies or disk paths.
    // elapsed_ms measures response-header readiness, not complete body delivery.
    let logged_response = |response: axum::response::Response| {
        eprintln!(
            "{}",
            json!({"event":"media_http_response","asset":asset_id,"node":node_id_str,"method":method,"range":has_range,"status":response.status().as_u16(),"content_length":response.headers().get("content-length").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()),"content_range_present":response.headers().contains_key("content-range"),"elapsed_ms":started.elapsed().as_millis()})
        );
        response
    };
    if node_id_str != "local" {
        drop(row);
        let Some(rec) = crate::nodes::load_node(store, &node_id_str)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let pq = format!("/internal/v1/node/assets/{asset_id}/content");
        return logged_response(
            crate::nodes::relay_from_remote_node(store, &rec, &pq, request).await,
        );
    }
    let grant = ReadGrant {
        target: ReadTarget::Asset(AssetId::from(asset_id)),
        library_id: LibraryId::from(row.get::<String, _>("library_id")),
        node_id: NodeId::from(node_id_str.clone()),
        root_snapshot: row.get("root"),
        relative_path: row.get("relative_path"),
        mime: row.get("mime"),
    };
    drop(row);
    logged_response(
        match crate::node::LocalNode.serve_grant(&grant, &request).await {
            Ok(res) => {
                let (parts, body) = res.into_parts();
                Response::from_parts(parts, Body::new(body))
            }
            Err(code @ NodeErrorCode::StreamError) => {
                crate::error(StatusCode::INTERNAL_SERVER_ERROR, code.as_str())
            }
            Err(code) => crate::error(StatusCode::NOT_FOUND, code.as_str()),
        },
    )
}

/// Session/library checks run before this handler, including HEAD and conditionals.
/// Resolves an authorized `ReadGrant` from SQLite and delegates confined handle validation
/// and streaming to `LocalNode` without reopening by path, or relays from a remote Node.
pub async fn image(
    store: &IdentityStore,
    image_id: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_contracts::{
        errors::NodeErrorCode,
        ids::{ImageId, LibraryId, NodeId},
        inventory::{ReadGrant, ReadTarget},
    };
    let preview_width = match media_node::image_preview_width(request.uri().query()) {
        Ok(width) => width,
        Err(_) => return crate::error(StatusCode::BAD_REQUEST, "invalid_image_width"),
    };
    let state_fp = image_state_fingerprint(store).await;
    let cached = state_fp.and_then(|fp| cached_image_meta(store.cache_scope, image_id, fp));
    let meta = if let Some(meta) = cached {
        meta
    } else {
        let row = query(
            "SELECT i.*,l.root,l.node_id \
             FROM inventory_images i \
             JOIN libraries l ON l.id=i.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE i.id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked'",
        )
        .bind(image_id)
        .fetch_optional(&store.pool)
        .await;
        let row = match row {
            Ok(Some(row)) => row,
            Ok(None) => {
                return crate::error(StatusCode::NOT_FOUND, NodeErrorCode::UnknownImage.as_str());
            }
            Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_error"),
        };
        let meta = CachedImageMeta {
            state_fp: state_fp.unwrap_or((0, 0, 0, 0)),
            node_id: row.get("node_id"),
            library_id: row.get("library_id"),
            root_snapshot: row.get("root"),
            relative_path: row.get("relative_path"),
            mime: row.get("mime"),
            size: row.get::<i64, _>("size") as u64,
            modified_ns: row.get("modified_ns"),
        };
        drop(row);
        if state_fp.is_some() {
            store_image_meta(store.cache_scope, image_id, meta.clone());
        }
        meta
    };
    // Catalogue URLs carry `?v=<modified>-<size>`: a changed image gets a new address, so a
    // versioned one can stay in the browser instead of being revalidated on every visit
    // (dozens of conditional requests slowed the posters of the home page down).
    let query_bytes = request.uri().query().unwrap_or("").as_bytes();
    let version_token = url::form_urlencoded::parse(query_bytes)
        .find(|(name, value)| name == "v" && !value.is_empty())
        .map(|(_, value)| value.into_owned());
    let versioned = version_token.is_some();
    let expected_etag = expected_image_etag(&meta.modified_ns, meta.size, preview_width);
    if matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && !request.headers().contains_key(axum::http::header::RANGE)
        && if_none_match_matches(request.headers(), &expected_etag)
    {
        let cache_control = if versioned {
            "private, max-age=31536000, immutable"
        } else {
            "private, no-cache"
        };
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(etag_val) = axum::http::HeaderValue::from_str(&expected_etag) {
            response
                .headers_mut()
                .insert(axum::http::header::ETAG, etag_val);
        }
        response.headers_mut().insert(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        );
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static(cache_control),
        );
        return response;
    }
    if meta.node_id != "local" {
        let Some(rec) = crate::nodes::load_node(store, &meta.node_id)
            .await
            .ok()
            .flatten()
        else {
            return crate::error(StatusCode::SERVICE_UNAVAILABLE, "node_unavailable");
        };
        let mut pq = format!("/internal/v1/node/assets/{image_id}/image");
        if let Some(query) = request.uri().query() {
            pq.push('?');
            pq.push_str(query);
        }
        return crate::nodes::relay_from_remote_node(store, &rec, &pq, request).await;
    }
    let mut grant = ReadGrant {
        target: ReadTarget::Image {
            id: ImageId::from(image_id),
            expected_size: meta.size,
            expected_modified_ns: meta.modified_ns,
        },
        library_id: LibraryId::from(meta.library_id),
        node_id: NodeId::from(meta.node_id),
        root_snapshot: meta.root_snapshot,
        relative_path: meta.relative_path,
        mime: meta.mime,
    };
    match crate::node::LocalNode.serve_grant(&grant, &request).await {
        Ok(res) => {
            let (mut parts, body) = res.into_parts();
            if versioned && matches!(parts.status, StatusCode::OK | StatusCode::NOT_MODIFIED) {
                parts.headers.insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("private, max-age=31536000, immutable"),
                );
            }
            Response::from_parts(parts, Body::new(body))
        }
        Err(NodeErrorCode::ImageChangedRescanRequired) => {
            // Sidecars may be replaced by Jellyfin between scans. Refresh only
            // this already-authorized image, with Node confinement/decoding limits.
            let refreshed = async move {
                let opened = media_node::OpenedFile::open_in_root(
                    std::path::Path::new(&grant.root_snapshot),
                    std::path::Path::new(&grant.relative_path),
                ).await.ok()?;
                let image = media_node::images::validate_opened(&opened).await.ok()?;
                query("UPDATE inventory_images SET size=?,modified_ns=?,mime=? WHERE id=? AND root_snapshot=? AND relative_path=? AND available=1")
                    .bind(image.size_bytes as i64).bind(&image.modified_ns).bind(image.mime)
                    .bind(image_id).bind(&grant.root_snapshot).bind(&grant.relative_path)
                    .execute(&store.pool).await.ok()?;
                invalidate_image_meta(store.cache_scope, image_id);
                super::invalidate_catalogue_cache();
                grant.target = ReadTarget::Image {
                    id: ImageId::from(image_id), expected_size: image.size_bytes,
                    expected_modified_ns: image.modified_ns.clone(),
                };
                grant.mime = image.mime.to_owned();
                let res = crate::node::LocalNode.serve_grant(&grant, &request).await.ok()?;
                Some((res, image.modified_ns, image.size_bytes))
            }.await;
            match refreshed {
                Some((res, modified_ns, size_bytes)) => {
                    let (mut parts, body) = res.into_parts();
                    if matches!(parts.status, StatusCode::OK | StatusCode::NOT_MODIFIED)
                        && let Some(cache_control) = refreshed_image_cache_control(
                            version_token.as_deref(),
                            &modified_ns,
                            size_bytes,
                        )
                    {
                        parts.headers.insert(
                            axum::http::header::CACHE_CONTROL,
                            axum::http::HeaderValue::from_static(cache_control),
                        );
                    }
                    Response::from_parts(parts, Body::new(body))
                }
                None => crate::error(
                    StatusCode::NOT_FOUND,
                    NodeErrorCode::ImageChangedRescanRequired.as_str(),
                ),
            }
        }
        Err(code @ NodeErrorCode::ImageStreamError) => {
            crate::error(StatusCode::INTERNAL_SERVER_ERROR, code.as_str())
        }
        Err(code) => crate::error(StatusCode::NOT_FOUND, code.as_str()),
    }
}

/// Every index in a transcode request must name a stream of the expected kind in this
/// asset's probe, and each option must make sense for that stream. The Node trusts
/// these checks: it never sees the probe.
pub(crate) fn transcode_allowed(
    probe: &media_contracts::media::Probe,
    request: &media_node::transcode::TranscodeRequest,
) -> bool {
    use media_node::transcode::{FLAG_COPY_AUDIO, FLAG_SURROUND, FLAG_TONEMAP};
    let stream = |index: u32, kind: &str| {
        probe
            .streams
            .iter()
            .find(|track| track.index == index && track.codec_type.as_deref() == Some(kind))
    };
    let duration = probe
        .format
        .get("duration")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);
    let Some(video) = stream(request.video, "video") else {
        return false;
    };
    // An attached cover is a "video" stream too; it is not a picture to transcode.
    if video
        .details
        .get("disposition")
        .and_then(|d| d.get("attached_pic"))
        .and_then(Value::as_i64)
        == Some(1)
    {
        return false;
    }
    if request.copies_video()
        && !matches!(
            video.codec_name.as_deref(),
            Some("h264" | "hevc" | "av1" | "vp9")
        )
    {
        return false;
    }
    if request.has(FLAG_TONEMAP) && video_dynamic_range(video).is_none() {
        return false;
    }
    if let Some(audio) = request.audio {
        let Some(track) = stream(audio, "audio") else {
            return false;
        };
        let channels = track.details.get("channels").and_then(Value::as_u64);
        if request.has(FLAG_SURROUND) && channels.is_none_or(|channels| channels < 6) {
            return false;
        }
        if request.has(FLAG_COPY_AUDIO)
            && !matches!(
                track.codec_name.as_deref(),
                Some("aac" | "ac3" | "eac3" | "opus" | "mp3" | "flac")
            )
        {
            return false;
        }
    }
    if let Some(subtitle) = request.subtitle
        && !stream(subtitle, "subtitle").is_some_and(|track| {
            matches!(
                track.codec_name.as_deref(),
                Some("hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle")
            )
        })
    {
        return false;
    }
    duration.is_some_and(|duration| f64::from(request.position_ms) / 1000.0 < duration)
}

/// `hdr10`, `hlg` or `dolby_vision_{profile}` read from the probe, `None` for SDR.
/// HDR10+ metadata lives in frames, not in the stream header: it reads as `hdr10`.
pub(crate) fn video_dynamic_range(video: &media_contracts::media::Track) -> Option<String> {
    let dovi = video
        .details
        .get("side_data_list")
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter().find(|item| {
                item.get("side_data_type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.contains("DOVI"))
            })
        });
    if let Some(dovi) = dovi {
        let profile = dovi.get("dv_profile").and_then(Value::as_u64).unwrap_or(0);
        return Some(format!("dolby_vision_{profile}"));
    }
    match video.details.get("color_transfer").and_then(Value::as_str) {
        Some("smpte2084") => Some("hdr10".to_owned()),
        Some("arib-std-b67") => Some("hlg".to_owned()),
        _ => None,
    }
}

/// Why the server settings refuse this encode, or `None`. `encodes` is (audio, video).
fn encode_refusal(
    settings: &crate::server_settings::Settings,
    encodes: (bool, bool),
    transcode: Option<&media_node::transcode::TranscodeRequest>,
    hls: Option<&media_node::hls::HlsRequest>,
) -> Option<&'static str> {
    if (encodes.0 && !settings.audio_transcoding_enabled)
        || (encodes.1 && !settings.video_transcoding_enabled)
    {
        return Some("transcode_disabled");
    }
    if let Some(request) = transcode.filter(|request| !request.copies_video()) {
        if request.has(media_node::transcode::FLAG_TONEMAP) && !settings.tone_mapping_enabled {
            return Some("tonemap_disabled");
        }
        if request.subtitle.is_some() && !settings.subtitle_burn_in_enabled {
            return Some("burn_in_disabled");
        }
        if settings.max_transcode_kbps != 0 && request.kbps > settings.max_transcode_kbps {
            return Some("transcode_bitrate_limited");
        }
    }
    if let Some(encode) = hls.and_then(|request| request.encode) {
        let kbps =
            media_node::hls::renditions::rendition(encode.height).map_or(u32::MAX, |r| r.kbps);
        if !super::hls::Encodes::from_settings(settings).allows(kbps, encode.tonemap) {
            return Some(if encode.tonemap && !settings.tone_mapping_enabled {
                "tonemap_disabled"
            } else {
                "transcode_bitrate_limited"
            });
        }
    }
    None
}

#[cfg(test)]
mod transcode_tests {
    use super::*;
    use media_node::transcode::request;

    fn probe() -> media_contracts::media::Probe {
        serde_json::from_value(json!({
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "hevc", "color_transfer": "smpte2084"},
                {"index": 1, "codec_type": "audio", "codec_name": "dts", "channels": 6},
                {"index": 2, "codec_type": "audio", "codec_name": "eac3", "channels": 2},
                {"index": 3, "codec_type": "subtitle", "codec_name": "hdmv_pgs_subtitle"},
                {"index": 4, "codec_type": "subtitle", "codec_name": "subrip"},
                {"index": 5, "codec_type": "video", "codec_name": "mjpeg", "disposition": {"attached_pic": 1}}
            ],
            "format": {"duration": "600.000"}
        }))
        .unwrap()
    }

    #[test]
    fn transcode_requests_must_match_the_probe() {
        let probe = probe();
        let allowed = |raw: &str| transcode_allowed(&probe, &request(raw).unwrap());
        assert!(allowed("transcode-v0a1s3h1080b8000f9-0"));
        assert!(allowed("transcode-v0a2sxh0b8000f0-599000"));
        assert!(allowed("transcode-v0a2sxh720b4000f16-0"));
        // Wrong kinds, text subtitles, covers, positions beyond the end.
        assert!(!allowed("transcode-v1a1sxh720b4000f0-0"));
        assert!(!allowed("transcode-v0a0sxh720b4000f0-0"));
        assert!(!allowed("transcode-v0a1s4h720b4000f0-0"));
        assert!(!allowed("transcode-v5a1sxh720b4000f0-0"));
        assert!(!allowed("transcode-v0a1sxh720b4000f0-600000"));
        // 5.1 from a stereo track, DTS copied into MP4.
        assert!(!allowed("transcode-v0a2sxh720b4000f8-0"));
        assert!(!allowed("transcode-v0a1sxh720b4000f16-0"));
    }

    #[test]
    fn dynamic_range_reads_transfer_and_dolby_vision_side_data() {
        let probe = probe();
        assert_eq!(
            video_dynamic_range(&probe.streams[0]).as_deref(),
            Some("hdr10")
        );
        let dovi: media_contracts::media::Track = serde_json::from_value(json!({
            "index": 0, "codec_type": "video", "codec_name": "hevc",
            "side_data_list": [{"side_data_type": "DOVI configuration record", "dv_profile": 8}]
        }))
        .unwrap();
        assert_eq!(
            video_dynamic_range(&dovi).as_deref(),
            Some("dolby_vision_8")
        );
        assert_eq!(video_dynamic_range(&probe.streams[5]), None);
    }

    #[test]
    fn image_etag_and_if_none_match_match_node_contract() {
        let preview_etag = expected_image_etag("12345", 256, Some(320));
        assert_eq!(preview_etag, "\"preview-v1-12345-100-320\"");
        let original_etag = expected_image_etag("12345", 256, None);
        assert_eq!(original_etag, "\"12345-100\"");

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::IF_NONE_MATCH,
            "W/\"preview-v1-12345-100-320\"".parse().unwrap(),
        );
        assert!(if_none_match_matches(&headers, &preview_etag));
        assert!(!if_none_match_matches(&headers, &original_etag));
    }

    #[test]
    fn image_meta_cache_invalidates_on_fingerprint_or_epoch_change() {
        let scope = 8_800_001u64;
        let fp1 = (10, 1, 1, 1);
        let fp2 = (11, 1, 1, 1);
        store_image_meta(
            scope,
            "img-1",
            CachedImageMeta {
                state_fp: fp1,
                node_id: "local".into(),
                library_id: "lib-1".into(),
                root_snapshot: "/media".into(),
                relative_path: "poster.jpg".into(),
                mime: "image/jpeg".into(),
                size: 4096,
                modified_ns: "12345".into(),
            },
        );
        assert!(cached_image_meta(scope, "img-1", fp1).is_some());
        assert!(cached_image_meta(scope, "img-1", fp2).is_none());
        invalidate_image_meta(scope, "img-1");
        assert!(cached_image_meta(scope, "img-1", fp1).is_none());
    }

    #[test]
    fn refreshed_image_cache_control_guards_stale_version_tokens() {
        assert_eq!(refreshed_image_cache_control(None, "200", 60), None);
        assert_eq!(
            refreshed_image_cache_control(Some("100-50"), "200", 60),
            Some("private, no-cache")
        );
        assert_eq!(
            refreshed_image_cache_control(Some("200-60"), "200", 60),
            Some("private, max-age=31536000, immutable")
        );
    }
}
