//! Authorized HTTP media delivery, preparation, response framing, and stream lifetime.

use super::control_transport::post_signed_to_core;
use super::control_transport::verify_signed_core_call;
use super::scans::yield_background_for_playback;
use super::state::NodeIdentityState;
use super::state::NodeServerState;
use crate::BoxStreamBody;
use axum::Json;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::extract::Query;
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use media_contracts::errors::NodeErrorCode;
use media_contracts::protocol::HDR_RES_NONCE;
use media_contracts::protocol::HDR_RES_SIGNATURE;
use media_contracts::protocol::HDR_RES_TIMESTAMP;
use media_contracts::protocol::random_nonce_hex;
use media_contracts::protocol::sign_internal_response;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_playback_token;
use serde::Deserialize;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

pub struct ActiveStreamGuard(Arc<AtomicU32>);

impl ActiveStreamGuard {
    pub fn enter(counter: &Arc<AtomicU32>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter.clone())
    }
}

impl Drop for ActiveStreamGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct TrackedBody {
    inner: Body,
    _guard: ActiveStreamGuard,
}

impl http_body::Body for TrackedBody {
    type Data = bytes::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        std::pin::Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }
}

pub fn track_stream_body(response: Response, counter: &Arc<AtomicU32>) -> Response {
    if !response.status().is_success() {
        return response;
    }
    let guard = ActiveStreamGuard::enter(counter);
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::new(TrackedBody {
            inner: body,
            _guard: guard,
        }),
    )
}

pub(super) async fn prepare_asset(
    state: &NodeServerState,
    id: &str,
    component: &str,
    req: Request,
) -> Response {
    use crate::packaging::Component;
    let entry = state
        .inventory
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .assets
        .get(id)
        .cloned();
    let Some(entry) = entry else {
        return cors_error_response(StatusCode::NOT_FOUND, "asset_unknown");
    };
    if !state
        .config
        .allowed_roots
        .iter()
        .any(|root| Path::new(&entry.root_snapshot).starts_with(root))
    {
        return cors_error_response(StatusCode::FORBIDDEN, "root_denied");
    }
    let native_audio = Component::opus_chunk(component);
    let video_position = crate::video_stream::position(component);
    let transcode = crate::transcode::request(component);
    let hls = crate::hls::request(component);
    if native_audio.is_some() || video_position.is_some() || transcode.is_some() || hls.is_some() {
        let opened = match crate::OpenedFile::open_in_root(
            Path::new(&entry.root_snapshot),
            Path::new(&entry.relative_path),
        )
        .await
        {
            Ok(opened)
                if opened.size() == entry.size && opened.modified_ns() == entry.modified_ns =>
            {
                opened
            }
            Ok(_) => {
                return packet_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "source_changed_rescan_required",
                );
            }
            Err(error) => return packet_stream_error(&error),
        };
        let streamed = if let Some(Component::OpusChunk(track, chunk)) = native_audio {
            state
                .preparations
                .native_audio()
                .serve(opened, track, chunk, &req)
                .await
        } else if let Some(transcode) = transcode {
            state
                .preparations
                .transcodes()
                .serve(opened, transcode, &req)
                .await
        } else if let Some(hls) = hls {
            // The cached keyframe index follows this exact version of the file.
            let source = format!("{id}:{}:{}", entry.size, entry.modified_ns);
            state
                .preparations
                .serve_hls(
                    opened,
                    Path::new(&entry.root_snapshot),
                    Path::new(&entry.relative_path),
                    entry.size,
                    &entry.modified_ns,
                    source,
                    hls,
                    req.method().clone(),
                )
                .await
        } else {
            state
                .preparations
                .video_streams()
                .serve(
                    opened,
                    video_position.map_or(0, |(index, _)| index),
                    video_position.map_or(0, |(_, position)| position),
                    &req,
                )
                .await
        };
        return match streamed {
            Ok(response) => convert_stream_response(response, false),
            Err(error) => packet_stream_error(&error),
        };
    }
    let selected = if let Some(chunk) = Component::audio_chunk(component) {
        Some(chunk)
    } else if component == "video" {
        Some(Component::Video)
    } else if let Some(index) = component
        .strip_prefix("audio-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        Some(Component::Audio(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-ass-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        Some(Component::AssSubtitle(index))
    } else if let Some(index) = component
        .strip_prefix("subtitle-pgs-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        Some(Component::PgsSubtitle(index))
    } else {
        component
            .strip_prefix("subtitle-")
            .and_then(|s| s.parse::<u32>().ok())
            .map(Component::Subtitle)
    };
    let Some(selected) = selected else {
        return cors_error_response(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_component");
    };
    let started = std::time::Instant::now();
    eprintln!(
        "{}",
        serde_json::json!({"event":"node_prepared_start","asset":id,"component":component,"method":req.method().as_str(),"range":req.headers().contains_key("range")})
    );
    let response = match state
        .preparations
        .prepare(
            Path::new(&entry.root_snapshot),
            Path::new(&entry.relative_path),
            entry.size,
            &entry.modified_ns,
            selected,
        )
        .await
    {
        Ok(opened) => match crate::serve_opened(opened, &req, selected.mime()).await {
            Ok(response) => convert_stream_response(response, false),
            Err(_) => {
                cors_error_response(StatusCode::INTERNAL_SERVER_ERROR, "prepared_stream_failed")
            }
        },
        Err(error) => {
            let code = crate::packaging::preparation_error_code(&error);
            let mut response = cors_error_response(StatusCode::SERVICE_UNAVAILABLE, code);
            response
                .headers_mut()
                .insert("x-media-preparation-error", HeaderValue::from_static(code));
            response
        }
    };
    eprintln!(
        "{}",
        serde_json::json!({"event":"node_prepared_response","asset":id,"component":component,"status":response.status().as_u16(),"elapsed_ms":started.elapsed().as_millis()})
    );
    response
}

