//! Read-only, bounded ISO BMFF Opus packet extraction. No decoder, subprocess,
//! temporary media, or whole-window payload buffer is involved.
//!
//! This intentionally accepts a narrow non-fragmented MP4 subset. Unsupported
//! layouts fail explicitly, so callers can retain the existing packet-copy path.
use crate::{
    files::OpenedFile,
    stream::{BoxStreamBody, empty_body, parse_single_range},
};
use bytes::Bytes;
use http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
    sync::Arc,
    time::Instant,
};
use tokio::{
    io::{AsyncRead, AsyncSeek},
    sync::Mutex,
};

const MAX_MOOV: usize = 16 * 1024 * 1024;
const MAX_CACHE: usize = 32 * 1024 * 1024;
const MAX_SAMPLES: u32 = 4_000_000;
const MAX_WINDOW_SAMPLES: usize = 8192;
const MAX_WINDOW_BYTES: u64 = 8 * 1024 * 1024;
const BLOCK: usize = 65_536;
const STRIDE: u32 = 256;

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
fn unsupported(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, reason)
}
fn u16be(bytes: &[u8], at: usize) -> io::Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}
fn u32be(bytes: &[u8], at: usize) -> io::Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}
fn u64be(bytes: &[u8], at: usize) -> io::Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .get(at..at + 8)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}

#[derive(Clone, Copy)]
struct Atom {
    start: usize,
    payload: usize,
    end: usize,
    kind: [u8; 4],
}
impl Atom {
    fn data(self, data: &[u8]) -> &[u8] {
        &data[self.payload..self.end]
    }
    fn raw(self, data: &[u8]) -> &[u8] {
        &data[self.start..self.end]
    }
}
fn atoms(data: &[u8], range: Range<usize>) -> io::Result<Vec<Atom>> {
    if range.end > data.len() || range.start > range.end {
        return Err(invalid("MP4 box outside parent"));
    }
    let mut result = Vec::new();
    let mut at = range.start;
    while at < range.end {
        if result.len() >= 4096 || range.end - at < 8 {
            return Err(invalid("MP4 box count or header invalid"));
        }
        let size32 = u32be(data, at)?;
        let header = if size32 == 1 { 16 } else { 8 };
        let size = match size32 {
            0 => (range.end - at) as u64,
            1 => u64be(data, at + 8)?,
            n => u64::from(n),
        };
        let size = usize::try_from(size).map_err(|_| invalid("MP4 box too large"))?;
        if size < header || size > range.end - at {
            return Err(invalid("MP4 box size invalid"));
        }
        result.push(Atom {
            start: at,
            payload: at + header,
            end: at + size,
            kind: data[at + 4..at + 8]
                .try_into()
                .map_err(|_| invalid("MP4 box type invalid"))?,
        });
        at += size;
    }
    Ok(result)
}
fn one(list: &[Atom], kind: &[u8; 4]) -> io::Result<Atom> {
    let mut matching = list.iter().filter(|a| &a.kind == kind);
    let found = *matching
        .next()
        .ok_or_else(|| unsupported("required MP4 box missing"))?;
    if matching.next().is_some() {
        return Err(invalid("duplicate MP4 box"));
    }
    Ok(found)
}
fn children(data: &[u8], parent: Atom) -> io::Result<Vec<Atom>> {
    atoms(data, parent.payload..parent.end)
}
fn table(data: &[u8], atom: Atom, width: usize, prefix: usize) -> io::Result<(usize, u32)> {
    let bytes = atom.data(data);
    if u32be(bytes, 0)? != 0 {
        return Err(unsupported("unsupported MP4 table version"));
    }
    let count = u32be(bytes, prefix - 4)?;
    if count > MAX_SAMPLES || bytes.len() != prefix + count as usize * width {
        return Err(invalid("MP4 table size invalid"));
    }
    Ok((atom.payload + prefix, count))
}

struct Track {
    id: u32,
    count: u32,
    media_time: u64,
    duration: u64,
    stts: (usize, u32),
    stsc: (usize, u32),
    sizes: usize,
    fixed_size: u32,
    offsets: (usize, u32),
    wide_offsets: bool,
    byte_prefix: Vec<u64>,
    init: Bytes,
}
struct Index {
    data: Vec<u8>,
    tracks: Vec<Track>,
    bytes: usize,
}

