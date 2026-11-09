//! Optional fallback transcoding for clients that cannot decode an original.
//!
//! Direct play stays the default: this path only runs when a client asks for it,
//! after the Core checked the account, the server setting and every stream index.
//! One FFmpeg process per request reads the confined source descriptor and writes a
//! fragmented MP4 (H.264 or the copied video, AAC or the copied audio) to its stdout.
//! Nothing is written beside the media and no file is cached: a seek replaces the
//! stream, exactly like the packet-copy fallback in `video_stream`.
use crate::{BoxStreamBody, OpenedFile};
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use std::{collections::HashSet, io, sync::Arc, time::Duration};
use tokio::{
    process::Command,
    sync::{OnceCell, Semaphore},
};

/// Output heights a client may ask for. `0` keeps (copies) the original video.
pub const HEIGHTS: [u32; 6] = [360, 480, 720, 1080, 1440, 2160];
/// Convert HDR (PQ/HLG) to SDR BT.709 before encoding.
pub const FLAG_TONEMAP: u8 = 1;
/// Even out loudness between scenes and titles.
pub const FLAG_NORMALIZE: u8 = 2;
/// Night mode: louder dialogue, softer explosions (dynamic range compression).
pub const FLAG_NIGHT: u8 = 4;
/// Keep 5.1 (AAC six channels) instead of the stereo downmix.
pub const FLAG_SURROUND: u8 = 8;
/// Copy the original audio packets (Dolby/DTS passthrough to a capable client).
pub const FLAG_COPY_AUDIO: u8 = 16;
const FLAG_MASK: u8 = 31;

/// One validated transcoding request, read from the resource name
/// `transcode-v{video}a{audio|x}s{subtitle|x}h{height}b{kbps}f{flags}-{milliseconds}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TranscodeRequest {
    pub video: u32,
    pub audio: Option<u32>,
    /// Image subtitle (PGS, VobSub, DVB) burnt into the picture.
    pub subtitle: Option<u32>,
    /// 0 copies the original video packets.
    pub height: u32,
    pub kbps: u32,
    pub flags: u8,
    pub position_ms: u32,
}

impl TranscodeRequest {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
    pub fn copies_video(&self) -> bool {
        self.height == 0
    }
    /// The exact resource name; parsing only accepts this canonical spelling.
    pub fn resource(&self) -> String {
        let optional = |value: Option<u32>| value.map_or_else(|| "x".to_owned(), |v| v.to_string());
        format!(
            "transcode-v{}a{}s{}h{}b{}f{}-{}",
            self.video,
            optional(self.audio),
            optional(self.subtitle),
            self.height,
            self.kbps,
            self.flags,
            self.position_ms
        )
    }
}

/// Strict parser: decimal fields without signs or leading zeros, every bound checked,
/// and incompatible options refused instead of being silently ignored.
pub fn request(raw: &str) -> Option<TranscodeRequest> {
    let (profile, position) = raw.strip_prefix("transcode-")?.rsplit_once('-')?;
    let mut rest = profile;
    let mut number = |tag: char, optional: bool| -> Option<Option<u32>> {
        rest = rest.strip_prefix(tag)?;
        if optional && let Some(after) = rest.strip_prefix('x') {
            rest = after;
            return Some(None);
        }
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let (value, after) = rest.split_at(digits);
        rest = after;
        Some(Some(value.parse().ok()?))
    };
    let video = number('v', false)??;
    let audio = number('a', true)?;
    let subtitle = number('s', true)?;
    let height = number('h', false)??;
    let kbps = number('b', false)??;
    let flags = u8::try_from(number('f', false)??).ok()?;
    if !rest.is_empty()
        || position.is_empty()
        || position.len() > 8
        || !position.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let parsed = TranscodeRequest {
        video,
        audio,
        subtitle,
        height,
        kbps,
        flags,
        position_ms: position.parse().ok()?,
    };
    let valid = parsed.position_ms < 86_400_000
        && (height == 0 || HEIGHTS.contains(&height))
        && (200..=120_000).contains(&kbps)
        && flags & !FLAG_MASK == 0
        // Copied video cannot be tone mapped or carry a burnt subtitle.
        && (height != 0 || (flags & FLAG_TONEMAP == 0 && subtitle.is_none()))
        // Audio filters and channel choices need decoded audio.
        && (flags & FLAG_COPY_AUDIO == 0
            || flags & (FLAG_NORMALIZE | FLAG_NIGHT | FLAG_SURROUND) == 0)
        && (audio.is_some() || flags & (FLAG_COPY_AUDIO | FLAG_NORMALIZE | FLAG_NIGHT | FLAG_SURROUND) == 0)
        // A pure remux of both streams is the packet-copy fallback's job.
        && !(height == 0 && (audio.is_none() || flags & FLAG_COPY_AUDIO != 0));
    (valid && parsed.resource() == raw).then_some(parsed)
}

