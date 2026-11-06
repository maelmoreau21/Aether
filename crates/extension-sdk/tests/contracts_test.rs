use media_extension_sdk::{
    ActorCredit, HostHttpRequest, ImageType, MediaImage, MediaType, ProviderRating, SearchQuery,
    SearchResultItem, StructuredMetadata, host_http_fetch,
};
use std::collections::BTreeMap;

#[test]
fn test_structured_metadata_roundtrip() {
    let mut ext_ids = BTreeMap::new();
    ext_ids.insert("tmdb".into(), "550".into());
    ext_ids.insert("imdb".into(), "tt0137523".into());

    let meta = StructuredMetadata {
        collection: Some(media_extension_sdk::MetadataCollection {
            id: "tmdb:42".into(),
            name: "Synthetic collection".into(),
        }),
        provider: "tmdb".into(),
        provider_id: "550".into(),
        external_ids: ext_ids,
        title: Some("Fight Club".into()),
        original_title: Some("Fight Club".into()),
        original_language: Some("en".into()),
        locale: Some("en-US".into()),
        tagline: Some("Mischief. Mayhem. Soap.".into()),
        synopsis: Some("An insomniac office worker...".into()),
        year: Some(1999),
        release_date: Some("1999-10-15".into()),
        runtime_minutes: Some(139),
        genres: vec!["Drama".into()],
        directors: vec!["David Fincher".into()],
        writers: vec!["Chuck Palahniuk".into()],
        actors: vec![
            ActorCredit {
                name: "Edward Norton".into(),
                role: Some("The Narrator".into()),
                thumb_url: Some("https://image.tmdb.org/t/p/w185/abc.jpg".into()),
                order: Some(0),
            },
            ActorCredit {
                name: "Brad Pitt".into(),
                role: Some("Tyler Durden".into()),
                thumb_url: Some("https://image.tmdb.org/t/p/w185/xyz.jpg".into()),
                order: Some(1),
            },
        ],
        ratings: vec![
            ProviderRating {
                provider: "tmdb".into(),
                value: 8.4,
                max: 10.0,
                votes: Some(26000),
            },
            ProviderRating {
                provider: "imdb".into(),
                value: 8.8,
                max: 10.0,
                votes: Some(2200000),
            },
        ],
        mpaa: Some("R".into()),
        status: Some("Released".into()),
        posters: vec![MediaImage {
            url: "https://image.tmdb.org/t/p/original/poster.jpg".into(),
            image_type: ImageType::Poster,
            width: Some(2000),
            height: Some(3000),
            language: Some("en".into()),
            rating: Some(9.2),
            vote_count: Some(150),
            is_primary: true,
        }],
        backdrops: vec![MediaImage {
            url: "https://image.tmdb.org/t/p/original/backdrop.jpg".into(),
            image_type: ImageType::Backdrop,
            width: Some(3840),
            height: Some(2160),
            language: None,
            rating: Some(8.9),
            vote_count: Some(85),
            is_primary: true,
        }],
        images: vec![],
        seasons: vec![],
        translations: BTreeMap::new(),
    };

    let json = serde_json::to_string(&meta).unwrap();
    let deserialized: StructuredMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(meta, deserialized);
}

#[test]
fn test_search_and_query_roundtrip() {
    let query = SearchQuery {
        title: "Inception".into(),
        year: Some(2010),
        media_type: MediaType::Movie,
        language: Some("fr".into()),
        external_ids: BTreeMap::new(),
    };
    let json = serde_json::to_string(&query).unwrap();
    let d: SearchQuery = serde_json::from_str(&json).unwrap();
    assert_eq!(query, d);

    let item = SearchResultItem {
        id: "27205".into(),
        title: "Inception".into(),
        artist: None,
        original_title: Some("Inception".into()),
        year: Some(2010),
        media_type: MediaType::Movie,
        overview: Some("Cobb vole des secrets...".into()),
        poster_url: Some("https://image.tmdb.org/t/p/w500/poster.jpg".into()),
        score: Some(8.3),
    };
    let item_json = serde_json::to_string(&item).unwrap();
    let item_d: SearchResultItem = serde_json::from_str(&item_json).unwrap();
    assert_eq!(item, item_d);
    let music: SearchResultItem = serde_json::from_value(
        serde_json::json!({"id":"album","title":"Album","mediaType":"music","artist":"Artist"}),
    )
    .unwrap();
    assert_eq!(music.artist.as_deref(), Some("Artist"));
    let legacy: SearchResultItem = serde_json::from_value(
        serde_json::json!({"id":"album","title":"Album","mediaType":"music"}),
    )
    .unwrap();
    assert_eq!(legacy.artist, None);
}

