//! Read-only, bounded ISO BMFF Opus packet extraction. No decoder, subprocess,
//! temporary media, or whole-window payload buffer is involved.
//!
//! This intentionally accepts a narrow non-fragmented MP4 subset. Unsupported
//! layouts fail explicitly, so callers can retain the existing packet-copy path.
//!
//! Module layout:
//! - `atoms`: big-endian readers and ISO BMFF atom walking.
//! - `track`: sample-table indexing and per-track fragment construction.
//! - `audio`: `NativeAudio` entry point (index cache, range requests).
//! - `body`: lazily generated virtual response body.

const MAX_MOOV: usize = 16 * 1024 * 1024;
const MAX_CACHE: usize = 32 * 1024 * 1024;
const MAX_SAMPLES: u32 = 4_000_000;
const MAX_WINDOW_SAMPLES: usize = 8192;
const MAX_WINDOW_BYTES: u64 = 8 * 1024 * 1024;
const BLOCK: usize = 65_536;
const STRIDE: u32 = 256;

mod atoms;
mod audio;
mod body;
#[cfg(test)]
mod tests;
mod track;

use atoms::*;
use body::*;
use track::*;

#[cfg(test)]
use audio::guarded_index_task;

pub use audio::NativeAudio;
pub use track::supported_tracks;