/// Hardware the administrator assigned to this Node's encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hardware {
    None,
    Nvenc,
    Qsv,
    Vaapi,
    Amf,
}
impl Hardware {
    fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "software" => Self::None,
            "nvenc" | "nvidia" | "cuda" => Self::Nvenc,
            "qsv" | "quicksync" | "intel" => Self::Qsv,
            "vaapi" => Self::Vaapi,
            "amf" | "amd" => Self::Amf,
            _ => return None,
        })
    }
    fn encoder(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Nvenc => Some("h264_nvenc"),
            Self::Qsv => Some("h264_qsv"),
            Self::Vaapi => Some("h264_vaapi"),
            Self::Amf => Some("h264_amf"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TranscodeConfig {
    pub enabled: bool,
    pub hardware: Hardware,
    pub vaapi_device: String,
    pub sessions: usize,
}
impl Default for TranscodeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            hardware: Hardware::None,
            vaapi_device: "/dev/dri/renderD128".to_owned(),
            sessions: 2,
        }
    }
}
impl TranscodeConfig {
    /// `MEDIA_TRANSCODE_ENABLED` (true), `MEDIA_TRANSCODE_HARDWARE` (none, nvenc, qsv,
    /// vaapi, amf), `MEDIA_TRANSCODE_VAAPI_DEVICE` (a `/dev/dri/` render node) and
    /// `MEDIA_TRANSCODE_MAX_SESSIONS` (1 to 16). Invalid values disable transcoding
    /// with a logged reason rather than guessing.
    pub fn from_env() -> Self {
        Self::from_values(|name| std::env::var(name).ok())
    }
    fn from_values(read: impl Fn(&str) -> Option<String>) -> Self {
        let mut config = Self::default();
        let mut invalid = Vec::new();
        if let Some(value) = read("MEDIA_TRANSCODE_ENABLED") {
            match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => {}
                "0" | "false" | "no" | "off" => config.enabled = false,
                _ => invalid.push("MEDIA_TRANSCODE_ENABLED"),
            }
        }
        if let Some(value) = read("MEDIA_TRANSCODE_HARDWARE") {
            match Hardware::parse(&value) {
                Some(hardware) => config.hardware = hardware,
                None => invalid.push("MEDIA_TRANSCODE_HARDWARE"),
            }
        }
        if let Some(value) = read("MEDIA_TRANSCODE_VAAPI_DEVICE") {
            let device = value.trim();
            match device.strip_prefix("/dev/dri/") {
                Some(node)
                    if !node.is_empty()
                        && node
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') =>
                {
                    config.vaapi_device = device.to_owned();
                }
                _ => invalid.push("MEDIA_TRANSCODE_VAAPI_DEVICE"),
            }
        }
        if let Some(value) = read("MEDIA_TRANSCODE_MAX_SESSIONS") {
            match value.trim().parse::<usize>() {
                Ok(sessions @ 1..=16) => config.sessions = sessions,
                _ => invalid.push("MEDIA_TRANSCODE_MAX_SESSIONS"),
            }
        }
        for variable in &invalid {
            eprintln!(
                "{}",
                serde_json::json!({"event":"transcode_configuration_invalid","variable":variable})
            );
        }
        if !invalid.is_empty() {
            config.enabled = false;
        }
        config
    }
}

