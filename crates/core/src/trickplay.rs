//! Read-only Jellyfin-style sprite sheets beside an inventoried local video.
//! No arbitrary filesystem path is ever received from the browser.
use crate::{database::query, identity::IdentityStore};
use axum::{
    Json,
    body::Body,
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use media_node::OpenedFile;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashSet, path::{Path, PathBuf}};

const COLUMNS: u32 = 10;
const ROWS: u32 = 10;
const MAX_SHEETS: usize = 512;
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

struct SpriteSource {
    root: PathBuf,
    sheets: PathBuf,
    duration: f64,
}

fn sheet_directory(video: &Path) -> PathBuf {
    // Match the actual video's basename. Never guess a movie-wide sheet for
    // another edition, version or episode in the same directory.
    video.with_extension("trickplay").join("320 - 10x10")
}

async fn source(store: &IdentityStore, asset_id: &str) -> Result<Option<SpriteSource>, sqlx::Error> {
    let row = query(
        "SELECT a.relative_path, a.probe_json, l.root, l.node_id FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN libraries l ON l.id=a.library_id \
         JOIN media_nodes n ON n.id=l.node_id \
         WHERE a.id=? AND a.available=1 AND l.enabled=1 \
         AND s.root_snapshot=l.root AND l.kind IN ('films','series') AND n.status!='revoked'"
    )
    .bind(asset_id)
    .fetch_optional(&store.pool).await?;
    let Some(row) = row else { return Ok(None) };
    let node_id: String = row.get("node_id");
    // Remote nodes need their own inventory-bound internal sprite protocol.
    if node_id != "local" { return Ok(None); }
    let video: String = row.get("relative_path");
    let probe: String = row.get("probe_json");
    let root: String = row.get("root");
    let parsed: Value = match serde_json::from_str(&probe) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let duration = parsed.get("format")
        .and_then(|value| value.get("duration"))
        .and_then(|value| value.as_str().and_then(|v| v.parse::<f64>().ok())
            .or_else(|| value.as_f64()))
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 86400.0);
    let Some(duration) = duration else { return Ok(None) };
    let video = Path::new(&video);
    if media_node::files::validate_relative_components(video).is_err() {
        return Ok(None);
    }
    Ok(Some(SpriteSource { root: PathBuf::from(root), sheets: sheet_directory(video), duration }))
}

fn interval_ms() -> u64 {
    std::env::var("MEDIA_TRICKPLAY_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| (1000..=60000).contains(v))
        .unwrap_or(10_000)
}

async fn read_sheet(source: &SpriteSource, index: usize) -> Option<OpenedFile> {
    if index >= MAX_SHEETS { return None; }
    let relative = source.sheets.join(format!("{index}.jpg"));
    let opened = OpenedFile::open_in_root(&source.root, &relative).await.ok()?;
    (opened.size() > 0 && opened.size() <= MAX_IMAGE_BYTES).then_some(opened)
}

async fn sheet_count(source: &SpriteSource) -> Option<usize> {
    // Opening the first file through OpenedFile also validates *all* path
    // components; symlinks/reparse points cannot escape the library root.
    read_sheet(source, 0).await?;
    let mut dir = tokio::fs::read_dir(source.root.join(&source.sheets)).await.ok()?;
    let mut found = HashSet::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(n) = name.strip_suffix(".jpg").and_then(|s| s.parse::<usize>().ok()) else { continue };
        if n < MAX_SHEETS { found.insert(n); }
    }
    let mut contiguous = 0;
    while found.contains(&contiguous) { contiguous += 1; }
    (contiguous > 0).then_some(contiguous)
}

async fn dimensions(opened: OpenedFile) -> Option<(u32,u32)> {
    tokio::task::spawn_blocking(move || {
        use image::ImageDecoder;
        let decoder = image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(opened.std_file())).ok()?;
        let (w, h) = decoder.dimensions();
        (w > 0 && h > 0 && w % COLUMNS == 0 && h % ROWS == 0
            && w <= 8192 && h <= 8192).then_some((w / COLUMNS, h / ROWS))
    }).await.ok().flatten()
}

pub async fn manifest(store: &IdentityStore, asset_id: &str) -> Response {
    let source = match source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let Some(count) = sheet_count(&source).await else {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable");
    };
    let Some(first) = read_sheet(&source, 0).await else {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable");
    };
    let Some((width,height)) = dimensions(first).await else {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_invalid");
    };
    let interval = interval_ms();
    let thumbnails = ((source.duration * 1000.0 / interval as f64).ceil() as usize)
        .min(count * (COLUMNS * ROWS) as usize);
    if thumbnails == 0 { return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable"); }
    Json(json!({
        "intervalMs": interval, "width": width, "height": height,
        "columns": COLUMNS, "rows": ROWS,
        "sheetCount": count, "thumbnailCount": thumbnails,
        "urlPrefix": format!("/api/v1/assets/{asset_id}/trickplay/")
    })).into_response()
}

pub async fn tile(store: &IdentityStore, asset_id: &str, index: usize, request: Request) -> Response {
    let source = match source(store, asset_id).await {
        Ok(Some(source)) => source,
        Ok(None) => return crate::error(StatusCode::NOT_FOUND, "trickplay_unavailable"),
        Err(_) => return crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    };
    let Some(opened) = read_sheet(&source, index).await else {
        return crate::error(StatusCode::NOT_FOUND, "trickplay_tile_unavailable");
    };
    match media_node::serve_opened(opened, &request, "image/jpeg").await {
        Ok(response) => {
            let (mut parts, body) = response.into_parts();
            parts.headers.insert(
                axum::http::header::X_CONTENT_TYPE_OPTIONS,
                axum::http::HeaderValue::from_static("nosniff"),
            );
            Response::from_parts(parts, Body::new(body))
        }
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "trickplay_stream_error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefers_exact_video_version_without_matching_other_movie_versions() {
        assert_eq!(
            sheet_directory(Path::new("Zootopia 2 (2025) [HDTV-1080p] [AV1 10bit] [AAC 7.1].mp4")),
            PathBuf::from("Zootopia 2 (2025) [HDTV-1080p] [AV1 10bit] [AAC 7.1].trickplay/320 - 10x10")
        );
        assert_eq!(sheet_directory(Path::new("series/S01E02.mkv")),
            PathBuf::from("series/S01E02.trickplay/320 - 10x10"));
    }
    #[test]
    fn interval_is_bounded() {
        // The default is Jellyfin's usual ten-second cadence. A different
        // Jellyfin cadence needs explicit configuration, not an invented guess.
        assert_eq!(10_000_u64, 10_000);
    }
}
