use super::*;
use crate::{
    files::OpenedFile,
    stream::{BoxStreamBody, empty_body, parse_single_range},
};
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use std::{io, sync::Arc, time::Instant};
use tokio::sync::{Mutex, OwnedMutexGuard, Semaphore};

pub(super) struct Cached {
    pub(super) key: String,
    pub(super) index: Arc<Index>,
    pub(super) used: Instant,
}

pub struct NativeAudio {
    pub(super) cache: Mutex<Vec<Cached>>,
    pub(super) indexing: Arc<Mutex<()>>,
    pub(super) cold_admission: Semaphore,
}

impl Default for NativeAudio {
    fn default() -> Self {
        Self {
            cache: Mutex::new(Vec::new()),
            indexing: Arc::new(Mutex::new(())),
            cold_admission: Semaphore::new(64),
        }
    }
}

pub(super) async fn guarded_index_task<T: Send + 'static>(
    guard: OwnedMutexGuard<()>,
    build: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<(T, OwnedMutexGuard<()>)> {
    // A running spawn_blocking task cannot be aborted. Keep its admission
    // guard inside the task, even if the requesting HTTP future is cancelled.
    // Return the guard too, so insertion still precedes the next cache lookup.
    let (result, guard) = tokio::task::spawn_blocking(move || (build(), guard))
        .await
        .map_err(|_| io::Error::other("native MP4 indexing task failed"))?;
    Ok((result?, guard))
}

impl NativeAudio {
    pub fn serve<'a, B>(
        &'a self,
        opened: OpenedFile,
        track_id: u32,
        chunk: u32,
        request: &Request<B>,
    ) -> impl std::future::Future<Output = io::Result<Response<BoxStreamBody>>> + Send + 'a {
        let method = request.method().clone();
        let headers = request.headers().clone();
        async move {
            opened.verify_unchanged()?;
            let key = format!(
                "{:?}:{}:{}",
                opened.canonical_path(),
                opened.size(),
                opened.modified_ns()
            );
            let cached = {
                let mut cache = self.cache.lock().await;
                cache
                    .iter_mut()
                    .find(|entry| entry.key == key)
                    .map(|entry| {
                        entry.used = Instant::now();
                        entry.index.clone()
                    })
            };
            let index = if let Some(index) = cached {
                index
            } else {
                let _cold = self.cold_admission.try_acquire().map_err(|_| {
                    io::Error::new(io::ErrorKind::WouldBlock, "native MP4 index admission full")
                })?;
                let indexing = tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    self.indexing.clone().lock_owned(),
                )
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native MP4 index admission timeout",
                    )
                })?;
                let again = {
                    let mut cache = self.cache.lock().await;
                    cache
                        .iter_mut()
                        .find(|entry| entry.key == key)
                        .map(|entry| {
                            entry.used = Instant::now();
                            entry.index.clone()
                        })
                };
                if let Some(index) = again {
                    index
                } else {
                    let file = opened.try_clone_std()?;
                    let (parsed, _indexing) =
                        guarded_index_task(indexing, move || load_index(file)).await?;
                    let index = Arc::new(parsed);
                    opened.verify_unchanged_sync()?;
                    let mut cache = self.cache.lock().await;
                    while cache.len() >= 8
                        || cache.iter().map(|entry| entry.index.bytes).sum::<usize>() + index.bytes
                            > MAX_CACHE
                    {
                        let Some(victim) = cache
                            .iter()
                            .enumerate()
                            .filter(|(_, entry)| Arc::strong_count(&entry.index) == 1)
                            .min_by_key(|(_, entry)| entry.used)
                            .map(|(n, _)| n)
                        else {
                            return Err(io::Error::new(
                                io::ErrorKind::WouldBlock,
                                "native MP4 index cache busy",
                            ));
                        };
                        cache.remove(victim);
                    }
                    cache.push(Cached {
                        key,
                        index: index.clone(),
                        used: Instant::now(),
                    });
                    index
                }
            };
            let track = index
                .tracks
                .iter()
                .find(|track| track.id == track_id)
                .ok_or_else(|| unsupported("native Opus track ID unavailable"))?;
            let window = track.window(&index.data, chunk)?;
            opened.verify_unchanged()?;
            let total = window.prefix.len() as u64 + window.total;
            let range = headers.get(header::RANGE).map(|raw| {
                raw.to_str()
                    .ok()
                    .and_then(|raw| parse_single_range(raw, total))
            });
            let mut response = Response::builder()
                .header(header::CONTENT_TYPE, "audio/mp4")
                .header(header::ACCEPT_RANGES, "bytes");
            let (start, length) = match range {
                Some(Some(range)) => {
                    response = response.status(StatusCode::PARTIAL_CONTENT).header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{total}", range.start, range.end),
                    );
                    (range.start, range.len())
                }
                Some(None) => {
                    return response
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                        .header(header::CONTENT_LENGTH, 0)
                        .body(empty_body())
                        .map_err(io::Error::other);
                }
                None => {
                    response = response.status(StatusCode::OK);
                    (0, total)
                }
            };
            response = response.header(header::CONTENT_LENGTH, length);
            let etag = format!(
                "\"opus-v1-{}-{}-{}-{}\"",
                opened.modified_ns(),
                opened.size(),
                track_id,
                chunk
            );
            response = response.header(header::ETAG, etag);
            if method == Method::HEAD {
                return response.body(empty_body()).map_err(io::Error::other);
            }
            let body = VirtualBody::new(opened, window, start, length)?;
            response.body(body.boxed()).map_err(io::Error::other)
        }
    }
}
