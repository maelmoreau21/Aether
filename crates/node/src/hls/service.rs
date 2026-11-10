//! `prepared/hls-…` on the Node. Two kinds of media playlists, both cut on
//! the keyframes of video stream `v`:
//!
//! - video rendition `hls-v{v}.m3u8` (+ `-init.mp4`, `-{n}.m4s`), cut on fragments;
//! - audio rendition `hls-v{v}-a{a}.m3u8` (same suffixes), cut on timestamps.
//!
//! Separate renditions are what lets both cuts be exact: B-frames make video
//! decode order differ from presentation order, which no single timestamp cut
//! can respect for video and audio at once. They also expose every audio track
//! to native players. Segments are independent packet copies, served in any
//! order without a session or temporary files.
use super::{
    keyframes::{self, Keyframes},
    playlist::{self, Segment},
    segments::{BoxFilter, MediaBody, Part, TIMELINE_OFFSET_MS, Window},
};
use crate::{
    BoxStreamBody, OpenedFile,
    transcode::coded,
    video_stream::{PipeBody, PipeLabels, spawn_pipe},
};
use bytes::Bytes;
use http::{Method, Response, StatusCode, header};
use http_body_util::{BodyExt, Full};
use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{process::Command, sync::Semaphore};

/// Keyframe indexes and initialization sections kept per Node (a few KB each).
const CACHE_ENTRIES: usize = 64;
const MAX_INIT_BYTES: usize = 4 * 1024 * 1024;
/// Segments are deterministic for one source version: devices may keep them.
const SEGMENT_CACHE: &str = "private, max-age=86400";
/// Video copies read past the segment end; the fragment cut stops them earlier.
const VIDEO_READ_MARGIN_MS: u64 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Playlist,
    Init,
    Segment(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HlsRequest {
    /// FFmpeg stream index of the video stream whose keyframes cut the timeline.
    pub video: u32,
    /// `Some` for an audio rendition (that stream index), `None` for the video one.
    pub audio: Option<u32>,
    pub resource: Resource,
}

impl HlsRequest {
    fn profile(&self) -> String {
        match self.audio {
            Some(audio) => format!("hls-v{}-a{audio}", self.video),
            None => format!("hls-v{}", self.video),
        }
    }
    /// The canonical component name; parsing only accepts this spelling.
    pub fn component(&self) -> String {
        let profile = self.profile();
        match self.resource {
            Resource::Playlist => format!("{profile}.m3u8"),
            Resource::Init => format!("{profile}-init.mp4"),
            Resource::Segment(n) => format!("{profile}-{n}.m4s"),
        }
    }
}

fn number(raw: &str, max_digits: usize) -> Option<(u32, &str)> {
    let digits = raw.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > max_digits {
        return None;
    }
    Some((raw[..digits].parse().ok()?, &raw[digits..]))
}

pub fn request(raw: &str) -> Option<HlsRequest> {
    let (video, rest) = number(raw.strip_prefix("hls-v")?, 4)?;
    let (audio, rest) = match rest.strip_prefix("-a") {
        Some(after) => {
            let (audio, rest) = number(after, 4)?;
            (Some(audio), rest)
        }
        None => (None, rest),
    };
    let resource = match rest {
        ".m3u8" => Resource::Playlist,
        "-init.mp4" => Resource::Init,
        _ => {
            let (n, suffix) = number(rest.strip_prefix('-')?, 6)?;
            if suffix != ".m4s" {
                return None;
            }
            Resource::Segment(n)
        }
    };
    let parsed = HlsRequest {
        video,
        audio,
        resource,
    };
    (parsed.component() == raw).then_some(parsed)
}

fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

/// FFmpeg arguments for one segment. `-copyts` keeps the source timeline and
/// `frag_discont` writes it into each fragment's decode time, so independently
/// produced segments line up after their rendition's initialization section.
pub fn arguments(request: &HlsRequest, segment: Segment, last: bool, hevc: bool) -> Vec<String> {
    let owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
    let mut args = owned(&[
        "-v",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "fd,pipe",
        "-format_whitelist",
        "matroska,webm,mov,mp4",
        "-max_alloc",
        "268435456",
        "-ss",
        &seconds(segment.start_ms),
        "-copyts",
        "-fd",
        "0",
        "-i",
        "fd:",
    ]);
    match request.audio {
        None => {
            if !last {
                args.extend(owned(&[
                    "-to",
                    &seconds(segment.end_ms + VIDEO_READ_MARGIN_MS),
                ]));
            }
            args.extend(owned(&["-map", &format!("0:{}", request.video), "-an"]));
            if hevc {
                // FFmpeg's default `hev1` entry is refused by AVPlayer and AirPlay.
                args.extend(owned(&["-tag:v", "hvc1"]));
            }
            // One fragment per keyframe: the fragment cut needs nothing else.
            args.extend(owned(&[
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof+frag_discont",
            ]));
        }
        Some(audio) => {
            if !last {
                args.extend(owned(&["-to", &seconds(segment.end_ms)]));
            }
            // Audio decode and presentation times coincide: dropping packets
            // before the start and stopping at the end partitions them exactly.
            args.extend(owned(&[
                "-map",
                &format!("0:{audio}"),
                "-vn",
                "-copypriorss",
                "0",
            ]));
            args.extend(owned(&[
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof+frag_discont",
                "-frag_duration",
                "2000000",
            ]));
        }
    }
    args.extend(owned(&[
        "-sn",
        "-dn",
        "-c",
        "copy",
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
        "-avoid_negative_ts",
        "disabled",
        "-output_ts_offset",
        &seconds(TIMELINE_OFFSET_MS),
        "-use_editlist",
        "0",
        "-max_muxing_queue_size",
        "1024",
        "-f",
        "mp4",
        "pipe:1",
    ]));
    args
}

fn remember<T>(map: &Mutex<HashMap<String, T>>, key: String, value: T) {
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= CACHE_ENTRIES && !map.contains_key(&key) {
        map.clear();
    }
    map.insert(key, value);
}

fn bytes_response(
    method: &Method,
    content_type: &'static str,
    cache: &'static str,
    bytes: Bytes,
) -> io::Result<Response<BoxStreamBody>> {
    let response = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache)
        .header(header::CONTENT_LENGTH, bytes.len());
    let body = if method == Method::HEAD {
        crate::stream::empty_body()
    } else {
        Full::new(bytes).map_err(|never| match never {}).boxed()
    };
    response.body(body).map_err(io::Error::other)
}

