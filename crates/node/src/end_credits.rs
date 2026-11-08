//! Picture measurement of the end of a film, for the end credits.
//!
//! Like the keyframe analyzer of the Jellyfin intro-skipper plugin, the last
//! [`WINDOW_SECONDS`] of the file are decoded **keyframes only** (`-skip_frame nokey`, a few
//! hundred pictures for twenty minutes), each scaled down and measured with ffmpeg's
//! `blackframe` (share of black pixels) and `signalstats` (luma percentiles). A keyframe is
//! black when at least [`BLACK_PERCENT`] % of its pixels are, and lettered when bright detail
//! stands out from its background (`YMAX − YLOW ≥` [`LETTERING_CONTRAST`]): text on black.
//!
//! The Node only measures: it reports the runs of consecutive black keyframes with how many of
//! them are lettered. The start of the latest long runs is then placed on the exact frame by a
//! short full decode of the keyframe gap before it. Which run is the credits, and how it agrees
//! with the chapters, is decided by the segment plugin.
use serde_json::{Value, json};
use std::io;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Name of the measurement, sent with the result; the plugin ignores any other.
pub const ALGORITHM: &str = "blackframe-v1";
/// Component of the `analyze` task that runs it.
pub const COMPONENT: &str = "credits";
/// The end of the file that is measured (seconds).
pub const WINDOW_SECONDS: f64 = 1200.0;
/// Luma at or under which a pixel is black (`blackframe` threshold, limited range).
const PIXEL_THRESHOLD: u32 = 28;
/// Share of black pixels (percent) from which a keyframe is black.
pub const BLACK_PERCENT: u32 = 85;
/// Luma spread between the brightest detail and the dark background of a lettered keyframe.
pub const LETTERING_CONTRAST: f64 = 60.0;
/// Width the keyframes are measured at: enough for credits lettering, cheap to filter.
const MEASURE_WIDTH: u32 = 480;
/// Keyframes closer than this to the previous one are skipped (codecs whose every frame is a
/// keyframe, or that ignore `-skip_frame`), which also bounds the measurement.
const MIN_PAGE_SPACING: f64 = 0.4;
const MAX_PAGES: usize = 6000;
/// Runs reported, the latest first kept.
const MAX_RUNS: usize = 128;
/// The latest runs at least this long get their start placed on the exact frame.
const REFINE_MIN_RUN: f64 = 30.0;
const MAX_REFINED: usize = 4;
/// Longest keyframe gap decoded to place a start, and the frame rate it is sampled at.
const MAX_REFINE_GAP: f64 = 20.0;
const REFINE_FPS: u32 = 8;
const SCAN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);
const REFINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One measured picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Page {
    /// File time (seconds).
    pub time: f64,
    pub pblack: u32,
    pub ylow: f64,
    pub ymax: f64,
}

impl Page {
    pub fn is_black(&self) -> bool {
        self.pblack >= BLACK_PERCENT
    }
    pub fn is_lettered(&self) -> bool {
        self.ymax - self.ylow >= LETTERING_CONTRAST
    }
}

/// A run of consecutive black keyframes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub start: f64,
    /// The first non-black keyframe after the run, or the end of the measured window.
    pub end: f64,
    pub pages: u32,
    pub lettered: u32,
    /// Index of the run's first page, and the time of the page before it (if any).
    first: usize,
    before: Option<f64>,
}

/// Parse the `metadata=mode=print` log of a measurement into pages (times relative to the
/// decode's seek point). Lines from other filters are ignored.
#[derive(Default)]
struct PageParser {
    pages: Vec<Page>,
    current: Option<Page>,
    last_kept: Option<f64>,
}

impl PageParser {
    fn flush(&mut self) {
        if let Some(page) = self.current.take() {
            if self
                .last_kept
                .is_some_and(|last| page.time - last < MIN_PAGE_SPACING)
            {
                return;
            }
            self.last_kept = Some(page.time);
            self.pages.push(page);
        }
    }