fn load_index(mut file: File) -> io::Result<Index> {
    let size = file.metadata()?.len();
    file.rewind()?;
    let mut at = 0u64;
    let mut moov = None;
    let mut mdats = Vec::new();
    let mut boxes = 0;
    while at < size {
        boxes += 1;
        if boxes > 4096 || size - at < 8 {
            return Err(invalid("top-level MP4 box count or size invalid"));
        }
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0; 16];
        file.read_exact(&mut header[..8])?;
        let small = u32be(&header, 0)?;
        let length = if small == 1 {
            file.read_exact(&mut header[8..])?;
            u64be(&header, 8)?
        } else if small == 0 {
            size - at
        } else {
            u64::from(small)
        };
        let head = if small == 1 { 16 } else { 8 };
        if length < head || length > size - at {
            return Err(invalid("top-level MP4 box outside file"));
        }
        match &header[4..8] {
            b"moov" => {
                if moov.is_some() || length > MAX_MOOV as u64 {
                    return Err(unsupported("MP4 moov missing, duplicated or too large"));
                }
                let mut data = vec![0; length as usize];
                file.seek(SeekFrom::Start(at))?;
                file.read_exact(&mut data)?;
                moov = Some(data);
            }
            b"mdat" => {
                if mdats.len() >= 64 {
                    return Err(unsupported("too many MP4 media extents"));
                }
                mdats.push(at + head..at + length);
            }
            b"moof" => return Err(unsupported("fragmented MP4 requires packet-copy fallback")),
            _ => {}
        }
        at += length;
    }
    let data = moov.ok_or_else(|| unsupported("MP4 moov not found"))?;
    let movie = one(&atoms(&data, 0..data.len())?, b"moov")?;
    let movie_children = children(&data, movie)?;
    if movie_children.iter().any(|a| a.kind == *b"mvex") {
        return Err(unsupported("fragmented MP4 requires packet-copy fallback"));
    }
    let mvhd = one(&movie_children, b"mvhd")?;
    let movie_scale = match mvhd.data(&data).first() {
        Some(0) => u32be(mvhd.data(&data), 12)?,
        Some(1) => u32be(mvhd.data(&data), 20)?,
        _ => return Err(unsupported("unsupported movie header")),
    };
    if movie_scale == 0 {
        return Err(invalid("zero movie timescale"));
    }
    let mut tracks = Vec::new();
    for trak in movie_children.iter().filter(|a| a.kind == *b"trak") {
        if tracks.len() >= 8 {
            return Err(unsupported("too many native Opus tracks"));
        }
        match parse_track(&data, *trak, mvhd, movie_scale, &mdats) {
            Ok(track) => {
                if tracks
                    .iter()
                    .any(|existing: &Track| existing.id == track.id)
                {
                    return Err(invalid("duplicate MP4 audio track ID"));
                }
                tracks.push(track);
            }
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {}
            Err(error) => return Err(error),
        }
    }
    if tracks.is_empty() {
        return Err(unsupported("no supported native MP4 Opus track"));
    }
    let bytes = data.len()
        + tracks
            .iter()
            .map(|t| t.byte_prefix.len() * 8 + t.init.len())
            .sum::<usize>();
    if bytes > MAX_CACHE {
        return Err(unsupported("native MP4 index exceeds cache budget"));
    }
    Ok(Index {
        data,
        tracks,
        bytes,
    })
}

