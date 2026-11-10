//! Splits FFmpeg's fragmented MP4 output into the HLS parts: the
//! initialization section (`ftyp` + `moov`) and media segments (`moof` +
//! `mdat`). Media data is streamed; only the small `moov`/`moof` boxes are held.
//!
//! Video segments are cut on fragments, not on timestamps: FFmpeg starts a
//! fragment at every keyframe, so keeping the fragments whose first sample is
//! presented in `[start, end)` partitions the packets exactly, whatever their
//! decode order (B-frames) and whatever FFmpeg's seek lands on.
use super::keyframes::{boxes, header_times};
use bytes::Bytes;
use std::{io, pin::Pin, task::Poll};

const MAX_HELD_BOX: usize = 16 * 1024 * 1024;
/// Every rendition's timeline is shifted by this much (`-output_ts_offset`):
/// B-frame delay and codec priming give the first packets negative decode
/// times, which an unsigned `tfdt` cannot carry. One shared shift keeps the
/// renditions in sync and the timeline continuous across segments.
pub const TIMELINE_OFFSET_MS: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Init,
    Media,
}

/// Presentation window of a video segment, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start_ms: u64,
    /// `None` for the last segment.
    pub end_ms: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Forward,
    Skip,
    Hold,
}

pub struct BoxFilter {
    part: Part,
    window: Option<Window>,
    timescale: Option<u64>,
    header: Vec<u8>,
    kind: [u8; 4],
    remaining: u64,
    mode: Mode,
    held: Vec<u8>,
    /// The `mdat` after a fragment outside the window is dropped with it.
    skip_data: bool,
    done: bool,
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn field(bytes: &[u8], at: usize, width: usize) -> io::Result<u64> {
    let slice = bytes
        .get(at..at + width)
        .ok_or_else(|| invalid("short fragment box"))?;
    Ok(slice.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b)))
}

/// Timescale of the single track of a `moov` box (header included).
fn track_timescale(moov: &[u8]) -> io::Result<u64> {
    let top = boxes(moov)?;
    let movie = boxes(
        top.first()
            .filter(|(k, _)| k == b"moov")
            .ok_or_else(|| invalid("moov"))?
            .1,
    )?;
    let trak = boxes(
        movie
            .iter()
            .find(|(k, _)| k == b"trak")
            .ok_or_else(|| invalid("trak"))?
            .1,
    )?;
    let mdia = boxes(
        trak.iter()
            .find(|(k, _)| k == b"mdia")
            .ok_or_else(|| invalid("mdia"))?
            .1,
    )?;
    let mdhd = mdia
        .iter()
        .find(|(k, _)| k == b"mdhd")
        .ok_or_else(|| invalid("mdhd"))?
        .1;
    let (scale, _) = header_times(mdhd)?;
    (scale > 0)
        .then_some(scale)
        .ok_or_else(|| invalid("zero timescale"))
}

/// Presentation time of the first sample of a `moof` box (header included),
/// in track units: `tfdt` plus the first sample's composition offset.
fn first_presentation(moof: &[u8]) -> io::Result<i128> {
    let top = boxes(moof)?;
    let fragment = boxes(
        top.first()
            .filter(|(k, _)| k == b"moof")
            .ok_or_else(|| invalid("moof"))?
            .1,
    )?;
    let traf = boxes(
        fragment
            .iter()
            .find(|(k, _)| k == b"traf")
            .ok_or_else(|| invalid("traf"))?
            .1,
    )?;
    let tfdt = traf
        .iter()
        .find(|(k, _)| k == b"tfdt")
        .ok_or_else(|| invalid("tfdt"))?
        .1;
    let decode = match tfdt.first() {
        Some(1) => field(tfdt, 4, 8)?,
        Some(0) => field(tfdt, 4, 4)?,
        _ => return Err(invalid("tfdt version")),
    };
    let mut offset = 0i128;
    if let Some((_, trun)) = traf.iter().find(|(k, _)| k == b"trun") {
        let flags = field(trun, 1, 3)?;
        if flags & 0x800 != 0 && field(trun, 4, 4)? > 0 {
            let mut at = 8;
            for (flag, width) in [(0x1, 4), (0x4, 4), (0x100, 4), (0x200, 4), (0x400, 4)] {
                if flags & flag != 0 {
                    at += width;
                }
            }
            let raw = field(trun, at, 4)? as u32;
            offset = if trun.first() == Some(&1) {
                i128::from(raw as i32)
            } else {
                i128::from(raw)
            };
        }
    }
    Ok(i128::from(decode) + offset)
}