/// What the installed FFmpeg can actually do, read once from `-encoders`/`-filters`.
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    encoders: HashSet<String>,
    filters: HashSet<String>,
}
impl Capabilities {
    fn parse(encoders: &str, filters: &str) -> Self {
        // Both listings put flags first and the name second, after a "------" legend.
        let names = |listing: &str| {
            listing
                .lines()
                .skip_while(|line| !line.trim_start().starts_with("---"))
                .skip(1)
                .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
                .collect::<HashSet<_>>()
        };
        Self {
            encoders: names(encoders),
            filters: names(filters),
        }
    }
    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders.contains(name)
    }
    pub fn has_filter(&self, name: &str) -> bool {
        self.filters.contains(name)
    }
    pub fn can_tonemap(&self) -> bool {
        self.has_filter("zscale") && self.has_filter("tonemap")
    }
}

/// The encoder actually used for a request, after checking the installed FFmpeg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderPlan {
    pub hardware: Hardware,
    pub encoder: &'static str,
    pub vaapi_device: String,
    pub tonemap: bool,
}

/// Prefers the configured hardware encoder, then libx264, then OpenH264.
pub fn plan(
    config: &TranscodeConfig,
    capabilities: &Capabilities,
    request: &TranscodeRequest,
) -> io::Result<EncoderPlan> {
    let hardware = config
        .hardware
        .encoder()
        .filter(|encoder| capabilities.has_encoder(encoder))
        .map(|encoder| (config.hardware, encoder));
    let chosen = hardware.or_else(|| {
        ["libx264", "libopenh264"]
            .into_iter()
            .find(|encoder| capabilities.has_encoder(encoder))
            .map(|encoder| (Hardware::None, encoder))
    });
    let audio_ready =
        request.audio.is_none() || request.has(FLAG_COPY_AUDIO) || capabilities.has_encoder("aac");
    match chosen {
        Some((hardware, encoder)) if audio_ready => Ok(EncoderPlan {
            hardware,
            encoder,
            vaapi_device: config.vaapi_device.clone(),
            tonemap: request.has(FLAG_TONEMAP) && capabilities.can_tonemap(),
        }),
        // Audio-only transcoding over copied video needs no video encoder.
        None if request.copies_video() && audio_ready => Ok(EncoderPlan {
            hardware: Hardware::None,
            encoder: "copy",
            vaapi_device: config.vaapi_device.clone(),
            tonemap: false,
        }),
        _ => Err(coded(io::ErrorKind::Unsupported, "transcode_unavailable")),
    }
}

/// An I/O error carrying the stable code the Core and the web client display.
#[derive(Debug)]
pub struct CodedError(pub &'static str);
impl std::fmt::Display for CodedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}
impl std::error::Error for CodedError {}
pub fn coded(kind: io::ErrorKind, code: &'static str) -> io::Error {
    io::Error::new(kind, CodedError(code))
}

fn level(height: u32) -> &'static str {
    match height {
        0..=1080 => "4.1",
        1081..=1440 => "5.0",
        _ => "5.1",
    }
}

/// Codec string of the produced H.264 (High profile at the level `arguments` sets).
pub fn h264_codec(height: u32) -> &'static str {
    match level(height) {
        "4.1" => "avc1.640029",
        "5.0" => "avc1.640032",
        _ => "avc1.640033",
    }
}