pub(super) async fn internal_prepared(
    State(state): State<NodeServerState>,
    AxumPath((id, component)): AxumPath<(String, String)>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(response) => return response,
        };
    let _demand = ActiveStreamGuard::enter(&state.active_streams);
    yield_background_for_playback(&state).await;
    let response = prepare_asset(&state, &id, &component, req).await;
    track_stream_body(
        sign_stream_response(response, &identity, &nonce),
        &state.active_streams,
    )
}

pub(super) async fn internal_stream_asset(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_asset(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownAsset, false),
            &identity,
            &nonce,
        );
    };
    if id.starts_with("sidecar-") {
        let expected = {
            let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
            inv.assets
                .get(&id)
                .map(|entry| (entry.size, entry.modified_ns.clone()))
        };
        let opened = state
            .local_node
            .open_media(
                Path::new(&grant.root_snapshot),
                Path::new(&grant.relative_path),
            )
            .await;
        let response = match (opened, expected) {
            (Ok(opened), Some((size, modified)))
                if opened.size() == size && opened.modified_ns() == modified =>
            {
                match crate::serve_opened(opened, &req, "audio/ogg").await {
                    Ok(response) => convert_stream_response(response, false),
                    Err(_) => node_error_response(NodeErrorCode::StreamError, false),
                }
            }
            _ => node_error_response(NodeErrorCode::UnavailableAsset, false),
        };
        return track_stream_body(
            sign_stream_response(response, &identity, &nonce),
            &state.active_streams,
        );
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => track_stream_body(
            sign_stream_response(convert_stream_response(res, false), &identity, &nonce),
            &state.active_streams,
        ),
        Err(code) => sign_stream_response(node_error_response(code, false), &identity, &nonce),
    }
}

pub(super) async fn internal_stream_image(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_image(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownImage, false),
            &identity,
            &nonce,
        );
    };
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => sign_stream_response(convert_stream_response(res, false), &identity, &nonce),
        Err(code) => sign_stream_response(node_error_response(code, false), &identity, &nonce),
    }
}

pub(super) fn chapter_request(
    state: &NodeServerState,
    id: &str,
    query: &str,
) -> Option<crate::trickplay::TrickplayRequest> {
    let (time, duration, width, quality) = crate::chapter_images::parameters(query)?;
    let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
    let asset = inv.assets.get(id)?;
    Some(crate::trickplay::TrickplayRequest {
        asset_id: id.into(),
        media_root: PathBuf::from(&asset.root_snapshot),
        relative_path: PathBuf::from(&asset.relative_path),
        size: asset.size,
        modified_ns: asset.modified_ns.clone(),
        duration,
        interval_ms: None,
        width: Some(width),
        quality: Some(quality),
        columns: None,
        rows: None,
        chapter_time: Some(time),
    })
}

