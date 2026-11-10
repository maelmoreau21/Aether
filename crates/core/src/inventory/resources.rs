//! Local/remote media delivery after common route authorization.

use super::decode;
use super::playback::{audio_operational_id, native_opus_track_id, playback_library_options};
use crate::database::{query, query_scalar};
use crate::identity::IdentityStore;
use serde_json::{Value, json};
use sqlx::Row;
use std::path::Path;

pub async fn prepared(
    store: &IdentityStore,
    preparations: &std::sync::Arc<media_node::packaging::Preparations>,
    asset_id: &str,
    component: &str,
    request: axum::extract::Request,
) -> axum::response::Response {
    use axum::{body::Body, http::StatusCode, response::Response};
    use media_node::packaging::Component;
    let row = query("SELECT a.relative_path,a.size,a.modified_ns,a.probe_json,a.library_id,l.root,l.node_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
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
    if transcode.is_some() || hls.is_some_and(|request| request.convert) {
        match crate::server_settings::load(store).await {
            Ok(settings) if settings.transcoding_enabled => {}
            Ok(_) => {
                let mut response =
                    crate::error(StatusCode::UNPROCESSABLE_ENTITY, "transcode_disabled");
                response.headers_mut().insert(
                    "x-media-preparation-error",
                    axum::http::HeaderValue::from_static("transcode_disabled"),
                );
                return response;
            }
            Err(status) => return crate::error(status, "settings_unavailable"),
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
                .serve_hls(
                    opened,
                    Path::new(&row.get::<String, _>("root")),
                    Path::new(&row.get::<String, _>("relative_path")),
                    row.get::<i64, _>("size") as u64,
                    &row.get::<String, _>("modified_ns"),
                    source,
                    hls,
                    request.method().clone(),
                )
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
    let row = query("SELECT au.audios_json,l.root,l.node_id FROM inventory_audio au JOIN inventory_assets a ON a.id=au.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND s.root_snapshot=l.root AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
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
    let value: Result<Option<String>, _> = query_scalar("SELECT t.subtitles_json FROM inventory_subtitles t JOIN inventory_assets a ON a.id=t.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND a.available=1 AND l.enabled=1 AND l.root=s.root_snapshot AND n.status != 'revoked'")
        .bind(asset_id).fetch_optional(&store.pool).await;
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
                        .unwrap(),
                );
                &bytes[range.start as usize..=range.end as usize]
            }
            None => {
                *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                response.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes */{}", bytes.len()).parse().unwrap(),
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
    if media_node::image_preview_width(request.uri().query()).is_err() {
        return crate::error(StatusCode::BAD_REQUEST, "invalid_image_width");
    }
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
    let node_id_str: String = row.get("node_id");
    if node_id_str != "local" {
        drop(row);
        let Some(rec) = crate::nodes::load_node(store, &node_id_str)
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
            expected_size: row.get::<i64, _>("size") as u64,
            expected_modified_ns: row.get("modified_ns"),
        },
        library_id: LibraryId::from(row.get::<String, _>("library_id")),
        node_id: NodeId::from(node_id_str),
        root_snapshot: row.get("root"),
        relative_path: row.get("relative_path"),
        mime: row.get("mime"),
    };
    drop(row);
    match crate::node::LocalNode.serve_grant(&grant, &request).await {
        Ok(res) => {
            let (parts, body) = res.into_parts();
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
                grant.target = ReadTarget::Image {
                    id: ImageId::from(image_id), expected_size: image.size_bytes,
                    expected_modified_ns: image.modified_ns,
                };
                grant.mime = image.mime.to_owned();
                crate::node::LocalNode.serve_grant(&grant, &request).await.ok()
            }.await;
            match refreshed {
                Some(res) => {
                    let (parts, body) = res.into_parts();
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
}
