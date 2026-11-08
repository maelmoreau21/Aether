//! Core inventory: scan publication, catalogue queries and authorized media delivery.
//!
//! Public paths remain stable here. Submodules depend on explicit imports; the
//! publication transaction owns editorial/consolidation and automation writes.

mod catalogue;
mod details;
mod editorial;
mod equivalence;
mod playback;
mod publication;
mod relocation;
mod resources;
mod scan;

pub use crate::{
    identity::IdentityStore,
    media::Probe,
    node::nfo::NfoOutcome,
    node::scanner::{CachedFile, InventoryBatch, ScanReport, ScannedFile},
};
pub use catalogue::{
    BoundedCatalogueCache, CatalogueFilter, EpisodePage, catalogue, catalogue_cache_stats,
    catalogue_filtered, catalogue_page, detail, detail_window, detail_window_localized,
    estimate_json_bytes, invalidate_catalogue_cache, resource_library,
};
pub(crate) use catalogue::{
    refresh_work_availability_from_editions, sort_edition_sources_by_score,
};
pub use details::asset_details;
pub use editorial::{editorial_warnings, refresh_editorial_for_works, refresh_work_editorial};
pub(crate) use playback::{
    audio_operational_id, report_track_allowed, usable_marker, valid_chapters,
};
pub use playback::{descriptor, descriptor_for_user};
pub use publication::publish_staged_scan;
pub use resources::{audio, content, image, prepared, subtitle};
pub use scan::{
    backfill_title_sort_unicode, clean_staging, ingest, load_cache, load_image_cache,
    load_scan_cache, stage_batch,
};

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn decode(text: &str) -> Result<Probe, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

#[cfg(test)]
mod tests;
