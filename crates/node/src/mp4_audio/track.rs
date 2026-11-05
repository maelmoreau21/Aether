use super::*;
use bytes::Bytes;
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
};

pub(super) struct Track {
    pub(super) id: u32,
    pub(super) count: u32,
    pub(super) media_time: u64,
    pub(super) duration: u64,
    pub(super) stts: (usize, u32),
    pub(super) stsc: (usize, u32),
    pub(super) sizes: usize,
    pub(super) fixed_size: u32,
    pub(super) offsets: (usize, u32),
    pub(super) wide_offsets: bool,
    pub(super) byte_prefix: Vec<u64>,
    pub(super) init: Bytes,
}

pub(super) struct Index {
    pub(super) data: Vec<u8>,
    pub(super) tracks: Vec<Track>,
    pub(super) bytes: usize,
}

pub(super) fn load_index(mut file: File) -> io::Result<Index> {
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

pub(super) fn parse_track(
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

pub(super) fn make_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend(((8 + payload.len()) as u32).to_be_bytes());
    bytes.extend(kind);
    bytes.extend(payload);
    bytes
}

pub(super) fn roll_boxes(distances: &[u16], counts: &[u32], fragment_local: bool) -> Vec<u8> {
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

pub(super) fn remember_durations(history: &mut VecDeque<u32>, delta: u32, count: u32) {
    if count >= 32 {
        history.clear();
    }
    for _ in 0..count.min(32) {
        history.push_back(delta);
        if history.len() > 32 {
            history.pop_front();
        }
    }
}

impl Track {
    pub(super) fn locate_samples(&self, data: &[u8], selected: &mut [Sample]) -> io::Result<u64> {
        let runs = self.chunk_runs(data)?;
        // Consecutive samples within one source chunk have consecutive byte
        // positions. At a chunk boundary there is no in-chunk prefix to sum.
        let mut cursor = None::<(u32, u32, u64)>;
        let mut total = 0u64;
        for sample in selected {
            let (limit, offset) = if let Some((limit, _, offset)) =
                cursor.filter(|&(limit, next, _)| sample.ordinal == next && sample.ordinal < limit)
            {
                (limit, offset)
            } else {
                let run = runs.partition_point(|&(_, _, _, base)| base <= sample.ordinal);
                let &(first, _, per, base) = runs
                    .get(run.saturating_sub(1))
                    .ok_or_else(|| invalid("sample missing chunk"))?;
                let chunk = first + (sample.ordinal - base) / per;
                let chunk_first = base + (chunk - first) * per;
                let displacement = if sample.ordinal == chunk_first {
                    0
                } else {
                    self.byte_sum(data, sample.ordinal)? - self.byte_sum(data, chunk_first)?
                };
                (chunk_first + per, self.offset(data, chunk)? + displacement)
            };
            sample.offset = offset;
            cursor = Some((limit, sample.ordinal + 1, offset + u64::from(sample.size)));
            total += u64::from(sample.size);
            if total > MAX_WINDOW_BYTES {
                return Err(unsupported("native audio window exceeds 8 MiB"));
            }
        }
        Ok(total)
    }

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
    pub(super) fn byte_sum(&self, data: &[u8], end: u32) -> io::Result<u64> {
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
    pub(super) fn offset(&self, data: &[u8], chunk: u32) -> io::Result<u64> {
        if chunk >= self.offsets.1 {
            return Err(invalid("chunk outside offset table"));
        }
        if self.wide_offsets {
            u64be(data, self.offsets.0 + chunk as usize * 8)
        } else {
            Ok(u64::from(u32be(data, self.offsets.0 + chunk as usize * 4)?))
        }
    }
    pub(super) fn chunk_runs(&self, data: &[u8]) -> io::Result<Vec<(u32, u32, u32, u32)>> {
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
    pub(super) fn window(&self, data: &[u8], chunk: u32) -> io::Result<Window> {
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
                remember_durations(&mut preceding, delta, count);
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
                remember_durations(&mut preceding, delta, skip);
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
        let total = self.locate_samples(data, &mut selected)?;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Sample {
    pub(super) ordinal: u32,
    pub(super) offset: u64,
    pub(super) size: u32,
    pub(super) duration: u32,
    pub(super) dts: u64,
    pub(super) roll: u16,
}

pub(super) struct Window {
    pub(super) prefix: Bytes,
    pub(super) samples: Vec<Sample>,
    pub(super) total: u64,
}

pub fn supported_tracks(file: File) -> io::Result<Vec<u32>> {
    Ok(load_index(file)?
        .tracks
        .iter()
        .map(|track| track.id)
        .collect())
}
