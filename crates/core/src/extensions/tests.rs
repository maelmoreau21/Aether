use super::*;
use crate::database::{query, query_as};
use serde_json::json;

use media_extension_sdk::MetadataInput;
#[test]
fn music_album_search_uses_album_and_explicit_artist() {
    let input = music_album_search_input("Track", "Artist", "Album").unwrap();
    assert_eq!(input["title"], "Album");
    assert_eq!(input["trackTitle"], "Track");
    assert_eq!(input["externalIds"]["artist"], "Artist");
    assert!(music_album_search_input("Track", "Artist", "Unknown Album").is_none());
    assert!(music_album_search_input("Track", "", "Album").is_none());
}

#[test]
fn provider_cache_depends_on_settings_generation_and_package_without_disclosing_secrets() {
    let payload = json!({"title":"Song"});
    let settings = json!({"api_key":"example-secret", "language":"fr"});
    let original = provider_cache_context(&payload, &settings, "gen1", "package1", true).unwrap();
    assert_eq!(
        original,
        provider_cache_context(&payload, &settings, "gen1", "package1", true).unwrap()
    );
    assert!(!original.to_string().contains("example-secret"));
    assert_ne!(
        original,
        provider_cache_context(
            &payload,
            &json!({"api_key":"different"}),
            "gen1",
            "package1",
            true
        )
        .unwrap()
    );
    assert_ne!(
        original,
        provider_cache_context(&payload, &settings, "gen2", "package1", true).unwrap()
    );
    assert_ne!(
        original,
        provider_cache_context(&payload, &settings, "gen1", "package2", true).unwrap()
    );
    assert_ne!(
        original,
        provider_cache_context(&payload, &settings, "gen1", "package1", false).unwrap()
    );
}

#[test]
fn music_candidates_and_details_require_consistent_identity() {
    let input = json!({"title":"Song", "artist":"Muse", "duration":180});
    let candidate =
        json!({"id":"42", "title":"Song", "artist":"Muse", "duration":182, "mediaType":"music"});
    assert!(trusted_music_candidate(std::slice::from_ref(&candidate), &input).is_some());
    for invalid in [
        json!({"id":"42", "title":"Song", "artist":"Museum", "duration":180, "mediaType":"music"}),
        json!({"id":"42", "title":"Song", "artist":"", "duration":180, "mediaType":"music"}),
        json!({"id":"", "title":"Song", "artist":"Muse", "duration":180, "mediaType":"music"}),
        json!({"id":"42", "title":"Song", "artist":"Muse", "duration":240, "mediaType":"music"}),
        json!({"id":"42", "title":"Song", "artist":"Muse", "duration":180, "mediaType":"movie"}),
    ] {
        assert!(trusted_music_candidate(&[invalid], &input).is_none());
    }
    assert!(trusted_music_candidate(&[candidate.clone(), candidate.clone()], &input).is_none());
    assert!(
        trusted_music_candidate(
            std::slice::from_ref(&candidate),
            &json!({"title":"Song", "artist":""})
        )
        .is_none()
    );
    assert!(music_details_match(
        &json!({"providerId":"42", "title":"Song", "actors":[{"name":"Muse", "role":"artist"}]}),
        &candidate
    ));
    assert!(!music_details_match(
        &json!({"providerId":"43", "title":"Song", "actors":[{"name":"Muse", "role":"artist"}]}),
        &candidate
    ));
    assert!(!music_details_match(
        &json!({"providerId":"42", "title":"Other", "actors":[{"name":"Muse", "role":"artist"}]}),
        &candidate
    ));
    assert!(!music_details_match(
        &json!({"providerId":"42", "title":"Song", "actors":[{"name":"Other", "role":"artist"}]}),
        &candidate
    ));
    assert!(!music_details_match(
        &json!({"providerId":"42", "title":"Song"}),
        &candidate
    ));
}

#[test]
fn lyrics_match_track_artist_and_duration() {
    let input = lyrics_request_input("Song", "Artist", "Album", Some(180.0));
    let mut result = json!({"id":42,"trackName":"Song","artistName":"Artist","durationSecs":181,"instrumental":false});
    assert!(lyrics_result_matches_input(&input, &result));
    result["artistName"] = json!("Different");
    assert!(!lyrics_result_matches_input(&input, &result));
    result["artistName"] = json!("Artist");
    result["durationSecs"] = json!(300);
    assert!(!lyrics_result_matches_input(&input, &result));
}

#[test]
fn lyrics_payload_matches_sdk_request() {
    let input = lyrics_request_input("Track", "Artist", "Album", Some(123.6));
    let decoded: media_extension_sdk::LyricsRequest = serde_json::from_value(input).unwrap();
    assert_eq!(decoded.track, "Track");
    assert_eq!(decoded.artist, "Artist");
    assert_eq!(decoded.album.as_deref(), Some("Album"));
    assert_eq!(decoded.duration_secs, Some(124));
}