pub(super) fn trickplay_request(
    state: &NodeServerState,
    id: &str,
    query: &str,
) -> Option<crate::trickplay::TrickplayRequest> {
    let (duration, interval, width, quality) = crate::trickplay::parameters(query)?;
    let (columns, rows) = crate::trickplay::grid_parameters(query)?;
    let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
    let asset = inv.assets.get(id)?;
    Some(crate::trickplay::TrickplayRequest {
        asset_id: id.into(),
        media_root: PathBuf::from(&asset.root_snapshot),
        relative_path: PathBuf::from(&asset.relative_path),
        size: asset.size,
        modified_ns: asset.modified_ns.clone(),
        duration,
        interval_ms: Some(interval),
        width: Some(width),
        quality: Some(quality),
        columns: Some(columns),
        rows: Some(rows),
        chapter_time: None,
    })
}

pub(super) async fn internal_chapter_image(
    State(state): State<NodeServerState>,
    AxumPath((id, _index)): AxumPath<(String, usize)>,
    req: Request,
) -> Response {
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &path, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let response =
        if let Some(request) = chapter_request(&state, &id, req.uri().query().unwrap_or("")) {
            crate::chapter_images::response(&state.trickplay, request, req).await
        } else {
            node_error_response(NodeErrorCode::UnknownAsset, false)
        };
    sign_stream_response(response, &identity, &nonce)
}

pub(super) async fn internal_trickplay_manifest(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let Some(trickplay_req) = trickplay_request(&state, &id, req.uri().query().unwrap_or(""))
    else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownAsset, false),
            &identity,
            &nonce,
        );
    };

    // 1. Check Aether native cache
    if let Some(cached) = state.trickplay.check_native_cache(&trickplay_req).await {
        let res = (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ready",
                "source": "aether",
                "intervalMs": cached.interval_ms,
                "width": cached.width,
                "height": cached.height,
                "columns": cached.columns,
                "rows": cached.rows,
                "sheetCount": cached.sheet_count,
                "mime": cached.mime,
                "thumbnailCount": cached.thumbnail_count,
                "urlPrefix": format!("/api/v1/assets/{id}/trickplay/")
            })),
        )
            .into_response();
        return sign_stream_response(res, &identity, &nonce);
    }

    // 2. Check Jellyfin read-only sheets
    if let Some(jellyfin) = state.trickplay.check_jellyfin(&trickplay_req).await {
        let res = (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ready",
                "source": "jellyfin",
                "intervalMs": jellyfin.interval_ms,
                "width": jellyfin.width,
                "height": jellyfin.height,
                "columns": jellyfin.columns,
                "rows": jellyfin.rows,
                "sheetCount": jellyfin.sheet_count,
                "mime": jellyfin.mime,
                "thumbnailCount": jellyfin.thumbnail_count,
                "urlPrefix": format!("/api/v1/assets/{id}/trickplay/")
            })),
        )
            .into_response();
        return sign_stream_response(res, &identity, &nonce);
    }

    // 3. Request background generation
    let queue_status = state.trickplay.enqueue(trickplay_req);
    let (status, http) = match queue_status {
        crate::trickplay::QueueStatus::Queued => ("generating", StatusCode::ACCEPTED),
        crate::trickplay::QueueStatus::Busy => ("busy", StatusCode::SERVICE_UNAVAILABLE),
        crate::trickplay::QueueStatus::Failed => ("failed", StatusCode::SERVICE_UNAVAILABLE),
        crate::trickplay::QueueStatus::Disabled => ("disabled", StatusCode::NOT_FOUND),
    };
    let res = (http, Json(serde_json::json!({ "status": status }))).into_response();
    sign_stream_response(res, &identity, &nonce)
}