fn parse_track(
    data: &[u8],
    trak: Atom,
    mvhd: Atom,
    movie_scale: u32,
    mdats: &[Range<u64>],
) -> io::Result<Track> {
    let tr = children(data, trak)?;
    let tkhd = one(&tr, b"tkhd")?;
    if mvhd.raw(data).len() > 128 || tkhd.raw(data).len() > 112 {
        return Err(unsupported("oversized MP4 movie or track header"));
    }
    let id = match tkhd.data(data).first() {
        Some(0) => u32be(tkhd.data(data), 12)?,
        Some(1) => u32be(tkhd.data(data), 20)?,
        _ => return Err(unsupported("unsupported track header")),
    };
    if id == 0 {
        return Err(invalid("zero MP4 track ID"));
    }
    let mdia = children(data, one(&tr, b"mdia")?)?;
    let mdhd = one(&mdia, b"mdhd")?;
    let hdlr = one(&mdia, b"hdlr")?;
    if mdhd.raw(data).len() > 64 || hdlr.raw(data).len() > 4096 {
        return Err(unsupported("oversized MP4 media header"));
    }
    if hdlr.data(data).get(8..12) != Some(b"soun") {
        return Err(unsupported("not an audio track"));
    }
    let timescale = match mdhd.data(data).first() {
        Some(0) => u32be(mdhd.data(data), 12)?,
        Some(1) => u32be(mdhd.data(data), 20)?,
        _ => return Err(unsupported("unsupported media header")),
    };
    if timescale != 48000 {
        return Err(unsupported("native Opus requires 48 kHz timescale"));
    }
    let minf = children(data, one(&mdia, b"minf")?)?;
    let dinf = one(&minf, b"dinf")?;
    if dinf.raw(data).len() > 256 || one(&minf, b"smhd")?.raw(data).len() > 64 {
        return Err(unsupported("oversized MP4 media information"));
    }
    let dref = one(&children(data, dinf)?, b"dref")?;
    if u32be(dref.data(data), 0)? != 0 || u32be(dref.data(data), 4)? != 1 {
        return Err(unsupported("non-local MP4 data reference"));
    }
    let references = atoms(data, dref.payload + 8..dref.end)?;
    let url = one(&references, b"url ")?;
    if references.len() != 1 || url.data(data) != [0, 0, 0, 1] {
        return Err(unsupported("external MP4 data reference"));
    }
    let stbl = children(data, one(&minf, b"stbl")?)?;
    let stsd = one(&stbl, b"stsd")?;
    if stsd.raw(data).len() > 4096 {
        return Err(unsupported("oversized Opus sample description"));
    }
    if u32be(stsd.data(data), 0)? != 0 || u32be(stsd.data(data), 4)? != 1 {
        return Err(unsupported("multiple MP4 sample descriptions"));
    }
    let entries = atoms(data, stsd.payload + 8..stsd.end)?;
    let opus = one(&entries, b"Opus")?;
    let entry = opus.data(data);
    if entries.len() != 1
        || u16be(entry, 6)? != 1
        || u16be(entry, 8)? != 0
        || u16be(entry, 18)? != 16
        || u32be(entry, 24)? != 48000u32 << 16
    {
        return Err(unsupported("unsupported Opus sample entry"));
    }
    let channels = u16be(entry, 16)?;
    let dops = one(&atoms(data, opus.payload + 28..opus.end)?, b"dOps")?;
    let config = dops.data(data);
    if config.len() < 11
        || config[0] != 0
        || channels == 0
        || channels > 8
        || u16::from(config[1]) != channels
    {
        return Err(unsupported("unsupported Opus channel configuration"));
    }
    match config[10] {
        0 if channels <= 2 && config.len() == 11 => {}
        1 if config.len() == 13 + usize::from(channels) => {
            let streams = config[11];
            let coupled = config[12];
            if streams == 0
                || coupled > streams
                || u16::from(streams) + u16::from(coupled) != channels
                || config[13..].iter().any(|n| u16::from(*n) >= channels)
            {
                return Err(invalid("invalid Opus channel mapping"));
            }
        }
        _ => return Err(unsupported("unsupported Opus channel mapping family")),
    }
    if stbl.iter().any(|a| {
        a.kind == *b"stss"
            || a.kind == *b"stz2"
            || a.kind == *b"senc"
            || a.kind == *b"saiz"
            || a.kind == *b"saio"
    }) {
        return Err(unsupported("unsupported Opus sample layout"));
    }
    if let Some(ctts) = stbl.iter().find(|a| a.kind == *b"ctts") {
        let (at, count) = table(data, *ctts, 8, 8)?;
        for n in 0..count {
            if u32be(data, at + n as usize * 8 + 4)? != 0 {
                return Err(unsupported("Opus composition offsets unsupported"));
            }
        }
    }
    let stts = table(data, one(&stbl, b"stts")?, 8, 8)?;
    let stsc = table(data, one(&stbl, b"stsc")?, 12, 8)?;
    if stts.1 > 65536 || stsc.1 > 65536 {
        return Err(unsupported("native MP4 timing or chunk run limit"));
    }
    let stsz = one(&stbl, b"stsz")?;
    let sizes = stsz.data(data);
    if u32be(sizes, 0)? != 0 {
        return Err(unsupported("unsupported sample sizes version"));
    }
    let fixed_size = u32be(sizes, 4)?;
    let count = u32be(sizes, 8)?;
    if count == 0
        || count > MAX_SAMPLES
        || sizes.len()
            != 12
                + if fixed_size == 0 {
                    count as usize * 4
                } else {
                    0
                }
    {
        return Err(invalid("Opus sample size count invalid"));
    }
    let offset_atom = if stbl.iter().any(|a| a.kind == *b"co64") {
        one(&stbl, b"co64")?
    } else {
        one(&stbl, b"stco")?
    };
    if stbl
        .iter()
        .filter(|a| a.kind == *b"co64" || a.kind == *b"stco")
        .count()
        != 1
    {
        return Err(invalid("ambiguous chunk offsets"));
    }
    let wide_offsets = offset_atom.kind == *b"co64";
    let offsets = table(data, offset_atom, if wide_offsets { 8 } else { 4 }, 8)?;
    let edts = one(&tr, b"edts")?;
    if edts.raw(data).len() > 128 {
        return Err(unsupported("oversized Opus edit list"));
    }
    let elst = one(&children(data, edts)?, b"elst")?;
    let edits = elst.data(data);
    if u32be(edits, 4)? != 1 {
        return Err(unsupported("complex Opus edit list"));
    }
    let (edit_duration, media_time, rate_at) = match edits.first() {
        Some(0) if edits.len() == 20 => (
            u64::from(u32be(edits, 8)?),
            u32be(edits, 12)? as i32 as i64,
            16,
        ),
        Some(1) if edits.len() == 28 => (u64be(edits, 8)?, u64be(edits, 16)? as i64, 24),
        _ => return Err(unsupported("unsupported Opus edit list")),
    };
    if media_time < 0 || u32be(edits, rate_at)? != 0x10000 {
        return Err(unsupported("non-unit Opus edit rate or empty edit"));
    }
    let mut samples = 0u64;
    let mut duration = 0u64;
    for n in 0..stts.1 {
        let base = stts.0 + n as usize * 8;
        let amount = u32be(data, base)?;
        let delta = u32be(data, base + 4)?;
        if amount == 0 || delta == 0 || delta > 5760 {
            return Err(invalid("Opus time-to-sample entry invalid"));
        }
        samples += u64::from(amount);
        duration += u64::from(amount) * u64::from(delta);
    }
    if samples != u64::from(count)
        || duration > 48000 * 86400
        || media_time as u64 >= duration
        || edit_duration == 0
        || edit_duration > 86400 * u64::from(movie_scale)
    {
        return Err(unsupported("Opus duration or edit range unsupported"));
    }
    let mut byte_prefix = Vec::with_capacity((count / STRIDE + 1) as usize);
    let mut total = 0u64;
    for n in 0..count {
        if n % STRIDE == 0 {
            byte_prefix.push(total);
        }
        let size = if fixed_size > 0 {
            fixed_size
        } else {
            u32be(data, stsz.payload + 12 + n as usize * 4)?
        };
        if size == 0 || size > 65536 {
            return Err(unsupported("Opus packet size unsupported"));
        }
        total += u64::from(size);
    }
    let mut track = Track {
        id,
        count,
        media_time: media_time as u64,
        duration,
        stts,
        stsc,
        sizes: stsz.payload + 12,
        fixed_size,
        offsets,
        wide_offsets,
        byte_prefix,
        init: Bytes::new(),
    };
    track.validate_chunks(data, mdats)?;
    // Initialization has no samples. Retain a track-level roll description,
    // but all real sample mappings belong to subsequent movie fragments.
    let roll = roll_boxes(&[4], &[0], false);
    let mut sample_table = stsd.raw(data).to_vec();
    for kind in [b"stts", b"stsc", b"stco"] {
        sample_table.extend(make_box(kind, &[0; 8]));
    }
    sample_table.extend(make_box(b"stsz", &[0; 12]));
    sample_table.extend(roll);
    let mut media_info = one(&minf, b"smhd")?.raw(data).to_vec();
    media_info.extend(dinf.raw(data));
    media_info.extend(make_box(b"stbl", &sample_table));
    let mut media = mdhd.raw(data).to_vec();
    media.extend(hdlr.raw(data));
    media.extend(make_box(b"minf", &media_info));
    let mut trk = tkhd.raw(data).to_vec();
    trk.extend(edts.raw(data));
    trk.extend(make_box(b"mdia", &media));
    let mut trex = vec![0; 4];
    trex.extend(id.to_be_bytes());
    trex.extend(1u32.to_be_bytes());
    trex.extend([0; 12]);
    let mut movie = mvhd.raw(data).to_vec();
    movie.extend(make_box(b"trak", &trk));
    movie.extend(make_box(b"mvex", &make_box(b"trex", &trex)));
    let mut init = make_box(b"ftyp", b"iso6\0\0\0\0iso6iso2mp41");
    init.extend(make_box(b"moov", &movie));
    track.init = Bytes::from(init);
    Ok(track)
}