pub struct HlsStreams {
    slots: Arc<Semaphore>,
    indexes: Mutex<HashMap<String, Arc<Keyframes>>>,
    inits: Mutex<HashMap<String, Bytes>>,
}
impl Default for HlsStreams {
    fn default() -> Self {
        Self {
            // Packet copies are I/O bound and take a fraction of their playback
            // time: a few in parallel cover several viewers.
            slots: Arc::new(Semaphore::new(6)),
            indexes: Mutex::new(HashMap::new()),
            inits: Mutex::new(HashMap::new()),
        }
    }
}

impl HlsStreams {
    async fn keyframes(
        &self,
        opened: &OpenedFile,
        source: &str,
        video: u32,
    ) -> io::Result<Arc<Keyframes>> {
        let key = format!("{source}:{video}");
        if let Some(found) = self
            .indexes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return Ok(found.clone());
        }
        let mut file = opened.try_clone_std()?;
        let read = tokio::task::spawn_blocking(move || keyframes::read(&mut file, video))
            .await
            .map_err(|_| io::Error::other("keyframe index worker failed"))?;
        let frames = Arc::new(read.map_err(|error| match error.kind() {
            // A layout without a usable index is not a changed source.
            io::ErrorKind::InvalidData
            | io::ErrorKind::Unsupported
            | io::ErrorKind::UnexpectedEof => coded(io::ErrorKind::Unsupported, "hls_unavailable"),
            _ => error,
        })?);
        remember(&self.indexes, key, frames.clone());
        Ok(frames)
    }

    async fn spawn(
        &self,
        opened: OpenedFile,
        request: &HlsRequest,
        segment: Segment,
        last: bool,
        hevc: bool,
    ) -> io::Result<PipeBody> {
        let permit = tokio::time::timeout(
            Duration::from_millis(250),
            self.slots.clone().acquire_owned(),
        )
        .await
        .map_err(|_| coded(io::ErrorKind::WouldBlock, "hls_capacity_reached"))?
        .map_err(|_| io::Error::other("hls service stopped"))?;
        let mut file = opened.try_clone_std()?;
        std::io::Seek::rewind(&mut file)?;
        let mut command = Command::new("ffmpeg");
        command.args(arguments(request, segment, last, hevc));
        spawn_pipe(
            opened,
            file,
            permit,
            command,
            PipeLabels {
                initialization: Duration::from_secs(15),
                timeout: "hls segment initialization timeout",
                failed: "hls segment failed",
                interrupted: "hls segment interrupted",
                stalled: "hls segment stalled",
                diagnostics: Some("hls_segment_failed"),
            },
        )
        .await
    }

    /// `source` identifies this version of the file (asset, size, mtime).
    pub async fn serve(
        &self,
        opened: OpenedFile,
        source: String,
        request: HlsRequest,
        method: Method,
    ) -> io::Result<Response<BoxStreamBody>> {
        if method != Method::GET && method != Method::HEAD {
            return Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(crate::stream::empty_body())
                .map_err(io::Error::other);
        }
        opened.verify_unchanged_sync()?;
        let frames = self.keyframes(&opened, &source, request.video).await?;
        let cut = playlist::segments(&frames, playlist::TARGET_MS);
        let profile = request.profile();
        match request.resource {
            Resource::Playlist => {
                let text = playlist::render(&cut, &format!("{profile}-init.mp4"), |i| {
                    format!("{profile}-{i}.m4s")
                });
                bytes_response(
                    &method,
                    "application/vnd.apple.mpegurl",
                    "private, no-cache",
                    Bytes::from(text),
                )
            }
            Resource::Init => {
                let key = format!("{source}:{profile}");
                let cached = self
                    .inits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&key)
                    .cloned();
                let init = match cached {
                    Some(init) => init,
                    None => {
                        let first = *cut
                            .first()
                            .ok_or_else(|| coded(io::ErrorKind::Unsupported, "hls_unavailable"))?;
                        let pipe = self
                            .spawn(opened, &request, first, cut.len() == 1, frames.hevc)
                            .await?;
                        let init = initialization(pipe).await?;
                        remember(&self.inits, key, init.clone());
                        init
                    }
                };
                bytes_response(&method, "video/mp4", SEGMENT_CACHE, init)
            }
            Resource::Segment(index) => {
                let segment = *cut
                    .get(index as usize)
                    .ok_or_else(|| coded(io::ErrorKind::InvalidInput, "hls_segment_unknown"))?;
                let last = index as usize + 1 == cut.len();
                let response = Response::builder()
                    .header(header::CONTENT_TYPE, "video/mp4")
                    .header(header::CACHE_CONTROL, SEGMENT_CACHE);
                if method == Method::HEAD {
                    return response
                        .body(crate::stream::empty_body())
                        .map_err(io::Error::other);
                }
                let window = request.audio.is_none().then_some(Window {
                    start_ms: segment.start_ms,
                    end_ms: (!last).then_some(segment.end_ms),
                });
                let pipe = self
                    .spawn(opened, &request, segment, last, frames.hevc)
                    .await?;
                response
                    .body(MediaBody::new(pipe, window).boxed())
                    .map_err(io::Error::other)
            }
        }
    }
}