pub(super) async fn internal_trickplay_tile(
    State(state): State<NodeServerState>,
    AxumPath((id, index)): AxumPath<(String, usize)>,
    req: Request,
) -> Response {
    let pq = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (identity, nonce) =
        match verify_signed_core_call(&state, req.headers(), req.method(), &pq, b"") {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let Some(trickplay_req) = trickplay_request(&state, &id, req.uri().query().unwrap_or(""))
    else {
        return sign_stream_response(
            node_error_response(NodeErrorCode::UnknownAsset, false),
            &identity,
            &nonce,
        );
    };

    match state
        .trickplay
        .open_sheet_with_mime(&trickplay_req, index)
        .await
    {
        Ok((opened, mime)) => match crate::stream::serve_opened(opened, &req, &mime).await {
            Ok(response) => {
                let (mut parts, body) = response.into_parts();
                parts.headers.insert(
                    axum::http::header::X_CONTENT_TYPE_OPTIONS,
                    axum::http::HeaderValue::from_static("nosniff"),
                );
                parts.headers.insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=86400"),
                );
                let res = Response::from_parts(parts, Body::new(body));
                sign_stream_response(res, &identity, &nonce)
            }
            Err(_) => sign_stream_response(
                node_error_response(NodeErrorCode::StreamError, false),
                &identity,
                &nonce,
            ),
        },
        Err(_) => sign_stream_response(
            node_error_response(NodeErrorCode::UnavailableAsset, false),
            &identity,
            &nonce,
        ),
    }
}

#[derive(Deserialize)]
pub(super) struct StreamQuery {
    token: Option<String>,
}

pub(super) async fn stream_options() -> Response {
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::NO_CONTENT;
    apply_cors_headers(res.headers_mut());
    res
}

pub(super) async fn direct_stream_asset(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    req: Request,
) -> Response {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return cors_error_response(StatusCode::FORBIDDEN, "node_revoked");
    }
    let Some(secret) = identity.shared_secret.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "node_unpaired");
    };
    let Some(token) = query.token.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "missing_playback_token");
    };
    let claims = match verify_playback_token(
        secret,
        &identity.key_id,
        identity.node_id.as_str(),
        "asset",
        &id,
        "stream",
        token,
        unix_now(),
    ) {
        Ok(claims) => claims,
        Err(reason) => return cors_error_response(StatusCode::FORBIDDEN, reason),
    };

    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_asset(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return node_error_response(NodeErrorCode::UnknownAsset, true);
    };
    if claims.library_id != grant.library_id
        || post_signed_to_core(&state, "/internal/v1/core/playback/authorize", &claims)
            .await
            .is_err()
    {
        return cors_error_response(StatusCode::FORBIDDEN, "playback_authorization_denied");
    }
    let started = std::time::Instant::now();
    eprintln!(
        "{}",
        serde_json::json!({"event":"node_media_start","asset":id,"method":req.method().as_str(),"range":req.headers().contains_key("range")})
    );
    let response = match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => track_stream_body(convert_stream_response(res, true), &state.active_streams),
        Err(code) => node_error_response(code, true),
    };
    eprintln!(
        "{}",
        serde_json::json!({"event":"node_media_response","asset":id,"status":response.status().as_u16(),"content_length":response.headers().get("content-length").and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok()),"content_range_present":response.headers().contains_key("content-range"),"elapsed_ms":started.elapsed().as_millis()})
    );
    response
}

pub(super) async fn direct_stream_image(
    State(state): State<NodeServerState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<StreamQuery>,
    req: Request,
) -> Response {
    let identity = state
        .identity
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if identity.revoked {
        return cors_error_response(StatusCode::FORBIDDEN, "node_revoked");
    }
    let Some(secret) = identity.shared_secret.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "node_unpaired");
    };
    let Some(token) = query.token.as_deref() else {
        return cors_error_response(StatusCode::UNAUTHORIZED, "missing_playback_token");
    };
    let claims = match verify_playback_token(
        secret,
        &identity.key_id,
        identity.node_id.as_str(),
        "image",
        &id,
        "stream",
        token,
        unix_now(),
    ) {
        Ok(claims) => claims,
        Err(reason) => return cors_error_response(StatusCode::FORBIDDEN, reason),
    };

    let grant = {
        let inv = state.inventory.read().unwrap_or_else(|e| e.into_inner());
        inv.grant_for_image(&identity.node_id, &id)
    };
    let Some(grant) = grant else {
        return node_error_response(NodeErrorCode::UnknownImage, true);
    };
    if claims.library_id != grant.library_id
        || post_signed_to_core(&state, "/internal/v1/core/playback/authorize", &claims)
            .await
            .is_err()
    {
        return cors_error_response(StatusCode::FORBIDDEN, "playback_authorization_denied");
    }
    match state
        .local_node
        .serve_grant_for_node(identity.node_id.clone(), &grant, &req)
        .await
    {
        Ok(res) => convert_stream_response(res, true),
        Err(code) => node_error_response(code, true),
    }
}

