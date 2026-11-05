use super::*;
use std::{
    io::{self},
    ops::Range,
};

pub(super) fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

pub(super) fn unsupported(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, reason)
}

pub(super) fn u16be(bytes: &[u8], at: usize) -> io::Result<u16> {
    Ok(u16::from_be_bytes(
        bytes
            .get(at..at + 2)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}

pub(super) fn u32be(bytes: &[u8], at: usize) -> io::Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}

pub(super) fn u64be(bytes: &[u8], at: usize) -> io::Result<u64> {
    Ok(u64::from_be_bytes(
        bytes
            .get(at..at + 8)
            .ok_or_else(|| invalid("short MP4 field"))?
            .try_into()
            .map_err(|_| invalid("short MP4 field"))?,
    ))
}

#[derive(Clone, Copy)]
pub(super) struct Atom {
    pub(super) start: usize,
    pub(super) payload: usize,
    pub(super) end: usize,
    pub(super) kind: [u8; 4],
}

impl Atom {
    pub(super) fn data(self, data: &[u8]) -> &[u8] {
        &data[self.payload..self.end]
    }
    pub(super) fn raw(self, data: &[u8]) -> &[u8] {
        &data[self.start..self.end]
    }
}

pub(super) fn atoms(data: &[u8], range: Range<usize>) -> io::Result<Vec<Atom>> {
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

pub(super) fn one(list: &[Atom], kind: &[u8; 4]) -> io::Result<Atom> {
    let mut matching = list.iter().filter(|a| &a.kind == kind);
    let found = *matching
        .next()
        .ok_or_else(|| unsupported("required MP4 box missing"))?;
    if matching.next().is_some() {
        return Err(invalid("duplicate MP4 box"));
    }
    Ok(found)
}

pub(super) fn children(data: &[u8], parent: Atom) -> io::Result<Vec<Atom>> {
    atoms(data, parent.payload..parent.end)
}

pub(super) fn table(
    data: &[u8],
    atom: Atom,
    width: usize,
    prefix: usize,
) -> io::Result<(usize, u32)> {
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
