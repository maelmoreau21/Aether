//! Cross-domain inventory regression tests.

use super::playback::playback_library_options;
use crate::database::{query, query_as, query_scalar};
use serde_json::json;
use std::path::Path;

use super::{
    BoundedCatalogueCache, CatalogueFilter, IdentityStore, NfoOutcome, Probe, ScanReport,
    backfill_title_sort_unicode, catalogue, catalogue_page, descriptor, detail, ingest, load_cache,
    prepared, subtitle,
};
use crate::node::scanner::ScannedFile;
#[tokio::test]
async fn embedded_subtitle_preferences_apply_locally_and_before_remote_relay() {
    use axum::http::StatusCode;
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("subtitle-options.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
        .execute(&store.pool).await.unwrap();
    let file = ScannedFile {
        relative_path: "film.mp4".into(), size: 42, modified_ns: "1".into(),
        title: "Film".into(), identity_key: "film".into(), mime: "video/mp4".into(),
        probe: serde_json::from_value(json!({"streams":[
            {"index":0,"codec_type":"video","codec_name":"av1"},
            {"index":1,"codec_type":"subtitle","codec_name":"ass","tags":{"language":"fr"}}
        ],"format":{}})).unwrap(),
        subtitles: vec![serde_json::from_value(json!({"language":"en","label":"External","forced":false,"sdh":false,"source_format":"webvtt","webvtt":"WEBVTT\n\n"})).unwrap()],
        ..Default::default()
    };
    ingest(
        &store,
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
    let asset: String = query_scalar("SELECT id FROM inventory_assets")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    // The legacy ingest fixture helper predates sidecars; publish their
    // snapshot explicitly as the staged scanner publication does.
    query("INSERT INTO inventory_subtitles(asset_id,subtitles_json) VALUES(?,?)")
        .bind(&asset)
        .bind(json!([{"language":"en","label":"External","forced":false,"sdh":false,"source_format":"webvtt","webvtt":"WEBVTT\n\n"}]).to_string())
        .execute(&store.pool).await.unwrap();
    // The original ASS snapshot uses the same authorized media endpoint and
    // range handling as the safe text fallback, without exposing a disk path.
    let ass = "[Script Info]\nTitle: Original styles\n";
    query("UPDATE inventory_subtitles SET subtitles_json=? WHERE asset_id=?")
        .bind(json!([{"language":"en","label":"External","forced":false,"sdh":false,"source_format":"ass","webvtt":"WEBVTT\n\n","ass":ass}]).to_string())
        .bind(&asset).execute(&store.pool).await.unwrap();
    for (method, range, status, expected) in [
        ("GET", None, StatusCode::OK, ass),
        ("HEAD", None, StatusCode::OK, ""),
        (
            "GET",
            Some("bytes=0-6"),
            StatusCode::PARTIAL_CONTENT,
            "[Script",
        ),
    ] {
        let mut request = axum::extract::Request::builder()
            .method(method)
            .uri("/?format=ass");
        if let Some(range) = range {
            request = request.header("range", range);
        }
        let response = subtitle(
            &store,
            &asset,
            0,
            request.body(axum::body::Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap(),
            expected
        );
    }
    query("INSERT INTO media_nodes(id,name,kind) VALUES('remote','Remote','remote')")
        .execute(&store.pool)
        .await
        .unwrap();
    let preparations = media_node::packaging::Preparations::default();
    for node in ["local", "remote"] {
        query("UPDATE libraries SET node_id=? WHERE id='lib'")
            .bind(node)
            .execute(&store.pool)
            .await
            .unwrap();
        for (policy, expected) in [("allow_all", 2), ("allow_text", 2), ("block_all", 1)] {
            query("UPDATE libraries SET options_json=? WHERE id='lib'")
                .bind(json!({"embeddedSubtitlePolicy":policy}).to_string())
                .execute(&store.pool)
                .await
                .unwrap();
            let descriptor = descriptor(&store, &asset, "synthetic-session")
                .await
                .unwrap()
                .unwrap();
            let subtitles = descriptor["subtitles"].as_array().unwrap();
            assert_eq!(subtitles.len(), expected, "node {node}, policy {policy}");
            assert!(
                subtitles
                    .iter()
                    .any(|subtitle| subtitle["label"] == "External")
            );
            if expected == 2 {
                assert!(subtitles.iter().any(|subtitle| {
                    subtitle["assUrl"]
                        .as_str()
                        .is_some_and(|url| url.ends_with("/prepared/subtitle-ass-1"))
                }));
            }
        }
        let request = axum::extract::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        let response = prepared(&store, &preparations, &asset, "subtitle-1", request).await;
        assert_eq!(
            response.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("embedded_subtitle_disabled"));
        let request = axum::extract::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            prepared(&store, &preparations, &asset, "subtitle-ass-1", request)
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('folder','Folder','films','local','/child',0,'{}')")
        .execute(&store.pool).await.unwrap();
    query("INSERT INTO library_folders(library_id,parent_id) VALUES('folder','lib')")
        .execute(&store.pool)
        .await
        .unwrap();
    let inherited = playback_library_options(&store, "folder").await.unwrap();
    assert!(!inherited.embedded_subtitle_allowed(Some("subrip")));
    query("UPDATE library_folders SET active=0 WHERE library_id='folder'")
        .execute(&store.pool)
        .await
        .unwrap();
    let detached = playback_library_options(&store, "folder").await.unwrap();
    assert!(detached.embedded_subtitle_allowed(Some("subrip")));
}

#[tokio::test]
async fn fresh_schema_preserves_editorial_fields() {
    sqlx::any::install_default_drivers();
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    crate::database::initialize_fresh_schema(&mut conn)
        .await
        .unwrap();
    drop(conn);
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/test',1)")
        .execute(&pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES('stable','lib','folder:Film','Film',2026,'{}','film','film')")
        .execute(&pool)
        .await
        .unwrap();
    let preserved: (String, String, i64, String) =
        query_as("SELECT id,title,year,editorial_json FROM inventory_works WHERE id='stable'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        preserved,
        ("stable".into(), "Film".into(), 2026, "{}".into())
    );
    let mut conn = pool.acquire().await.unwrap();
    backfill_title_sort_unicode(&mut conn).await.unwrap();
    drop(conn);
    let after_unicode: (String, String, i64, String, String) = query_as(
        "SELECT id,title,year,editorial_json,title_sort FROM inventory_works WHERE id='stable'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after_unicode,
        (
            "stable".into(),
            "Film".into(),
            2026,
            "{}".into(),
            "film".into()
        )
    );
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json,title_sort,kind) VALUES('new','lib','new','New',2026,'{}','new','film')").execute(&pool).await.unwrap();
    let added: i64 = query_scalar("SELECT added_at FROM inventory_works WHERE id='new'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(added > 0);
}
#[tokio::test]
async fn nfo_refresh_preserves_graph_technical_data_and_invalid_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("editorial.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
        .execute(&store.pool)
        .await
        .unwrap();
    let mut report = ScanReport { files: vec![ScannedFile { audios: Vec::new(), subtitles: Vec::new(),
        relative_path: "Film (2026)/film.mp4".into(), size: 42, modified_ns: "100".into(),
        title: "Film".into(), year: Some(2026), identity_key: "folder:Film (2026)".into(),
        external_ids: Default::default(), mime: "video/mp4".into(),
        probe: serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1"}],"format":{"duration":"2"}})).unwrap(),
        editorial: NfoOutcome::Missing, poster: None, reused: false,
        ..Default::default()
    }], complete: true, ..Default::default() };
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    let work_id: String = query_scalar("SELECT id FROM inventory_works")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let graph: Vec<(String,String,String,String)> = query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
    report.files[0].editorial = NfoOutcome::Valid {
        metadata: serde_json::from_value(json!({"title":"Titre NFO <script>","year":2001,"plot":"A & B","genres":["Drame"],"external_ids":{"tmdb":"123"}})).unwrap(),
        path: "Film (2026)/movie.nfo".into(), priority:1,
    };
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    let detail = super::detail(&store, &work_id).await.unwrap().unwrap();
    assert_eq!(detail["title"], "Titre NFO <script>");
    assert_eq!(detail["description"], "A & B");
    assert_eq!(detail["genres"], json!(["Drame"]));
    assert_eq!(detail["metadataProvenance"], "nfo");
    report.files[0].editorial = NfoOutcome::Invalid {
        path: "Film (2026)/movie.nfo".into(),
        message: "invalid XML".into(),
    };
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    assert_eq!(
        detail,
        super::detail(&store, &work_id).await.unwrap().unwrap()
    );
    let after: Vec<(String,String,String,String)> = query_as("SELECT a.id,s.id,v.id,e.id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id").fetch_all(&store.pool).await.unwrap();
    assert_eq!(graph, after);
    assert_eq!(
        load_cache(&store, "lib").await.unwrap()["Film (2026)/film.mp4"]
            .probe
            .streams[0]
            .codec_name
            .as_deref(),
        Some("av1")
    );
    store.pool.close().await;
    let store = IdentityStore::open(&db).await.unwrap();
    assert_eq!(
        detail,
        super::detail(&store, &work_id).await.unwrap().unwrap()
    );
    report.files[0].editorial = NfoOutcome::Missing;
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    let fallback = super::detail(&store, &work_id).await.unwrap().unwrap();
    assert_eq!(fallback["title"], "Film");
    assert_eq!(fallback["year"], 2026);
    assert_eq!(fallback["metadataProvenance"], "local");
    report.files[0].editorial = NfoOutcome::Valid {
        metadata: serde_json::from_value(json!({"title":"Old source NFO"})).unwrap(),
        path: "Film (2026)/movie.nfo".into(),
        priority: 1,
    };
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    query("UPDATE libraries SET root='/replacement' WHERE id='lib'")
        .execute(&store.pool)
        .await
        .unwrap();
    report.files[0].editorial = NfoOutcome::Invalid {
        path: "Film (2026)/movie.nfo".into(),
        message: "invalid replacement".into(),
    };
    report.files[0].title = "Replacement film".into();
    ingest(&store, "lib", Path::new("/replacement"), &report)
        .await
        .unwrap();
    let replacement = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(replacement["total"], 1);
    assert_eq!(replacement["items"][0]["title"], "Replacement film");
    assert_eq!(replacement["items"][0]["metadataProvenance"], "local");
    assert_ne!(replacement["items"][0]["id"], work_id);
    assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 1);
    report.files[0].editorial = NfoOutcome::Valid {
        metadata: serde_json::from_value(json!({"title":"Ignored NFO"})).unwrap(),
        path: "Film (2026)/movie.nfo".into(),
        priority: 1,
    };
    report.files[0].poster = Some(media_contracts::media::LocalImage {
        path: "poster.png".into(),
        size_bytes: 10,
        modified_ns: "1".into(),
        mime: "image/png".into(),
        priority: 1,
    });
    let logo = report.files[0].poster.clone().unwrap();
    report.files[0].artwork.insert("logo".into(), logo);
    query("UPDATE libraries SET options_json='{\"readLocalNfo\":false,\"fetchPoster\":false,\"fetchLogo\":false}' WHERE id='lib'").execute(&store.pool).await.unwrap();
    ingest(&store, "lib", Path::new("/replacement"), &report)
        .await
        .unwrap();
    let filtered = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(filtered["items"][0]["title"], "Replacement film");
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM inventory_images WHERE available=1")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn inventory_survives_reopen_is_idempotent_and_preserves_failed_scan() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("inventory.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Films','films','local','/movies',1)")
        .execute(&store.pool)
        .await
        .unwrap();
    let probe:Probe=serde_json::from_value(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1","pix_fmt":"yuv420p10le"},{"index":1,"codec_type":"audio","codec_name":"opus","tags":{"language":"fra"}}],"format":{"duration":"2"}})).unwrap();
    let file = |path: &str| ScannedFile {
        audios: Vec::new(),
        subtitles: Vec::new(),
        relative_path: path.into(),
        size: 42,
        modified_ns: "100".into(),
        title: "Film".into(),
        year: Some(2026),
        identity_key: "tmdb:123".into(),
        external_ids: Default::default(),
        probe: probe.clone(),
        mime: "video/mp4".into(),
        editorial: NfoOutcome::Missing,
        poster: None,
        reused: false,
        ..Default::default()
    };
    let report = ScanReport {
        files: vec![file("film.mp4"), file("film.mkv")],
        complete: true,
        ..Default::default()
    };
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    let first = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(first["total"], 1);
    assert!(first["items"][0]["addedAt"].as_i64().is_some_and(|n| n > 0));
    let work_id = first["items"][0]["id"].as_str().unwrap();
    let initial = detail(&store, work_id).await.unwrap().unwrap();
    ingest(&store, "lib", Path::new("/movies"), &report)
        .await
        .unwrap();
    assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
    assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 2);
    ingest(&store, "lib", Path::new("/movies"), &ScanReport::default())
        .await
        .unwrap();
    assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
    store.pool.close().await;
    let store = IdentityStore::open(&db).await.unwrap();
    assert_eq!(initial, detail(&store, work_id).await.unwrap().unwrap());
    query("UPDATE libraries SET root='/other' WHERE id='lib'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert!(load_cache(&store, "lib").await.unwrap().is_empty());
    assert_eq!(
        catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap()["total"],
        0
    );
    assert!(
        ingest(&store, "lib", Path::new("/movies"), &report)
            .await
            .is_err()
    );
    let mut new_report = report;
    for file in &mut new_report.files {
        file.title = "New Film".into();
        file.year = Some(2020);
        file.identity_key = "tmdb:456".into();
    }
    ingest(&store, "lib", Path::new("/other"), &new_report)
        .await
        .unwrap();
    assert_eq!(load_cache(&store, "lib").await.unwrap().len(), 2);
    let updated = catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(updated["total"], 1);
    assert_eq!(updated["items"][0]["title"], "New Film");
    assert_eq!(updated["items"][0]["year"], 2020);
    assert_ne!(updated["items"][0]["id"], work_id);
    assert!(
        detail(&store, work_id).await.unwrap().unwrap()["editions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    query("UPDATE libraries SET root='/movies' WHERE id='lib'")
        .execute(&store.pool)
        .await
        .unwrap();
    ingest(
        &store,
        "lib",
        Path::new("/movies"),
        &ScanReport {
            complete: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        catalogue(&store, &["lib".into()], None, "", "title-asc", 0, 24)
            .await
            .unwrap()["total"],
        0
    );
}

#[tokio::test]
async fn settings_latest_filters_watched_before_pagination_without_stale_account_cache() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("latest.sqlite"))
        .await
        .unwrap();
    for sql in [
        "INSERT INTO users(id,role) VALUES('reader','user'),('other','user')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('latest','Latest','films','local','/latest',1)",
        "INSERT INTO library_permissions(library_id,user_id) VALUES('latest','reader'),('latest','other')",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,has_available_asset) VALUES('a','latest','a','A','a','film','{}',1),('b','latest','b','B','b','film','{}',1),('c','latest','c','C','c','film','{}',1)",
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT id,id,'latest','/latest' FROM inventory_works",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT id,id,'latest',id||'.mp4',1,'0','video/mp4','{}',1 FROM inventory_works",
        "INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES('reader','a',100,100,1,1,1)",
    ] {
        query(sql).execute(&store.pool).await.unwrap();
    }
    let visible = ["latest".into()];
    let mut filter = CatalogueFilter {
        user_id: Some("reader".into()),
        hide_watched: true,
        ..Default::default()
    };
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        0,
        1,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"][0]["id"], "b");
    assert_eq!(page["hasMore"], true);
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        1,
        1,
        &filter,
        false,
    )
    .await
    .unwrap();
    assert!(page["total"].is_null());
    assert_eq!(page["items"][0]["id"], "c");
    assert_eq!(page["hasMore"], false);
    // Watched state is shared by canonical copies; filtering must happen
    // before a representative is selected, not after the page is limited.
    for sql in [
        "INSERT INTO media_groups(id,label) VALUES('b','Copies')",
        "INSERT INTO media_group_members(work_id,group_id) VALUES('b','b'),('c','b')",
        "INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES('reader','b',100,100,1,1,1)",
    ] {
        query(sql).execute(&store.pool).await.unwrap();
    }
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        0,
        2,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 0);
    query("UPDATE playback_progress SET watched=0 WHERE user_id='reader'")
        .execute(&store.pool)
        .await
        .unwrap();
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        0,
        2,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 2);
    filter.user_id = Some("other".into());
    query("UPDATE playback_progress SET watched=1 WHERE user_id='reader'")
        .execute(&store.pool)
        .await
        .unwrap();
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        0,
        2,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 2);
    filter.hide_watched = false;
    filter.user_id = Some("reader".into());
    let page = catalogue_page(
        &store,
        &visible,
        Some("film"),
        "",
        "title-asc",
        0,
        2,
        &filter,
        true,
    )
    .await
    .unwrap();
    assert_eq!(page["total"], 2);
}

#[test]
fn test_bounded_catalogue_cache_budget_and_lru_eviction() {
    let mut cache = BoundedCatalogueCache::new(2000);

    let val1 = serde_json::json!({"items": ["movie1", "movie2"], "total": 2});
    let val2 = serde_json::json!({"items": ["movie3", "movie4"], "total": 2});

    cache.insert("key1".into(), 1, val1.clone());
    let (bytes1, max_b, count1) = cache.stats();
    assert_eq!(count1, 1);
    assert!(bytes1 > 0);
    assert!(bytes1 <= max_b);

    cache.insert("key2".into(), 1, val2.clone());
    let (bytes2, _, count2) = cache.stats();
    assert_eq!(count2, 2);
    assert!(bytes2 > bytes1);

    // Access key1 to update its recency
    assert!(cache.get("key1", 1).is_some());

    // Stale generation removes entry
    assert!(cache.get("key2", 2).is_none());
    assert_eq!(cache.stats().2, 1);

    cache.clear();
    assert_eq!(cache.stats().0, 0);
    assert_eq!(cache.stats().2, 0);
}