/// Reads FFmpeg's output until `moov` is complete; dropping the pipe then stops it.
async fn initialization(mut body: PipeBody) -> io::Result<Bytes> {
    let mut filter = BoxFilter::new(Part::Init, None);
    let mut out = Vec::new();
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame?.into_data() {
            filter.push(&data, &mut out)?;
        }
        if filter.done() {
            return Ok(Bytes::from(out));
        }
        if out.len() > MAX_INIT_BYTES {
            break;
        }
    }
    Err(coded(io::ErrorKind::Unsupported, "hls_unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_names_are_strict_and_canonical() {
        let ok = |raw: &str, video, audio, resource| {
            let expected = HlsRequest {
                video,
                audio,
                resource,
            };
            assert_eq!(request(raw), Some(expected), "{raw}");
        };
        ok("hls-v0.m3u8", 0, None, Resource::Playlist);
        ok("hls-v0-a1.m3u8", 0, Some(1), Resource::Playlist);
        ok("hls-v2-init.mp4", 2, None, Resource::Init);
        ok("hls-v2-a3-init.mp4", 2, Some(3), Resource::Init);
        ok("hls-v0-0.m4s", 0, None, Resource::Segment(0));
        ok(
            "hls-v0-a12-999999.m4s",
            0,
            Some(12),
            Resource::Segment(999_999),
        );
        for raw in [
            "hls-v0",
            "hls-v.m3u8",
            "hls-v00.m3u8",
            "hls-v0-a01-1.m4s",
            "hls-v0-01.m4s",
            "hls-v0--1.m4s",
            "hls-v0-1000000.m4s",
            "hls-v0-1.mp4",
            "hls-v0-a.m3u8",
            "hls-v0.m3u8/../x",
            "hls-v0-+1.m4s",
        ] {
            assert_eq!(request(raw), None, "{raw}");
        }
    }

    #[test]
    fn video_and_audio_segments_copy_on_the_source_timeline() {
        let segment = Segment {
            start_ms: 18_042,
            end_ms: 24_000,
        };
        let video = arguments(&request("hls-v0-3.m4s").unwrap(), segment, false, false).join(" ");
        let hevc = arguments(&request("hls-v0-3.m4s").unwrap(), segment, false, true).join(" ");
        assert!(hevc.contains("-an -tag:v hvc1") && !video.contains("hvc1"));
        assert!(video.contains("-ss 18.042 -copyts -fd 0 -i fd: -to 26.000 -map 0:0 -an"));
        assert!(!video.contains("-frag_duration"));
        let audio = arguments(&request("hls-v0-a1-3.m4s").unwrap(), segment, false, true).join(" ");
        assert!(!audio.contains("hvc1"));
        assert!(audio.contains("-i fd: -to 24.000 -map 0:1 -vn -copypriorss 0"));
        for args in [&video, &audio] {
            assert!(args.contains("-c copy") && args.contains("frag_discont"));
            assert!(args.ends_with("-f mp4 pipe:1"));
        }
        let last = arguments(&request("hls-v0-a1-4.m4s").unwrap(), segment, true, false).join(" ");
        assert!(!last.contains("-to "));
    }
}