/// FFmpeg arguments after the input. Pure, so every combination is unit-tested.
pub fn arguments(request: &TranscodeRequest, plan: &EncoderPlan) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let push = |args: &mut Vec<String>, values: &[&str]| {
        args.extend(values.iter().map(|value| (*value).to_owned()));
    };
    if request.copies_video() {
        push(
            &mut args,
            &["-map", &format!("0:{}", request.video), "-c:v", "copy"],
        );
    } else {
        // HDR → SDR BT.709 (Hable curve, as Jellyfin and Plex do in software).
        let tonemap = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
             tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv";
        let scale = format!("scale=w=-2:h='min(ih,{})'", request.height);
        let pixels = match plan.hardware {
            Hardware::Qsv | Hardware::Vaapi => "format=nv12",
            _ => "format=yuv420p",
        };
        let upload = if plan.hardware == Hardware::Vaapi {
            ",hwupload"
        } else {
            ""
        };
        let base = if plan.tonemap { tonemap } else { "null" };
        let graph = match request.subtitle {
            // The picture is tone mapped before the 8-bit subtitle bitmap is drawn on it.
            Some(subtitle) => format!(
                "[0:{video}]{base}[base];[base][0:{subtitle}]overlay=eof_action=pass:repeatlast=0[burnt];\
                 [burnt]{scale},{pixels}{upload}[video]",
                video = request.video
            ),
            None => format!(
                "[0:{video}]{base},{scale},{pixels}{upload}[video]",
                video = request.video
            ),
        };
        push(&mut args, &["-filter_complex", &graph, "-map", "[video]"]);
        let rate = format!("{}k", request.kbps);
        let buffer = format!("{}k", request.kbps.saturating_mul(2));
        push(
            &mut args,
            &[
                "-c:v",
                plan.encoder,
                "-b:v",
                &rate,
                "-maxrate",
                &rate,
                "-bufsize",
                &buffer,
            ],
        );
        if plan.encoder != "libopenh264" {
            push(
                &mut args,
                &["-profile:v", "high", "-level:v", level(request.height)],
            );
        }
        match plan.encoder {
            "libx264" => push(&mut args, &["-preset", "veryfast", "-tune", "zerolatency"]),
            "h264_nvenc" => push(
                &mut args,
                &["-preset", "p4", "-rc", "vbr", "-forced-idr", "1"],
            ),
            "h264_qsv" => push(&mut args, &["-preset", "veryfast"]),
            "h264_amf" => push(&mut args, &["-quality", "speed"]),
            _ => {}
        }
        // A keyframe every two seconds keeps seeks and the first fragment short.
        push(&mut args, &["-force_key_frames", "expr:gte(t,n_forced*2)"]);
    }
    match request.audio {
        None => push(&mut args, &["-an"]),
        Some(audio) => {
            push(&mut args, &["-map", &format!("0:{audio}")]);
            if request.has(FLAG_COPY_AUDIO) {
                push(&mut args, &["-c:a", "copy"]);
            } else {
                let surround = request.has(FLAG_SURROUND);
                let mut filters = vec!["aresample=async=1".to_owned()];
                if request.has(FLAG_NIGHT) {
                    // Compress peaks, then lift the quiet passages where dialogue lives.
                    filters.push(
                        "acompressor=threshold=0.0625:ratio=6:attack=5:release=150:makeup=3"
                            .to_owned(),
                    );
                }
                if request.has(FLAG_NORMALIZE) {
                    filters.push("dynaudnorm=f=250:g=15:p=0.9".to_owned());
                }
                if request.has(FLAG_NIGHT) || request.has(FLAG_NORMALIZE) {
                    filters.push("alimiter=limit=0.95".to_owned());
                }
                push(
                    &mut args,
                    &[
                        "-af",
                        &filters.join(","),
                        "-c:a",
                        "aac",
                        "-ac",
                        if surround { "6" } else { "2" },
                        "-b:a",
                        if surround { "384k" } else { "192k" },
                    ],
                );
            }
        }
    }
    push(
        &mut args,
        &[
            "-sn",
            "-dn",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-max_muxing_queue_size",
            "1024",
            "-movflags",
            "frag_keyframe+delay_moov+default_base_moof",
            "-frag_duration",
            "1000000",
            "-f",
            "mp4",
            "pipe:1",
        ],
    );
    args
}

/// Input-side arguments: hardware decoding when an accelerator is configured.
fn input_arguments(plan: &EncoderPlan, request: &TranscodeRequest) -> Vec<String> {
    let mut args = Vec::new();
    if request.copies_video() {
        return args;
    }
    match plan.hardware {
        Hardware::Nvenc => args.extend(["-hwaccel".to_owned(), "cuda".to_owned()]),
        Hardware::Vaapi => args.extend([
            "-hwaccel".to_owned(),
            "vaapi".to_owned(),
            "-hwaccel_device".to_owned(),
            plan.vaapi_device.clone(),
            "-init_hw_device".to_owned(),
            format!("vaapi=va:{}", plan.vaapi_device),
            "-filter_hw_device".to_owned(),
            "va".to_owned(),
        ]),
        // QuickSync and AMF encode system-memory frames; decoding stays in software,
        // which keeps the CPU filters (tone mapping, subtitles) on one code path.
        Hardware::Qsv | Hardware::Amf | Hardware::None => {}
    }
    args
}