impl BoxFilter {
    /// `window` selects video fragments; `None` keeps every media fragment
    /// (audio renditions are already cut on timestamps by FFmpeg).
    pub fn new(part: Part, window: Option<Window>) -> Self {
        Self {
            part,
            window,
            timescale: None,
            header: Vec::with_capacity(16),
            kind: [0; 4],
            remaining: 0,
            mode: Mode::Skip,
            held: Vec::new(),
            skip_data: false,
            done: false,
        }
    }
    /// Init: `moov` is complete. Video media: the next segment has begun.
    pub fn done(&self) -> bool {
        self.done
    }
    fn mode_for(&self, kind: &[u8; 4]) -> Mode {
        match (self.part, kind) {
            (Part::Init, b"ftyp" | b"moov") => Mode::Forward,
            (Part::Init, _) => Mode::Skip,
            // `mfra` is a whole-file random access index, meaningless per segment.
            (Part::Media, b"ftyp" | b"mfra") => Mode::Skip,
            (Part::Media, b"moov" | b"moof") if self.window.is_some() => Mode::Hold,
            (Part::Media, b"moov") => Mode::Skip,
            (Part::Media, b"mdat") if self.skip_data => Mode::Skip,
            (Part::Media, _) => Mode::Forward,
        }
    }
    fn finish_box(&mut self, out: &mut Vec<u8>) -> io::Result<()> {
        if self.part == Part::Init {
            self.done |= &self.kind == b"moov";
            return Ok(());
        }
        if &self.kind == b"mdat" {
            self.skip_data = false;
        }
        if self.mode != Mode::Hold {
            return Ok(());
        }
        let held = std::mem::take(&mut self.held);
        if &self.kind == b"moov" {
            self.timescale = Some(track_timescale(&held)?);
            return Ok(());
        }
        let (Some(window), Some(scale)) = (self.window, self.timescale) else {
            return Err(invalid("fragment before its track header"));
        };
        // Compare in ms·units to stay exact: pts/scale vs ms/1000.
        let at = first_presentation(&held)? * 1000;
        let units = |ms: u64| i128::from(ms + TIMELINE_OFFSET_MS) * i128::from(scale);
        if window.end_ms.is_some_and(|end| at >= units(end)) {
            self.done = true;
        } else if at < units(window.start_ms) {
            self.skip_data = true;
        } else {
            out.extend_from_slice(&held);
        }
        Ok(())
    }
    pub fn push(&mut self, mut chunk: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        while !chunk.is_empty() && !self.done {
            if self.remaining == 0 {
                let large = self.header.len() >= 4 && self.header[..4] == [0, 0, 0, 1];
                let wanted = if large { 16 } else { 8 };
                let take = (wanted - self.header.len()).min(chunk.len());
                self.header.extend_from_slice(&chunk[..take]);
                chunk = &chunk[take..];
                let large = self.header[..4.min(self.header.len())] == [0, 0, 0, 1];
                if self.header.len() < 8 || (large && self.header.len() < 16) {
                    continue;
                }
                let size = if large {
                    field(&self.header, 8, 8)?
                } else {
                    field(&self.header, 0, 4)?
                };
                let head = self.header.len() as u64;
                if size < head {
                    return Err(invalid("segment box size invalid"));
                }
                self.kind.copy_from_slice(&self.header[4..8]);
                self.mode = self.mode_for(&self.kind);
                match self.mode {
                    Mode::Forward => out.extend_from_slice(&self.header),
                    Mode::Hold if size as usize > MAX_HELD_BOX => {
                        return Err(invalid("fragment header too large"));
                    }
                    Mode::Hold => self.held.extend_from_slice(&self.header),
                    Mode::Skip => {}
                }
                self.header.clear();
                self.remaining = size - head;
                if self.remaining == 0 {
                    self.finish_box(out)?;
                }
                continue;
            }
            let take = usize::try_from(self.remaining)
                .unwrap_or(usize::MAX)
                .min(chunk.len());
            match self.mode {
                Mode::Forward => out.extend_from_slice(&chunk[..take]),
                Mode::Hold => self.held.extend_from_slice(&chunk[..take]),
                Mode::Skip => {}
            }
            self.remaining -= take as u64;
            chunk = &chunk[take..];
            if self.remaining == 0 {
                self.finish_box(out)?;
            }
        }
        Ok(())
    }
}

