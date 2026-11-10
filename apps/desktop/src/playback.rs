//! Playback rules that do not depend on mpv or GPUI: where to resume, when to
//! save the position, how to name tracks and at which size to render frames.
use aether_client::Progress;

/// Wall-clock seconds between two position saves while playing.
pub const SAVE_INTERVAL: f64 = 10.0;
/// Below this position a film starts from the beginning.
pub const MIN_RESUME: f64 = 5.0;
/// Largest rendered frame side: the GPU scales the texture above it.
pub const MAX_RENDER_SIDE: u32 = 4096;

/// Where playback starts from a saved position.
pub fn resume_at(progress: Option<&Progress>) -> f64 {
    match progress {
        Some(p) if !p.watched && p.position >= MIN_RESUME => {
            if p.duration > 0.0 && p.position >= p.duration - 1.0 {
                0.0
            } else {
                p.position
            }
        }
        _ => 0.0,
    }
}

/// Decides when the position is written to the Core: every
/// [`SAVE_INTERVAL`] while playing, at once on pause, seek or close, never for
/// an unchanged position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgressSaver {
    /// Pass it back to the Core as the expected revision.
    pub revision: i64,
    last_position: Option<f64>,
    last_at: Option<f64>,
}

impl ProgressSaver {
    pub fn new(revision: i64) -> Self {
        Self {
            revision,
            ..Self::default()
        }
    }
    /// `now` is a monotonic clock in seconds; `force` for pause, seek and close.
    pub fn due(&self, now: f64, position: f64, force: bool) -> bool {
        if !position.is_finite() || (self.last_position.is_none() && position < 1.0) {
            return false;
        }
        if self
            .last_position
            .is_some_and(|saved| (saved - position).abs() < 0.5)
        {
            return false;
        }
        force || self.last_at.is_none_or(|at| now - at >= SAVE_INTERVAL)
    }
    /// A save started: do not start another one for the same position.
    pub fn started(&mut self, now: f64, position: f64) {
        self.last_position = Some(position);
        self.last_at = Some(now);
    }
    /// The Core answered with the revision to send next time.
    pub fn saved(&mut self, revision: i64) {
        self.revision = self.revision.max(revision);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

/// An audio or subtitle track as mpv lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<i64>,
    pub selected: bool,
    pub default: bool,
    pub forced: bool,
    pub external: bool,
}

impl Track {
    /// Reads one entry of mpv's `track-list`; `get("lang")` returns the
    /// string value of `track-list/N/lang`.
    pub fn from_mpv(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let kind = match get("type")?.as_str() {
            "video" => TrackKind::Video,
            "audio" => TrackKind::Audio,
            "sub" => TrackKind::Subtitle,
            _ => return None,
        };
        let flag = |key: &str| get(key).is_some_and(|v| v == "yes");
        let text = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        Some(Self {
            id: get("id")?.parse().ok()?,
            kind,
            title: text("title"),
            lang: text("lang"),
            codec: text("codec"),
            channels: get("demux-channel-count").and_then(|v| v.parse().ok()),
            selected: flag("selected"),
            default: flag("default"),
            forced: flag("forced"),
            external: flag("external"),
        })
    }
    /// "Français · 5.1 · AC3", "Anglais · forcés", "Piste 3".
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        match (&self.lang, &self.title) {
            (Some(lang), Some(title)) if !title.eq_ignore_ascii_case(lang) => {
                parts.push(format!("{} – {title}", language_name(lang)));
            }
            (Some(lang), _) => parts.push(language_name(lang)),
            (None, Some(title)) => parts.push(title.clone()),
            (None, None) => parts.push(format!("Piste {}", self.id)),
        }
        if let Some(layout) = self.channels.and_then(channel_layout) {
            parts.push(layout.to_owned());
        }
        if let Some(codec) = &self.codec {
            parts.push(codec.to_uppercase());
        }
        if self.forced {
            parts.push("forcés".to_owned());
        }
        if self.external {
            parts.push("externe".to_owned());
        }
        parts.join(" · ")
    }
}

fn channel_layout(channels: i64) -> Option<&'static str> {
    Some(match channels {
        1 => "mono",
        2 => "stéréo",
        6 => "5.1",
        8 => "7.1",
        _ => return None,
    })
}

/// French name of an ISO 639-1/639-2 language code; the code itself otherwise.
pub fn language_name(code: &str) -> String {
    let name = match code.to_ascii_lowercase().as_str() {
        "fr" | "fre" | "fra" => "Français",
        "en" | "eng" => "Anglais",
        "de" | "ger" | "deu" => "Allemand",
        "es" | "spa" => "Espagnol",
        "it" | "ita" => "Italien",
        "pt" | "por" => "Portugais",
        "nl" | "dut" | "nld" => "Néerlandais",
        "ja" | "jpn" => "Japonais",
        "ko" | "kor" => "Coréen",
        "zh" | "chi" | "zho" => "Chinois",
        "ru" | "rus" => "Russe",
        "ar" | "ara" => "Arabe",
        "und" => "Indéterminée",
        _ => return code.to_owned(),
    };
    name.to_owned()
}

