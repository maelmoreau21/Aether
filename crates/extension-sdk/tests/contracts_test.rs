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
        provider: "tmdb".into(),
        provider_id: "550".into(),
        external_ids: ext_ids,
        title: Some("Fight Club".into()),
        original_title: Some("Fight Club".into()),
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
