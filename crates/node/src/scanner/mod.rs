//! Local Media Node discovery. No writes, database access, or HTTP concerns.
//!
//! Module layout:
//! - `folder`: per-directory video index and artwork memoisation.
//! - `filter`: ignore markers and exclusion patterns.
//! - `naming`: release-name parsing and movie identity keys.
//! - `series`: series/season/episode filename parsing.
//! - `walk`: streaming traversal producing inventory batches.
pub use crate::files::confined_file;
pub use media_contracts::inventory::{
    CachedFile, InventoryBatch, ScanIssue, ScanReport, ScannedFile,
};

mod filter;
mod folder;
mod naming;
mod series;
#[cfg(test)]
mod tests;
mod walk;

use folder::*;
use naming::*;

pub(crate) use filter::{directory_is_ignored, ignored, is_excluded};
pub use folder::FolderVideos;
pub(crate) use naming::{identity, is_generic_video_stem};
pub use series::{ParsedSeriesEpisode, parse_series_episode};
pub use walk::{MAX_REPORT_ISSUES, scan, scan_streaming, scan_with_images};