fn make_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend(((8 + payload.len()) as u32).to_be_bytes());
    bytes.extend(kind);
    bytes.extend(payload);
    bytes
}
fn roll_boxes(distances: &[u16], counts: &[u32], fragment_local: bool) -> Vec<u8> {
    let mut unique = Vec::<u16>::new();
    let mut groups = Vec::<(u32, u32)>::new();
    for (&distance, &count) in distances.iter().zip(counts) {
        let index = if let Some(index) = unique.iter().position(|n| *n == distance) {
            index
        } else {
            unique.push(distance);
            unique.len() - 1
        };
        // ISO BMFF sample group references 1..65535 select the track's stbl
        // descriptions; 65537 and above select descriptions in this traf.
        // Our per-packet roll distances belong to the fragment, not init's -4.
        let index = index as u32 + 1 + if fragment_local { 0x10000 } else { 0 };
        if count == 0 {
            continue;
        }
        if let Some(last) = groups.last_mut().filter(|last| last.1 == index) {
            last.0 += count;
        } else {
            groups.push((count, index));
        }
    }
    let mut description = vec![1, 0, 0, 0];
    description.extend(b"roll");
    description.extend(2u32.to_be_bytes());
    description.extend((unique.len() as u32).to_be_bytes());
    for distance in unique {
        description.extend((-(distance as i16)).to_be_bytes());
    }
    let mut grouping = vec![0; 4];
    grouping.extend(b"roll");
    grouping.extend((groups.len() as u32).to_be_bytes());
    for (count, index) in groups {
        grouping.extend(count.to_be_bytes());
        grouping.extend(index.to_be_bytes());
    }
    let mut bytes = make_box(b"sgpd", &description);
    bytes.extend(make_box(b"sbgp", &grouping));
    bytes
}