/// Size of the frames mpv renders: the window's size in pixels, reduced to the
/// video's own resolution (the GPU enlarges the texture for free, mpv's software
/// scaler does not). The window's aspect ratio is kept, so mpv letterboxes.
pub fn render_size(window: (u32, u32), video: Option<(u32, u32)>) -> (u32, u32) {
    let (ww, wh) = (window.0.max(1) as f64, window.1.max(1) as f64);
    let mut scale = 1.0f64;
    if let Some((vw, vh)) = video.filter(|(w, h)| *w > 0 && *h > 0) {
        // The largest scale at which the video, fitted, keeps its own pixels.
        let fitted = (ww / vw as f64).min(wh / vh as f64);
        if fitted > 1.0 {
            scale = 1.0 / fitted;
        }
    }
    scale = scale.min(MAX_RENDER_SIDE as f64 / ww.max(wh));
    let side = |v: f64| ((v * scale).round() as u32).clamp(2, MAX_RENDER_SIDE);
    (side(ww), side(wh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn progress(position: f64, duration: f64, watched: bool) -> Progress {
        Progress {
            position,
            duration,
            watched,
            revision: 3,
        }
    }

    #[test]
    fn resume_skips_the_start_the_end_and_finished_items() {
        assert_eq!(resume_at(None), 0.0);
        assert_eq!(resume_at(Some(&progress(3.0, 600.0, false))), 0.0);
        assert_eq!(resume_at(Some(&progress(120.0, 600.0, false))), 120.0);
        assert_eq!(resume_at(Some(&progress(120.0, 600.0, true))), 0.0);
        assert_eq!(resume_at(Some(&progress(599.5, 600.0, false))), 0.0);
        assert_eq!(resume_at(Some(&progress(120.0, 0.0, false))), 120.0);
    }

    #[test]
    fn positions_are_saved_periodically_and_on_demand() {
        let mut saver = ProgressSaver::new(4);
        assert!(!saver.due(0.0, 0.2, true), "nothing watched yet");
        assert!(saver.due(0.0, 2.0, false), "first save");
        saver.started(0.0, 2.0);
        assert!(!saver.due(5.0, 7.0, false), "too soon");
        assert!(saver.due(5.0, 7.0, true), "pause saves at once");
        assert!(!saver.due(30.0, 2.2, true), "unchanged position");
        assert!(saver.due(10.0, 12.0, false), "interval elapsed");
        assert!(!saver.due(10.0, f64::NAN, true));
        saver.saved(9);
        saver.saved(7);
        assert_eq!(saver.revision, 9, "an older answer never rewinds");
    }

    #[test]
    fn tracks_are_read_from_mpv_and_labelled() {
        let entry = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, String> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            Track::from_mpv(|key| map.get(key).cloned())
        };
        let audio = entry(&[
            ("type", "audio"),
            ("id", "2"),
            ("lang", "fre"),
            ("codec", "ac3"),
            ("demux-channel-count", "6"),
            ("selected", "yes"),
        ])
        .unwrap();
        assert_eq!(audio.kind, TrackKind::Audio);
        assert!(audio.selected && !audio.default);
        assert_eq!(audio.label(), "Français · 5.1 · AC3");
        let sub = entry(&[
            ("type", "sub"),
            ("id", "4"),
            ("lang", "en"),
            ("title", "SDH"),
            ("forced", "yes"),
            ("external", "yes"),
        ])
        .unwrap();
        assert_eq!(sub.label(), "Anglais – SDH · forcés · externe");
        let bare = entry(&[("type", "sub"), ("id", "7"), ("title", " ")]).unwrap();
        assert_eq!(bare.label(), "Piste 7");
        assert_eq!(entry(&[("type", "attachment"), ("id", "1")]), None);
        assert_eq!(entry(&[("type", "audio"), ("id", "x")]), None);
        assert_eq!(language_name("tlh"), "tlh");
    }

    #[test]
    fn frames_are_rendered_at_the_video_resolution_at_most() {
        // A 1080p film in a 4K window: rendered at 1080p, enlarged by the GPU.
        assert_eq!(render_size((3840, 2160), Some((1920, 1080))), (1920, 1080));
        // A small window: the window's own size.
        assert_eq!(render_size((1280, 720), Some((1920, 1080))), (1280, 720));
        // A 4:3 video in a wide window keeps the window's shape.
        assert_eq!(render_size((2560, 1080), Some((720, 540))), (1280, 540));
        // Unknown video size; huge windows are capped.
        assert_eq!(render_size((800, 600), None), (800, 600));
        assert_eq!(render_size((8192, 4096), None), (4096, 2048));
        assert_eq!(render_size((0, 0), Some((0, 0))), (2, 2));
    }
}
