//! Stored translations against a real SQLite database: two accounts, regions, absent
//! languages, hand edits, caches and restarts.

use super::*;
use crate::database::{query, query_scalar};
use crate::inventory::{
    CatalogueFilter, ScanReport, catalogue_page, detail_window_localized, ingest,
};
use crate::node::scanner::ScannedFile;
use std::path::Path;

/// Where a scenario runs. SQLite is always available; PostgreSQL 18 needs
/// `MEDIA_TEST_POSTGRES_URL` (the `#[ignore]` variants below), each in its own schema.
#[derive(Clone)]
enum Backend {
    Sqlite(std::path::PathBuf),
    Postgres(String),
}

impl Backend {
    async fn open(&self) -> IdentityStore {
        match self {
            Self::Sqlite(path) => IdentityStore::open(path).await.unwrap(),
            Self::Postgres(url) => IdentityStore::open_postgres(url).await.unwrap(),
        }
    }
}

async fn postgres_backend() -> (Backend, String, sqlx::PgPool) {
    let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("translations_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    (Backend::Postgres(url.to_string()), schema, pool)
}

macro_rules! on_both_engines {
    ($($name:ident),+ $(,)?) => {
        mod sqlite {
            $(#[tokio::test]
            async fn $name() {
                let dir = tempfile::tempdir().unwrap();
                let backend = super::Backend::Sqlite(dir.path().join("translations.sqlite"));
                super::$name(&backend).await;
            })+
        }
        mod postgres {
            $(#[tokio::test]
            #[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
            async fn $name() {
                let (backend, schema, pool) = super::postgres_backend().await;
                super::$name(&backend).await;
                sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE"))).execute(&pool).await.unwrap();
            })+
        }
    };
}

/// One library (default options: metadata language `fr`) holding one film.
async fn film(store: &IdentityStore) -> String {
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
        .execute(&store.pool).await.unwrap();
    let file = ScannedFile {
        relative_path: "shawshank.mp4".into(),
        size: 42,
        modified_ns: "1".into(),
        title: "Shawshank".into(),
        identity_key: "shawshank".into(),
        mime: "video/mp4".into(),
        probe: serde_json::from_value(
            json!({"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}],"format":{}}),
        )
        .unwrap(),
        ..Default::default()
    };
    ingest(
        store,
        "lib",
        Path::new("/movies"),
        &ScanReport {
            files: vec![file],
            complete: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    query_scalar("SELECT id FROM inventory_works WHERE kind='film'")
        .fetch_one(&store.pool)
        .await
        .unwrap()
}

/// Direct writes to the work row, as scans and edit endpoints do. Those code paths clear
/// the in-memory catalogue cache themselves (PostgreSQL has no triggers to do it).
async fn set_json(store: &IdentityStore, work: &str, column: &str, value: Value) {
    let sql = format!("UPDATE inventory_works SET {column}=? WHERE id=?");
    query(sqlx::AssertSqlSafe(sql))
        .bind(value.to_string())
        .bind(work)
        .execute(&store.pool)
        .await
        .unwrap();
    crate::inventory::invalidate_catalogue_cache();
}

async fn account(store: &IdentityStore, id: &str, prefs: Value) {
    query("INSERT INTO users(id,role) VALUES(?, 'user')")
        .bind(id)
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO user_preferences(user_id,preferences_json) VALUES(?,?)")
        .bind(id)
        .bind(prefs.to_string())
        .execute(&store.pool)
        .await
        .unwrap();
}

async fn put(
    store: &IdentityStore,
    work: &str,
    locale: &str,
    source: &str,
    title: &str,
    overview: &str,
) {
    let mut tx = store.pool.begin().await.unwrap();
    let stored = upsert_work(
        &mut tx,
        work,
        &Translation {
            locale: locale.into(),
            source: source.into(),
            title: Some(title.into()),
            overview: Some(overview.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(stored, "{locale}/{source}");
    tx.commit().await.unwrap();
}

/// Title and plot of the single film as `user` sees it in the catalogue list.
async fn listed(store: &IdentityStore, user: &str) -> (String, String, Value) {
    let chain = locale_chain(store, Some(user), None).await.unwrap();
    let filter = CatalogueFilter {
        locale_chain: chain,
        user_id: Some(user.into()),
        ..Default::default()
    };
    let page = catalogue_page(
        store,
        &["lib".to_owned()],
        None,
        "",
        "title-asc",
        0,
        10,
        &filter,
        true,
    )
    .await
    .unwrap();
    let item = &page["items"][0];
    (
        item["title"].as_str().unwrap().into(),
        item["plot"].as_str().unwrap_or("").into(),
        item["localization"].clone(),
    )
}

async fn two_accounts_read_the_same_work_in_their_own_language_at_the_same_time(backend: &Backend) {
    let store = backend.open().await;
    let work = film(&store).await;
    account(&store, "ana", json!({"locale": "fr"})).await;
    account(
        &store,
        "bob",
        json!({"locale": "en", "metadataLocale": "en-GB"}),
    )
    .await;
    put(
        &store,
        &work,
        "fr-FR",
        "provider:tmdb",
        "Les Évadés",
        "Un banquier.",
    )
    .await;
    put(
        &store,
        &work,
        "en-US",
        "provider:tmdb",
        "The Shawshank Redemption",
        "A banker.",
    )
    .await;

    // Interleaved requests so a shared cache entry would be exposed.
    let (a, b, a2, b2) = tokio::join!(
        listed(&store, "ana"),
        listed(&store, "bob"),
        listed(&store, "ana"),
        listed(&store, "bob")
    );
    assert_eq!((a.0.as_str(), a.1.as_str()), ("Les Évadés", "Un banquier."));
    assert_eq!(
        (b.0.as_str(), b.1.as_str()),
        ("The Shawshank Redemption", "A banker.")
    );
    assert_eq!((a2.0, b2.0), (a.0, b.0));
    // en-GB has no row of its own: the page says which locale actually served the text.
    assert_eq!(b.2["requested"], "en-GB");
    assert_eq!(b.2["title"], "en-US");

    // Same identifier in the detail endpoint.
    let chain_b = locale_chain(&store, Some("bob"), None).await.unwrap();
    let detail = detail_window_localized(&store, &work, None, &chain_b)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail["id"], work.as_str());
    assert_eq!(detail["title"], "The Shawshank Redemption");
}

async fn regions_missing_languages_and_explicit_request(backend: &Backend) {
    let store = backend.open().await;
    let work = film(&store).await;
    put(&store, &work, "fr-FR", "provider:tmdb", "Les Évadés", "").await;
    put(
        &store,
        &work,
        "fr-CA",
        "provider:tmdb",
        "Les évadés (Québec)",
        "",
    )
    .await;
    for (user, prefs, expected) in [
        (
            "qc",
            json!({"metadataLocale": "fr-CA"}),
            "Les évadés (Québec)",
        ),
        ("fr", json!({"metadataLocale": "fr-FR"}), "Les Évadés"),
        // fr-BE is stored nowhere: a sibling region of French beats the default.
        (
            "be",
            json!({"metadataLocale": "fr-BE", "metadataFallbackLocales": ["en"]}),
            "Les Évadés",
        ),
        // Japanese is absent everywhere: fall to the server default language (fr).
        ("ja", json!({"metadataLocale": "ja"}), "Les Évadés"),
        // No preference at all: the server default.
        ("none", json!({}), "Les Évadés"),
    ] {
        account(&store, user, prefs).await;
        assert_eq!(listed(&store, user).await.0, expected, "{user}");
    }
    // `?locale=` overrides the account for one request, with its own cache entry.
    let chain = locale_chain(&store, Some("fr"), Some("fr_ca"))
        .await
        .unwrap();
    assert_eq!(chain[0], "fr-CA");
    let filter = CatalogueFilter {
        locale_chain: chain,
        user_id: Some("fr".into()),
        ..Default::default()
    };
    let page = catalogue_page(
        &store,
        &["lib".to_owned()],
        None,
        "",
        "title-asc",
        0,
        10,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["items"][0]["title"], "Les évadés (Québec)");
    // No translation of any language in the chain: the stored title is untouched.
    let filter = CatalogueFilter {
        locale_chain: vec!["ko".into()],
        ..Default::default()
    };
    let page = catalogue_page(
        &store,
        &["lib".to_owned()],
        None,
        "",
        "title-asc",
        0,
        10,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["items"][0]["title"], "Shawshank");
}

async fn cache_follows_translation_edits_and_never_mixes_languages(backend: &Backend) {
    let store = backend.open().await;
    let work = film(&store).await;
    account(&store, "ana", json!({"metadataLocale": "fr-FR"})).await;
    account(&store, "bob", json!({"metadataLocale": "en-US"})).await;
    put(&store, &work, "fr-FR", "provider:tmdb", "Les Évadés", "").await;
    put(&store, &work, "en-US", "provider:tmdb", "Shawshank EN", "").await;
    assert_eq!(listed(&store, "ana").await.0, "Les Évadés");
    assert_eq!(listed(&store, "bob").await.0, "Shawshank EN");
    // Both pages are now cached. A provider refresh must reach the next request.
    put(
        &store,
        &work,
        "fr-FR",
        "provider:tmdb",
        "Les Évadés (corrigé)",
        "",
    )
    .await;
    assert_eq!(listed(&store, "ana").await.0, "Les Évadés (corrigé)");
    assert_eq!(listed(&store, "bob").await.0, "Shawshank EN");
}

async fn manual_edits_nfo_and_identification_keep_their_priority(backend: &Backend) {
    let store = backend.open().await;
    let work = film(&store).await;
    account(&store, "ana", json!({"metadataLocale": "fr-FR"})).await;
    account(&store, "bob", json!({"metadataLocale": "en-US"})).await;
    put(&store, &work, "fr-FR", "provider:tmdb", "Provider FR", "").await;
    put(&store, &work, "en-US", "provider:tmdb", "Provider EN", "").await;

    // An NFO in the library language outranks the provider for French readers only.
    set_json(
        &store,
        &work,
        "editorial_json",
        json!({"provenance": "nfo", "title": "NFO FR", "external_ids": {"imdb": "tt0111161"}}),
    )
    .await;
    assert_eq!(listed(&store, "ana").await.0, "NFO FR");
    assert_eq!(listed(&store, "bob").await.0, "Provider EN");

    // An identification copies its result into manual_json: not a human decision.
    set_json(
        &store,
        &work,
        "manual_json",
        json!({"provenance": "identify:tmdb", "title": "Identifié FR"}),
    )
    .await;
    assert_eq!(listed(&store, "bob").await.0, "Provider EN");

    // A hand edit made without a language applies to everyone ...
    set_json(
        &store,
        &work,
        "manual_json",
        json!({"provenance": "extension", "title": "Titre manuel"}),
    )
    .await;
    assert_eq!(listed(&store, "ana").await.0, "Titre manuel");
    assert_eq!(listed(&store, "bob").await.0, "Titre manuel");

    // ... until a correction scoped to one language is written, and is removable.
    put(&store, &work, "en-US", "manual", "Manual EN", "").await;
    assert_eq!(listed(&store, "bob").await.0, "Manual EN");
    assert_eq!(listed(&store, "ana").await.0, "Titre manuel");
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        delete_work_source(&mut tx, &work, "en-US", "manual")
            .await
            .unwrap(),
        1
    );
    tx.commit().await.unwrap();
    assert_eq!(listed(&store, "bob").await.0, "Titre manuel");

    // Identifiers are never touched by translations.
    let ids: String = query_scalar("SELECT editorial_json FROM inventory_works WHERE id=?")
        .bind(&work)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(ids.contains("tt0111161"));
}

async fn seasons_and_episodes_are_localized_in_one_pass(backend: &Backend) {
    let store = backend.open().await;
    film(&store).await;
    for (id, kind) in [("series", "series"), ("ep1", "episode")] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,editorial_json,kind) VALUES(?, 'lib', ?, ?, '{}', '{}', ?)")
            .bind(id).bind(id).bind(format!("{id} base")).bind(kind).execute(&store.pool).await.unwrap();
    }
    query("INSERT INTO inventory_seasons(id,series_id,season_number,title) VALUES('s1','series',1,'Saison 1 base')")
        .execute(&store.pool).await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    upsert_season(
        &mut tx,
        "s1",
        &Translation {
            locale: "en-US".into(),
            source: "provider:tvdb".into(),
            title: Some("Season 1".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    upsert_work(
        &mut tx,
        "ep1",
        &Translation {
            locale: "en-US".into(),
            source: "provider:tvdb".into(),
            title: Some("Pilot".into()),
            overview: Some("It begins.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    upsert_work(
        &mut tx,
        "ep1",
        &Translation {
            locale: "fr-FR".into(),
            source: "provider:tvdb".into(),
            title: Some("Pilote".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let detail = |chain: &[&str]| {
        let chain: Vec<String> = chain.iter().map(|c| (*c).into()).collect();
        let store = store.clone();
        async move {
            let mut value = json!({"id": "series", "title": "series base", "seasons": [
                {"id": "s1", "number": 1, "title": "Saison 1 base", "episodes": [{"id": "ep1", "title": "ep1 base"}]},
                {"id": "empty", "number": 2, "title": "vide", "episodes": []}
            ]});
            localize_detail(&store, &mut value, &chain).await.unwrap();
            value
        }
    };
    let en = detail(&["en-GB"]).await;
    assert_eq!(en["seasons"][0]["title"], "Season 1");
    assert_eq!(en["seasons"][0]["episodes"][0]["title"], "Pilot");
    assert_eq!(en["seasons"][0]["episodes"][0]["plot"], "It begins.");
    assert_eq!(
        en["seasons"][1]["title"], "vide",
        "no translation keeps the stored text"
    );
    let fr = detail(&["fr-FR", "en"]).await;
    assert_eq!(fr["seasons"][0]["episodes"][0]["title"], "Pilote");
    // The French episode has no overview: the English one fills the gap per field.
    assert_eq!(fr["seasons"][0]["episodes"][0]["plot"], "It begins.");
}

async fn invalid_rows_are_rejected_without_touching_the_cache(backend: &Backend) {
    let store = backend.open().await;
    let work = film(&store).await;
    let before: i64 = query_scalar("SELECT gen FROM inventory_cache_generation WHERE id=1")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    for (locale, source, title) in [
        ("not a locale", "manual", "x"),
        ("fr", "root", "x"),
        ("fr", "provider:../x", "x"),
        ("fr", "manual", "   "),
    ] {
        let item = Translation {
            locale: locale.into(),
            source: source.into(),
            title: Some(title.into()),
            ..Default::default()
        };
        assert!(
            !upsert_work(&mut tx, &work, &item).await.unwrap(),
            "{locale}/{source}"
        );
    }
    tx.commit().await.unwrap();
    let after: i64 = query_scalar("SELECT gen FROM inventory_cache_generation WHERE id=1")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}

/// Several films in one library, in the order given. Their stored titles are the library
/// language (French); returns the work ids in the same order.
async fn films(store: &IdentityStore, titles: &[&str]) -> Vec<String> {
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
        .execute(&store.pool).await.unwrap();
    let files = titles
        .iter()
        .enumerate()
        .map(|(index, title)| ScannedFile {
            relative_path: format!("{index}.mp4"),
            size: 42 + index as u64,
            modified_ns: "1".into(),
            title: (*title).into(),
            identity_key: title.to_lowercase(),
            mime: "video/mp4".into(),
            probe: serde_json::from_value(
                json!({"streams":[{"index":0,"codec_type":"video","codec_name":"h264"}],"format":{}}),
            )
            .unwrap(),
            ..Default::default()
        })
        .collect();
    ingest(
        store,
        "lib",
        Path::new("/movies"),
        &ScanReport {
            files,
            complete: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut ids = Vec::new();
    for title in titles {
        ids.push(
            query_scalar("SELECT id FROM inventory_works WHERE title=?")
                .bind(*title)
                .fetch_one(&store.pool)
                .await
                .unwrap(),
        );
    }
    ids
}

/// Titles of a catalogue page as `user` sees them, for a search text and a sort.
async fn titles_for(store: &IdentityStore, user: &str, search: &str, sort: &str) -> Vec<String> {
    let chain = locale_chain(store, Some(user), None).await.unwrap();
    let filter = CatalogueFilter {
        locale_chain: chain,
        user_id: Some(user.into()),
        ..Default::default()
    };
    let page = catalogue_page(
        store,
        &["lib".to_owned()],
        None,
        search,
        sort,
        0,
        50,
        &filter,
        true,
    )
    .await
    .unwrap();
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}

async fn a_work_is_found_by_translated_and_original_titles_without_being_duplicated(
    backend: &Backend,
) {
    let store = backend.open().await;
    let work = film(&store).await;
    account(&store, "ana", json!({"locale": "fr"})).await;
    put(
        &store,
        &work,
        "fr-FR",
        "provider:tmdb",
        "Les Évadés",
        "Un banquier.",
    )
    .await;
    put(
        &store,
        &work,
        "en-GB",
        "provider:tmdb",
        "The Shawshank Redemption",
        "A banker.",
    )
    .await;
    set_json(
        &store,
        &work,
        "editorial_json",
        json!({"original_title": "Rita Hayworth Prison Story"}),
    )
    .await;
    {
        let mut conn = store.pool.acquire().await.unwrap();
        crate::consolidation::sync_work_summary_tx(&mut conn, &work)
            .await
            .unwrap();
    }

    // The French reader finds the work by its library title, its English title and its original title.
    for text in ["shawshank", "évadés", "redemption", "rita hayworth", "RITA"] {
        assert_eq!(
            titles_for(&store, "ana", text, "title-asc").await.len(),
            1,
            "{text}"
        );
    }
    assert!(
        titles_for(&store, "ana", "inexistant", "title-asc")
            .await
            .is_empty()
    );
    // Two-letter searches take the non-indexed path and behave the same.
    assert_eq!(titles_for(&store, "ana", "év", "title-asc").await.len(), 1);

    // A work is stored once, whatever the number of titles that lead to it.
    let works: i64 = query_scalar("SELECT COUNT(*) FROM inventory_works")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(works, 1);
    let indexed: i64 = query_scalar("SELECT COUNT(*) FROM work_search_titles WHERE work_id=?")
        .bind(&work)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(indexed, 3);

    // Removing a correction removes its searchable title.
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        delete_work_source(&mut tx, &work, "fr-FR", "provider:tmdb")
            .await
            .unwrap(),
        1
    );
    tx.commit().await.unwrap();
    assert!(
        titles_for(&store, "ana", "évadés", "title-asc")
            .await
            .is_empty()
    );
    assert_eq!(
        titles_for(&store, "ana", "redemption", "title-asc")
            .await
            .len(),
        1
    );
}

async fn titles_sort_in_the_readers_language_and_fall_back_to_the_library_title(backend: &Backend) {
    let store = backend.open().await;
    let ids = films(&store, &["Alpha", "Bravo", "Charlie"]).await;
    account(&store, "ana", json!({"locale": "fr"})).await;
    account(
        &store,
        "bob",
        json!({"locale": "en", "metadataLocale": "en-GB"}),
    )
    .await;
    account(
        &store,
        "uli",
        json!({"locale": "fr", "metadataLocale": "es"}),
    )
    .await;
    for (id, english) in ids.iter().zip(["Zulu", "Mike", "Echo"]) {
        put(&store, id, "en-US", "provider:tmdb", english, "").await;
    }
    assert_eq!(
        titles_for(&store, "ana", "", "title-asc").await,
        ["Alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles_for(&store, "bob", "", "title-asc").await,
        ["Echo", "Mike", "Zulu"]
    );
    assert_eq!(
        titles_for(&store, "bob", "", "title-desc").await,
        ["Zulu", "Mike", "Echo"]
    );
    // No Spanish translation exists: the library-language order is kept.
    assert_eq!(
        titles_for(&store, "uli", "", "title-asc").await,
        ["Alpha", "Bravo", "Charlie"]
    );
    // The same search finds each work by either title and shows the reader's title.
    assert_eq!(
        titles_for(&store, "bob", "zulu", "title-asc").await,
        ["Zulu"]
    );
    assert_eq!(
        titles_for(&store, "bob", "alpha", "title-asc").await,
        ["Zulu"]
    );
}

on_both_engines!(
    two_accounts_read_the_same_work_in_their_own_language_at_the_same_time,
    regions_missing_languages_and_explicit_request,
    cache_follows_translation_edits_and_never_mixes_languages,
    manual_edits_nfo_and_identification_keep_their_priority,
    seasons_and_episodes_are_localized_in_one_pass,
    invalid_rows_are_rejected_without_touching_the_cache,
    a_work_is_found_by_translated_and_original_titles_without_being_duplicated,
    titles_sort_in_the_readers_language_and_fall_back_to_the_library_title,
);