/// A media-segment body: FFmpeg's stdout reduced to this segment's fragments.
/// It ends as soon as the next segment's first fragment appears.
pub struct MediaBody<B> {
    inner: Option<B>,
    filter: BoxFilter,
}
impl<B> MediaBody<B> {
    pub fn new(inner: B, window: Option<Window>) -> Self {
        Self {
            inner: Some(inner),
            filter: BoxFilter::new(Part::Media, window),
        }
    }
}
impl<B> http_body::Body for MediaBody<B>
where
    B: http_body::Body<Data = Bytes, Error = io::Error> + Unpin,
{
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<io::Result<http_body::Frame<Bytes>>>> {
        loop {
            let this = &mut *self;
            let Some(inner) = this.inner.as_mut() else {
                return Poll::Ready(None);
            };
            let frame = match Pin::new(inner).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => frame,
                other => return other,
            };
            let Ok(data) = frame.into_data() else {
                continue;
            };
            let mut out = Vec::with_capacity(data.len());
            if let Err(error) = this.filter.push(&data, &mut out) {
                return Poll::Ready(Some(Err(error)));
            }
            if this.filter.done() {
                // Dropping the pipe stops FFmpeg now instead of at its bound.
                this.inner = None;
            }
            if !out.is_empty() {
                return Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from(out)))));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }
    fn large_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = 1u32.to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(&((payload.len() + 16) as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
    /// Minimal one-track moov with a 1000 Hz timescale.
    fn moov() -> Vec<u8> {
        let mut mdhd = vec![0; 4]; // version 0, flags
        mdhd.extend_from_slice(&[0; 8]); // creation, modification
        mdhd.extend_from_slice(&1000u32.to_be_bytes());
        mdhd.extend_from_slice(&[0; 8]); // duration, language
        let mdia = mp4_box(b"mdia", &mp4_box(b"mdhd", &mdhd));
        mp4_box(b"moov", &mp4_box(b"trak", &mdia))
    }
    /// A fragment whose first sample decodes at `decode` and is shown `offset` later.
    fn moof(decode: u64, offset: u32) -> Vec<u8> {
        let mut tfdt = vec![1, 0, 0, 0];
        tfdt.extend_from_slice(&decode.to_be_bytes());
        let mut trun = vec![0, 0, 0x08, 0x01]; // data offset + composition offsets
        trun.extend_from_slice(&1u32.to_be_bytes());
        trun.extend_from_slice(&0u32.to_be_bytes());
        trun.extend_from_slice(&offset.to_be_bytes());
        let traf = [mp4_box(b"tfdt", &tfdt), mp4_box(b"trun", &trun)].concat();
        mp4_box(b"moof", &mp4_box(b"traf", &traf))
    }
    fn run(filter: &mut BoxFilter, input: &[u8], chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for piece in input.chunks(chunk) {
            filter.push(piece, &mut out).unwrap();
        }
        out
    }

    #[test]
    fn splits_initialization_and_media_whatever_the_chunking() {
        let (ftyp, moov) = (mp4_box(b"ftyp", b"iso6"), mp4_box(b"moov", &[7; 300]));
        let (moof, mdat) = (mp4_box(b"moof", &[1; 40]), large_box(b"mdat", &[2; 1000]));
        let mfra = mp4_box(b"mfra", &[3; 20]);
        let stream = [&ftyp[..], &moov, &moof, &mdat, &moof, &mdat, &mfra].concat();
        for chunk in [1, 3, 7, 64, 100_000] {
            let mut init = BoxFilter::new(Part::Init, None);
            assert_eq!(run(&mut init, &stream, chunk), [&ftyp[..], &moov].concat());
            assert!(init.done());
            let mut media = BoxFilter::new(Part::Media, None);
            let kept = run(&mut media, &stream, chunk);
            assert_eq!(
                kept,
                [&moof[..], &mdat, &moof, &mdat].concat(),
                "chunk {chunk}"
            );
            assert!(!media.done());
        }
    }

    #[test]
    fn video_fragments_are_selected_by_presentation_time_not_decode_order() {
        // Keyframes shown at 4 s, 6 s and 8 s, decoded 83 ms earlier (B-frames),
        // on the shifted output timeline.
        let shifted = |ms: u64| moof(ms + TIMELINE_OFFSET_MS - 83, 83);
        let (a, b, c) = (shifted(4_000), shifted(6_000), shifted(8_000));
        let data = |n: u8| large_box(b"mdat", &[n; 500]);
        let stream = [
            mp4_box(b"ftyp", b"iso6"),
            moov(),
            a,
            data(1),
            b.clone(),
            data(2),
            c,
            data(3),
        ]
        .concat();
        for chunk in [1, 5, 4096] {
            let mut filter = BoxFilter::new(
                Part::Media,
                Some(Window {
                    start_ms: 6_000,
                    end_ms: Some(8_000),
                }),
            );
            assert_eq!(
                run(&mut filter, &stream, chunk),
                [b.clone(), data(2)].concat()
            );
            assert!(filter.done(), "chunk {chunk}");
        }
    }

    #[test]
    fn a_box_smaller_than_its_header_is_rejected() {
        let mut filter = BoxFilter::new(Part::Media, None);
        let error = filter.push(&[0, 0, 0, 4, b'm', b'o', b'o', b'f'], &mut Vec::new());
        assert_eq!(error.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
