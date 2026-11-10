//! Keyframe presentation times read from container indexes only (Matroska
//! Cues, MP4 sample tables): a few bounded reads, never a scan of the payload,
//! so a 50 GB remux on a NAS is indexed in milliseconds.
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

/// Largest index element (moov, Cues, Tracks…) loaded in memory.
const MAX_INDEX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_KEYFRAMES: usize = 1_000_000;
/// Top-level Matroska elements walked before giving up on finding Cues.
const MAX_TOP_LEVEL: usize = 200_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyframes {
    /// Presentation times of the copied video track's keyframes, ascending, in ms.
    pub times_ms: Vec<u64>,
    pub duration_ms: u64,
    /// HEVC video: Apple players only accept it under the `hvc1` sample entry.
    pub hevc: bool,
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn unsupported(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, reason)
}

/// `video_index` is the FFmpeg stream index of the video stream that is copied.
pub fn read(file: &mut File, video_index: u32) -> io::Result<Keyframes> {
    let mut magic = [0u8; 8];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut magic)?;
    let (mut times_ms, duration_ms, hevc) = if magic[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        matroska(file, video_index)?
    } else if matches!(
        &magic[4..8],
        b"ftyp" | b"moov" | b"free" | b"wide" | b"skip" | b"mdat"
    ) {
        mp4(file, video_index)?
    } else {
        return Err(unsupported("hls_container_unsupported"));
    };
    times_ms.sort_unstable();
    times_ms.dedup();
    if times_ms.is_empty() {
        return Err(unsupported("hls_keyframes_unavailable"));
    }
    let last = *times_ms.last().unwrap_or(&0);
    Ok(Keyframes {
        times_ms,
        duration_ms: duration_ms.max(last),
        hevc,
    })
}

fn be(bytes: &[u8], at: usize, width: usize) -> io::Result<u64> {
    let field = bytes
        .get(at..at + width)
        .ok_or_else(|| invalid("short index field"))?;
    Ok(field
        .iter()
        .fold(0u64, |value, b| (value << 8) | u64::from(*b)))
}

// ---- MP4 / ISO BMFF -------------------------------------------------------

/// Child boxes of a payload as (type, payload).
pub(super) fn boxes(data: &[u8]) -> io::Result<Vec<([u8; 4], &[u8])>> {
    let mut found = Vec::new();
    let mut at = 0usize;
    while at < data.len() {
        if found.len() >= 4096 || data.len() - at < 8 {
            return Err(invalid("MP4 box header invalid"));
        }
        let small = be(data, at, 4)?;
        let (size, head) = match small {
            0 => ((data.len() - at) as u64, 8),
            1 => (be(data, at + 8, 8)?, 16),
            n => (n, 8),
        };
        let size = usize::try_from(size).map_err(|_| invalid("MP4 box too large"))?;
        if size < head || size > data.len() - at {
            return Err(invalid("MP4 box size invalid"));
        }
        let kind = data[at + 4..at + 8]
            .try_into()
            .map_err(|_| invalid("MP4 box type invalid"))?;
        found.push((kind, &data[at + head..at + size]));
        at += size;
    }
    Ok(found)
}
fn child<'a>(list: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> Option<&'a [u8]> {
    list.iter()
        .find(|(k, _)| k == kind)
        .map(|(_, payload)| *payload)
}
fn required<'a>(list: &[([u8; 4], &'a [u8])], kind: &[u8; 4]) -> io::Result<&'a [u8]> {
    child(list, kind).ok_or_else(|| unsupported("hls_keyframes_unavailable"))
}
/// (timescale, duration) of an `mvhd` or `mdhd` full box.
pub(super) fn header_times(payload: &[u8]) -> io::Result<(u64, u64)> {
    match payload.first() {
        Some(0) => Ok((be(payload, 12, 4)?, be(payload, 16, 4)?)),
        Some(1) => Ok((be(payload, 20, 4)?, be(payload, 24, 8)?)),
        _ => Err(unsupported("unsupported MP4 header version")),
    }
}
/// Entries of a run table (`stts`, `ctts`, `stss`) after version/flags and count.
fn entries(payload: &[u8], width: usize) -> io::Result<&[u8]> {
    let count = usize::try_from(be(payload, 4, 4)?).map_err(|_| invalid("MP4 table"))?;
    let bytes = count
        .checked_mul(width)
        .filter(|bytes| *bytes == payload.len().saturating_sub(8))
        .ok_or_else(|| invalid("MP4 table size invalid"))?;
    Ok(&payload[8..8 + bytes])
}

