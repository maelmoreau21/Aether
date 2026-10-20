//! Core-side adapter to the Media Node (`media-node`) crate.
pub use media_node::{
    DEMO_FIXTURES, Descriptor, FIXTURES, Inventory, LanguageRemux, LocalNode, OpenedFile,
    REMUX_FIXTURES, REPRESENTATIVE_FIXTURES, RepresentativeCase, images, nfo, scanner,
    validate_library_root,
};

use crate::error;
use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::StatusCode,
    response::Response,
};
use std::{io, path::Path as FsPath, sync::Arc};

fn into_axum_response(res: axum::http::Response<media_node::BoxStreamBody>) -> Response {
    let (parts, body) = res.into_parts();
    Response::from_parts(parts, Body::new(body))
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
    let Ok(opened) = s.open_relative(FsPath::new(&relative)).await else {
        return error(StatusCode::NOT_FOUND, "experiment_asset_unavailable");
    };
    match media_node::serve_opened(opened, &req, mime).await {
        Ok(res) => into_axum_response(res),
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
    let relative = FsPath::new("mse").join("verified").join(&name);
    let Ok(opened) = s.open_relative(&relative).await else {
        return error(StatusCode::NOT_FOUND, "experiment_asset_unavailable");
    };
    match media_node::serve_opened(opened, &req, mime).await {
        Ok(res) => into_axum_response(res),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "experiment_read_failed"),
    }
}

pub async fn content(
    State(s): State<Arc<Inventory>>,
    Path(id): Path<String>,
    store: Option<axum::Extension<crate::identity::IdentityStore>>,
    req: Request,
) -> Response {
    if !id.starts_with("fixture-") && !id.starts_with("demo-") {
        return match store {
            Some(axum::Extension(store)) => crate::inventory::content(&store, &id, req).await,
            None => error(StatusCode::NOT_FOUND, "unknown_asset"),
        };
    }
    let (opened, mime) = match s.open_fixture_asset(&id).await {
        Ok(Some(found)) => found,
        Ok(None) => return error(StatusCode::NOT_FOUND, "unknown_asset"),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return error(StatusCode::FORBIDDEN, "asset_outside_fixture_root");
        }
        Err(_) => return error(StatusCode::NOT_FOUND, "asset_unavailable"),
    };
    match media_node::serve_opened(opened, &req, mime).await {
        Ok(res) => into_axum_response(res),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "stream_error"),
    }
}

// Fixed WebVTT exports reused from P0; no MSE player dependency.
pub async fn demo_subtitle(
    state: State<Arc<Inventory>>,
    path: Path<String>,
    req: Request,
) -> Response {
    if !matches!(
        path.0.as_str(),
        "fr-full.vtt" | "fr-forced.vtt" | "en-full.vtt"
    ) {
        return error(StatusCode::NOT_FOUND, "unknown_subtitle");
    }
    mse_content(state, path, req).await
}
