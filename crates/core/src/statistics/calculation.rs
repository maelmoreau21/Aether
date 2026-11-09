//! Observed wall time and the union of actually traversed media intervals are distinct.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Start,
    Heartbeat,
    Pause,
    Resume,
    Seek,
    Buffering,
    Buffered,
    AudioChange,
    SubtitleChange,
    SpeedChange,
    SourceChange,
    SkipIntro,
    SkipCredits,
    SkipRecap,
    SkipPreview,
    Stop,
    Ended,
    Error,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Origin {
    Manual,
    #[default]
    Automatic,
    Sync,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Track {
    pub id: String,
    pub language: String,
    pub codec: String,
}
impl Track {
    fn valid(&self) -> bool {
        [&self.id, &self.language, &self.codec]
            .iter()
            .all(|s| s.len() <= 160 && !s.chars().any(char::is_control))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Event {
    pub sequence: i64,
    pub kind: Kind,
    pub elapsed_ms: f64,
    pub position: f64,
    pub duration: f64,
    pub playing: bool,
    pub buffering: bool,
    pub rate: f64,
    #[serde(default)]
    pub origin: Origin,
    pub from_position: Option<f64>,
    pub previous_track: Option<Track>,
    pub track: Option<Track>,
    pub method: Option<String>,
    pub error_code: Option<String>,
    pub source_media_id: Option<String>,
    pub stop_reason: Option<String>,
}
impl Event {
    pub fn valid(&self) -> bool {
        self.sequence > 0
            && self.sequence <= 10_000_000
            && self.elapsed_ms.is_finite()
            && (0.0..=604_800_000.0).contains(&self.elapsed_ms)
            && self.duration.is_finite()
            && (0.0..=604_800.0).contains(&self.duration)
            && self.position.is_finite()
            && (0.0..=604_800.0).contains(&self.position)
            && (self.duration == 0.0 || self.position <= self.duration + 0.5)
            && self.rate.is_finite()
            && (0.25..=4.0).contains(&self.rate)
            && self
                .from_position
                .is_none_or(|p| p.is_finite() && (0.0..=604_800.0).contains(&p))
            && self.track.as_ref().is_none_or(Track::valid)
            && self.previous_track.as_ref().is_none_or(Track::valid)
            && self.method.as_deref().is_none_or(|s| {
                matches!(
                    s,
                    "direct" | "remux" | "separate_audio" | "prepared" | "transcode" | "unknown"
                )
            })
            && self.error_code.as_deref().is_none_or(|s| {
                matches!(
                    s,
                    "network" | "decode" | "unsupported" | "aborted" | "playback"
                )
            })
            && self
                .source_media_id
                .as_deref()
                .is_none_or(|s| !s.is_empty() && s.len() <= 160 && self.kind == Kind::SourceChange)
            && self
                .stop_reason
                .as_deref()
                .is_none_or(|s| matches!(s, "closed" | "next" | "previous" | "queue_change"))
    }
    pub fn is_seek(&self) -> bool {
        matches!(
            self.kind,
            Kind::Seek | Kind::SkipIntro | Kind::SkipCredits | Kind::SkipRecap | Kind::SkipPreview
        )
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct State {
    pub previous: Option<Event>,
    pub ranges: Vec<[f64; 2]>,
    pub seconds: f64,
    pub covered_seconds: f64,
    pub duration: f64,
    pub media_seconds: f64,
    pub resumes: i64,
    pub pauses: i64,
    pub seeks_forward: i64,
    pub seeks_backward: i64,
    pub audio_changes: i64,
    pub subtitle_changes: i64,
    pub speed_changes: i64,
    pub intro_skips: i64,
    pub credits_skips: i64,
    pub other_skips: i64,
    pub buffering_count: i64,
    pub buffering_seconds: f64,
    pub errors: i64,
    pub startup_ms: Option<f64>,
    pub initial_audio: Option<Track>,
    pub initial_subtitle: Option<Track>,
    pub audio: Option<Track>,
    pub subtitle: Option<Track>,
    pub method: String,
}
impl State {
    pub fn completed(&self) -> bool {
        self.duration > 0.0 && self.covered_seconds / self.duration >= 0.9
    }

    /// Returns the newly observed wall seconds. Discontinuities never fill coverage gaps.
    pub fn apply(&mut self, event: &Event) -> Result<f64, ()> {
        let mut gained = 0.0;
        if let Some(previous) = &self.previous {
            if event.elapsed_ms < previous.elapsed_ms {
                return Err(());
            }
            let dt = (event.elapsed_ms - previous.elapsed_ms) / 1000.0;
            if dt <= 90.0 {
                if previous.buffering {
                    self.buffering_seconds += dt;
                }
                if previous.playing && !previous.buffering {
                    let end = if event.is_seek() {
                        event.from_position.unwrap_or(previous.position)
                    } else {
                        event.position
                    };
                    let end = if event.duration > 0.0 {
                        end.min(event.duration)
                    } else {
                        end
                    };
                    let delta = end - previous.position;
                    if delta > 0.0 && delta <= dt * previous.rate + 0.75 {
                        gained = dt.min(delta / previous.rate);
                        self.seconds += gained;
                        self.media_seconds += delta;
                        self.ranges.push([previous.position, end]);
                        self.ranges.sort_by(|a, b| a[0].total_cmp(&b[0]));
                        let mut merged: Vec<[f64; 2]> = Vec::new();
                        for range in &self.ranges {
                            if let Some(last) =
                                merged.last_mut().filter(|last| range[0] <= last[1] + 0.001)
                            {
                                last[1] = last[1].max(range[1]);
                            } else {
                                merged.push(*range);
                            }
                        }
                        if merged.len() > 4096 {
                            return Err(());
                        }
                        self.ranges = merged;
                        self.covered_seconds = self.ranges.iter().map(|r| r[1] - r[0]).sum();
                    }
                }
            }
        }
        if event.duration > 0.0 {
            self.duration = event.duration;
        }
        if event.playing && !event.buffering && self.startup_ms.is_none() {
            self.startup_ms = Some(event.elapsed_ms);
        }
        match event.kind {
            Kind::Pause => self.pauses += 1,
            Kind::Resume => {
                if self
                    .previous
                    .as_ref()
                    .is_some_and(|e| e.kind == Kind::Pause)
                {
                    self.resumes += 1;
                }
            }
            Kind::Seek
            | Kind::SkipIntro
            | Kind::SkipCredits
            | Kind::SkipRecap
            | Kind::SkipPreview => {
                if let Some(from) = event.from_position {
                    if event.position > from {
                        self.seeks_forward += 1;
                    }
                    if event.position < from {
                        self.seeks_backward += 1;
                    }
                }
                match event.kind {
                    Kind::SkipIntro => self.intro_skips += 1,
                    Kind::SkipCredits => self.credits_skips += 1,
                    Kind::SkipRecap | Kind::SkipPreview => self.other_skips += 1,
                    _ => {}
                }
            }
            Kind::AudioChange => {
                if event.previous_track.is_some() {
                    self.audio_changes += 1;
                }
                if self.initial_audio.is_none() {
                    self.initial_audio = event.track.clone();
                }
                self.audio = event.track.clone();
            }
            Kind::SubtitleChange => {
                if event.previous_track.is_some() {
                    self.subtitle_changes += 1;
                }
                if self.initial_subtitle.is_none() {
                    self.initial_subtitle = event.track.clone();
                }
                self.subtitle = event.track.clone();
            }
            Kind::SpeedChange => self.speed_changes += 1,
            Kind::Buffering => self.buffering_count += 1,
            Kind::Error => self.errors += 1,
            _ => {}
        }
        if let Some(method) = &event.method {
            self.method = method.clone();
        }
        self.previous = Some(event.clone());
        Ok(gained)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(seq: i64, time: f64, pos: f64, kind: Kind) -> Event {
        Event {
            sequence: seq,
            elapsed_ms: time * 1000.0,
            position: pos,
            duration: 100.0,
            playing: true,
            buffering: false,
            rate: 1.0,
            kind,
            origin: Origin::Manual,
            from_position: None,
            previous_track: None,
            track: None,
            method: None,
            error_code: None,
            source_media_id: None,
            stop_reason: None,
        }
    }
    #[test]
    fn pauses_seeks_repeats_speed_and_stalls_do_not_fabricate_consumption() {
        let mut s = State::default();
        s.apply(&sample(1, 0.0, 0.0, Kind::Start)).unwrap();
        s.apply(&sample(2, 10.0, 10.0, Kind::Heartbeat)).unwrap();
        let mut seek = sample(3, 10.0, 80.0, Kind::Seek);
        seek.from_position = Some(10.0);
        s.apply(&seek).unwrap();
        s.apply(&sample(4, 20.0, 90.0, Kind::Heartbeat)).unwrap();
        assert_eq!((s.seconds, s.covered_seconds), (20.0, 20.0));
        assert!(!s.completed());
        let mut back = sample(5, 20.0, 0.0, Kind::Seek);
        back.from_position = Some(90.0);
        s.apply(&back).unwrap();
        let mut fast = sample(6, 20.0, 0.0, Kind::SpeedChange);
        fast.rate = 2.0;
        s.apply(&fast).unwrap();
        let mut next = sample(7, 25.0, 10.0, Kind::Pause);
        next.playing = false;
        next.rate = 2.0;
        s.apply(&next).unwrap();
        s.apply(&sample(8, 55.0, 10.0, Kind::Resume)).unwrap();
        let mut stalled = sample(9, 60.0, 10.0, Kind::Buffering);
        stalled.buffering = true;
        s.apply(&stalled).unwrap();
        s.apply(&sample(10, 65.0, 10.0, Kind::Buffered)).unwrap();
        assert_eq!(
            (s.seconds, s.covered_seconds, s.buffering_seconds),
            (25.0, 20.0, 5.0)
        );
        assert_eq!((s.seeks_forward, s.seeks_backward, s.pauses), (1, 1, 1));
        assert!(s.apply(&sample(11, 64.0, 11.0, Kind::Heartbeat)).is_err());
    }
    #[test]
    fn ending_at_the_last_frame_is_not_completion_and_track_origins_are_preserved() {
        let mut state = State::default();
        state.apply(&sample(1, 0.0, 90.0, Kind::Start)).unwrap();
        let mut audio = sample(2, 0.0, 90.0, Kind::AudioChange);
        audio.origin = Origin::Automatic;
        audio.track = Some(Track {
            id: "fr".into(),
            language: "fr".into(),
            codec: "opus".into(),
        });
        state.apply(&audio).unwrap();
        let mut changed = sample(3, 5.0, 95.0, Kind::AudioChange);
        changed.previous_track = audio.track;
        changed.track = Some(Track {
            id: "en".into(),
            language: "en".into(),
            codec: "opus".into(),
        });
        changed.origin = Origin::Sync;
        state.apply(&changed).unwrap();
        state.apply(&sample(4, 10.0, 100.0, Kind::Ended)).unwrap();
        assert!(!state.completed());
        assert_eq!(state.covered_seconds, 10.0);
        assert_eq!(state.audio_changes, 1);
        assert_eq!(state.initial_audio.unwrap().language, "fr");
        assert_eq!(state.audio.unwrap().language, "en");
        assert_eq!(changed.origin, Origin::Sync);
    }
}