fn mp4(file: &mut File, video_index: u32) -> io::Result<(Vec<u64>, u64, bool)> {
    let size = file.metadata()?.len();
    let mut at = 0u64;
    let moov = loop {
        if size.saturating_sub(at) < 8 {
            return Err(unsupported("hls_keyframes_unavailable"));
        }
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; 16];
        file.read_exact(&mut header[..8])?;
        let (length, head) = match be(&header, 0, 4)? {
            0 => (size - at, 8),
            1 => {
                file.read_exact(&mut header[8..])?;
                (be(&header, 8, 8)?, 16)
            }
            n => (n, 8),
        };
        if length < head || length > size - at {
            return Err(invalid("top-level MP4 box outside file"));
        }
        match &header[4..8] {
            b"moov" => {
                if length > MAX_INDEX_BYTES {
                    return Err(unsupported("MP4 index too large"));
                }
                let mut data = vec![0; (length - head) as usize];
                file.read_exact(&mut data)?;
                break data;
            }
            // Fragmented files carry their sync samples in each fragment.
            b"moof" => return Err(unsupported("hls_keyframes_unavailable")),
            _ => at += length,
        }
    };
    let movie = boxes(&moov)?;
    let (movie_scale, movie_duration) = header_times(required(&movie, b"mvhd")?)?;
    let trak = movie
        .iter()
        .filter(|(kind, _)| kind == b"trak")
        .nth(video_index as usize)
        .map(|(_, payload)| *payload)
        .ok_or_else(|| unsupported("hls_video_stream_unknown"))?;
    let track = boxes(trak)?;
    let media = boxes(required(&track, b"mdia")?)?;
    if be(required(&media, b"hdlr")?, 8, 4)? != u64::from(u32::from_be_bytes(*b"vide")) {
        return Err(unsupported("hls_video_stream_unknown"));
    }
    let (scale, _) = header_times(required(&media, b"mdhd")?)?;
    if scale == 0 || movie_scale == 0 {
        return Err(invalid("zero MP4 timescale"));
    }
    let tables = boxes(required(&boxes(required(&media, b"minf")?)?, b"stbl")?)?;
    let stts = entries(required(&tables, b"stts")?, 8)?;
    // First sample entry type, after version/flags, entry count and entry size.
    let hevc = child(&tables, b"stsd")
        .and_then(|stsd| stsd.get(12..16))
        .is_some_and(|kind| kind == b"hvc1" || kind == b"hev1");
    let sync = child(&tables, b"stss")
        .map(|payload| entries(payload, 4))
        .transpose()?;
    let ctts = child(&tables, b"ctts")
        .map(|payload| Ok::<_, io::Error>((payload.first() == Some(&1), entries(payload, 8)?)))
        .transpose()?;

    // Edit list: leading empty edits delay the track, the first media edit
    // trims its start. FFmpeg's seek timeline applies both.
    let (mut delay, mut trim) = (0i128, 0i128);
    if let Some(edits) = child(&track, b"edts").map(boxes).transpose()?
        && let Some(elst) = child(&edits, b"elst")
    {
        let wide = elst.first() == Some(&1);
        let width = if wide { 20 } else { 12 };
        for entry in entries(elst, width)?.chunks_exact(width) {
            let (duration, media_time) = if wide {
                (be(entry, 0, 8)? as i128, be(entry, 8, 8)? as i64 as i128)
            } else {
                (
                    be(entry, 0, 4)? as i128,
                    be(entry, 4, 4)? as u32 as i32 as i128,
                )
            };
            if media_time == -1 {
                delay += duration * i128::from(scale) / i128::from(movie_scale);
            } else {
                trim = media_time;
                break;
            }
        }
    }

    let mut times = Vec::new();
    let mut sync_samples = sync.map(|table| table.as_chunks::<4>().0.iter());
    let mut next_sync = || -> io::Result<Option<u64>> {
        match sync_samples.as_mut() {
            Some(samples) => samples.next().map(|s| be(s, 0, 4)).transpose(),
            None => Ok(None),
        }
    };
    let every_sample_is_sync = sync.is_none();
    let mut wanted = next_sync()?;
    let mut offsets = ctts.map(|(signed, table)| (signed, table.as_chunks::<8>().0.iter()));
    let (mut offset_left, mut offset) = (0u64, 0i128);
    let (mut sample, mut decode) = (1u64, 0i128);
    for run in stts.as_chunks::<8>().0 {
        let (count, delta) = (be(run, 0, 4)?, i128::from(be(run, 4, 4)? as u32));
        for _ in 0..count {
            if let Some((signed, table)) = offsets.as_mut() {
                if offset_left == 0 {
                    if let Some(entry) = table.next() {
                        offset_left = be(entry, 0, 4)?;
                        let raw = be(entry, 4, 4)? as u32;
                        offset = if *signed {
                            i128::from(raw as i32)
                        } else {
                            i128::from(raw)
                        };
                    } else {
                        offset = 0;
                    }
                }
                offset_left = offset_left.saturating_sub(1);
            }
            if every_sample_is_sync || wanted == Some(sample) {
                let pts = decode + offset - trim + delay;
                if pts >= 0 {
                    times.push((pts * 1000 / i128::from(scale)) as u64);
                    if times.len() > MAX_KEYFRAMES {
                        return Err(unsupported("too many keyframes"));
                    }
                }
                if !every_sample_is_sync {
                    wanted = next_sync()?;
                }
            }
            decode += delta;
            sample += 1;
        }
    }
    Ok((
        times,
        movie_duration.saturating_mul(1000) / movie_scale,
        hevc,
    ))
}