    fn line(&mut self, line: &str) {
        if let Some(rest) = line.split("pts_time:").nth(1) {
            self.flush();
            if let Some(time) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|time| time.is_finite())
            {
                self.current = Some(Page {
                    time,
                    // A frame without the black-frame value is not black.
                    pblack: 0,
                    ylow: 0.0,
                    ymax: 0.0,
                });
            }
            return;
        }
        let Some(page) = self.current.as_mut() else {
            return;
        };
        let value = |key: &str| {
            line.split(key)
                .nth(1)
                .and_then(|rest| rest.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        if let Some(pblack) = value("lavfi.blackframe.pblack=") {
            page.pblack = pblack.clamp(0.0, 100.0) as u32;
        } else if let Some(ylow) = value("lavfi.signalstats.YLOW=") {
            page.ylow = ylow;
        } else if let Some(ymax) = value("lavfi.signalstats.YMAX=") {
            page.ymax = ymax;
        }
    }

    fn finish(mut self) -> Vec<Page> {
        self.flush();
        self.pages
    }
}

/// Runs of consecutive black pages, in time order. `window_end` closes a run that lasts to the
/// end of the measured window.
pub fn black_runs(pages: &[Page], window_end: f64) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut open: Option<Run> = None;
    for (index, page) in pages.iter().enumerate() {
        if page.is_black() {
            let run = open.get_or_insert(Run {
                start: page.time,
                end: page.time,
                pages: 0,
                lettered: 0,
                first: index,
                before: index.checked_sub(1).map(|before| pages[before].time),
            });
            run.pages += 1;
            run.lettered += u32::from(page.is_lettered());
        } else if let Some(mut run) = open.take() {
            run.end = page.time;
            runs.push(run);
        }
    }
    if let Some(mut run) = open {
        run.end = window_end.max(run.start);
        runs.push(run);
    }
    runs
}

/// The earliest time from which every sampled frame of a decoded gap is black, if the gap ends
/// black: where the cut to the run really is.
pub fn black_from(frames: &[Page]) -> Option<f64> {
    if !frames.last()?.is_black() {
        return None;
    }
    let first_black = frames
        .iter()
        .rposition(|frame| !frame.is_black())
        .map_or(0, |last_lit| last_lit + 1);
    Some(frames[first_black].time)
}

fn filters(fps: Option<u32>, stats: bool) -> String {
    let mut chain = String::new();
    if let Some(fps) = fps {
        chain.push_str(&format!("fps={fps},"));
    }
    chain.push_str(&format!(
        "scale=w='min({MEASURE_WIDTH},iw)':h=-2:flags=bilinear,format=yuv420p,blackframe=amount=0:threshold={PIXEL_THRESHOLD}"
    ));
    if stats {
        chain.push_str(",signalstats");
    }
    chain.push_str(",metadata=mode=print");
    chain
}

/// One measurement decode from `seek` (for `duration` seconds, or to the end of the file).
/// The file is ffmpeg's standard input, read through `fd:` when `seekable` (ffmpeg 6.1 and
/// later), through the pipe otherwise, as for the audio fingerprint.
async fn measure(
    opened: &crate::files::OpenedFile,
    seek: f64,
    duration: Option<f64>,
    keyframes_only: bool,
    seekable: bool,
) -> io::Result<Vec<Page>> {
    let mut file = opened.try_clone_std()?;
    std::io::Seek::rewind(&mut file)?;
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args([
        "-nostdin",
        "-hide_banner",
        "-nostats",
        "-v",
        "info",
        "-protocol_whitelist",
        "fd,pipe",
        "-probesize",
        "5000000",
        "-analyzeduration",
        "5000000",
    ]);
    if keyframes_only {
        cmd.args(["-skip_frame", "nokey"]);
    }
    if seek > 0.0 {
        cmd.args(["-ss", &format!("{seek:.3}")]);
    }
    if seekable {
        cmd.args(["-fd", "0", "-i", "fd:"]);
    } else {
        cmd.args(["-i", "-"]);
    }
    if let Some(duration) = duration {
        cmd.args(["-t", &format!("{duration:.3}")]);
    }
    let chain = filters((!keyframes_only).then_some(REFINE_FPS), keyframes_only);
    cmd.args([
        "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", &chain, "-f", "null", "-",
    ])
    .stdin(Stdio::from(file))
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("credits_scan_output_unavailable"))?;
    let timeout = if keyframes_only {
        SCAN_TIMEOUT
    } else {
        REFINE_TIMEOUT
    };
    let pages = tokio::time::timeout(timeout, async {
        let mut parser = PageParser::default();
        let mut lines = BufReader::new(stderr).lines();
        while let Some(line) = lines.next_line().await? {
            parser.line(&line);
            if parser.pages.len() > MAX_PAGES {
                return Err(io::Error::other("credits_scan_too_many_frames"));
            }
        }
        if !child.wait().await?.success() {
            return Err(io::Error::other("credits_scan_decoder_failed"));
        }
        Ok(parser.finish())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "credits_scan_timeout"))??;
    opened.verify_unchanged()?;
    Ok(pages
        .into_iter()
        .map(|page| Page {
            time: seek + page.time,
            ..page
        })
        .collect())
}

