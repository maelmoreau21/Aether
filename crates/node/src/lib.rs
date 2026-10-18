//! Media Node (`media-node`): read-only local disk confinement, directory traversal,
//! bounded `ffprobe` analysis, NFO XML parsing, image decoding/validation, and
//! 64 KiB chunked HTTP `GET`/`HEAD`/`Range` streaming from confined file handles.
//!
//! This crate has zero dependency on `media-poc` (Core), `sqlx`, or Core users/sessions/permissions.

pub mod files;
pub mod images;
pub mod inventory;
pub mod nfo;
pub mod probe;
pub mod scanner;
pub mod server;
pub mod stream;
pub mod transport;

pub use files::{OpenedFile, confined_file, validate_library_root};
pub use inventory::{
    DEMO_FIXTURES, Descriptor, FIXTURES, Inventory, LanguageRemux, REMUX_FIXTURES,
    REPRESENTATIVE_FIXTURES, RepresentativeCase,
};
pub use stream::{
    BoxStreamBody, ByteRange, STREAM_CHUNK_BYTES, parse_single_range, serve_opened,
    serve_opened_parts,
};

use http::{HeaderValue, Request, Response, header};
use media_contracts::{
    errors::NodeErrorCode,
    ids::NodeId,
    inventory::{CachedFile, InventoryBatch, ReadGrant, ReadTarget, ScanReport},
    media::{LocalImage, ValidatedImage},
};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

/// In-process local Media Node service implementing the Core ↔ Node boundary for Milestone J1/P2.
#[derive(Debug, Clone, Default)]
pub struct LocalNode;

impl LocalNode {
    /// Validate that `root` is a confined, existing directory inside `allowed_root`
    /// without parent traversal or symlink/reparse-point escapes.
    pub async fn validate_root(&self, root: &Path, allowed_root: &Path) -> io::Result<PathBuf> {
        validate_library_root(root, allowed_root).await
    }

    /// Execute a read-only library scan with optional probe and poster caches.
    pub async fn scan_library(
        &self,
        root: &Path,
        cache: &HashMap<String, CachedFile>,
        image_cache: &HashMap<String, LocalImage>,
        cancel: &AtomicBool,
    ) -> io::Result<ScanReport> {
        scanner::scan_with_images(root, cache, image_cache, cancel).await
    }

    /// Execute a bounded batch-streaming read-only library scan with optional probe and poster caches.
    #[allow(clippy::too_many_arguments)]
    pub async fn scan_library_streaming<F, Fut>(
        &self,
        root: &Path,
        kind: &str,
        cache: &HashMap<String, CachedFile>,
        image_cache: &HashMap<String, LocalImage>,
        cancel: &AtomicBool,
        batch_size: usize,
        on_batch: F,
    ) -> io::Result<ScanReport>
    where
        F: FnMut(InventoryBatch) -> Fut,
        Fut: std::future::Future<Output = Result<(), io::Error>>,
    {
        scanner::scan_streaming(root, kind, cache, image_cache, cancel, batch_size, on_batch).await
    }

    /// Open a confined media file handle relative to `root`.
    pub async fn open_media(&self, root: &Path, relative: &Path) -> io::Result<OpenedFile> {
        OpenedFile::open_in_root(root, relative).await
    }

    /// Open and validate a confined poster image relative to `root`, returning both
    /// the validated image metadata and the same `OpenedFile` handle for streaming.
    pub async fn open_validated_image(
        &self,
        root: &Path,
        relative: &Path,
    ) -> io::Result<(ValidatedImage, OpenedFile)> {
        let opened = OpenedFile::open_in_root(root, relative).await?;
        let validated = images::validate_opened(&opened).await?;
        Ok((validated, opened))
    }

    /// Execute an authorized `ReadGrant` issued by Core for the local standalone Node.
    pub fn serve_grant<B>(
        &self,
        grant: &ReadGrant,
        req: &Request<B>,
    ) -> impl std::future::Future<Output = Result<Response<BoxStreamBody>, NodeErrorCode>> + Send + 'static
    {
        self.serve_grant_for_node(NodeId::local(), grant, req)
    }

