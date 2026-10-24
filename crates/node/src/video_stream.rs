//! Confined, cancellable AV1 packet-copy streaming with bounded backpressure.
//! A single fragmented MP4 timeline avoids independent GOP overlap at joins.
use crate::{BoxStreamBody, OpenedFile};
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use std::{io, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::{Semaphore, mpsc},
};

pub fn position(raw: &str) -> Option<(u32, u32)> {
    let (index, value) = raw.strip_prefix("video-stream-")?.split_once('-')?;
    if index.is_empty()
        || value.is_empty()
        || !index
            .bytes()
            .chain(value.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let index = index.parse().ok()?;
    let value = value.parse::<u32>().ok()?;
    (value < 86_400_000).then_some((index, value))
}

pub struct VideoStreams {
    slots: Arc<Semaphore>,
}
impl Default for VideoStreams {
    fn default() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(4)),
        }
    }
}
impl VideoStreams {
    pub fn serve<'a, B>(
        &'a self,
        opened: OpenedFile,
        index: u32,
        milliseconds: u32,
        request: &Request<B>,
    ) -> impl Future<Output = io::Result<Response<BoxStreamBody>>> + Send + 'a {
        let method = request.method().clone();
        let has_range = request.headers().contains_key(header::RANGE);
        async move {
            if milliseconds >= 86_400_000 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "video position out of bounds",
                ));
            }
            opened.verify_unchanged_sync()?;
            let response = Response::builder()
                .header(header::CONTENT_TYPE, "video/mp4")
                .header(header::CACHE_CONTROL, "private, no-store")
                .header(header::ACCEPT_RANGES, "none");
            if has_range {
                return response
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .body(crate::stream::empty_body())
                    .map_err(io::Error::other);
            }
            if method == Method::HEAD {
                return response
                    .body(crate::stream::empty_body())
                    .map_err(io::Error::other);
            }
            if method != Method::GET {
                return response
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .body(crate::stream::empty_body())
                    .map_err(io::Error::other);
            }
            let permit = tokio::time::timeout(
                Duration::from_millis(250),
                self.slots.clone().acquire_owned(),
            )
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "video stream capacity reached")
            })?
            .map_err(|_| io::Error::other("video stream service stopped"))?;
            let mut file = opened.try_clone_std()?;
            std::io::Seek::rewind(&mut file)?;
            let mut child = Command::new("ffmpeg");
            child.args([
                "-v",
                "error",
                "-nostdin",
                "-protocol_whitelist",
                "fd,pipe",
                "-format_whitelist",
                "matroska,webm,mov,mp4",
                "-max_alloc",
                "67108864",
                "-ss",
                &format!("{:.3}", f64::from(milliseconds) / 1000.0),
                "-copyts",
                "-start_at_zero",
                "-fd",
                "0",
                "-i",
                "fd:",
                "-map",
                &format!("0:{index}"),
                "-an",
                "-sn",
                "-c:v",
                "copy",
                "-movflags",
                "frag_keyframe+delay_moov+default_base_moof",
                "-frag_duration",
                "1000000",
                "-f",
                "mp4",
                "pipe:1",
            ]);
            let mut child = child
                .stdin(Stdio::from(file))
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("video pipe missing"))?;
            let mut first = vec![0; 65_536];
            let read = tokio::time::timeout(Duration::from_secs(10), stdout.read(&mut first))
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "video stream initialization timeout",
                    )
                })??;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "video packet copy failed",
                ));
            }
            first.truncate(read);
            opened.verify_unchanged_sync()?;
            let (sender, receiver) = mpsc::channel(1);
            let task = tokio::spawn(async move {
                let _permit = permit;
                let _opened = opened;
                if sender.send(Ok(Bytes::from(first))).await.is_err() {
                    return;
                }
                loop {
                    let mut bytes = vec![0; 65_536];
                    let result = tokio::select! {
                        _ = sender.closed() => return,
                        result = tokio::time::timeout(Duration::from_secs(30), stdout.read(&mut bytes)) => result,
                    };
                    match result {
                        Ok(Ok(0)) => {
                            match child.wait().await {
                                Ok(status) if status.success() => {}
                                _ => {
                                    let _ = sender
                                        .send(Err(io::Error::other(
                                            "video packet copy interrupted",
                                        )))
                                        .await;
                                }
                            }
                            return;
                        }
                        Ok(Ok(read)) => {
                            bytes.truncate(read);
                            if sender.send(Ok(Bytes::from(bytes))).await.is_err() {
                                return;
                            }
                        }
                        Ok(Err(error)) => {
                            let _ = sender.send(Err(error)).await;
                            return;
                        }
                        Err(_) => {
                            let _ = sender
                                .send(Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "video packet copy stalled",
                                )))
                                .await;
                            return;
                        }
                    }
                }
            });
            response
                .body(PipeBody { receiver, task }.boxed())
                .map_err(io::Error::other)
        }
    }
}
struct PipeBody {
    receiver: mpsc::Receiver<io::Result<Bytes>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for PipeBody {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl http_body::Body for PipeBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<io::Result<http_body::Frame<Bytes>>>> {
        self.receiver
            .poll_recv(cx)
            .map(|value| value.map(|value| value.map(http_body::Frame::data)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Seek, path::Path};
    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative")
    }
    async fn opened(extension: &str) -> OpenedFile {
        OpenedFile::open_in_root(&root(), Path::new(&format!("representative.{extension}")))
            .await
            .unwrap()
    }
    async fn packets(file: &OpenedFile) -> Vec<String> {
        let mut input = file.try_clone_std().unwrap();
        input.rewind().unwrap();
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "fd,pipe",
                "-select_streams",
                "v:0",
                "-show_packets",
                "-show_data_hash",
                "sha256",
                "-show_entries",
                "packet=data_hash",
                "-of",
                "json",
                "-",
            ])
            .stdin(Stdio::from(input))
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        value["packets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|packet| packet["data_hash"].as_str().unwrap().to_owned())
            .collect()
    }
    #[test]
    fn stream_positions_are_decimal_milliseconds_bounded_to_one_day() {
        assert_eq!(position("video-stream-0-0"), Some((0, 0)));
        assert_eq!(position("video-stream-2-86399999"), Some((2, 86_399_999)));
        for value in [
            "video-stream-",
            "video-stream-0",
            "video-stream--1",
            "video-stream-+1",
            "video-stream-0-86400000",
            "video-stream-0-4294967296",
            "video-stream-0-0/../etc",
        ] {
            assert_eq!(position(value), None);
        }
    }
    #[tokio::test]
    async fn streams_av1_packet_copy_from_confined_mkv_and_mp4_without_prepared_files() {
        let streams = VideoStreams::default();
        let temp = tempfile::tempdir().unwrap();
        for extension in ["mkv", "mp4"] {
            let original = opened(extension).await;
            let all = packets(&original).await;
            // Probe above advanced the inherited descriptor: serve must rewind.
            let response = streams
                .serve(original, 0, 0, &Request::new(()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::ACCEPT_RANGES], "none");
            assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.len() > 1000);
            std::fs::write(temp.path().join("stream.mp4"), &bytes).unwrap();
            let output = OpenedFile::open_in_root(temp.path(), Path::new("stream.mp4"))
                .await
                .unwrap();
            assert_eq!(packets(&output).await, all);
            let response = streams
                .serve(opened(extension).await, 0, 10_250, &Request::new(()))
                .await
                .unwrap();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            std::fs::write(temp.path().join("stream.mp4"), &bytes).unwrap();
            let output = OpenedFile::open_in_root(temp.path(), Path::new("stream.mp4"))
                .await
                .unwrap();
            let selected = packets(&output).await;
            assert!(!selected.is_empty());
            assert!(selected.len() < all.len());
            assert!(selected.iter().all(|packet| all.contains(packet)));
        }
    }
    #[tokio::test]
    async fn head_range_and_invalid_method_do_not_acquire_process_slots() {
        let streams = VideoStreams::default();
        let _held = streams.slots.acquire_many(4).await.unwrap();
        for (request, status) in [
            (
                Request::builder().method(Method::HEAD).body(()).unwrap(),
                StatusCode::OK,
            ),
            (
                Request::builder()
                    .header(header::RANGE, "bytes=0-1023")
                    .body(())
                    .unwrap(),
                StatusCode::RANGE_NOT_SATISFIABLE,
            ),
            (
                Request::builder().method(Method::POST).body(()).unwrap(),
                StatusCode::METHOD_NOT_ALLOWED,
            ),
        ] {
            let response = tokio::time::timeout(
                Duration::from_millis(150),
                streams.serve(opened("mp4").await, 0, 0, &request),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(response.status(), status);
            assert!(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .is_empty()
            );
        }
        assert_eq!(streams.slots.available_permits(), 0);
    }
    #[tokio::test]
    async fn cancelling_an_unread_pipe_releases_the_child_and_process_permit() {
        let streams = VideoStreams::default();
        let mut response = streams
            .serve(opened("mp4").await, 0, 0, &Request::new(()))
            .await
            .unwrap();
        response.body_mut().frame().await.unwrap().unwrap();
        assert!(streams.slots.available_permits() < 4);
        drop(response);
        tokio::time::timeout(Duration::from_secs(2), async {
            while streams.slots.available_permits() != 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let response = streams
            .serve(opened("mp4").await, 0, 86_400_000, &Request::new(()))
            .await;
        assert_eq!(response.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn saturated_video_admission_stays_bounded_for_four_eight_and_sixteen_requests() {
        for requests in [4, 8, 16] {
            let streams = Arc::new(VideoStreams::default());
            let held = streams.slots.clone().acquire_many_owned(4).await.unwrap();
            let mut tasks = tokio::task::JoinSet::new();
            for _ in 0..requests {
                let streams = streams.clone();
                tasks.spawn(async move {
                    let error = streams
                        .serve(opened("mp4").await, 0, 0, &Request::new(()))
                        .await
                        .unwrap_err();
                    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                });
            }
            while let Some(result) = tasks.join_next().await {
                result.unwrap();
            }
            assert_eq!(streams.slots.available_permits(), 0);
            drop(held);
            assert_eq!(streams.slots.available_permits(), 4);
        }
    }

    #[tokio::test]
    async fn rejected_media_releases_the_child_slot_and_a_later_valid_stream_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("broken.mp4"), b"not a media container").unwrap();
        let streams = VideoStreams::default();
        let source = OpenedFile::open_in_root(temp.path(), Path::new("broken.mp4"))
            .await
            .unwrap();
        let error = streams
            .serve(source, 0, 0, &Request::new(()))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(streams.slots.available_permits(), 4);
        let response = streams
            .serve(opened("mp4").await, 0, 0, &Request::new(()))
            .await
            .unwrap();
        assert!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .len()
                > 0
        );
        assert_eq!(streams.slots.available_permits(), 4);
    }
}
