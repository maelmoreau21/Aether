//! Library configuration. Reserved preferences are persisted but do not grant media write access.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPreference {
    pub id: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedSubtitlePolicy {
    #[default]
    AllowAll,
    AllowText,
    BlockAll,
}

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
    pub prefer_embedded_titles: bool,
    pub embedded_subtitle_policy: EmbeddedSubtitlePolicy,
    pub metadata_providers: Vec<ProviderPreference>,
    pub image_providers: Vec<ProviderPreference>,
    pub similar_items_providers: Vec<ProviderPreference>,
    pub segment_providers: Vec<ProviderPreference>,
    pub subtitle_providers: Vec<ProviderPreference>,
    pub online_refresh_interval_days: u16,
    pub realtime_monitoring: bool,
    pub automatic_collections: bool,
    pub save_nfo_to_media: bool,
    pub save_images_to_media: bool,
    pub trickplay_enabled: bool,
    pub trickplay_during_scan: bool,
    pub trickplay_save_to_media: bool,
    pub trickplay_interval_seconds: u16,
    pub trickplay_width: u16,
    pub chapter_images_enabled: bool,
    pub chapter_images_during_scan: bool,
    pub chapter_images_save_to_media: bool,
    pub subtitle_languages: Vec<String>,
    pub subtitle_only_missing: bool,
    pub subtitle_match_audio: bool,
    pub subtitle_require_hash: bool,
    pub save_subtitles_to_media: bool,
    pub intro_detection_enabled: bool,
    pub subtitle_download_enabled: bool,
    pub poster_rotation_enabled: bool,
    pub artwork_deduplicate: bool,
    pub artwork_similarity: u8,
    pub artwork_min_poster_width: u16,
    pub artwork_min_poster_height: u16,
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
            prefer_embedded_titles: false,
            embedded_subtitle_policy: EmbeddedSubtitlePolicy::AllowAll,
            metadata_providers: Vec::new(),
            image_providers: Vec::new(),
            similar_items_providers: Vec::new(),
            segment_providers: Vec::new(),
            subtitle_providers: Vec::new(),
            online_refresh_interval_days: 0,
            realtime_monitoring: false,
            automatic_collections: false,
            save_nfo_to_media: false,
            save_images_to_media: false,
            trickplay_enabled: false,
            trickplay_during_scan: false,
            trickplay_save_to_media: false,
            trickplay_interval_seconds: 10,
            trickplay_width: 320,
            chapter_images_enabled: false,
            chapter_images_during_scan: false,
            chapter_images_save_to_media: false,
            subtitle_languages: vec!["fr".into()],
            subtitle_only_missing: true,
            subtitle_match_audio: false,
            subtitle_require_hash: true,
            save_subtitles_to_media: false,
            intro_detection_enabled: false,
            subtitle_download_enabled: false,
            poster_rotation_enabled: false,
            artwork_deduplicate: true,
            artwork_similarity: 5,
            artwork_min_poster_width: 0,
            artwork_min_poster_height: 0,
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
        let has_editorial_title = matches!(
            &file.editorial,
            media_contracts::media::NfoOutcome::Valid { metadata, .. }
                if metadata.title.as_ref().is_some_and(|title| !title.trim().is_empty())
        );
        if self.prefer_embedded_titles
            && !has_editorial_title
            && let Some(title) = file
                .probe
                .format
                .get("tags")
                .and_then(|tags| tags.as_object())
                .and_then(|tags| {
                    tags.iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case("title"))
                })
                .and_then(|(_, value)| value.as_str())
        {
            let title: String = title
                .chars()
                .filter(|character| !character.is_control())
                .take(512)
                .collect();
            let title = title.trim();
            if !title.is_empty() {
                file.title = title.to_owned();
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
        valid_language(&self.preferred_metadata_language)
            && self.metadata_country_code.len() == 2
            && self
                .metadata_country_code
                .bytes()
                .all(|c| c.is_ascii_uppercase())
            && matches!(self.automatic_refresh_interval_days, 0 | 30 | 60 | 90)
            && matches!(self.online_refresh_interval_days, 0 | 30 | 60 | 90)
            && (1..=60).contains(&self.trickplay_interval_seconds)
            && (160..=1920).contains(&self.trickplay_width)
            && self.artwork_similarity <= 16
            && self.artwork_min_poster_width <= 4096
            && self.artwork_min_poster_height <= 4096
            && self.subtitle_languages.len() <= 16
            && self
                .subtitle_languages
                .iter()
                .all(|language| valid_language(language))
            && self
                .subtitle_languages
                .iter()
                .enumerate()
                .all(|(index, language)| {
                    !self.subtitle_languages[..index]
                        .iter()
                        .any(|other| other.eq_ignore_ascii_case(language))
                })
            && [
                &self.metadata_providers,
                &self.image_providers,
                &self.similar_items_providers,
                &self.segment_providers,
                &self.subtitle_providers,
            ]
            .into_iter()
            .all(|providers| valid_providers(providers))
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
    pub fn embedded_subtitle_allowed(&self, codec: Option<&str>) -> bool {
        match self.embedded_subtitle_policy {
            EmbeddedSubtitlePolicy::AllowAll => true,
            EmbeddedSubtitlePolicy::BlockAll => false,
            EmbeddedSubtitlePolicy::AllowText => matches!(
                codec,
                Some("subrip" | "webvtt" | "ass" | "ssa" | "mov_text")
            ),
        }
    }
}

fn valid_language(language: &str) -> bool {
    (2..=35).contains(&language.len())
        && language.split('-').next().is_some_and(|part| {
            (2..=8).contains(&part.len()) && part.bytes().all(|c| c.is_ascii_alphabetic())
        })
        && language.split('-').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.bytes().all(|c| c.is_ascii_alphanumeric())
        })
}