// ---- Matroska / WebM ------------------------------------------------------

const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CUES: u32 = 0x1C53_BB6B;
const CLUSTER: u32 = 0x1F43_B675;

/// EBML variable-length integer: (value, length). Element IDs keep their marker bit.
fn vint(bytes: &[u8], keep_marker: bool) -> io::Result<(u64, usize)> {
    let first = *bytes.first().ok_or_else(|| invalid("short EBML field"))?;
    let length = first.leading_zeros() as usize + 1;
    if length > 8 || (keep_marker && length > 4) || bytes.len() < length {
        return Err(invalid("EBML field invalid"));
    }
    let head = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & (0xFF >> length)
    };
    let value = bytes[1..length]
        .iter()
        .fold(head, |value, b| (value << 8) | u64::from(*b));
    Ok((value, length))
}
/// `None` size means "unknown" (all value bits set), only legal for Segment/Cluster.
fn element_header(bytes: &[u8]) -> io::Result<(u32, Option<u64>, usize)> {
    let (id, id_len) = vint(bytes, true)?;
    let (size, size_len) = vint(&bytes[id_len..], false)?;
    let unknown = size == (1u64 << (7 * size_len)) - 1;
    Ok((id as u32, (!unknown).then_some(size), id_len + size_len))
}
fn elements(data: &[u8]) -> io::Result<Vec<(u32, &[u8])>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < data.len() {
        if found.len() >= MAX_KEYFRAMES * 2 {
            return Err(invalid("too many EBML elements"));
        }
        let (id, size, head) = element_header(&data[at..])?;
        let size = size.ok_or_else(|| invalid("unknown EBML size inside index"))?;
        let end = usize::try_from(size)
            .ok()
            .and_then(|size| (at + head).checked_add(size))
            .filter(|end| *end <= data.len())
            .ok_or_else(|| invalid("EBML element outside parent"))?;
        found.push((id, &data[at + head..end]));
        at = end;
    }
    Ok(found)
}
fn uint(payload: &[u8]) -> io::Result<u64> {
    if payload.len() > 8 {
        return Err(invalid("EBML integer too long"));
    }
    be(payload, 0, payload.len())
}
fn float(payload: &[u8]) -> io::Result<f64> {
    match payload.len() {
        4 => Ok(f64::from(f32::from_bits(be(payload, 0, 4)? as u32))),
        8 => Ok(f64::from_bits(be(payload, 0, 8)?)),
        0 => Ok(0.0),
        _ => Err(invalid("EBML float invalid")),
    }
}

/// Reads the element header at `at` in the file: (id, size, header length).
fn file_header(file: &mut File, at: u64) -> io::Result<(u32, Option<u64>, u64)> {
    let mut header = [0u8; 12];
    file.seek(SeekFrom::Start(at))?;
    let read = file.read(&mut header)?;
    let (id, size, head) = element_header(&header[..read])?;
    Ok((id, size, head as u64))
}
fn load(file: &mut File, at: u64, expected: u32, end: u64) -> io::Result<Vec<u8>> {
    let (id, size, head) = file_header(file, at)?;
    let size = size.ok_or_else(|| invalid("unknown EBML index size"))?;
    if id != expected || size > MAX_INDEX_BYTES || at + head + size > end {
        return Err(invalid("EBML index element invalid"));
    }
    let mut data = vec![0; size as usize];
    file.seek(SeekFrom::Start(at + head))?;
    file.read_exact(&mut data)?;
    Ok(data)
}

