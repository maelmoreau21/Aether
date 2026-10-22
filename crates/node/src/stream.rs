//! Bounded 64 KiB HTTP streaming (`GET`, `HEAD`, RFC 9110 single `Range`) directly
//! from an already-confined `OpenedFile` handle, eliminating path-reopen TOCTOU races.

use crate::files::OpenedFile;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use http_body_util::{BodyExt, Empty, combinators::BoxBody};
use std::io::{self, SeekFrom};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt};

pub const STREAM_CHUNK_BYTES: usize = 65_536;

pub type BoxStreamBody = BoxBody<Bytes, io::Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn len(&self) -> u64 {
        if self.start > self.end {
            0
        } else {
            self.end.saturating_sub(self.start).saturating_add(1)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start > self.end
    }
}

/// Parse an RFC 9110 single byte-range header against `file_size`.
/// Multi-range requests and malformed or out-of-bounds ranges return `None` (416).
pub fn parse_single_range(header_value: &str, file_size: u64) -> Option<ByteRange> {
    if file_size == 0 {
        return None;
    }
    let spec = header_value.strip_prefix("bytes=")?.trim();
    if spec.is_empty() || spec.contains(',') {
        return None;
    }
    let (start_str, end_str) = spec.split_once('-')?;
    let start_str = start_str.trim();
    let end_str = end_str.trim();

    if start_str.is_empty() {
        // Suffix byte-range-spec: "bytes=-N"
        if end_str.is_empty() || !end_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let suffix: u64 = end_str.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let length = suffix.min(file_size);
        Some(ByteRange {
            start: file_size - length,
            end: file_size - 1,
        })
    } else if end_str.is_empty() {
        // Open-ended byte-range-spec: "bytes=S-"
        if !start_str.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let start: u64 = start_str.parse().ok()?;
        if start >= file_size {
            return None;
        }
        Some(ByteRange {
            start,
            end: file_size - 1,
        })
    } else {
        // Closed byte-range-spec: "bytes=S-E"
        if !start_str.bytes().all(|b| b.is_ascii_digit())
            || !end_str.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let start: u64 = start_str.parse().ok()?;
        let end: u64 = end_str.parse().ok()?;
        if start > end || start >= file_size {
            return None;
        }
        Some(ByteRange {
            start,
            end: end.min(file_size - 1),
        })
    }
}

pub(crate) fn empty_body() -> BoxStreamBody {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

/// Stream an already-opened, confined `OpenedFile` in 64 KiB chunks for `GET` or `HEAD`
/// requests, honoring RFC 9110 single `Range` headers without re-opening the file by path.
pub fn serve_opened<B>(
    opened: OpenedFile,
    req: &Request<B>,
    mime: &str,
) -> impl std::future::Future<Output = io::Result<Response<BoxStreamBody>>> + Send + 'static {
    serve_opened_parts(opened, req.method(), req.headers(), mime)
}

pub fn serve_opened_parts(
    mut opened: OpenedFile,
    method: &Method,
    headers: &HeaderMap,
    mime: &str,
) -> impl std::future::Future<Output = io::Result<Response<BoxStreamBody>>> + Send + 'static {
    let is_head = *method == Method::HEAD;
    let range_header = headers
        .get(header::RANGE)
        .map(|raw| raw.to_str().unwrap_or("").to_owned());
    let content_type = HeaderValue::from_str(mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));

    async move {
        let lease = opened.take_cache_lease();
        opened.verify_unchanged()?;
        let file_size = opened.size();
        let etag_str = format!("\"{}-{:x}\"", opened.modified_ns(), file_size);
        let etag_value = HeaderValue::from_str(&etag_str)
            .unwrap_or_else(|_| HeaderValue::from_static("\"0-0\""));

        if let Some(range_str) = range_header {
            match parse_single_range(&range_str, file_size) {
                Some(range) => {
                    let length = range.len();
                    let content_range =
                        format!("bytes {}-{}/{}", range.start, range.end, file_size);
                    let builder = Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_TYPE, content_type)
                        .header(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"))
                        .header(header::CONTENT_LENGTH, length.to_string())
                        .header(header::CONTENT_RANGE, content_range)
                        .header(header::ETAG, etag_value);

                    let body = if is_head {
                        empty_body()
                    } else {
                        let mut tokio_file = opened.into_tokio_file();
                        tokio_file.seek(SeekFrom::Start(range.start)).await?;
                        BodyExt::boxed(
                            ChunkedFileBody::new(tokio_file.take(length), length).with_lease(lease),
                        )
                    };
                    Ok(builder.body(body).expect("valid 206 response"))
                }
                None => {
                    let content_range = format!("bytes */{file_size}");
                    Ok(Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(header::CONTENT_TYPE, content_type)
                        .header(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"))
                        .header(header::CONTENT_LENGTH, "0")
                        .header(header::CONTENT_RANGE, content_range)
                        .body(empty_body())
                        .expect("valid 416 response"))
                }
            }
        } else {
            let body = if is_head || file_size == 0 {
                empty_body()
            } else {
                let mut tokio_file = opened.into_tokio_file();
                tokio_file.seek(SeekFrom::Start(0)).await?;
                BodyExt::boxed(
                    ChunkedFileBody::new(tokio_file.take(file_size), file_size).with_lease(lease),
                )
            };
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"))
                .header(header::CONTENT_LENGTH, file_size.to_string())
                .header(header::ETAG, etag_value)
                .body(body)
                .expect("valid 200 response"))
        }
    }
}