/// The file's duration from its probe.
async fn duration_of(opened: &crate::files::OpenedFile) -> Option<f64> {
    let probe = crate::probe::probe_opened(opened).await.ok()?;
    probe.format.get("duration").and_then(|value| {
        value
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| value.as_f64())
            .filter(|d| d.is_finite() && *d > 0.0)
    })
}

/// Measure the end of a film: black keyframe runs of its last [`WINDOW_SECONDS`].
pub async fn scan(opened: &crate::files::OpenedFile) -> io::Result<Value> {
    let duration = duration_of(opened)
        .await
        .ok_or_else(|| io::Error::other("credits_scan_duration_unknown"))?;
    let window_start = (duration - WINDOW_SECONDS).max(0.0);
    let (pages, seekable) = match measure(opened, window_start, None, true, true).await {
        Ok(pages) if !pages.is_empty() => (pages, true),
        Ok(_) | Err(_) => (
            measure(opened, window_start, None, true, false).await?,
            false,
        ),
    };
    if pages.is_empty() {
        return Err(io::Error::other("credits_scan_no_video"));
    }
    let mut runs = black_runs(&pages, duration);
    // The exact start of the latest long runs: the frame where the black begins inside the
    // keyframe gap before the run. A failed decode keeps the keyframe's time.
    let mut refined = 0;
    for run in runs.iter_mut().rev() {
        if refined == MAX_REFINED {
            break;
        }
        if run.end - run.start < REFINE_MIN_RUN || run.first == 0 {
            continue;
        }
        refined += 1;
        let from = run
            .before
            .unwrap_or(run.start - MAX_REFINE_GAP)
            .max(run.start - MAX_REFINE_GAP)
            .max(window_start);
        let length = run.start - from + 0.5;
        if let Ok(frames) = measure(opened, from, Some(length), false, seekable).await
            && let Some(start) = black_from(&frames)
            && start < run.start
        {
            run.start = start;
        }
    }
    let skip = runs.len().saturating_sub(MAX_RUNS);
    let runs: Vec<Value> = runs
        .iter()
        .skip(skip)
        .map(|run| {
            json!({
                "start": (run.start * 1000.0).round() / 1000.0,
                "end": (run.end * 1000.0).round() / 1000.0,
                "pages": run.pages,
                "lettered": run.lettered,
            })
        })
        .collect();
    Ok(json!({
        "algorithm": ALGORITHM,
        "windowStart": window_start,
        "windowEnd": duration,
        "pages": pages.len(),
        "runs": runs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(time: f64, pblack: u32, ymax: f64) -> Page {
        Page {
            time,
            pblack,
            ylow: 16.0,
            ymax,
        }
    }

    #[test]
    fn metadata_log_becomes_pages() {
        let mut parser = PageParser::default();
        for line in [
            "[Parsed_metadata_5 @ 0x5581] frame:0    pts:0       pts_time:0",
            "[Parsed_metadata_5 @ 0x5581] lavfi.blackframe.pblack=12",
            "[Parsed_metadata_5 @ 0x5581] lavfi.signalstats.YMIN=16",
            "[Parsed_metadata_5 @ 0x5581] lavfi.signalstats.YLOW=40",
            "[Parsed_metadata_5 @ 0x5581] lavfi.signalstats.YMAX=235",
            "[Parsed_blackframe_3 @ 0x5582] frame:1 pblack:97 pts:2000 t:2.000000 type:I last_keyframe:1",
            "[Parsed_metadata_5 @ 0x5581] frame:1    pts:2000    pts_time:2",
            "[Parsed_metadata_5 @ 0x5581] lavfi.blackframe.pblack=97",
            "[Parsed_metadata_5 @ 0x5581] lavfi.signalstats.YLOW=16",
            "[Parsed_metadata_5 @ 0x5581] lavfi.signalstats.YMAX=200",
            // Too close to the previous page: skipped.
            "[Parsed_metadata_5 @ 0x5581] frame:2    pts:2100    pts_time:2.1",
            "[Parsed_metadata_5 @ 0x5581] lavfi.blackframe.pblack=97",
        ] {
            parser.line(line);
        }
        let pages = parser.finish();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], page(0.0, 12, 235.0).with_ylow(40.0));
        assert!(pages[1].is_black() && pages[1].is_lettered());
        assert!(!pages[0].is_black());
    }

    impl Page {
        fn with_ylow(self, ylow: f64) -> Self {
            Self { ylow, ..self }
        }
    }

    #[test]
    fn runs_end_on_the_next_lit_keyframe_or_the_window_end() {
        let pages = [
            page(0.0, 10, 230.0),
            page(2.0, 99, 20.0),
            page(4.0, 10, 230.0),
            page(6.0, 95, 200.0),
            page(8.0, 92, 210.0),
            page(10.0, 99, 18.0),
        ];
        let runs = black_runs(&pages, 12.0);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].start, runs[0].end, runs[0].pages), (2.0, 4.0, 1));
        assert_eq!(runs[0].lettered, 0);
        assert_eq!((runs[1].start, runs[1].end), (6.0, 12.0));
        assert_eq!((runs[1].pages, runs[1].lettered), (3, 2));
        assert_eq!(runs[1].before, Some(4.0));
    }

    /// 150 s of colour picture, then `tail` (an ffmpeg lavfi source of 90 s), as MPEG-4 Part 2
    /// with a keyframe every 2 s (an encoder every ffmpeg build has).
    fn fixture(name: &str, tail: Option<&str>) -> Option<(std::path::PathBuf, String)> {
        let root = std::env::temp_dir().join(format!("aether-credits-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).ok()?;
        let file = format!("{name}.mkv");
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"]);
        match tail {
            Some(tail) => {
                cmd.args(["testsrc2=s=320x180:r=24:d=150", "-f", "lavfi", "-i", tail]);
                cmd.args([
                    "-filter_complex",
                    "[0:v][1:v]concat=n=2:v=1:a=0[v]",
                    "-map",
                    "[v]",
                ]);
            }
            // A film that fades out over its last 4 s.
            None => {
                cmd.args([
                    "testsrc2=s=320x180:r=24:d=240",
                    "-vf",
                    "fade=t=out:st=236:d=4",
                ]);
            }
        }
        cmd.args(["-c:v", "mpeg4", "-q:v", "5", "-g", "48"]);
        let made = cmd.arg(root.join(&file)).status().ok()?;
        made.success().then_some((root, file))
    }

    async fn scan_fixture(name: &str, tail: Option<&str>) -> Option<Value> {
        let Some((root, file)) = fixture(name, tail) else {
            eprintln!("ffmpeg unavailable: end credits measurement not exercised");
            return None;
        };
        let opened = crate::LocalNode
            .open_media(&root, std::path::Path::new(&file))
            .await
            .unwrap();
        let result = scan(&opened).await.unwrap();
        let _ = std::fs::remove_dir_all(&root);
        Some(result)
    }

    /// White lettering (three lines) on black for the last 90 s.
    pub(crate) const ROLL: &str = "color=c=black:s=320x180:r=24:d=90,drawbox=x=90:y=50:w=140:h=6:color=white:t=fill,drawbox=x=110:y=80:w=100:h=6:color=white:t=fill,drawbox=x=100:y=110:w=120:h=6:color=white:t=fill";

    #[tokio::test]
    async fn lettering_on_black_at_the_end_is_one_lettered_run() {
        let Some(result) = scan_fixture("roll", Some(ROLL)).await else {
            return;
        };
        assert_eq!(result["algorithm"], ALGORITHM);
        assert_eq!(result["windowStart"], 0.0);
        let runs = result["runs"].as_array().unwrap();
        let roll = runs.last().unwrap();
        let start = roll["start"].as_f64().unwrap();
        let end = roll["end"].as_f64().unwrap();
        assert!((start - 150.0).abs() <= 0.5, "{result}");
        assert!((end - 240.0).abs() <= 0.5, "{result}");
        let (pages, lettered) = (
            roll["pages"].as_u64().unwrap(),
            roll["lettered"].as_u64().unwrap(),
        );
        assert!(pages >= 40 && lettered * 10 >= pages * 9, "{result}");
        assert!(result.to_string().len() < 8 * 1024);
    }

    #[tokio::test]
    async fn a_fade_out_is_only_a_few_seconds_of_black() {
        let Some(result) = scan_fixture("fade", None).await else {
            return;
        };
        for run in result["runs"].as_array().unwrap() {
            let length = run["end"].as_f64().unwrap() - run["start"].as_f64().unwrap();
            assert!(length < 10.0, "{result}");
            assert_eq!(run["lettered"], 0, "{result}");
        }
    }

    #[test]
    fn the_cut_is_where_the_black_lasts_to_the_end_of_the_gap() {
        let frames = [
            page(10.0, 5, 230.0),
            page(10.125, 90, 30.0),
            page(10.25, 10, 230.0),
            page(10.375, 96, 30.0),
            page(10.5, 99, 200.0),
        ];
        assert_eq!(black_from(&frames), Some(10.375));
        assert_eq!(black_from(&frames[..3]), None);
        assert_eq!(black_from(&[]), None);
    }
}