fn valid_providers(providers: &[ProviderPreference]) -> bool {
    providers.len() <= 32
        && providers.iter().enumerate().all(|(index, provider)| {
            (1..=100).contains(&provider.id.len())
                && provider
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
                && !providers[..index]
                    .iter()
                    .any(|other| other.id == provider.id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_policy_defaults_and_bounds_are_stable() {
        let defaults: LibraryOptions = serde_json::from_str("{}").unwrap();
        assert!(defaults.artwork_deduplicate);
        assert_eq!(defaults.artwork_similarity, 5);
        assert_eq!(
            (
                defaults.artwork_min_poster_width,
                defaults.artwork_min_poster_height
            ),
            (0, 0)
        );
        let custom: LibraryOptions = serde_json::from_value(serde_json::json!({"artworkDeduplicate":false,"artworkSimilarity":16,"artworkMinPosterWidth":4096,"artworkMinPosterHeight":4096})).unwrap();
        assert!(custom.validate());
        let saved = serde_json::to_value(custom).unwrap();
        assert_eq!(saved["artworkDeduplicate"], false);
        for value in [
            serde_json::json!({"artworkSimilarity":17}),
            serde_json::json!({"artworkMinPosterWidth":4097}),
            serde_json::json!({"artworkMinPosterHeight":4097}),
        ] {
            assert!(
                !serde_json::from_value::<LibraryOptions>(value)
                    .unwrap()
                    .validate()
            );
        }
    }

    #[test]
    fn embedded_titles_are_opt_in_bounded_and_secondary_to_nfo() {
        use media_contracts::{
            inventory::ScannedFile,
            media::{FilmMetadata, NfoOutcome},
        };
        let mut file = ScannedFile {
            title: "Filename title".into(),
            identity_key: "file:original".into(),
            ..Default::default()
        };
        file.probe.format.insert(
            "tags".into(),
            serde_json::json!({"TITLE":"\u{0} Embedded title \n"}),
        );
        LibraryOptions::default().filter_file(&mut file);
        assert_eq!(file.title, "Filename title");
        let options = LibraryOptions {
            prefer_embedded_titles: true,
            ..Default::default()
        };
        options.filter_file(&mut file);
        assert_eq!(file.title, "Embedded title");
        assert_eq!(file.identity_key, "file:original");
        file.editorial = NfoOutcome::Valid {
            metadata: FilmMetadata {
                title: Some("NFO title".into()),
                ..Default::default()
            },
            path: "movie.nfo".into(),
            priority: 0,
        };
        file.title = "NFO title".into();
        options.filter_file(&mut file);
        assert_eq!(file.title, "NFO title");
        let options = LibraryOptions {
            read_local_nfo: false,
            ..options
        };
        options.filter_file(&mut file);
        assert_eq!(file.title, "Embedded title");
        file.probe
            .format
            .insert("tags".into(), serde_json::json!({"title":"x".repeat(1000)}));
        options.filter_file(&mut file);
        assert_eq!(file.title.len(), 512);
        file.probe
            .format
            .insert("tags".into(), serde_json::json!({"title":"\n\t"}));
        options.filter_file(&mut file);
        assert_eq!(file.title.len(), 512);
    }

    #[test]
    fn subtitle_policy_preserves_all_by_default_and_limits_text_when_selected() {
        let mut options = LibraryOptions::default();
        assert!(options.embedded_subtitle_allowed(Some("hdmv_pgs_subtitle")));
        options.embedded_subtitle_policy = EmbeddedSubtitlePolicy::AllowText;
        assert!(options.embedded_subtitle_allowed(Some("subrip")));
        assert!(!options.embedded_subtitle_allowed(Some("hdmv_pgs_subtitle")));
        assert!(!options.embedded_subtitle_allowed(None));
        options.embedded_subtitle_policy = EmbeddedSubtitlePolicy::BlockAll;
        assert!(!options.embedded_subtitle_allowed(Some("subrip")));
    }

    #[test]
    fn advanced_options_round_trip_and_old_json_defaults() {
        let old: LibraryOptions = serde_json::from_str(r#"{"readLocalNfo":false}"#).unwrap();
        assert!(!old.read_local_nfo);
        assert!(!old.trickplay_enabled);
        assert_eq!(old.trickplay_interval_seconds, 10);
        assert_eq!(old.subtitle_languages, ["fr"]);
        assert!(old.validate());
        let value = serde_json::json!({
            "preferEmbeddedTitles": true, "embeddedSubtitlePolicy": "allow_text",
            "metadataProviders": [{"id":"tmdb","enabled":true},{"id":"omdb","enabled":false}],
            "subtitleProviders": [{"id":"opensubtitles","enabled":true}],
            "trickplayEnabled": true, "trickplayWidth":640,"trickplayIntervalSeconds":5,
            "subtitleLanguages":["fr-CA","en"],"saveNfoToMedia":true,"saveSubtitlesToMedia":true
        });
        let options: LibraryOptions = serde_json::from_value(value).unwrap();
        assert!(options.validate());
        let encoded = serde_json::to_value(&options).unwrap();
        let decoded: LibraryOptions = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
        assert_eq!(options.metadata_providers[0].id, "tmdb");
        assert_eq!(
            options.embedded_subtitle_policy,
            EmbeddedSubtitlePolicy::AllowText
        );
        // These booleans are configuration only; filter_file does not execute writers.
        assert!(options.save_nfo_to_media);
    }

    #[test]
    fn advanced_options_reject_invalid_limits_duplicates_and_unknown_fields() {
        for value in [
            serde_json::json!({"trickplayWidth": 159}),
            serde_json::json!({"trickplayWidth": 1921}),
            serde_json::json!({"trickplayIntervalSeconds":0}),
            serde_json::json!({"trickplayIntervalSeconds":61}),
            serde_json::json!({"onlineRefreshIntervalDays":1}),
            serde_json::json!({"subtitleLanguages":["fr","FR"]}),
            serde_json::json!({"subtitleLanguages":["fr/../en"]}),
            serde_json::json!({"metadataProviders":[{"id":"x","enabled":true},{"id":"x","enabled":false}]}),
            serde_json::json!({"imageProviders":[{"id":"https://example.test","enabled":true}]}),
            serde_json::json!({"segmentProviders":[{"id":"","enabled":true}]}),
        ] {
            assert!(
                !serde_json::from_value::<LibraryOptions>(value.clone())
                    .unwrap()
                    .validate(),
                "accepted {value}"
            );
        }
        for value in [
            serde_json::json!({"unknownSetting":true}),
            serde_json::json!({"embeddedSubtitlePolicy":"invalid"}),
            serde_json::json!({"subtitleProviders":[{"id":"x","enabled":true,"url":"https://example.test"}]}),
        ] {
            assert!(serde_json::from_value::<LibraryOptions>(value).is_err());
        }
        let too_many = LibraryOptions {
            subtitle_languages: (0..17).map(|i| format!("en-{i}")).collect(),
            ..Default::default()
        };
        assert!(!too_many.validate());
        let too_many = LibraryOptions {
            metadata_providers: (0..33)
                .map(|i| ProviderPreference {
                    id: format!("provider-{i}"),
                    enabled: true,
                })
                .collect(),
            ..Default::default()
        };
        assert!(!too_many.validate());
    }
}