#[test]
fn automatic_candidate_requires_unique_title_type_and_year() {
    let input = json!({"title":"  Film  Test ","mediaType":"movie","year":2024});
    let candidate = json!({"id":"42","title":"Film Test","mediaType":"movie","year":2024});
    assert_eq!(
        trusted_metadata_candidate(std::slice::from_ref(&candidate), &input).unwrap()["id"],
        "42"
    );
    assert_eq!(
        trusted_metadata_candidate(&[candidate.clone(), candidate.clone()], &input).unwrap()["id"],
        "42"
    );
    let candidate_no_year = json!({"id":"43","title":"Film Test","mediaType":"movie"});
    assert_eq!(
        trusted_metadata_candidate(std::slice::from_ref(&candidate_no_year), &input).unwrap()["id"],
        "43"
    );
    for other in [
        json!({"id":"42","title":"Film Test","mediaType":"tv_show","year":2024}),
        json!({"id":"42","title":"Film Test","mediaType":"movie","year":2023}),
        json!({"id":"42","title":"Different","mediaType":"movie","year":2024}),
    ] {
        assert!(trusted_metadata_candidate(&[other], &input).is_none());
    }
}
#[test]
fn known_metadata_identity_is_provider_scoped_and_translated_titles_match_originals() {
    let input = json!({"title":"Wrong release name","externalIds":{"tmdb":"27205"}});
    assert_eq!(known_metadata_id(&input, "tmdb"), Some("27205"));
    assert_eq!(known_metadata_id(&input, "imdb"), None);
    assert_eq!(
        known_metadata_id(&json!({"externalIds":{"tmdb":" "}}), "tmdb"),
        None
    );
    let input = json!({"title":"The Movie","mediaType":"movie","year":2024});
    let candidate = json!({"id":"42","title":"Le Film","originalTitle":"The Movie","mediaType":"movie","year":2024});
    assert!(trusted_metadata_candidate(std::slice::from_ref(&candidate), &input).is_some());
    let input = json!({"title":"Wrong filename","year":1999,"mediaType":"movie","externalIds":{"imdb":"tt123"}});
    assert!(trusted_metadata_candidate(std::slice::from_ref(&candidate), &input).is_none());
    assert!(external_id_candidate(std::slice::from_ref(&candidate), &input).is_some());
    assert!(external_id_candidate(&[candidate.clone(), candidate.clone()], &input).is_none());
    assert!(external_id_candidate(&[json!({"id":"42","mediaType":"tv_show"})], &input).is_none());
    assert!(metadata_external_ids_match(
        &json!({"externalIds":{"tmdb":"42","imdb":"tt123"}}),
        &input
    ));
    assert!(!metadata_external_ids_match(
        &json!({"externalIds":{"tmdb":"42"}}),
        &input
    ));
    assert!(!metadata_external_ids_match(
        &json!({"externalIds":{"imdb":"tt999"}}),
        &input
    ));
    assert!(metadata_external_ids_conflict(
        &json!({"externalIds":{"imdb":"tt999"}}),
        &input
    ));
}
#[test]
fn historical_empty_endpoint_keeps_provider_default_and_credentials() {
    let value = metadata_input_with_settings(
        json!({"title":"Fixture"}),
        json!({"api_key":"synthetic","base_url":"  "}),
    );
    assert!(value["config"].get("base_url").is_none());
    assert_eq!(value["config"]["api_key"], "synthetic");
    let value =
        metadata_input_with_settings(json!({}), json!({"base_url":"https://custom.example/api"}));
    assert_eq!(value["config"]["base_url"], "https://custom.example/api");
}

#[test]
fn worker_receives_only_the_selected_extensions_configuration() {
    let value = metadata_input_with_settings(
        json!(MetadataInput {
            api_version: 1,
            work_id: "work".into(),
            library_id: "library".into(),
            title: "Film".into(),
        }),
        json!({"api_key": "synthetic-key", "language": "fr-FR"}),
    );
    assert_eq!(value["config"]["api_key"], "synthetic-key");
    assert_eq!(value["title"], "Film");
    assert!(value.get("settings").is_none());
    let legacy = metadata_input_with_settings(
        json!(MetadataInput {
            api_version: 1,
            work_id: "work".into(),
            library_id: "library".into(),
            title: "Film".into(),
        }),
        json!({}),
    );
    assert!(serde_json::from_value::<MetadataInput>(legacy).is_ok());
}

#[tokio::test]
async fn test_bootstrap_essential_plugins_installs_tmdb_and_imdb_enabled() {
    let root = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&root.path().join("core.sqlite3"))
        .await
        .unwrap();
    let initial_count: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM extensions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(initial_count, 0);

    bootstrap_essential_plugins(&store).await;

    let tmdb_row: Option<(i64, i64)> =
        crate::database::query_as("SELECT enabled, all_libraries FROM extensions WHERE id='tmdb'")
            .fetch_optional(store.pool())
            .await
            .unwrap();
    assert_eq!(tmdb_row, Some((1, 1)));

    let imdb_row: Option<(i64, i64)> =
        crate::database::query_as("SELECT enabled, all_libraries FROM extensions WHERE id='imdb'")
            .fetch_optional(store.pool())
            .await
            .unwrap();
    assert_eq!(imdb_row, Some((1, 1)));

    query("UPDATE extensions SET enabled=0, all_libraries=0 WHERE id='tmdb'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE extensions SET enabled=1, all_libraries=0 WHERE id='imdb'")
        .execute(store.pool())
        .await
        .unwrap();
    bootstrap_essential_plugins(&store).await;
    let choices: Vec<(String, i64, i64)> =
        query_as("SELECT id, enabled, all_libraries FROM extensions ORDER BY id")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(choices, vec![("imdb".into(), 1, 0), ("tmdb".into(), 0, 0)]);
}