impl Track {
    fn size(&self, data: &[u8], sample: u32) -> io::Result<u32> {
        if sample >= self.count {
            return Err(invalid("sample outside MP4 table"));
        }
        if self.fixed_size > 0 {
            Ok(self.fixed_size)
        } else {
            u32be(data, self.sizes + sample as usize * 4)
        }
    }
    fn byte_sum(&self, data: &[u8], end: u32) -> io::Result<u64> {
        if end > self.count {
            return Err(invalid("sample prefix outside table"));
        }
        if end == 0 {
            return Ok(0);
        }
        let start = (end - 1) / STRIDE * STRIDE;
        let mut total = self.byte_prefix[(start / STRIDE) as usize];
        for n in start..end {
            total += u64::from(self.size(data, n)?);
        }
        Ok(total)
    }
    fn offset(&self, data: &[u8], chunk: u32) -> io::Result<u64> {
        if chunk >= self.offsets.1 {
            return Err(invalid("chunk outside offset table"));
        }
        if self.wide_offsets {
            u64be(data, self.offsets.0 + chunk as usize * 8)
        } else {
            Ok(u64::from(u32be(data, self.offsets.0 + chunk as usize * 4)?))
        }
    }
    fn chunk_runs(&self, data: &[u8]) -> io::Result<Vec<(u32, u32, u32, u32)>> {
        let mut runs = Vec::new();
        let mut sample = 0u64;
        for n in 0..self.stsc.1 {
            let at = self.stsc.0 + n as usize * 12;
            let first = u32be(data, at)?;
            let per = u32be(data, at + 4)?;
            let next = if n + 1 < self.stsc.1 {
                u32be(data, at + 12)?
            } else {
                self.offsets.1 + 1
            };
            if (n == 0 && first != 1)
                || first == 0
                || next <= first
                || next > self.offsets.1 + 1
                || per == 0
                || u32be(data, at + 8)? != 1
            {
                return Err(invalid("sample-to-chunk entry invalid"));
            }
            let following = sample + u64::from(next - first) * u64::from(per);
            if following > u64::from(self.count) {
                return Err(invalid("sample-to-chunk count overflow"));
            }
            runs.push((first - 1, next - 1, per, sample as u32));
            sample = following;
        }
        if sample != u64::from(self.count) {
            return Err(invalid("sample-to-chunk total mismatch"));
        }
        Ok(runs)
    }
    fn validate_chunks(&self, data: &[u8], mdats: &[Range<u64>]) -> io::Result<()> {
        let mut previous_end = 0u64;
        for (first, end, per, base) in self.chunk_runs(data)? {
            for chunk in first..end {
                let sample = base + (chunk - first) * per;
                let mut size = 0u64;
                for ordinal in sample..sample + per {
                    size += u64::from(self.size(data, ordinal)?);
                }
                let offset = self.offset(data, chunk)?;
                let finish = offset
                    .checked_add(size)
                    .ok_or_else(|| invalid("MP4 chunk overflow"))?;
                if offset < previous_end
                    || !mdats
                        .iter()
                        .any(|mdat| offset >= mdat.start && finish <= mdat.end)
                {
                    return Err(invalid("MP4 audio chunk outside media data or unordered"));
                }
                previous_end = finish;
            }
        }
        Ok(())
    }
    fn window(&self, data: &[u8], chunk: u32) -> io::Result<Window> {
        if chunk >= 8640 {
            return Err(unsupported("audio window outside supported timeline"));
        }
        let target = u64::from(chunk) * 10 * 48000 + self.media_time;
        if target >= self.duration {
            return Err(unsupported("audio window past end of track"));
        }
        let begin = target.saturating_sub(48000);
        let end = target.saturating_add(11 * 48000).min(self.duration);
        let mut selected = Vec::<Sample>::new();
        let mut pts = 0u64;
        let mut ordinal = 0u32;
        let mut preceding = VecDeque::<u32>::new();
        for n in 0..self.stts.1 {
            let base = self.stts.0 + n as usize * 8;
            let count = u32be(data, base)?;
            let delta = u32be(data, base + 4)?;
            if pts + u64::from(count) * u64::from(delta) <= begin {
                pts += u64::from(count) * u64::from(delta);
                ordinal += count;
                preceding.clear();
                for _ in 0..count.min(32) {
                    preceding.push_back(delta);
                }
                continue;
            }
            let skip = if pts < begin {
                ((begin - pts) / u64::from(delta)).min(u64::from(count)) as u32
            } else {
                0
            };
            pts += u64::from(skip) * u64::from(delta);
            ordinal += skip;
            if skip > 0 {
                preceding.clear();
                for _ in 0..skip.min(32) {
                    preceding.push_back(delta);
                }
            }
            for _ in skip..count {
                if pts >= end {
                    break;
                }
                if selected.len() >= MAX_WINDOW_SAMPLES {
                    return Err(unsupported("audio window sample limit"));
                }
                let mut preroll = 0u64;
                let mut distance = 0u16;
                for duration in preceding.iter().rev() {
                    preroll += u64::from(*duration);
                    distance += 1;
                    if preroll >= 3840 {
                        break;
                    }
                }
                if preroll < 3840 {
                    distance += ((3840 - preroll).div_ceil(u64::from(delta))) as u16;
                }
                selected.push(Sample {
                    ordinal,
                    offset: 0,
                    size: self.size(data, ordinal)?,
                    duration: delta,
                    dts: pts,
                    roll: distance,
                });
                preceding.push_back(delta);
                if preceding.len() > 32 {
                    preceding.pop_front();
                }
                pts += u64::from(delta);
                ordinal += 1;
            }
            if pts >= end {
                break;
            }
        }
        if selected.is_empty() {
            return Err(unsupported("empty native audio window"));
        }
        let mut total = 0u64;
        let runs = self.chunk_runs(data)?;
        for sample in &mut selected {
            let run = runs.partition_point(|&(_, _, _, base)| base <= sample.ordinal);
            let &(first, _, per, base) = runs
                .get(run.saturating_sub(1))
                .ok_or_else(|| invalid("sample missing chunk"))?;
            let chunk = first + (sample.ordinal - base) / per;
            let chunk_first = base + (chunk - first) * per;
            sample.offset = self.offset(data, chunk)? + self.byte_sum(data, sample.ordinal)?
                - self.byte_sum(data, chunk_first)?;
            total += u64::from(sample.size);
            if total > MAX_WINDOW_BYTES {
                return Err(unsupported("native audio window exceeds 8 MiB"));
            }
        }
        let mut tfhd = vec![0, 2, 0, 0];
        tfhd.extend(self.id.to_be_bytes());
        let mut tfdt = vec![1, 0, 0, 0];
        tfdt.extend(selected[0].dts.to_be_bytes());
        let mut run = vec![0, 0, 3, 1];
        run.extend((selected.len() as u32).to_be_bytes());
        run.extend(0u32.to_be_bytes());
        for sample in &selected {
            run.extend(sample.duration.to_be_bytes());
            run.extend(sample.size.to_be_bytes());
        }
        let rolls = roll_boxes(
            &selected.iter().map(|s| s.roll).collect::<Vec<_>>(),
            &vec![1; selected.len()],
            true,
        );
        let mut traf = make_box(b"tfhd", &tfhd);
        traf.extend(make_box(b"tfdt", &tfdt));
        let run_at = traf.len() + 8;
        traf.extend(make_box(b"trun", &run));
        traf.extend(rolls);
        let mut mfhd = vec![0; 4];
        mfhd.extend((chunk + 1).to_be_bytes());
        let mut moof = make_box(b"mfhd", &mfhd);
        let traf_at = moof.len() + 8;
        moof.extend(make_box(b"traf", &traf));
        moof = make_box(b"moof", &moof);
        let data_offset = (moof.len() + 8) as u32;
        let offset_at = 8 + traf_at + run_at + 8;
        moof[offset_at..offset_at + 4].copy_from_slice(&data_offset.to_be_bytes());
        let mut prefix = self.init.to_vec();
        prefix.extend(moof);
        prefix.extend(((total + 8) as u32).to_be_bytes());
        prefix.extend(b"mdat");
        Ok(Window {
            prefix: Bytes::from(prefix),
            samples: selected,
            total,
        })
    }
}
struct Sample {
    ordinal: u32,
    offset: u64,
    size: u32,
    duration: u32,
    dts: u64,
    roll: u16,
}
struct Window {
    prefix: Bytes,
    samples: Vec<Sample>,
    total: u64,
}

