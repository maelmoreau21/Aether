//! Options supported by the local ingestion path; unsupported writers are rejected.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct LibraryOptions {
    pub preferred_metadata_language: String,
    pub metadata_country_code: String,
    pub read_local_nfo: bool,
    pub fetch_poster: bool,
    pub fetch_backdrop: bool,
    pub fetch_logo: bool,
    pub fetch_thumb: bool,
    pub fetch_banner: bool,
    pub fetch_disc: bool,
    pub automatic_refresh_interval_days: u16,
}
impl Default for LibraryOptions {
    fn default() -> Self {
        Self {
            preferred_metadata_language: "fr".into(),
            metadata_country_code: "FR".into(),
            read_local_nfo: true,
            fetch_poster: true,
            fetch_backdrop: true,
            fetch_logo: true,
            fetch_thumb: true,
            fetch_banner: true,
            fetch_disc: true,
            automatic_refresh_interval_days: 0,
        }
    }
}
impl LibraryOptions {
    pub fn filter_file(&self, file: &mut media_contracts::inventory::ScannedFile) {
        if !self.read_local_nfo {
            file.editorial = media_contracts::media::NfoOutcome::Missing;
            file.series_editorial = None;
            if file.series_identity_key.is_some()
                && let Some(parsed) = media_node::scanner::parse_series_episode(
                    std::path::Path::new(&file.relative_path),
                )
            {
                file.title = parsed.episode_title;
                file.year = parsed.series_year;
                file.series_title = Some(parsed.series_raw_title);
                file.series_year = parsed.series_year;
                file.series_external_ids = Some(parsed.series_external_ids);
            }
        }
        if !self.fetch_poster {
            file.poster = None;
            file.series_poster = None;
        }
        file.artwork
            .retain(|role, _| self.image_enabled(role.strip_prefix("series/").unwrap_or(role)));
    }
    pub fn validate(&self) -> bool {
        let language = &self.preferred_metadata_language;
        (2..=35).contains(&language.len())
            && language.split('-').next().is_some_and(|part| {
                (2..=8).contains(&part.len()) && part.bytes().all(|c| c.is_ascii_alphabetic())
            })
            && language.split('-').all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|c| c.is_ascii_alphanumeric())
            })
            && self.metadata_country_code.len() == 2
            && self
                .metadata_country_code
                .bytes()
                .all(|c| c.is_ascii_uppercase())
            && matches!(self.automatic_refresh_interval_days, 0 | 30 | 60 | 90)
    }
    pub fn image_enabled(&self, kind: &str) -> bool {
        match kind {
            "poster" => self.fetch_poster,
            "backdrop" => self.fetch_backdrop,
            "logo" => self.fetch_logo,
            "thumb" => self.fetch_thumb,
            "banner" => self.fetch_banner,
            "disc" => self.fetch_disc,
            _ => true,
        }
    }
}