    /// Execute an authorized `ReadGrant` for a specific `expected_node_id`, streaming directly
    /// from the confined `OpenedFile` handle without reopening by path.
    pub fn serve_grant_for_node<B>(
        &self,
        expected_node_id: NodeId,
        grant: &ReadGrant,
        req: &Request<B>,
    ) -> impl std::future::Future<Output = Result<Response<BoxStreamBody>, NodeErrorCode>> + Send + 'static
    {
        let grant = grant.clone();
        let method = req.method().clone();
        let headers = req.headers().clone();
        let node = self.clone();
        async move {
            let root = Path::new(&grant.root_snapshot);
            let relative = Path::new(&grant.relative_path);
            match &grant.target {
                ReadTarget::Asset(_) => {
                    if grant.node_id != expected_node_id {
                        return Err(NodeErrorCode::UnknownAsset);
                    }
                    let opened = node
                        .open_media(root, relative)
                        .await
                        .map_err(|_| NodeErrorCode::UnavailableAsset)?;
                    serve_opened_parts(opened, &method, &headers, &grant.mime)
                        .await
                        .map_err(|_| NodeErrorCode::StreamError)
                }
                ReadTarget::Image {
                    expected_size,
                    expected_modified_ns,
                    ..
                } => {
                    if grant.node_id != expected_node_id {
                        return Err(NodeErrorCode::UnknownImage);
                    }
                    let opened = node
                        .open_media(root, relative)
                        .await
                        .map_err(|_| NodeErrorCode::UnavailableImage)?;
                    let checked = images::validate_opened(&opened)
                        .await
                        .map_err(|_| NodeErrorCode::InvalidImage)?;
                    if checked.size_bytes != *expected_size
                        || checked.modified_ns != *expected_modified_ns
                        || checked.mime != grant.mime
                    {
                        return Err(NodeErrorCode::ImageChangedRescanRequired);
                    }
                    let mut res = serve_opened_parts(opened, &method, &headers, checked.mime)
                        .await
                        .map_err(|_| NodeErrorCode::ImageStreamError)?;
                    // Never let a browser cache bypass a later permission/revocation check.
                    res.headers_mut().insert(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("private, no-store"),
                    );
                    Ok(res)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{Method, StatusCode};
    use http_body_util::BodyExt;
    use media_contracts::ids::{AssetId, ImageId, LibraryId};

    #[tokio::test]
    async fn local_node_serve_grant_streams_asset_and_validates_image_handle() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        tokio::fs::write(root.join("clip.mp4"), b"0123456789abcdef")
            .await
            .unwrap();
        let poster_path = root.join("poster.png");
        image::DynamicImage::new_rgb8(2, 2)
            .save(&poster_path)
            .unwrap();
        let poster_opened = OpenedFile::open_in_root(root, Path::new("poster.png"))
            .await
            .unwrap();
        let poster_size = poster_opened.size();
        let poster_modified = poster_opened.modified_ns().to_owned();
        drop(poster_opened);

        let node = LocalNode;
        let asset_grant = ReadGrant {
            target: ReadTarget::Asset(AssetId::from("asset-1")),
            library_id: LibraryId::from("lib-1"),
            node_id: NodeId::local(),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "clip.mp4".into(),
            mime: "video/mp4".into(),
        };
        let req = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/assets/asset-1/content")
            .header(header::RANGE, "bytes=4-7")
            .body(())
            .unwrap();
        let res = node.serve_grant(&asset_grant, &req).await.unwrap();
        assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), b"4567");

        let image_grant = ReadGrant {
            target: ReadTarget::Image {
                id: ImageId::from("img-1"),
                expected_size: poster_size,
                expected_modified_ns: poster_modified,
            },
            library_id: LibraryId::from("lib-1"),
            node_id: NodeId::local(),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "poster.png".into(),
            mime: "image/png".into(),
        };
        let img_req = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/assets/img-1/image")
            .body(())
            .unwrap();
        let img_res = node.serve_grant(&image_grant, &img_req).await.unwrap();
        assert_eq!(img_res.status(), StatusCode::OK);
        assert_eq!(
            img_res.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );

        // Mutate poster size -> must return ImageChangedRescanRequired
        image::DynamicImage::new_rgb8(6, 6)
            .save(&poster_path)
            .unwrap();
        assert_eq!(
            node.serve_grant(&image_grant, &img_req).await.unwrap_err(),
            NodeErrorCode::ImageChangedRescanRequired
        );
    }
}