pub fn supported_tracks(file: File) -> io::Result<Vec<u32>> {
    Ok(load_index(file)?
        .tracks
        .iter()
        .map(|track| track.id)
        .collect())
}

struct Cached {
    key: String,
    index: Arc<Index>,
    used: Instant,
}
#[derive(Default)]
pub struct NativeAudio {
    cache: Mutex<Vec<Cached>>,
    indexing: Mutex<()>,
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
                let _indexing = self.indexing.lock().await;
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
                    let index = Arc::new(
                        tokio::task::spawn_blocking(move || load_index(file))
                            .await
                            .map_err(|_| io::Error::other("native MP4 indexing task failed"))??,
                    );
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

struct Extent {
    offset: u64,
    length: usize,
}
struct Batch {
    offset: u64,
    length: usize,
    slices: Vec<Range<usize>>,
}
struct VirtualBody {
    file: tokio::fs::File,
    prefix: Bytes,
    batches: Vec<Batch>,
    batch: usize,
    seeking: bool,
    reading: bool,
    filled: usize,
    buffer: Vec<u8>,
    remaining: u64,
    _lease: Option<Arc<crate::cache_dirs::CacheDirectory>>,
}
impl VirtualBody {
    fn new(mut opened: OpenedFile, window: Window, start: u64, length: u64) -> io::Result<Self> {
        let end = start + length;
        let prefix_end = (window.prefix.len() as u64).min(end);
        let prefix = if start < prefix_end {
            window.prefix.slice(start as usize..prefix_end as usize)
        } else {
            Bytes::new()
        };
        let mut position = window.prefix.len() as u64;
        let mut extents: Vec<Extent> = Vec::new();
        for sample in window.samples {
            let next = position + u64::from(sample.size);
            let left = start.max(position);
            let right = end.min(next);
            if left < right {
                let offset = sample.offset + left - position;
                let length = (right - left) as usize;
                if let Some(last) = extents
                    .last_mut()
                    .filter(|last| last.offset + last.length as u64 == offset)
                {
                    last.length += length;
                } else {
                    extents.push(Extent { offset, length });
                }
            }
            position = next;
        }
        let mut batches: Vec<Batch> = Vec::new();
        for extent in extents {
            let mut offset = extent.offset;
            let mut remaining = extent.length;
            while remaining > 0 {
                let take = remaining.min(BLOCK);
                if let Some(last) = batches.last_mut().filter(|last| {
                    offset >= last.offset + last.length as u64
                        && offset + take as u64 - last.offset <= BLOCK as u64
                }) {
                    let begin = (offset - last.offset) as usize;
                    last.slices.push(begin..begin + take);
                    last.length = begin + take;
                } else {
                    batches.push(Batch {
                        offset,
                        length: take,
                        slices: std::iter::once(0..take).collect(),
                    });
                }
                offset += take as u64;
                remaining -= take;
            }
        }
        let lease = opened.take_cache_lease();
        Ok(Self {
            file: opened.into_tokio_file(),
            prefix,
            batches,
            batch: 0,
            seeking: false,
            reading: false,
            filled: 0,
            buffer: vec![0; BLOCK],
            remaining: length,
            _lease: lease,
        })
    }
}
impl http_body::Body for VirtualBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<io::Result<http_body::Frame<Bytes>>>> {
        use std::{pin::Pin, task::Poll};
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        if !self.prefix.is_empty() {
            let take = self.prefix.len().min(BLOCK);
            let data = self.prefix.split_to(take);
            self.remaining -= data.len() as u64;
            return Poll::Ready(Some(Ok(http_body::Frame::data(data))));
        }
        let this = &mut *self;
        let Some(batch) = this.batches.get(this.batch) else {
            this.remaining = 0;
            return Poll::Ready(Some(Err(invalid("native audio virtual body truncated"))));
        };
        if !this.seeking && !this.reading {
            if let Err(error) = Pin::new(&mut this.file).start_seek(SeekFrom::Start(batch.offset)) {
                this.remaining = 0;
                return Poll::Ready(Some(Err(error)));
            }
            this.seeking = true;
        }
        if this.seeking {
            match Pin::new(&mut this.file).poll_complete(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    this.remaining = 0;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(Ok(_)) => {
                    this.seeking = false;
                    this.reading = true;
                }
            }
        }
        while this.filled < batch.length {
            let mut read = tokio::io::ReadBuf::new(&mut this.buffer[this.filled..batch.length]);
            match Pin::new(&mut this.file).poll_read(cx, &mut read) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    this.remaining = 0;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(Ok(())) => {
                    let n = read.filled().len();
                    if n == 0 {
                        this.remaining = 0;
                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "source truncated during native audio stream",
                        ))));
                    }
                    this.filled += n;
                }
            }
        }
        let total = batch.slices.iter().map(|slice| slice.len()).sum();
        let mut output = Vec::with_capacity(total);
        for slice in &batch.slices {
            output.extend_from_slice(&this.buffer[slice.clone()]);
        }
        this.batch += 1;
        this.filled = 0;
        this.reading = false;
        this.remaining -= output.len() as u64;
        Poll::Ready(Some(Ok(http_body::Frame::data(Bytes::from(output)))))
    }
    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }
    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use std::{path::Path, process::Stdio};
    use tokio::process::Command;

    #[test]
    fn variable_packet_durations_resolve_fragment_local_roll_descriptions() {
        // One legal Opus timing sequence with 10, 20 and 40 ms packets. A seek
        // needs three prior packets at sample 3, then four at 4..6, five at 7.
        // Reusing init's generic -4 would be wrong even though hashes/PTS pass.
        let deltas = [960u32, 960, 1920, 480, 480, 960, 960, 960];
        let mut data = Vec::new();
        for delta in deltas {
            data.extend(1u32.to_be_bytes());
            data.extend(delta.to_be_bytes());
        }
        let stsc = data.len();
        data.extend(1u32.to_be_bytes());
        data.extend(8u32.to_be_bytes());
        data.extend(1u32.to_be_bytes());
        let offsets = data.len();
        data.extend(1024u32.to_be_bytes());
        let global = roll_boxes(&[4], &[0], false);
        let track = Track {
            id: 2,
            count: 8,
            media_time: 0,
            duration: deltas.iter().map(|d| u64::from(*d)).sum(),
            stts: (0, 8),
            stsc: (stsc, 1),
            sizes: 0,
            fixed_size: 1,
            offsets: (offsets, 1),
            wide_offsets: false,
            byte_prefix: vec![0],
            init: Bytes::copy_from_slice(&global),
        };
        let window = track.window(&data, 0).unwrap();
        let expected = window
            .samples
            .iter()
            .map(|sample| -(sample.roll as i16))
            .collect::<Vec<_>>();
        assert_eq!(&expected[3..], &[-3, -4, -4, -4, -5]);
        // The virtual prefix ends at the mdat header; payload is streamed later.
        let boxes = atoms(&window.prefix, 0..window.prefix.len() - 8).unwrap();
        let moof = one(&boxes, b"moof").unwrap();
        let traf = one(&children(&window.prefix, moof).unwrap(), b"traf").unwrap();
        let fragment = children(&window.prefix, traf).unwrap();
        let global_descriptions = one(&boxes, b"sgpd").unwrap().data(&window.prefix);
        let descriptions = one(&fragment, b"sgpd").unwrap().data(&window.prefix);
        let mapping = one(&fragment, b"sbgp").unwrap().data(&window.prefix);
        let count = u32be(mapping, 8).unwrap();
        let mut resolved = Vec::new();
        for entry in 0..count {
            let base = 12 + entry as usize * 8;
            let samples = u32be(mapping, base).unwrap();
            let index = u32be(mapping, base + 4).unwrap();
            assert!(
                index >= 0x10001,
                "traf must reference its own variable roll descriptions"
            );
            let (table, index) = if index >= 0x10001 {
                (descriptions, index - 0x10000)
            } else {
                (global_descriptions, index)
            };
            assert!(index > 0 && index <= u32be(table, 12).unwrap());
            let distance = u16be(table, 16 + (index - 1) as usize * 2).unwrap() as i16;
            resolved.extend(std::iter::repeat_n(distance, samples as usize));
        }
        assert_eq!(resolved, expected);
        let global_mapping = one(&boxes, b"sbgp").unwrap().data(&window.prefix);
        assert_eq!(
            u32be(global_mapping, 8).unwrap(),
            0,
            "initialization maps zero samples"
        );
        assert_eq!(u32be(global_descriptions, 12).unwrap(), 1);
        assert_eq!(
            u16be(global_descriptions, 16).unwrap() as i16,
            -4,
            "track-level roll description stays available"
        );
    }

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/representative")
    }
    async fn opened() -> OpenedFile {
        OpenedFile::open_in_root(&root(), Path::new("representative.mp4"))
            .await
            .unwrap()
    }
    async fn packets(file: &OpenedFile, track: &str) -> serde_json::Value {
        let mut input = file.try_clone_std().unwrap();
        input.rewind().unwrap();
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "fd,pipe",
                "-select_streams",
                track,
                "-show_streams",
                "-show_packets",
                "-show_data_hash",
                "sha256",
                "-show_entries",
                "stream=channels,codec_name:packet=pts_time,dts_time,data_hash",
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
        serde_json::from_slice(&output.stdout).unwrap()
    }
    #[tokio::test]
    async fn native_windows_preserve_opus_packets_timestamps_channels_and_track_ids() {
        let native = NativeAudio::default();
        assert_eq!(
            supported_tracks(opened().await.try_clone_std().unwrap()).unwrap(),
            vec![2, 3]
        );
        let temp = tempfile::tempdir().unwrap();
        for (track, selection, channels) in [(2, "a:0", 6), (3, "a:1", 2)] {
            let source = packets(&opened().await, selection).await;
            for chunk in [0, 1] {
                let response = native
                    .serve(opened().await, track, chunk, &Request::new(()))
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let length = response.headers()[header::CONTENT_LENGTH]
                    .to_str()
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(length, bytes.len());
                std::fs::write(temp.path().join("audio.mp4"), &bytes).unwrap();
                let remux = OpenedFile::open_in_root(temp.path(), Path::new("audio.mp4"))
                    .await
                    .unwrap();
                let copied = packets(&remux, "a:0").await;
                assert_eq!(copied["streams"][0]["codec_name"], "opus");
                assert_eq!(copied["streams"][0]["channels"], channels);
                let actual = copied["packets"].as_array().unwrap();
                assert!(!actual.is_empty());
                if chunk == 1 {
                    assert!(
                        actual[0]["pts_time"]
                            .as_str()
                            .unwrap()
                            .parse::<f64>()
                            .unwrap()
                            > 8.9,
                        "timestamps must stay absolute"
                    );
                }
                for packet in actual {
                    assert!(
                        source["packets"].as_array().unwrap().iter().any(
                            |original| original["data_hash"] == packet["data_hash"]
                                && (original["pts_time"]
                                    .as_str()
                                    .unwrap()
                                    .parse::<f64>()
                                    .unwrap()
                                    - packet["pts_time"].as_str().unwrap().parse::<f64>().unwrap())
                                .abs()
                                    < 0.000022
                        ),
                        "packet payload and presentation timestamp must match source: {packet}"
                    );
                }
            }
        }
        assert_eq!(native.cache.lock().await.len(), 1);
    }
    #[tokio::test]
    async fn virtual_ranges_match_full_representation_head_and_cancellation() {
        let native = NativeAudio::default();
        let response = native
            .serve(opened().await, 2, 0, &Request::new(()))
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        for (start, end) in [(0, 11), (512, 4096), (bytes.len() - 99, bytes.len() - 1)] {
            let request = Request::builder()
                .header(header::RANGE, format!("bytes={start}-{end}"))
                .body(())
                .unwrap();
            let response = native.serve(opened().await, 2, 0, &request).await.unwrap();
            assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                bytes.slice(start..end + 1)
            );
        }
        let head = Request::builder().method(Method::HEAD).body(()).unwrap();
        let response = native.serve(opened().await, 2, 0, &head).await.unwrap();
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            bytes.len().to_string()
        );
        assert!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        for range in ["bytes=999999999-", "bytes=5-1", "bytes=1-2,3-4"] {
            let request = Request::builder()
                .header(header::RANGE, range)
                .body(())
                .unwrap();
            let response = native.serve(opened().await, 2, 0, &request).await.unwrap();
            assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        }
        let mut response = native
            .serve(opened().await, 2, 0, &Request::new(()))
            .await
            .unwrap();
        response.body_mut().frame().await.unwrap().unwrap();
        drop(response);
        let response = native
            .serve(opened().await, 3, 1, &Request::new(()))
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
                < MAX_WINDOW_BYTES as usize
        );
    }
    #[tokio::test]
    async fn unsupported_tracks_out_of_bounds_windows_and_changed_sources_fail() {
        let native = NativeAudio::default();
        assert_eq!(
            native
                .serve(opened().await, 1, 0, &Request::new(()))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            native
                .serve(opened().await, 2, 8640, &Request::new(()))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        assert_eq!(
            native
                .serve(opened().await, 2, 2, &Request::new(()))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::Unsupported
        );
        let temp = tempfile::tempdir().unwrap();
        std::fs::copy(
            root().join("representative.mp4"),
            temp.path().join("changed.mp4"),
        )
        .unwrap();
        let file = OpenedFile::open_in_root(temp.path(), Path::new("changed.mp4"))
            .await
            .unwrap();
        std::fs::write(temp.path().join("changed.mp4"), b"changed").unwrap();
        assert!(native.serve(file, 2, 0, &Request::new(())).await.is_err());
    }
    #[test]
    fn rejects_malformed_boxes_huge_moov_external_references_and_invalid_tables() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bad.mp4");
        for bytes in [
            vec![0; 7],
            vec![0, 0, 0, 4, b'm', b'o', b'o', b'v'],
            vec![1, 0, 0, 0, b'm', b'o', b'o', b'v'],
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(supported_tracks(File::open(&path).unwrap()).is_err());
        }
        let source = std::fs::read(root().join("representative.mp4")).unwrap();
        for kind in [b"dOps", b"stts", b"stsc", b"stsz", b"stco", b"dref"] {
            let mut bad = source.clone();
            // Invalid version or flags in each required table/config/reference.
            for at in source
                .windows(4)
                .enumerate()
                .filter_map(|(at, bytes)| (bytes == kind).then_some(at))
            {
                bad[at + 4] = 255;
            }
            std::fs::write(&path, bad).unwrap();
            let ids = supported_tracks(File::open(&path).unwrap());
            assert!(
                ids.is_err() || !ids.unwrap().contains(&2),
                "mutated {kind:?} must never advertise track 2"
            );
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_native_windows_share_one_index_without_a_preparation_worker() {
        let native = Arc::new(NativeAudio::default());
        let mut tasks = tokio::task::JoinSet::new();
        for n in 0..8 {
            let native = native.clone();
            tasks.spawn(async move {
                let response = native
                    .serve(
                        opened().await,
                        if n % 2 == 0 { 2 } else { 3 },
                        n % 2,
                        &Request::new(()),
                    )
                    .await
                    .unwrap();
                let mut body = response.into_body();
                let mut total = 0;
                while let Some(frame) = body.frame().await {
                    total += frame.unwrap().into_data().unwrap().len();
                }
                assert!(total > 0);
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
        assert_eq!(native.cache.lock().await.len(), 1);
    }
}
