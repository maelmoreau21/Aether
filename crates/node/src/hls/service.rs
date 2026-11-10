//! `prepared/hls-…` on the Node. Two kinds of media playlists, both cut on
//! the keyframes of video stream `v`:
//!
//! - video rendition `hls-v{v}.m3u8` (+ `-init.mp4`, `-{n}.m4s`), cut on fragments;
//! - audio rendition `hls-v{v}-a{a}.m3u8` (same suffixes), cut on timestamps.
//!
//! Two more are encoded per segment on the same cuts: AAC conversions of audio
//! native players cannot decode (`hls-v{v}-aac{a}…`) and reduced H.264 video
//! renditions (`hls-v{v}-h{height}…`, see `renditions`).
//!
//! Separate renditions are what lets both cuts be exact: B-frames make video
//! decode order differ from presentation order, which no single timestamp cut
//! can respect for video and audio at once. They also expose every audio track
//! to native players. Segments are independent packet copies, served in any
//! order without a session or temporary files.
use super::{
    keyframes::{self, Keyframes},
    playlist::{self, Segment},
    renditions,
    segments::{BoxFilter, MediaBody, Part, TIMELINE_OFFSET_MS, Window},
};
use crate::{
    BoxStreamBody, OpenedFile,
    transcode::{self, EncoderPlan, FLAG_TONEMAP, TranscodeRequest, Transcodes, coded},
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

/// A reduced video rendition (`hls-v{v}-h{height}…`, `-t{height}…` when the
/// HDR picture is tone mapped to SDR): one of `renditions::RENDITIONS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encode {
    pub height: u32,
    pub tonemap: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HlsRequest {
    /// FFmpeg stream index of the video stream whose keyframes cut the timeline.
    pub video: u32,
    /// `Some` for an audio rendition (that stream index), `None` for the video one.
    pub audio: Option<u32>,
    /// The audio track is served from its AAC conversion (`hls-v{v}-aac{a}…`).
    pub convert: bool,
    /// The video is re-encoded in H.264 instead of copied.
    pub encode: Option<Encode>,
    pub resource: Resource,
}

impl HlsRequest {
    /// Whether serving it spends encoder time (a conversion or a re-encode).
    pub fn encodes(&self) -> bool {
        self.convert || self.encode.is_some()
    }
    fn profile(&self) -> String {
        match (self.audio, self.convert, self.encode) {
            (Some(audio), true, _) => format!("hls-v{}-aac{audio}", self.video),
            (Some(audio), false, _) => format!("hls-v{}-a{audio}", self.video),
            (None, _, Some(encode)) => format!(
                "hls-v{}-{}{}",
                self.video,
                if encode.tonemap { 't' } else { 'h' },
                encode.height
            ),
            (None, _, None) => format!("hls-v{}", self.video),
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
    let reduced = match (rest.strip_prefix("-h"), rest.strip_prefix("-t")) {
        (Some(after), _) => Some((false, after)),
        (_, Some(after)) => Some((true, after)),
        _ => None,
    };
    let (encode, rest) = match reduced {
        Some((tonemap, after)) => {
            let (height, rest) = number(after, 4)?;
            renditions::rendition(height)?;
            (Some(Encode { height, tonemap }), rest)
        }
        None => (None, rest),
    };
    let (convert, after) = match rest.strip_prefix("-aac") {
        Some(after) => (true, Some(after)),
        None => (false, rest.strip_prefix("-a")),
    };
    let (audio, rest) = match after {
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
        convert,
        encode,
        resource,
    };
    // Also refuses a reduced rendition combined with an audio track.
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

/// Converted audio is AAC at this rate, in frames of this many samples.
const AAC_RATE: u64 = 48_000;
const AAC_FRAME: u64 = 1_024;
/// Frames encoded before a segment and dropped: the encoder warms up on the
/// same signal as the end of the previous segment (half a second).
const AAC_PREROLL: u64 = 24;
/// Frames encoded after it: the last kept frame is complete, not padded.
const AAC_POSTROLL: u64 = 3;
/// Decoding starts this much before the first encoded sample (codec warm-up).
const AAC_DECODE_LEAD_MS: u64 = 500;

/// First AAC frame (on the film-wide grid) at or after `ms`.
fn aac_frame(ms: u64) -> u64 {
    (ms * AAC_RATE).div_ceil(1000 * AAC_FRAME)
}

/// FFmpeg arguments for one segment of an AAC conversion (DTS, TrueHD, PCM…).
///
/// Every segment is encoded on its own, yet the joined rendition reads as one
/// encode: AAC frames sit on one grid of 1024 samples from the film's start.
/// The encoder is fed from a grid point `AAC_PREROLL` frames early (`atrim`
/// in samples), so its packets land on that grid, and the `noise` bitstream
/// filter keeps exactly the packets of frames `[aac_frame(start),
/// aac_frame(end))`. Neighbouring segments thus share no frame and miss none,
/// and each starts warmed up instead of on a fresh encoder's priming.
pub fn convert_arguments(audio: u32, segment: Segment, last: bool) -> Vec<String> {
    let owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
    let first = aac_frame(segment.start_ms);
    let fed = first.saturating_sub(AAC_PREROLL);
    let seek_ms = (fed * AAC_FRAME * 1000 / AAC_RATE).saturating_sub(AAC_DECODE_LEAD_MS);
    let mut trim = format!("atrim=start_pts={}", fed * AAC_FRAME);
    let mut keep = format!("lt(pts\\,{})", first * AAC_FRAME);
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
        &seconds(seek_ms),
        "-copyts",
        "-fd",
        "0",
        "-i",
        "fd:",
    ]);
    if !last {
        let end = aac_frame(segment.end_ms);
        let fed_end = end + AAC_POSTROLL;
        trim.push_str(&format!(":end_pts={}", fed_end * AAC_FRAME));
        keep.push_str(&format!("+gte(pts\\,{})", end * AAC_FRAME));
        // Stops reading once the trim is done (one second of slack).
        let stop_ms = (fed_end * AAC_FRAME * 1000).div_ceil(AAC_RATE) + 1000;
        args.extend(owned(&["-to", &seconds(stop_ms)]));
    }
    args.extend(owned(&[
        "-map",
        &format!("0:{audio}"),
        "-vn",
        "-sn",
        "-dn",
        // One rate for the whole grid, the first sample pinned on it (a track
        // starting a few samples late is padded), 5.1 at most (7.1 folded down).
        "-af",
        &format!(
            "aresample={AAC_RATE}:first_pts={}:min_comp=0.00001,\
             aformat=channel_layouts=5.1|stereo|mono,{trim}",
            fed * AAC_FRAME
        ),
        "-c:a",
        "aac",
        "-b:a",
        "320k",
        // Packet times here are the source's, before `-output_ts_offset`.
        "-bsf:a",
        &format!("noise=drop={keep}"),
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
        "-movflags",
        "frag_keyframe+empty_moov+default_base_moof+frag_discont",
        "-frag_duration",
        "2000000",
        "-f",
        "mp4",
        "pipe:1",
    ]));
    args
}

/// The transcoding request a reduced rendition stands for.
fn transcode_request(video: u32, encode: Encode) -> TranscodeRequest {
    let rendition = renditions::rendition(encode.height).unwrap_or(renditions::RENDITIONS[0]);
    TranscodeRequest {
        video,
        audio: None,
        subtitle: None,
        height: rendition.height,
        kbps: rendition.kbps,
        flags: if encode.tonemap { FLAG_TONEMAP } else { 0 },
        position_ms: 0,
    }
}

/// FFmpeg arguments for one segment of a reduced rendition. Decoding starts
/// on the segment's own keyframe and `-to` stops before the next segment's,
/// so each encode holds exactly the frames shown in `[start, end)` and opens
/// on an IDR frame. `-copyts`, the shared offset and passthrough timing keep
/// the source timeline: segments encoded apart play as one continuous stream.
pub fn encode_arguments(
    request: &HlsRequest,
    encode: Encode,
    segment: Segment,
    last: bool,
    plan: &EncoderPlan,
) -> Vec<String> {
    let owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
    let transcode = transcode_request(request.video, encode);
    let rendition = renditions::rendition(encode.height).unwrap_or(renditions::RENDITIONS[0]);
    let mut args = owned(&["-v", "error", "-nostdin"]);
    args.extend(transcode::input_arguments(plan, &transcode));
    args.extend(owned(&[
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
    ]));
    if !last {
        args.extend(owned(&["-to", &seconds(segment.end_ms)]));
    }
    args.extend(transcode::video_arguments(
        &transcode,
        plan,
        &renditions::scale_filter(rendition),
        false,
    ));
    args.extend(owned(&[
        "-fps_mode",
        "passthrough",
        "-an",
        "-sn",
        "-dn",
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
        "-movflags",
        "frag_keyframe+empty_moov+default_base_moof+frag_discont",
        "-frag_duration",
        "2000000",
        "-f",
        "mp4",
        "pipe:1",
    ]));
    args
}

/// How long an encoded segment waits for a free encoder: a download asks for
/// segments back to back and would rather wait than fail.
const ENCODE_SLOT_WAIT: Duration = Duration::from_secs(20);
/// How long a packet copy waits for one of the 6 slots. A copy takes about
/// 0.1 s (measured: 85 ms to the first byte of a 6 s 1080p segment), so the
/// queue drains quickly: a download fetching segments must not make a
/// viewer's segment fail after a quarter of a second.
const COPY_SLOT_WAIT: Duration = Duration::from_secs(3);

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

    /// A packet copy, an AAC conversion, or a video encode when `encoder` is
    /// given (reduced renditions, which share the transcoding slots).
    async fn spawn(
        &self,
        opened: OpenedFile,
        request: &HlsRequest,
        segment: Segment,
        last: bool,
        hevc: bool,
        encoder: Option<(&EncoderPlan, &Transcodes)>,
    ) -> io::Result<PipeBody> {
        // Converting a few seconds of audio costs about as little as a copy.
        let quick = |args| (args, self.slots.clone(), COPY_SLOT_WAIT);
        let (args, slots, wait) = match (request.encode, encoder, request.audio) {
            (Some(encode), Some((plan, transcodes)), _) => (
                encode_arguments(request, encode, segment, last, plan),
                transcodes.slots(),
                ENCODE_SLOT_WAIT,
            ),
            (_, _, Some(audio)) if request.convert => {
                quick(convert_arguments(audio, segment, last))
            }
            _ => quick(arguments(request, segment, last, hevc)),
        };
        let permit = tokio::time::timeout(wait, slots.acquire_owned())
            .await
            .map_err(|_| coded(io::ErrorKind::WouldBlock, "hls_capacity_reached"))?
            .map_err(|_| io::Error::other("hls service stopped"))?;
        let mut file = opened.try_clone_std()?;
        std::io::Seek::rewind(&mut file)?;
        let mut command = Command::new("ffmpeg");
        command.args(args);
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
    /// `transcodes` encodes reduced renditions, with its hardware and its slots.
    pub async fn serve(
        &self,
        opened: OpenedFile,
        source: String,
        request: HlsRequest,
        method: Method,
        transcodes: &Transcodes,
    ) -> io::Result<Response<BoxStreamBody>> {
        if method != Method::GET && method != Method::HEAD {
            return Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(crate::stream::empty_body())
                .map_err(io::Error::other);
        }
        opened.verify_unchanged_sync()?;
        // Checked from the playlist on, so a player drops a variant this Node cannot encode.
        let plan = match request.encode {
            Some(encode) => Some(
                transcodes
                    .encoder(&transcode_request(request.video, encode))
                    .await?,
            ),
            None => None,
        };
        let encoder = plan.as_ref().map(|plan| (plan, transcodes));
        let frames = self.keyframes(&opened, &source, request.video).await?;
        let mut cut = playlist::segments(&frames, playlist::TARGET_MS);
        if request.convert
            && let Some(first) = cut.first_mut()
        {
            // An encode covers the whole track, even before the first keyframe.
            first.start_ms = 0;
        }
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
                            .spawn(
                                opened,
                                &request,
                                first,
                                cut.len() == 1,
                                frames.hevc,
                                encoder,
                            )
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
                // Copied video reads past its bounds and is cut on fragments;
                // audio and encoded video are already cut by FFmpeg.
                let window =
                    (request.audio.is_none() && request.encode.is_none()).then_some(Window {
                        start_ms: segment.start_ms,
                        end_ms: (!last).then_some(segment.end_ms),
                    });
                let pipe = self
                    .spawn(opened, &request, segment, last, frames.hevc, encoder)
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
                convert: false,
                encode: None,
                resource,
            };
            assert_eq!(request(raw), Some(expected), "{raw}");
        };
        let converted = request("hls-v0-aac3-7.m4s").unwrap();
        assert!(converted.convert && converted.audio == Some(3));
        assert_eq!(converted.resource, Resource::Segment(7));
        assert_eq!(converted.component(), "hls-v0-aac3-7.m4s");
        for raw in ["hls-v0-aac.m3u8", "hls-v0-aac03.m3u8", "hls-v0-aaca1.m3u8"] {
            assert_eq!(request(raw), None, "{raw}");
        }
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
    fn converted_segments_keep_their_frames_of_the_aac_grid() {
        // 6 s to 12 s: frames 282 (6.016 s) to 563 excluded, fed from 258.
        let middle = Segment {
            start_ms: 6_000,
            end_ms: 12_000,
        };
        let args = convert_arguments(3, middle, false).join(" ");
        assert!(
            args.contains("-ss 5.004 -copyts -fd 0 -i fd: -to 13.075 -map 0:3 -vn"),
            "{args}"
        );
        assert!(args.contains("-af aresample=48000:first_pts=264192:min_comp=0.00001,aformat=channel_layouts=5.1|stereo|mono,atrim=start_pts=264192:end_pts=579584 -c:a aac -b:a 320k"), "{args}");
        assert!(
            args.contains(r"-bsf:a noise=drop=lt(pts\,288768)+gte(pts\,576512)"),
            "{args}"
        );
        assert!(args.contains("-output_ts_offset 10.000") && args.ends_with("-f mp4 pipe:1"));
        // The next segment starts on the frame this one stops before.
        let next = convert_arguments(
            3,
            Segment {
                start_ms: 12_000,
                end_ms: 18_000,
            },
            true,
        )
        .join(" ");
        assert!(next.contains(r"noise=drop=lt(pts\,576512) "), "{next}");
        assert!(!next.contains("-to ") && !next.contains("end_pts"));
        let first = convert_arguments(
            3,
            Segment {
                start_ms: 0,
                end_ms: 6_000,
            },
            false,
        )
        .join(" ");
        assert!(first.contains("-ss 0.000 ") && first.contains("atrim=start_pts=0:"));
    }

    #[test]
    fn reduced_renditions_have_strict_names() {
        let sdr = request("hls-v0-h720-12.m4s").unwrap();
        assert_eq!(
            sdr.encode,
            Some(Encode {
                height: 720,
                tonemap: false
            })
        );
        assert_eq!((sdr.audio, sdr.resource), (None, Resource::Segment(12)));
        assert!(sdr.encodes() && !request("hls-v0-a1.m3u8").unwrap().encodes());
        let hdr = request("hls-v1-t1080-init.mp4").unwrap();
        assert!(hdr.encode.unwrap().tonemap && hdr.resource == Resource::Init);
        assert_eq!(hdr.component(), "hls-v1-t1080-init.mp4");
        assert!(request("hls-v0-h1080.m3u8").is_some());
        for raw in [
            "hls-v0-h.m3u8",
            "hls-v0-h0720.m3u8",
            "hls-v0-h1440.m3u8",
            "hls-v0-h480.m3u8",
            "hls-v0-h720-a1.m3u8",
            "hls-v0-h720-aac1-3.m4s",
            "hls-v0-x720.m3u8",
            "hls-v0-t720t.m3u8",
        ] {
            assert_eq!(request(raw), None, "{raw}");
        }
    }

    fn plan(hardware: transcode::Hardware, encoder: &'static str, tonemap: bool) -> EncoderPlan {
        EncoderPlan {
            hardware,
            encoder,
            vaapi_device: "/dev/dri/renderD128".to_owned(),
            tonemap,
        }
    }

    #[test]
    fn reduced_segments_encode_exactly_their_window() {
        let segment = Segment {
            start_ms: 18_042,
            end_ms: 24_000,
        };
        let sdr = request("hls-v0-h1080-3.m4s").unwrap();
        let software = plan(transcode::Hardware::None, "libx264", false);
        let args = encode_arguments(&sdr, sdr.encode.unwrap(), segment, false, &software).join(" ");
        assert!(
            args.starts_with("-v error -nostdin -protocol_whitelist fd,pipe"),
            "{args}"
        );
        assert!(args.contains("-ss 18.042 -copyts -fd 0 -i fd: -to 24.000 -filter_complex [0:0]null,scale=w='min(iw,1920)':h='min(ih,1080)':force_original_aspect_ratio=decrease:force_divisible_by=2,format=yuv420p[video] -map [video]"), "{args}");
        assert!(args.contains("-c:v libx264 -b:v 8000k -maxrate 8000k -bufsize 16000k -profile:v high -level:v 4.1 -preset veryfast -fps_mode passthrough -an"), "{args}");
        assert!(!args.contains("zerolatency") && !args.contains("-force_key_frames"));
        assert!(args.contains("-output_ts_offset 10.000 -use_editlist 0"));
        assert!(args.contains("frag_discont -frag_duration 2000000"));
        assert!(args.ends_with("-f mp4 pipe:1"));
        let last = encode_arguments(&sdr, sdr.encode.unwrap(), segment, true, &software).join(" ");
        assert!(!last.contains("-to "));
        // HDR tone mapped, on a VAAPI Node: hardware decode, upload, encoder.
        let hdr = request("hls-v0-t720-3.m4s").unwrap();
        let vaapi = plan(transcode::Hardware::Vaapi, "h264_vaapi", true);
        let args = encode_arguments(&hdr, hdr.encode.unwrap(), segment, false, &vaapi).join(" ");
        assert!(
            args.starts_with("-v error -nostdin -hwaccel vaapi"),
            "{args}"
        );
        assert!(args.contains("tonemap=tonemap=hable"), "{args}");
        assert!(args.contains("format=nv12,hwupload[video]"), "{args}");
        assert!(args.contains("-c:v h264_vaapi -b:v 4000k"), "{args}");
        let request = transcode_request(0, hdr.encode.unwrap());
        assert!(request.has(FLAG_TONEMAP) && request.height == 720 && request.audio.is_none());
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