#[test]
fn test_host_http_contract() {
    let mut headers = BTreeMap::new();
    headers.insert("accept".into(), "application/json".into());
    let req = HostHttpRequest {
        url: "https://api.themoviedb.org/3/movie/550".into(),
        method: "GET".into(),
        headers,
        body: None,
        timeout_secs: Some(5),
    };
    let json = serde_json::to_string(&req).unwrap();
    let d: HostHttpRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.url, d.url);

    // On non-wasm32, host_http_fetch returns a typed unsupported_platform error
    let err = host_http_fetch(&req).unwrap_err();
    assert_eq!(err.code, "unsupported_platform");
}

#[test]
fn localization_fields_are_additive_in_both_directions() {
    use media_extension_sdk::{
        DetailsRequest, EpisodeMetadata, LOCALIZATION_CONTRACT, SeasonMetadata, TranslationItem,
    };
    assert_eq!(LOCALIZATION_CONTRACT, 2);

    // A request from a first-contract Core has no `languages`: it still parses, empty.
    let old: DetailsRequest = serde_json::from_str(r#"{"id":"550","mediaType":"movie"}"#).unwrap();
    assert!(old.languages.is_empty());
    // A second-contract request round-trips; the field is omitted when empty so a
    // first-contract plugin never sees a key it does not know.
    let new = DetailsRequest {
        id: "550".into(),
        languages: vec!["fr-FR".into(), "en-GB".into()],
        ..Default::default()
    };
    let json = serde_json::to_value(&new).unwrap();
    assert_eq!(json["languages"], serde_json::json!(["fr-FR", "en-GB"]));
    assert!(
        serde_json::to_value(DetailsRequest::default())
            .unwrap()
            .get("languages")
            .is_none()
    );

    // A first-contract answer (no locale, no season/episode translations) still parses.
    let legacy: StructuredMetadata = serde_json::from_str(
        r#"{"provider":"tmdb","providerId":"1","title":"T","translations":{"fr":{"title":"Titre"}},
            "seasons":[{"seasonNumber":1,"episodes":[{"episodeNumber":1,"title":"E"}]}]}"#,
    )
    .unwrap();
    assert_eq!(legacy.locale, None);
    assert!(
        legacy.seasons[0].translations.is_empty()
            && legacy.seasons[0].episodes[0].translations.is_empty()
    );

    // A second-contract answer keeps regional keys and per-episode languages.
    let item = |t: &str| TranslationItem {
        title: Some(t.into()),
        ..Default::default()
    };
    let meta = StructuredMetadata {
        provider: "tmdb".into(),
        provider_id: "1".into(),
        locale: Some("fr-FR".into()),
        translations: BTreeMap::from([
            ("fr-FR".into(), item("Titre")),
            ("fr-CA".into(), item("Titre QC")),
        ]),
        seasons: vec![SeasonMetadata {
            season_number: 1,
            translations: BTreeMap::from([("en-US".into(), item("Season 1"))]),
            episodes: vec![EpisodeMetadata {
                episode_number: 1,
                translations: BTreeMap::from([("en-US".into(), item("Pilot"))]),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let back: StructuredMetadata =
        serde_json::from_value(serde_json::to_value(&meta).unwrap()).unwrap();
    assert_eq!(back, meta);
    assert_eq!(back.translations.len(), 2, "fr-FR and fr-CA stay distinct");
}