fn matroska(file: &mut File, video_index: u32) -> io::Result<(Vec<u64>, u64, bool)> {
    let file_size = file.metadata()?.len();
    let (_, header_size, head) = file_header(file, 0)?;
    let mut at = head + header_size.ok_or_else(|| invalid("EBML header size unknown"))?;
    let (id, segment_size, head) = file_header(file, at)?;
    if id != SEGMENT {
        return Err(invalid("Matroska segment missing"));
    }
    let start = at + head;
    let end = segment_size.map_or(file_size, |size| start.saturating_add(size).min(file_size));
    let (mut info, mut tracks, mut cues) = (None, None, None);
    at = start;
    for _ in 0..MAX_TOP_LEVEL {
        if at >= end || end - at < 2 || (info.is_some() && tracks.is_some() && cues.is_some()) {
            break;
        }
        let (id, size, head) = file_header(file, at)?;
        match id {
            SEEK_HEAD => {
                // Only Seek entries: CRC-32 and Void siblings are opaque bytes.
                let head_data = load(file, at, SEEK_HEAD, end)?;
                for (_, seek) in elements(&head_data)?
                    .into_iter()
                    .filter(|(id, _)| *id == 0x4DBB)
                {
                    let fields = elements(seek)?;
                    let target = fields.iter().find(|(id, _)| *id == 0x53AB);
                    let position = fields.iter().find(|(id, _)| *id == 0x53AC);
                    if let (Some((_, target)), Some((_, position))) = (target, position) {
                        let target = vint(target, true)?.0 as u32;
                        let position = start.saturating_add(uint(position)?);
                        let slot = match target {
                            INFO => &mut info,
                            TRACKS => &mut tracks,
                            CUES => &mut cues,
                            _ => continue,
                        };
                        slot.get_or_insert(position);
                    }
                }
            }
            INFO => {
                info.get_or_insert(at);
            }
            TRACKS => {
                tracks.get_or_insert(at);
            }
            CUES => {
                cues.get_or_insert(at);
            }
            _ => {}
        }
        // A live-written file (unknown cluster size) cannot be skipped cheaply.
        let Some(size) = size else {
            if id == CLUSTER {
                break;
            }
            return Err(invalid("unknown EBML size"));
        };
        at = at.saturating_add(head).saturating_add(size);
    }
    let cues = cues.ok_or_else(|| unsupported("hls_keyframes_unavailable"))?;
    let tracks = tracks.ok_or_else(|| unsupported("hls_keyframes_unavailable"))?;

    let (mut scale, mut duration) = (1_000_000u64, 0f64);
    if let Some(info) = info {
        for (id, payload) in elements(&load(file, info, INFO, end)?)? {
            match id {
                0x2A_D7B1 => scale = uint(payload)?,
                0x4489 => duration = float(payload)?,
                _ => {}
            }
        }
    }
    if scale == 0 {
        return Err(invalid("zero Matroska timestamp scale"));
    }
    let tracks = load(file, tracks, TRACKS, end)?;
    let entry = elements(&tracks)?
        .into_iter()
        .filter(|(id, _)| *id == 0xAE)
        .nth(video_index as usize)
        .ok_or_else(|| unsupported("hls_video_stream_unknown"))?;
    let (mut number, mut kind, mut hevc) = (None, None, false);
    for (id, payload) in elements(entry.1)? {
        match id {
            0xD7 => number = Some(uint(payload)?),
            0x83 => kind = Some(uint(payload)?),
            0x86 => hevc = payload.starts_with(b"V_MPEGH/ISO/HEVC"),
            _ => {}
        }
    }
    let number = number
        .filter(|_| kind == Some(1))
        .ok_or_else(|| unsupported("hls_video_stream_unknown"))?;

    let to_ms = |ticks: u64| (u128::from(ticks) * u128::from(scale) / 1_000_000) as u64;
    let mut times = Vec::new();
    for (id, point) in elements(&load(file, cues, CUES, end)?)? {
        if id != 0xBB {
            continue;
        }
        let fields = elements(point)?;
        let Some((_, time)) = fields.iter().find(|(id, _)| *id == 0xB3) else {
            continue;
        };
        let mut for_video = false;
        for (_, position) in fields.iter().filter(|(id, _)| *id == 0xB7) {
            for_video |= elements(position)?
                .iter()
                .any(|(id, track)| *id == 0xF7 && uint(track).ok() == Some(number));
        }
        if for_video {
            times.push(to_ms(uint(time)?));
            if times.len() > MAX_KEYFRAMES {
                return Err(unsupported("too many keyframes"));
            }
        }
    }
    let duration_ms = (duration.max(0.0) * scale as f64 / 1_000_000.0) as u64;
    Ok((times, duration_ms, hevc))
}