struct ChunkedFileBody {
    reader: tokio::io::Take<tokio::fs::File>,
    remaining: u64,
    buf: Vec<u8>,
    _lease: Option<std::sync::Arc<crate::cache_dirs::CacheDirectory>>,
}

impl ChunkedFileBody {
    fn with_lease(
        mut self,
        lease: Option<std::sync::Arc<crate::cache_dirs::CacheDirectory>>,
    ) -> Self {
        self._lease = lease;
        self
    }
    fn new(reader: tokio::io::Take<tokio::fs::File>, remaining: u64) -> Self {
        let cap = remaining.min(STREAM_CHUNK_BYTES as u64) as usize;
        Self {
            reader,
            remaining,
            buf: vec![0u8; cap],
            _lease: None,
        }
    }
}

impl http_body::Body for ChunkedFileBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if self.remaining == 0 {
            return std::task::Poll::Ready(None);
        }
        let this = &mut *self;
        let max_read = this.remaining.min(this.buf.len() as u64) as usize;
        let mut read_buf = tokio::io::ReadBuf::new(&mut this.buf[..max_read]);
        match std::pin::Pin::new(&mut this.reader).poll_read(cx, &mut read_buf) {
            std::task::Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if filled.is_empty() {
                    this.remaining = 0;
                    std::task::Poll::Ready(None)
                } else {
                    let n = filled.len();
                    this.remaining = this.remaining.saturating_sub(n as u64);
                    let chunk = Bytes::copy_from_slice(filled);
                    std::task::Poll::Ready(Some(Ok(http_body::Frame::data(chunk))))
                }
            }
            std::task::Poll::Ready(Err(err)) => {
                this.remaining = 0;
                std::task::Poll::Ready(Some(Err(err)))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn active_response_retains_preparation_cache_lease() {
        let directory = std::sync::Arc::new(crate::cache_dirs::CacheDirectory::new().unwrap());
        let path = directory.path().to_owned();
        std::fs::write(path.join("content"), b"packet-copy").unwrap();
        let opened = OpenedFile::open_in_root(&path, Path::new("content"))
            .await
            .unwrap()
            .with_cache_lease(directory.clone());
        let request = Request::builder().body(()).unwrap();
        let response = serve_opened(opened, &request, "audio/mp4").await.unwrap();
        drop(directory);
        assert!(path.exists());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], b"packet-copy");
        assert!(!path.exists());
    }

    #[test]
    fn range_parser_and_byte_range_resist_u64_max_overflow_and_boundary_cases() {
        let max_range = ByteRange {
            start: 0,
            end: u64::MAX,
        };
        assert_eq!(max_range.len(), u64::MAX);
        assert!(!max_range.is_empty());

        let inverted = ByteRange { start: 10, end: 5 };
        assert_eq!(inverted.len(), 0);
        assert!(inverted.is_empty());

        assert_eq!(parse_single_range("bytes=0-0", 0), None);
        assert_eq!(
            parse_single_range(&format!("bytes=0-{}", u64::MAX), 100),
            Some(ByteRange { start: 0, end: 99 })
        );
        assert_eq!(
            parse_single_range(&format!("bytes={}-{}", u64::MAX - 1, u64::MAX), u64::MAX),
            Some(ByteRange {
                start: u64::MAX - 1,
                end: u64::MAX - 1
            })
        );
        assert_eq!(
            parse_single_range(&format!("bytes={}-", u64::MAX), u64::MAX),
            None
        );
        assert_eq!(
            parse_single_range(&format!("bytes=-{}", u64::MAX), 50),
            Some(ByteRange { start: 0, end: 49 })
        );
    }

    #[tokio::test]
    async fn streams_get_head_and_ranges_from_confined_handle_even_if_path_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let payload: Vec<u8> = (0..150_000u32).map(|i| (i % 251) as u8).collect();
        tokio::fs::write(root.join("video.mp4"), &payload)
            .await
            .unwrap();

        // 1. Full GET streams in <= 64 KiB chunks
        let opened = OpenedFile::open_in_root(root, Path::new("video.mp4"))
            .await
            .unwrap();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/video.mp4")
            .body(())
            .unwrap();
        let mut res = serve_opened(opened, &req, "video/mp4").await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CONTENT_TYPE], "video/mp4");
        assert_eq!(res.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(
            res.headers()[header::CONTENT_LENGTH],
            payload.len().to_string()
        );
        let first_frame = res
            .body_mut()
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(first_frame.len(), STREAM_CHUNK_BYTES);
        let rest = res.into_body().collect().await.unwrap().to_bytes();
        let mut combined = first_frame.to_vec();
        combined.extend_from_slice(&rest);
        assert_eq!(combined, payload);

        // 2. HEAD returns headers and empty body
        let opened = OpenedFile::open_in_root(root, Path::new("video.mp4"))
            .await
            .unwrap();
        let head_req = Request::builder()
            .method(Method::HEAD)
            .uri("/video.mp4")
            .body(())
            .unwrap();
        let head_res = serve_opened(opened, &head_req, "video/mp4").await.unwrap();
        assert_eq!(head_res.status(), StatusCode::OK);
        assert_eq!(
            head_res.headers()[header::CONTENT_LENGTH],
            payload.len().to_string()
        );
        assert!(
            head_res
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );

        // 3. Post-open path substitution: open handle first, then rename/replace path on disk
        let opened = OpenedFile::open_in_root(root, Path::new("video.mp4"))
            .await
            .unwrap();
        let moved = root.join("video_original_moved.mp4");
        tokio::fs::rename(root.join("video.mp4"), &moved)
            .await
            .unwrap();
        tokio::fs::write(root.join("video.mp4"), b"attacker replacement")
            .await
            .unwrap();
        let range_req = Request::builder()
            .method(Method::GET)
            .uri("/video.mp4")
            .header(header::RANGE, "bytes=10-41")
            .body(())
            .unwrap();
        let range_res = serve_opened(opened, &range_req, "video/mp4").await.unwrap();
        assert_eq!(range_res.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            range_res.headers()[header::CONTENT_RANGE],
            format!("bytes 10-41/{}", payload.len())
        );
        let range_bytes = range_res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(range_bytes.as_ref(), &payload[10..=41]);

        // 4. In-place truncation/mutation of the opened file is rejected by serve_opened
        tokio::fs::write(root.join("video.mp4"), &payload)
            .await
            .unwrap();
        let opened_mutated = OpenedFile::open_in_root(root, Path::new("video.mp4"))
            .await
            .unwrap();
        tokio::fs::write(root.join("video.mp4"), b"truncated in place")
            .await
            .unwrap();
        assert!(
            serve_opened(opened_mutated, &range_req, "video/mp4")
                .await
                .is_err()
        );

        // 5. Invalid ranges return 416 Range Not Satisfiable
        tokio::fs::write(root.join("video.mp4"), &payload)
            .await
            .unwrap();
        for bad_range in [
            "bytes=99999999-",
            "bytes=20-10",
            "bytes=-0",
            "bytes=invalid",
            "bytes=0-1,3-4",
        ] {
            let opened = OpenedFile::open_in_root(root, Path::new("video.mp4"))
                .await
                .unwrap();
            let bad_req = Request::builder()
                .method(Method::GET)
                .uri("/video.mp4")
                .header(header::RANGE, bad_range)
                .body(())
                .unwrap();
            let bad_res = serve_opened(opened, &bad_req, "video/mp4").await.unwrap();
            assert_eq!(bad_res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
            assert_eq!(
                bad_res.headers()[header::CONTENT_RANGE],
                format!("bytes */{}", payload.len())
            );
        }
    }
}