fn convert_stream_response(res: http::Response<BoxStreamBody>, with_cors: bool) -> Response {
    let (mut parts, body) = res.into_parts();
    if with_cors {
        apply_cors_headers(&mut parts.headers);
    }
    Response::from_parts(parts, Body::new(body))
}

pub(super) fn node_error_response(code: NodeErrorCode, with_cors: bool) -> Response {
    let status = match code {
        NodeErrorCode::UnknownAsset | NodeErrorCode::UnknownImage => StatusCode::NOT_FOUND,
        NodeErrorCode::UnavailableAsset | NodeErrorCode::UnavailableImage => StatusCode::NOT_FOUND,
        NodeErrorCode::InvalidImage | NodeErrorCode::ImageChangedRescanRequired => {
            StatusCode::CONFLICT
        }
        NodeErrorCode::StreamError | NodeErrorCode::ImageStreamError => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        _ => StatusCode::BAD_REQUEST,
    };
    let mut res = (status, Json(serde_json::json!({ "error": code.as_str() }))).into_response();
    if with_cors {
        apply_cors_headers(res.headers_mut());
    }
    res
}

fn cors_error_response(status: StatusCode, reason: &'static str) -> Response {
    let mut res = (status, Json(serde_json::json!({ "error": reason }))).into_response();
    apply_cors_headers(res.headers_mut());
    res
}

fn packet_error_response(status: StatusCode, code: &'static str) -> Response {
    let mut response = cors_error_response(status, code);
    response
        .headers_mut()
        .insert("x-media-preparation-error", HeaderValue::from_static(code));
    response
}

pub(super) fn packet_stream_error(error: &io::Error) -> Response {
    let status = match error.kind() {
        io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    packet_error_response(status, crate::packaging::preparation_error_code(error))
}

// Hyper can infer Content-Length from an exact body hint after the handler
// returns. Include that same length before signing the representation metadata.
pub(super) fn normalize_stream_content_length(response: &mut Response) {
    if response.status().is_informational() || response.status() == StatusCode::NO_CONTENT {
        response.headers_mut().remove(header::CONTENT_LENGTH);
        return;
    }
    if response.status() == StatusCode::NOT_MODIFIED
        || response.headers().contains_key(header::CONTENT_LENGTH)
    {
        return;
    }
    if let Some(length) = http_body::Body::size_hint(response.body()).exact()
        && let Ok(value) = HeaderValue::from_str(&length.to_string())
    {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
}

fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Range, Accept, Content-Type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(
            "Accept-Ranges, Content-Length, Content-Range, Content-Type, x-media-preparation-error",
        ),
    );
    headers.insert(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("cross-origin"),
    );
}

fn sign_stream_response(
    mut response: Response,
    identity: &NodeIdentityState,
    request_nonce: &str,
) -> Response {
    normalize_stream_content_length(&mut response);
    let now = unix_now();
    let nonce = random_nonce_hex();
    let digest = crate::transport::stream_metadata_digest(response.headers());
    let signature = sign_internal_response(
        identity.shared_secret.as_deref().unwrap_or(""),
        request_nonce,
        response.status().as_u16(),
        identity.node_id.as_str(),
        &identity.key_id,
        now,
        &nonce,
        &digest,
    );
    for (key, value) in [
        (HDR_RES_TIMESTAMP, now.to_string()),
        (HDR_RES_NONCE, nonce),
        (HDR_RES_SIGNATURE, signature),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            response.headers_mut().insert(key, value);
        }
    }
    response
}