pub struct Transcodes {
    config: TranscodeConfig,
    slots: Arc<Semaphore>,
    capabilities: OnceCell<Capabilities>,
}
impl Default for Transcodes {
    fn default() -> Self {
        Self::new(TranscodeConfig::from_env())
    }
}
impl Transcodes {
    pub fn new(config: TranscodeConfig) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(config.sessions)),
            config,
            capabilities: OnceCell::new(),
        }
    }
    pub fn config(&self) -> &TranscodeConfig {
        &self.config
    }
    /// Reads the FFmpeg listings once; a missing FFmpeg yields empty capabilities.
    pub async fn capabilities(&self) -> &Capabilities {
        self.capabilities
            .get_or_init(|| async {
                let listing = |flag: &'static str| async move {
                    let output = tokio::time::timeout(
                        Duration::from_secs(10),
                        Command::new("ffmpeg")
                            .args(["-hide_banner", "-nostdin", flag])
                            .stdin(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .output(),
                    )
                    .await;
                    match output {
                        Ok(Ok(output)) if output.status.success() => {
                            String::from_utf8_lossy(&output.stdout).into_owned()
                        }
                        _ => String::new(),
                    }
                };
                let capabilities =
                    Capabilities::parse(&listing("-encoders").await, &listing("-filters").await);
                let h264: Vec<&str> = [
                    "libx264",
                    "libopenh264",
                    "h264_nvenc",
                    "h264_qsv",
                    "h264_vaapi",
                    "h264_amf",
                ]
                .into_iter()
                .filter(|name| capabilities.has_encoder(name))
                .collect();
                eprintln!(
                    "{}",
                    serde_json::json!({"event":"transcode_capabilities","hardware":format!("{:?}",self.config.hardware),
                        "h264":h264,"aac":capabilities.has_encoder("aac"),"tonemap":capabilities.can_tonemap()})
                );
                capabilities
            })
            .await
    }

    pub fn serve<'a, B>(
        &'a self,
        opened: OpenedFile,
        request: TranscodeRequest,
        http: &Request<B>,
    ) -> impl Future<Output = io::Result<Response<BoxStreamBody>>> + Send + 'a {
        let method = http.method().clone();
        let has_range = http.headers().contains_key(header::RANGE);
        async move {
            if !self.config.enabled {
                return Err(coded(io::ErrorKind::Unsupported, "transcode_disabled"));
            }
            opened.verify_unchanged_sync()?;
            let plan = plan(&self.config, self.capabilities().await, &request)?;
            let response = Response::builder()
                .header(header::CONTENT_TYPE, "video/mp4")
                .header(header::CACHE_CONTROL, "private, no-store")
                .header(header::ACCEPT_RANGES, "none")
                .header("x-media-transcode-encoder", plan.encoder)
                .header(
                    "x-media-transcode-tonemap",
                    if !request.has(FLAG_TONEMAP) {
                        "off"
                    } else if plan.tonemap {
                        "on"
                    } else {
                        "unavailable"
                    },
                );
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
            .map_err(|_| coded(io::ErrorKind::WouldBlock, "transcode_capacity_reached"))?
            .map_err(|_| io::Error::other("transcode service stopped"))?;
            let mut file = opened.try_clone_std()?;
            std::io::Seek::rewind(&mut file)?;
            let mut command = Command::new("ffmpeg");
            command.args(["-v", "error", "-nostdin"]);
            command.args(input_arguments(&plan, &request));
            command.args([
                "-protocol_whitelist",
                "fd,pipe",
                "-format_whitelist",
                "matroska,webm,mov,mp4,obu,mpegts,avi",
                "-max_alloc",
                "268435456",
                "-ss",
                &format!("{:.3}", f64::from(request.position_ms) / 1000.0),
                "-copyts",
                "-start_at_zero",
                "-fd",
                "0",
                "-i",
                "fd:",
            ]);
            command.args(arguments(&request, &plan));
            eprintln!(
                "{}",
                serde_json::json!({"event":"transcode_start","resource":request.resource(),"encoder":plan.encoder,"tonemap":plan.tonemap})
            );
            let body = crate::video_stream::spawn_pipe(
                opened,
                file,
                permit,
                command,
                crate::video_stream::PipeLabels {
                    // Hardware initialization and tone mapping a 4K keyframe take longer
                    // than a packet copy before the first fragment leaves FFmpeg.
                    initialization: Duration::from_secs(25),
                    timeout: "transcode initialization timeout",
                    failed: "transcode failed",
                    interrupted: "transcode interrupted",
                    stalled: "transcode stalled",
                    diagnostics: Some("transcode_failed"),
                },
            )
            .await?;
            response.body(body.boxed()).map_err(io::Error::other)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &str) -> TranscodeRequest {
        request(raw).unwrap_or_else(|| panic!("{raw} should parse"))
    }
    fn software(tonemap: bool) -> EncoderPlan {
        EncoderPlan {
            hardware: Hardware::None,
            encoder: "libx264",
            vaapi_device: "/dev/dri/renderD128".into(),
            tonemap,
        }
    }

    #[test]
    fn resource_names_round_trip_and_reject_every_ambiguous_spelling() {
        let full = parsed("transcode-v0a2s5h720b4000f3-125000");
        assert_eq!(
            full,
            TranscodeRequest {
                video: 0,
                audio: Some(2),
                subtitle: Some(5),
                height: 720,
                kbps: 4000,
                flags: 3,
                position_ms: 125_000
            }
        );
        assert_eq!(full.resource(), "transcode-v0a2s5h720b4000f3-125000");
        assert_eq!(parsed("transcode-v1axsxh1080b8000f0-0").audio, None);
        assert!(parsed("transcode-v0a1sxh0b4000f0-0").copies_video());
        for raw in [
            "transcode-",
            "transcode-v0a1sxh720b4000f0",
            "transcode-v0a1sxh720b4000f0-",
            "transcode-v0a1sxh720b4000f0-86400000",
            "transcode-v00a1sxh720b4000f0-0",
            "transcode-v0a01sxh720b4000f0-0",
            "transcode-v0a1sxh721b4000f0-0",
            "transcode-v0a1sxh720b100f0-0",
            "transcode-v0a1sxh720b4000f32-0",
            "transcode-v0a1sxh720b4000f0-01",
            "transcode-v0a1sxh720b4000f0-+1",
            "transcode-v0a1sxh720b4000f0-0/../x",
            "transcode-v0a1sxh720b4000f0x-0",
            "transcode-v-1a1sxh720b4000f0-0",
            "transcode-a1v0sxh720b4000f0-0",
            "transcode-v9999999999a1sxh720b4000f0-0",
            // Copied video cannot be tone mapped nor carry a burnt subtitle.
            "transcode-v0a1sxh0b4000f1-0",
            "transcode-v0a1s3h0b4000f0-0",
            // Copied audio refuses audio filters; filters need an audio track.
            "transcode-v0a1sxh720b4000f18-0",
            "transcode-v0axsxh720b4000f2-0",
            // Copying both streams is the packet-copy fallback, not a transcode.
            "transcode-v0a1sxh0b4000f16-0",
            "transcode-v0axsxh0b4000f0-0",
        ] {
            assert_eq!(request(raw), None, "{raw}");
        }
    }

    #[test]
    fn capabilities_read_names_after_the_listing_legend() {
        let encoders = "Encoders:\n V..... = Video\n ------\n V....D libx264  H.264\n V....D h264_nvenc NVIDIA\n A....D aac AAC\n";
        let filters = "Filters:\n  T.. = Timeline\n ------\n ... zscale  V->V\n ... tonemap V->V\n";
        let capabilities = Capabilities::parse(encoders, filters);
        assert!(capabilities.has_encoder("libx264") && capabilities.has_encoder("aac"));
        assert!(!capabilities.has_encoder("Video") && !capabilities.has_encoder("="));
        assert!(capabilities.can_tonemap());
        assert!(!Capabilities::parse(encoders, "").can_tonemap());
    }

    #[test]
    fn plan_prefers_configured_hardware_then_software_and_reports_absence() {
        let capabilities = Capabilities::parse(
            " ------\n V. libx264 x\n V. h264_vaapi x\n A. aac x\n",
            " ------\n . zscale x\n . tonemap x\n",
        );
        let request = parsed("transcode-v0a1sxh720b4000f1-0");
        let mut config = TranscodeConfig {
            hardware: Hardware::Vaapi,
            ..TranscodeConfig::default()
        };
        let chosen = plan(&config, &capabilities, &request).unwrap();
        assert_eq!((chosen.encoder, chosen.tonemap), ("h264_vaapi", true));
        config.hardware = Hardware::Nvenc;
        assert_eq!(
            plan(&config, &capabilities, &request).unwrap().encoder,
            "libx264"
        );
        let none = Capabilities::parse(" ------\n A. aac x\n", "");
        let error = plan(&config, &none, &request).unwrap_err();
        assert_eq!(error.to_string(), "transcode_unavailable");
        // Audio-only conversion over copied video needs no video encoder.
        let remux = parsed("transcode-v0a1sxh0b4000f0-0");
        assert_eq!(plan(&config, &none, &remux).unwrap().encoder, "copy");
        assert!(plan(&config, &Capabilities::default(), &remux).is_err());
    }

    #[test]
    fn software_arguments_scale_tonemap_and_downmix() {
        let request = parsed("transcode-v0a2sxh720b4000f3-0");
        let args = arguments(&request, &software(true)).join(" ");
        assert!(args.contains("[0:0]zscale=t=linear"), "{args}");
        assert!(args.contains("scale=w=-2:h='min(ih,720)',format=yuv420p[video]"));
        assert!(args.contains("-c:v libx264 -b:v 4000k -maxrate 4000k -bufsize 8000k"));
        assert!(args.contains("-profile:v high -level:v 4.1"));
        assert!(args.contains("-map 0:2 -af aresample=async=1,dynaudnorm"));
        assert!(args.contains("-c:a aac -ac 2 -b:a 192k"));
        assert!(args.ends_with("-f mp4 pipe:1"));
        let untoned = arguments(&request, &software(false)).join(" ");
        assert!(untoned.contains("[0:0]null,scale"));
    }

    #[test]
    fn burnt_subtitles_hardware_upload_and_passthrough() {
        let request = parsed("transcode-v0a1s4h1080b8000f16-0");
        let vaapi = EncoderPlan {
            hardware: Hardware::Vaapi,
            encoder: "h264_vaapi",
            vaapi_device: "/dev/dri/renderD129".into(),
            tonemap: false,
        };
        let args = arguments(&request, &vaapi).join(" ");
        assert!(
            args.contains("[base][0:4]overlay=eof_action=pass"),
            "{args}"
        );
        assert!(args.contains("format=nv12,hwupload[video]"));
        assert!(args.contains("-map 0:1 -c:a copy"));
        let input = input_arguments(&vaapi, &request).join(" ");
        assert!(input.contains("-hwaccel vaapi -hwaccel_device /dev/dri/renderD129"));
        assert!(input.contains("-filter_hw_device va"));
        let night = parsed("transcode-v0a1sxh0b4000f12-0");
        let args = arguments(&night, &software(false)).join(" ");
        assert!(args.starts_with("-map 0:0 -c:v copy"));
        assert!(args.contains("acompressor") && args.contains("-ac 6 -b:a 384k"));
        assert!(input_arguments(&software(false), &night).is_empty());
    }

    #[test]
    fn configuration_rejects_unknown_hardware_and_device_paths() {
        let values = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let config = TranscodeConfig::from_values(values(&[
            ("MEDIA_TRANSCODE_HARDWARE", "QuickSync"),
            ("MEDIA_TRANSCODE_MAX_SESSIONS", "4"),
            ("MEDIA_TRANSCODE_VAAPI_DEVICE", "/dev/dri/renderD129"),
        ]));
        assert!(config.enabled);
        assert_eq!(config.hardware, Hardware::Qsv);
        assert_eq!(
            (config.sessions, config.vaapi_device.as_str()),
            (4, "/dev/dri/renderD129")
        );
        for pairs in [
            &[("MEDIA_TRANSCODE_HARDWARE", "cuda-12")][..],
            &[("MEDIA_TRANSCODE_VAAPI_DEVICE", "/dev/dri/../sda")][..],
            &[("MEDIA_TRANSCODE_MAX_SESSIONS", "0")][..],
            &[("MEDIA_TRANSCODE_ENABLED", "maybe")][..],
        ] {
            let owned: Vec<(String, String)> = pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            let config = TranscodeConfig::from_values(|name| {
                owned
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.clone())
            });
            assert!(!config.enabled, "{pairs:?}");
        }
        assert!(
            !TranscodeConfig::from_values(values(&[("MEDIA_TRANSCODE_ENABLED", "off")])).enabled
        );
    }

    /// Real FFmpeg run: `MEDIA_TRANSCODE_E2E_SOURCE` names a file with HDR10 video at
    /// stream 0 and 5.1 audio at 1 (see `docs/STREAMING_COMPATIBILITY.md`).
    #[tokio::test]
    #[ignore = "needs MEDIA_TRANSCODE_E2E_SOURCE and an FFmpeg with x264, zimg and AAC"]
    async fn transcodes_a_real_file_to_fragmented_mp4() {
        use http_body_util::BodyExt;
        use std::path::Path;
        let source = std::env::var("MEDIA_TRANSCODE_E2E_SOURCE").unwrap();
        let source = Path::new(&source);
        let temp = tempfile::tempdir().unwrap();
        let transcodes = Transcodes::new(TranscodeConfig::default());
        for (raw, video, channels, transfer) in [
            ("transcode-v0a1sxh480b1500f11-2000", "h264", 6, Some("bt709")),
            ("transcode-v0a1sxh720b2500f4-0", "h264", 2, None),
            ("transcode-v0a1sxh0b1500f2-1000", "hevc", 2, Some("smpte2084")),
        ] {
            let opened = OpenedFile::open_in_root(
                source.parent().unwrap(),
                Path::new(source.file_name().unwrap()),
            )
            .await
            .unwrap();
            let response = transcodes
                .serve(opened, request(raw).unwrap(), &Request::new(()))
                .await
                .unwrap_or_else(|error| panic!("{raw}: {error}"));
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let output = temp.path().join("output.mp4");
            std::fs::write(&output, &bytes).unwrap();
            let probe = Command::new("ffprobe")
                .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
                .arg(&output)
                .output()
                .await
                .unwrap();
            assert!(probe.status.success(), "{raw}");
            let value: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = value["streams"].as_array().unwrap();
            let picture = streams.iter().find(|s| s["codec_type"] == "video").unwrap();
            let sound = streams.iter().find(|s| s["codec_type"] == "audio").unwrap();
            assert_eq!(picture["codec_name"], video, "{raw}");
            assert_eq!(sound["codec_name"], "aac", "{raw}");
            assert_eq!(sound["channels"], channels, "{raw}");
            if let Some(transfer) = transfer {
                assert_eq!(picture["color_transfer"], transfer, "{raw}");
            }
            let duration: f64 = value["format"]["duration"].as_str().unwrap().parse().unwrap();
            assert!(duration > 2.0, "{raw}: {duration}");
            eprintln!("{raw}: {} bytes, {} {}x{}, {duration:.2} s", bytes.len(), video, picture["width"], picture["height"]);
        }
    }

    #[test]
    fn codec_strings_match_the_levels_set_on_the_encoder() {
        assert_eq!(h264_codec(720), "avc1.640029");
        assert_eq!(h264_codec(1440), "avc1.640032");
        assert_eq!(h264_codec(2160), "avc1.640033");
    }
}
