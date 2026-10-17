use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::AuthState, identity::IdentityStore, node::Inventory, scans::ScanJobs,
    secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;
use zeroize::Zeroizing;

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: Value,
    range: Option<&str>,
) -> (StatusCode, Value, Vec<u8>, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3000")
        .header("origin", "http://127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if !cookie.is_empty() {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-csrf-token", csrf);
    }
    if let Some(range) = range {
        req = req.header(header::RANGE, range);
    }
    let response = app
        .clone()
        .oneshot(
            req.body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, bytes, cookie)
}

async fn create_library(
    app: &Router,
    root: &Path,
    kind: &str,
    cookie: &str,
    csrf: &str,
    name: &str,
    user_ids: Vec<&str>,
) -> String {
    let (status, body, _, _) = request(
        app,
        "POST",
        "/api/v1/admin/libraries",
        cookie,
        Some(csrf),
        json!({
            "name": name,
            "kind": kind,
            "nodeId": "local",
            "root": root.to_string_lossy(),
            "enabled": true,
            "userIds": user_ids
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().into()
}

async fn scan(app: &Router, library: &str, cookie: &str, csrf: &str) -> Value {
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    assert_eq!(
        request(app, "POST", &uri, cookie, Some(csrf), Value::Null, None)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (status, body, _, _) =
                request(app, "GET", &uri, cookie, None, Value::Null, None).await;
            assert_eq!(status, StatusCode::OK);
            if body["status"] == "completed" {
                return body;
            }
            assert_ne!(body["status"], "error", "{body}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}

async fn router(root: &Path, store: &IdentityStore, jobs: Arc<ScanJobs>) -> Router {
    secure_app_with_jobs(
        Inventory::local(root.to_path_buf()).await.unwrap(),
        3000,
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
        jobs,
    )
    .await
    .unwrap()
}

fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 3, 8, 2,
        0, 0, 0, 54, 136, 73, 214, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 136, 48, 234, 7, 34,
        6, 20, 10, 0, 67, 242, 6, 151, 143, 92, 131, 231, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

#[tokio::test]
async fn comprehensive_series_flow_indexing_playback_permissions_and_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let series_root = root.join("TV Shows");
    let films_root = root.join("Films");

    // 1. Series 1: Cosmos Alpha (2023) [tvdb-101]
    let series1_dir = series_root.join("Cosmos Alpha (2023) [tvdb-101]");
    let s1_s00_dir = series1_dir.join("Specials");
    let s1_s01_dir = series1_dir.join("Season 01");
    let s1_s02_dir = series1_dir.join("Season 02");
    tokio::fs::create_dir_all(&s1_s00_dir).await.unwrap();
    tokio::fs::create_dir_all(&s1_s01_dir).await.unwrap();
    tokio::fs::create_dir_all(&s1_s02_dir).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mkv = generated.join("representative/representative.mkv");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");

    // Series 1 poster and tvshow.nfo
    tokio::fs::write(series1_dir.join("poster.png"), png())
        .await
        .unwrap();
    tokio::fs::write(
        series1_dir.join("tvshow.nfo"),
        r#"<tvshow>
            <title>Cosmos Alpha</title>
            <year>2023</year>
            <plot>A deep journey through stars.</plot>
            <id>101</id>
        </tvshow>"#,
    )
    .await
    .unwrap();

    // S00E01
    tokio::fs::copy(&sample_mkv, s1_s00_dir.join("S00E01.mkv"))
        .await
        .unwrap();

    // S01E01 with NFO
    let s01e01_path = s1_s01_dir.join("Cosmos Alpha - S01E01.mkv");
    tokio::fs::copy(&sample_mkv, &s01e01_path).await.unwrap();
    tokio::fs::write(
        s1_s01_dir.join("Cosmos Alpha - S01E01.nfo"),
        r#"<episodedetails>
            <title>First Contact</title>
            <plot>Humans meet extraterrestrial life.</plot>
        </episodedetails>"#,
    )
    .await
    .unwrap();

    // S01E02: two versions of the same episode
    tokio::fs::copy(&sample_mkv, s1_s01_dir.join("Cosmos Alpha - S01E02.mkv"))
        .await
        .unwrap();
    tokio::fs::copy(
        &sample_mkv,
        s1_s01_dir.join("Cosmos Alpha - S01E02 - 1080p.mkv"),
    )
    .await
    .unwrap();

    // S02E01
    let s02e01_path = s1_s02_dir.join("02x01.mp4");
    tokio::fs::copy(&sample_mp4, &s02e01_path).await.unwrap();

    // 2. Series 2: Planets (2024) with malformed NFO (graceful fallback)
    let series2_dir = series_root.join("Planets (2024)");
    let s2_s01_dir = series2_dir.join("Saison 01");
    tokio::fs::create_dir_all(&s2_s01_dir).await.unwrap();
    tokio::fs::write(
        series2_dir.join("tvshow.nfo"),
        b"<tvshow><title>Unclosed tag",
    )
    .await
    .unwrap();
    tokio::fs::copy(&sample_mp4, s2_s01_dir.join("S01E01.mp4"))
        .await
        .unwrap();

    // 3. Film library to verify non-regression
    let film_dir = films_root.join("Folder Film (2026) [tmdb-123]");
    tokio::fs::create_dir_all(&film_dir).await.unwrap();
    tokio::fs::copy(&sample_mp4, film_dir.join("movie.mp4"))
        .await
        .unwrap();

    // 4. Identity store & users
    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    let admin_id = store
        .bootstrap("admin", Zeroizing::new("admin password 1234".into()))
        .await
        .unwrap();

    let user_id = "standard-user-id".to_string();
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM local_credentials WHERE user_id=?")
            .bind(&admin_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO users(id, role) VALUES (?, 'user')")
        .bind(&user_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, 'standard-user', ?)")
        .bind(&user_id)
        .bind(&hash)
        .execute(store.pool())
        .await
        .unwrap();

    let jobs = Arc::new(ScanJobs::default());
    let app = router(&root, &store, jobs.clone()).await;

    // Login admin
    let (status, login, _, admin_cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "admin", "password": "admin password 1234"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let admin_csrf = login["csrfToken"].as_str().unwrap().to_owned();

    // Login normal user
    let (status, user_login, _, user_cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "standard-user", "password": "admin password 1234"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let _user_csrf = user_login["csrfToken"].as_str().unwrap().to_owned();

    // Create libraries
    // TV Shows initially not granted to normal user
    let series_lib_id = create_library(
        &app,
        &series_root,
        "series",
        &admin_cookie,
        &admin_csrf,
        "TV Shows",
        vec![],
    )
    .await;
    let films_lib_id = create_library(
        &app,
        &films_root,
        "films",
        &admin_cookie,
        &admin_csrf,
        "Films",
        vec![&user_id],
    )
    .await;

    // Scan TV Shows
    let scan_result = scan(&app, &series_lib_id, &admin_cookie, &admin_csrf).await;
    assert_eq!(scan_result["discovered"], 6);
    assert_eq!(scan_result["probed"], 6);
    assert_eq!(scan_result["reused"], 0);
    assert_eq!(scan_result["errors"].as_array().unwrap().len(), 1);
    assert!(
        scan_result["errors"][0]
            .as_str()
            .unwrap()
            .contains("incomplete NFO XML")
    );

    // Scan Films
    let film_scan = scan(&app, &films_lib_id, &admin_cookie, &admin_csrf).await;
    assert_eq!(film_scan["discovered"], 1);
    assert_eq!(film_scan["probed"], 1);

    // 5. Query Catalog: Series separation
    let (_, series_catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=series",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(series_catalog["total"], 2, "{series_catalog}");
    let items = series_catalog["items"].as_array().unwrap();

    let s1_item = items
        .iter()
        .find(|i| i["title"] == "Cosmos Alpha")
        .expect("Cosmos Alpha in catalog");
    assert_eq!(s1_item["year"], 2023);
    assert_eq!(s1_item["kind"], "series");
    assert_eq!(s1_item["seasonCount"], 3);
    assert_eq!(s1_item["episodeCount"], 4);
    assert!(s1_item["posterUrl"].is_string());
    let s1_id = s1_item["id"].as_str().unwrap().to_owned();

    let s2_item = items
        .iter()
        .find(|i| i["title"] == "Planets")
        .expect("Planets in catalog");
    assert_eq!(s2_item["year"], 2024);
    assert_eq!(s2_item["seasonCount"], 1);
    assert_eq!(s2_item["episodeCount"], 1);

    // Query Catalog: Film separation
    let (_, film_catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=film",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(film_catalog["total"], 1);
    assert_eq!(film_catalog["items"][0]["title"], "Folder Film");

    // 6. Series Detail & Ordering
    let (status, s1_detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s1_detail["title"], "Cosmos Alpha");
    assert_eq!(s1_detail["description"], "A deep journey through stars.");
    assert_eq!(s1_detail["plot"], "A deep journey through stars.");
    let seasons = s1_detail["seasons"].as_array().unwrap();
    assert_eq!(seasons.len(), 3);

    // Season 0 (Specials)
    assert_eq!(seasons[0]["number"], 0);
    assert_eq!(seasons[0]["title"], "Hors-série");
    let s0_episodes = seasons[0]["episodes"].as_array().unwrap();
    assert_eq!(s0_episodes.len(), 1);
    assert_eq!(s0_episodes[0]["number"], 1);
    let _s0e1_media_id =
        s0_episodes[0]["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
            .as_str()
            .unwrap()
            .to_owned();

    // Season 1
    assert_eq!(seasons[1]["number"], 1);
    assert_eq!(seasons[1]["title"], "Saison 1");
    let s1_episodes = seasons[1]["episodes"].as_array().unwrap();
    assert_eq!(s1_episodes.len(), 2);

    // S01E01 with NFO metadata
    assert_eq!(s1_episodes[0]["number"], 1);
    assert_eq!(s1_episodes[0]["title"], "First Contact");
    assert_eq!(
        s1_episodes[0]["description"],
        "Humans meet extraterrestrial life."
    );
    assert_eq!(s1_episodes[0]["plot"], "Humans meet extraterrestrial life.");
    let s1e1_media_id =
        s1_episodes[0]["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
            .as_str()
            .unwrap()
            .to_owned();

    // S01E02 with two versions
    assert_eq!(s1_episodes[1]["number"], 2);
    let s1e2_versions = s1_episodes[1]["editions"][0]["versions"]
        .as_array()
        .unwrap();
    assert_eq!(s1e2_versions.len(), 2);

    // Season 2
    assert_eq!(seasons[2]["number"], 2);
    assert_eq!(seasons[2]["title"], "Saison 2");
    let s2_episodes = seasons[2]["episodes"].as_array().unwrap();
    assert_eq!(s2_episodes.len(), 1);
    assert_eq!(s2_episodes[0]["number"], 1);
    let s2e1_media_id =
        s2_episodes[0]["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
            .as_str()
            .unwrap()
            .to_owned();

    // Path confinement: assert no absolute filesystem path was exposed
    let detail_str = serde_json::to_string(&s1_detail).unwrap();
    assert!(!detail_str.contains("C:\\"));
    assert!(!detail_str.contains("TV Shows"));

    // 7. Streaming & Playback: HEAD, Range GET, 206 Partial Content, Seek
    // HEAD S01E01
    let head_req = Request::builder()
        .method("HEAD")
        .uri(format!("/api/v1/assets/{s1e1_media_id}/content"))
        .header("host", "127.0.0.1:3000")
        .header("cookie", &admin_cookie)
        .body(Body::empty())
        .unwrap();
    let head_resp = app.clone().oneshot(head_req).await.unwrap();
    assert_eq!(head_resp.status(), StatusCode::OK);
    assert_eq!(
        head_resp
            .headers()
            .get(header::ACCEPT_RANGES)
            .unwrap()
            .to_str()
            .unwrap(),
        "bytes"
    );
    let s1e1_total_size: u64 = head_resp
        .headers()
        .get(header::CONTENT_LENGTH)
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(s1e1_total_size > 0);

    // Range GET S01E01 (first 1024 bytes)
    let (status, _, range_bytes, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{s1e1_media_id}/content"),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=0-1023"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(range_bytes.len(), 1024);

    // Seek / invalid range -> 416
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{s1e1_media_id}/content"),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=999999999-999999999"),
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);

    // Range GET S02E01 (MP4 container)
    let (status, _, mp4_bytes, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{s2e1_media_id}/content"),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=0-511"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(mp4_bytes.len(), 512);

    // Poster image GET
    let poster_url = s1_item["posterUrl"].as_str().unwrap();
    let (status, _, poster_bytes, _) = request(
        &app,
        "GET",
        poster_url,
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(poster_bytes, png());

    // 8. RBAC: standard-user permissions enforcement
    // Standard user has no permission on TV Shows:
    let (_, user_cat, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=series",
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(user_cat["total"], 0);

    // Detail forbidden for standard user
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Streaming forbidden for standard user
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{s1e1_media_id}/content"),
        &user_cookie,
        None,
        Value::Null,
        Some("bytes=0-100"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Poster forbidden for standard user
    let (status, _, _, _) = request(
        &app,
        "GET",
        poster_url,
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // But standard user CAN see Films
    let (_, user_films, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=film",
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(user_films["total"], 1);

    // Update library permissions: grant standard user access to TV Shows
    let (status, _, _, _) = request(
        &app,
        "PUT",
        &format!("/api/v1/admin/libraries/{series_lib_id}"),
        &admin_cookie,
        Some(&admin_csrf),
        json!({
            "name": "TV Shows",
            "kind": "series",
            "nodeId": "local",
            "root": series_root.to_string_lossy(),
            "enabled": true,
            "userIds": [&user_id]
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Now standard user can access series catalog and detail
    let (_, user_cat_granted, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=series",
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(user_cat_granted["total"], 2);

    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // And standard user can stream!
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{s1e1_media_id}/content"),
        &user_cookie,
        None,
        Value::Null,
        Some("bytes=0-100"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);

    // 9. Rescan: verify probe reuse and stable IDs
    let rescan_result = scan(&app, &series_lib_id, &admin_cookie, &admin_csrf).await;
    assert_eq!(rescan_result["discovered"], 6);
    assert_eq!(rescan_result["probed"], 0);
    assert_eq!(rescan_result["reused"], 6);

    let (_, rescan_detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(rescan_detail["id"], s1_id);
    assert_eq!(
        rescan_detail["seasons"][1]["episodes"][0]["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"],
        s1e1_media_id
    );

    // 10. File modification and episode deletion on rescan
    // Delete Season 02 episode
    tokio::fs::remove_file(&s02e01_path).await.unwrap();

    // Touch S01E01 with appended comment to change size & modification time
    let mut modified_s01e01 = tokio::fs::read(&s01e01_path).await.unwrap();
    modified_s01e01.extend_from_slice(b"\n// modified episode payload");
    tokio::fs::write(&s01e01_path, &modified_s01e01)
        .await
        .unwrap();

    let mod_scan = scan(&app, &series_lib_id, &admin_cookie, &admin_csrf).await;
    assert_eq!(mod_scan["discovered"], 5); // 6 - 1 deleted
    assert_eq!(mod_scan["probed"], 1); // modified s01e01 reprobed
    assert_eq!(mod_scan["reused"], 4); // others reused

    // Verify Season 2 now has 0 available episodes (or omitted), and S01E01 still works
    let (_, after_delete_detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let after_seasons = after_delete_detail["seasons"].as_array().unwrap();
    // Either Season 2 is omitted or has empty episodes
    if let Some(s2) = after_seasons.iter().find(|s| s["number"] == 2) {
        assert!(s2["episodes"].as_array().unwrap().is_empty());
    }

    // 11. Scan Cancellation
    jobs.cancel_all();
    let uri = format!("/api/v1/admin/libraries/{series_lib_id}/scan");
    let (status, _, _, _) = request(
        &app,
        "POST",
        &uri,
        &admin_cookie,
        Some(&admin_csrf),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    jobs.reset();

    // 12. SQLite Restart & Persistence Verification
    // Drop in-memory router and recreate from existing on-disk DB file
    drop(app);
    let store_reopened = IdentityStore::open(&db).await.unwrap();
    let jobs_new = Arc::new(ScanJobs::default());
    let app_reopened = router(&root, &store_reopened, jobs_new).await;

    // Login after restart
    let (status, _relogin, _, new_cookie) = request(
        &app_reopened,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "admin", "password": "admin password 1234"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, restart_cat, _, _) = request(
        &app_reopened,
        "GET",
        "/api/v1/catalog?kind=series",
        &new_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(restart_cat["total"], 2);

    let (_, restart_detail, _, _) = request(
        &app_reopened,
        "GET",
        &format!("/api/v1/catalog/{s1_id}"),
        &new_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(restart_detail["id"], s1_id);
    let restart_s1e1_id = restart_detail["seasons"][1]["episodes"][0]["editions"][0]["versions"][0]
        ["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap();

    let (status, _, range_after_restart, _) = request(
        &app_reopened,
        "GET",
        &format!("/api/v1/assets/{restart_s1e1_id}/content"),
        &new_cookie,
        None,
        Value::Null,
        Some("bytes=0-511"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(range_after_restart.len(), 512);
}

#[tokio::test]
async fn a3_contradictory_series_staging_deterministic_selection_and_200_episodes() {
    use media_poc::{
        inventory::{
            InventoryBatch, NfoOutcome, Probe, ScannedFile, clean_staging, publish_staged_scan,
            stage_batch,
        },
        node::images::{LocalImage, validate},
    };
    use sqlx::Row;
    use std::collections::BTreeMap;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let series_root = root.join("TV Shows");
    let films_root = root.join("Films");
    let show_dir = series_root.join("Show");
    let s01_dir = show_dir.join("Season 01");
    let film_dir = films_root.join("Blade Horizon (2024)");
    tokio::fs::create_dir_all(&s01_dir).await.unwrap();
    tokio::fs::create_dir_all(&film_dir).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mkv = generated.join("representative/representative.mkv");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");

    // Copy real playable media files for S01E01 (two versions: MKV & MP4), S01E02, and a Film
    let ep1_v1_disk = s01_dir.join("S01E01 - 1080p.mkv");
    let ep1_v2_disk = s01_dir.join("S01E01 - 2160p.mp4");
    let ep2_disk = s01_dir.join("S01E02.mkv");
    let film_disk = film_dir.join("Blade Horizon (2024).mkv");
    tokio::fs::copy(&sample_mkv, &ep1_v1_disk).await.unwrap();
    tokio::fs::copy(&sample_mp4, &ep1_v2_disk).await.unwrap();
    tokio::fs::copy(&sample_mkv, &ep2_disk).await.unwrap();
    tokio::fs::copy(&sample_mkv, &film_disk).await.unwrap();

    // Create two distinct valid posters on disk with different sizes, mtimes, priorities, and paths:
    // - Show/poster.png (priority 1, smaller size, lexicographically > "Show/folder.png")
    // - Show/folder.png (priority 2, larger padded file on disk? No: valid PNG with larger dimensions or extra chunk)
    // Wait: png() is a valid 2x3 PNG (71 bytes). Let's also create a second valid PNG with an extra trailing IEND-safe or create Show/poster.png and Show/folder.png with different modification times and priorities!
    // Even better: let's write two valid PNGs of different byte sizes by using png() for poster.png and writing a second valid PNG (or WebP/JPEG) if needed, or let's build a valid 1x1 PNG vs 2x3 PNG!
    // 1x1 RGB PNG (67 bytes) vs 2x3 RGB PNG (71 bytes):
    let png_small = png(); // 71 bytes
    // Let's write poster.png first, then sleep briefly and write folder.png with a different valid PNG or test with real validated files.
    let poster_a_disk = show_dir.join("poster.png");
    let poster_b_disk = show_dir.join("folder.png");
    tokio::fs::write(&poster_a_disk, &png_small).await.unwrap();
    tokio::fs::write(&poster_b_disk, &png_small).await.unwrap();

    let val_a = validate(&poster_a_disk).await.unwrap();
    let poster_a = LocalImage {
        path: "Show/poster.png".into(),
        size_bytes: val_a.size_bytes,
        modified_ns: val_a.modified_ns.clone(),
        mime: val_a.mime.into(),
        priority: 1, // higher priority (lower number)
    };
    // Poster B has contradictory technical metadata (larger size_bytes, larger modified_ns, larger mime/priority)
    // Under the old MAX() query, MAX(series_poster_path) picked "Show/poster.png" (since 'p' > 'f'),
    // while MAX(series_poster_size) picked 99999, MAX(series_poster_modified_ns) picked "9999999999999999999",
    // MAX(series_poster_mime) picked "image/webp", and MAX(series_poster_priority) picked 6!
    // That corrupted the inventory_images row for Show/poster.png and broke image serving (404)!
    let poster_b = LocalImage {
        path: "Show/folder.png".into(),
        size_bytes: val_a.size_bytes + 50_000,
        modified_ns: "9999999999999999999".into(),
        mime: "image/webp".into(),
        priority: 6,
    };

    let ep1_v1_meta = tokio::fs::metadata(&ep1_v1_disk).await.unwrap();
    let ep1_v2_meta = tokio::fs::metadata(&ep1_v2_disk).await.unwrap();
    let ep2_meta = tokio::fs::metadata(&ep2_disk).await.unwrap();

    let probe_mkv: Probe = serde_json::from_value(json!({
        "streams": [
            {"index": 0, "codec_type": "video", "codec_name": "av1"},
            {"index": 1, "codec_type": "audio", "codec_name": "opus", "tags": {"language": "fra"}}
        ],
        "format": {"duration": "2.0"}
    }))
    .unwrap();
    let probe_mp4: Probe = serde_json::from_value(json!({
        "streams": [
            {"index": 0, "codec_type": "video", "codec_name": "av1"},
            {"index": 1, "codec_type": "audio", "codec_name": "opus", "tags": {"language": "eng"}}
        ],
        "format": {"duration": "2.0"}
    }))
    .unwrap();

    let mut ext_ids_a = BTreeMap::new();
    ext_ids_a.insert("tvdb".to_string(), "101".to_string());
    ext_ids_a.insert("tmdb".to_string(), "202".to_string());

    let mut ext_ids_b = BTreeMap::new();
    ext_ids_b.insert("imdb".to_string(), "tt9999999".to_string());

    // Row A: S01E01 version 1 (MKV)
    // Has valid series NFO, title "Alpha Chronicles", year 2021, external_ids {tvdb:101, tmdb:202}, poster_a (priority 1)
    let row_a = ScannedFile {
        relative_path: "Show/Season 01/S01E01 - 1080p.mkv".into(),
        size: ep1_v1_meta.len(),
        modified_ns: "1001".into(),
        title: "Episode 1".into(),
        year: Some(2021),
        identity_key: "series:tvdb:101:s01e01".into(),
        external_ids: BTreeMap::new(),
        probe: probe_mkv.clone(),
        mime: "video/x-matroska".into(),
        editorial: NfoOutcome::Valid {
            metadata: serde_json::from_value(json!({
                "title": "Pilot NFO",
                "plot": "First episode valid plot."
            }))
            .unwrap(),
            path: "Show/Season 01/S01E01.nfo".into(),
            priority: 1,
        },
        poster: None,
        reused: false,
        series_identity_key: Some("tvdb:101".into()),
        series_title: Some("Alpha Chronicles".into()),
        series_year: Some(2021),
        series_external_ids: Some(ext_ids_a.clone()),
        series_editorial: Some(NfoOutcome::Valid {
            metadata: serde_json::from_value(json!({
                "title": "Alpha Chronicles NFO",
                "original_title": "Alpha Original",
                "year": 2021,
                "plot": "Canonical series plot from valid NFO.",
                "genres": ["Sci-Fi", "Drama"],
                "external_ids": {"tvdb": "101", "tmdb": "202"}
            }))
            .unwrap(),
            path: "Show/tvshow.nfo".into(),
            priority: 1,
        }),
        series_poster: Some(poster_a.clone()),
        season_number: Some(1),
        episode_number: Some(1),
    };

    // Row B: S01E01 version 2 (MP4) — same episode (multiple versions!), same series_identity_key,
    // but contradictory series_title ("Zeta Override" > "Alpha Chronicles"), contradictory series_year (2025 > 2021),
    // invalid series NFO, contradictory external IDs, and poster_b (priority 6, larger size/mtime/mime).
    let row_b = ScannedFile {
        relative_path: "Show/Season 01/S01E01 - 2160p.mp4".into(),
        size: ep1_v2_meta.len(),
        modified_ns: "1002".into(),
        title: "Episode 1 4K".into(),
        year: Some(2025),
        identity_key: "series:tvdb:101:s01e01".into(),
        external_ids: BTreeMap::new(),
        probe: probe_mp4.clone(),
        mime: "video/mp4".into(),
        editorial: NfoOutcome::Invalid {
            path: "Show/Season 01/S01E01 - 2160p.nfo".into(),
            message: "corrupt episode xml".into(),
        },
        poster: None,
        reused: false,
        series_identity_key: Some("tvdb:101".into()),
        series_title: Some("Zeta Override".into()),
        series_year: Some(2025),
        series_external_ids: Some(ext_ids_b.clone()),
        series_editorial: Some(NfoOutcome::Invalid {
            path: "Show/Season 01/tvshow.nfo".into(),
            message: "corrupt series xml".into(),
        }),
        series_poster: Some(poster_b.clone()),
        season_number: Some(1),
        episode_number: Some(1),
    };

    // Row C: S01E02
    // Missing NFO, another title/year combination, no poster.
    let row_c = ScannedFile {
        relative_path: "Show/Season 01/S01E02.mkv".into(),
        size: ep2_meta.len(),
        modified_ns: "1003".into(),
        title: "Episode 2".into(),
        year: Some(2023),
        identity_key: "series:tvdb:101:s01e02".into(),
        external_ids: BTreeMap::new(),
        probe: probe_mkv.clone(),
        mime: "video/x-matroska".into(),
        editorial: NfoOutcome::Missing,
        poster: None,
        reused: false,
        series_identity_key: Some("tvdb:101".into()),
        series_title: Some("Middle Title".into()),
        series_year: Some(2023),
        series_external_ids: None,
        series_editorial: Some(NfoOutcome::Missing),
        series_poster: None,
        season_number: Some(1),
        episode_number: Some(2),
    };

    // 1. Verify insertion order invariance across two independent databases:
    // DB 1 stages [row_a, row_b, row_c] (forward order)
    // DB 2 stages [row_b, row_c, row_a] (contradictory row_b inserted first, row_a last)
    let db_forward = temp.path().join("forward.sqlite");
    let db_reverse = temp.path().join("reverse.sqlite");
    let store_fwd = IdentityStore::open(&db_forward).await.unwrap();
    let store_rev = IdentityStore::open(&db_reverse).await.unwrap();

    let admin_id = store_fwd
        .bootstrap("admin", Zeroizing::new("admin password 1234".into()))
        .await
        .unwrap();

    let user_id = "viewer-user-id".to_string();
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM local_credentials WHERE user_id=?")
            .bind(&admin_id)
            .fetch_one(store_fwd.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO users(id, role) VALUES (?, 'user')")
        .bind(&user_id)
        .execute(store_fwd.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, 'viewer', ?)",
    )
    .bind(&user_id)
    .bind(&hash)
    .execute(store_fwd.pool())
    .await
    .unwrap();

    let jobs_fwd = Arc::new(ScanJobs::default());
    let app_fwd = router(&root, &store_fwd, jobs_fwd).await;

    let (status, login, _, admin_cookie) = request(
        &app_fwd,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "admin", "password": "admin password 1234"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let admin_csrf = login["csrfToken"].as_str().unwrap().to_owned();

    let (status, user_login, _, user_cookie) = request(
        &app_fwd,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "viewer", "password": "admin password 1234"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let _user_csrf = user_login["csrfToken"].as_str().unwrap().to_owned();

    let series_lib_id = create_library(
        &app_fwd,
        &series_root,
        "series",
        &admin_cookie,
        &admin_csrf,
        "Series Lib",
        vec![],
    )
    .await;
    let films_lib_id = create_library(
        &app_fwd,
        &films_root,
        "films",
        &admin_cookie,
        &admin_csrf,
        "Films Lib",
        vec![&user_id],
    )
    .await;

    // Create matching library row in store_rev
    sqlx::query("INSERT INTO libraries VALUES(?, 'Series Lib', 'series', 'local', ?, 1)")
        .bind(&series_lib_id)
        .bind(series_root.to_string_lossy().as_ref())
        .execute(store_rev.pool())
        .await
        .unwrap();

    // Stage forward in store_fwd and reverse in store_rev
    stage_batch(
        &store_fwd,
        "scan-fwd-1",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "scan-fwd-1".into(),
            sequence: 0,
            files: vec![row_a.clone(), row_b.clone(), row_c.clone()],
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(&store_fwd, "scan-fwd-1", &series_lib_id, &series_root, true)
        .await
        .unwrap();
    clean_staging(&store_fwd, "scan-fwd-1").await.unwrap();

    stage_batch(
        &store_rev,
        "scan-rev-1",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "scan-rev-1".into(),
            sequence: 0,
            files: vec![row_b.clone(), row_c.clone(), row_a.clone()],
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(&store_rev, "scan-rev-1", &series_lib_id, &series_root, true)
        .await
        .unwrap();
    clean_staging(&store_rev, "scan-rev-1").await.unwrap();

    // Compare published series work row, external IDs, and image row between forward and reverse stores
    let fwd_work = sqlx::query(
        "SELECT id, title, title_sort, year, external_ids_json, editorial_json \
         FROM inventory_works WHERE library_id=? AND kind='series'",
    )
    .bind(&series_lib_id)
    .fetch_one(store_fwd.pool())
    .await
    .unwrap();

    let rev_work = sqlx::query(
        "SELECT id, title, title_sort, year, external_ids_json, editorial_json \
         FROM inventory_works WHERE library_id=? AND kind='series'",
    )
    .bind(&series_lib_id)
    .fetch_one(store_rev.pool())
    .await
    .unwrap();

    // Verify deterministic metadata selection: Row A wins in both orders, no Frankenstein mix with Row B or Row C!
    for w in [&fwd_work, &rev_work] {
        assert_eq!(w.get::<String, _>("title"), "Alpha Chronicles NFO");
        assert_eq!(w.get::<String, _>("title_sort"), "alpha chronicles nfo");
        assert_eq!(w.get::<Option<i64>, _>("year"), Some(2021));
        let ed: Value = serde_json::from_str(&w.get::<String, _>("editorial_json")).unwrap();
        assert_eq!(ed["original_title"], "Alpha Original");
        assert_eq!(ed["plot"], "Canonical series plot from valid NFO.");
        assert_eq!(ed["genres"], json!(["Sci-Fi", "Drama"]));
    }

    // Verify external IDs come coherently from Row A (tvdb=101, tmdb=202) and NOT mixed with Row B (imdb=tt9999999)
    let fwd_series_id: String = fwd_work.get("id");
    let rev_series_id: String = rev_work.get("id");
    let fwd_ext: Value =
        serde_json::from_str(&fwd_work.get::<String, _>("external_ids_json")).unwrap();
    let rev_ext: Value =
        serde_json::from_str(&rev_work.get::<String, _>("external_ids_json")).unwrap();
    assert_eq!(fwd_ext, json!({"tmdb": "202", "tvdb": "101"}));
    assert_eq!(fwd_ext, rev_ext);

    // Verify poster coherence in inventory_images: all columns match poster_a, never mixed with poster_b
    let fwd_img = sqlx::query(
        "SELECT id, relative_path, mime, size, modified_ns, priority, available \
         FROM inventory_images WHERE work_id=? AND available=1",
    )
    .bind(&fwd_series_id)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let rev_img = sqlx::query(
        "SELECT id, relative_path, mime, size, modified_ns, priority, available \
         FROM inventory_images WHERE work_id=? AND available=1",
    )
    .bind(&rev_series_id)
    .fetch_all(store_rev.pool())
    .await
    .unwrap();
    assert_eq!(fwd_img.len(), 1);
    assert_eq!(rev_img.len(), 1);
    for img in [&fwd_img[0], &rev_img[0]] {
        assert_eq!(img.get::<String, _>("relative_path"), "Show/poster.png");
        assert_eq!(img.get::<String, _>("mime"), "image/png");
        assert_eq!(img.get::<i64, _>("size") as u64, poster_a.size_bytes);
        assert_eq!(img.get::<String, _>("modified_ns"), poster_a.modified_ns);
        assert_eq!(img.get::<i64, _>("priority"), 1);
    }
    let initial_image_id: String = fwd_img[0].get("id");

    // Also verify insertion-order invariance when NO row has a valid NFO (local fallback tie-breaking)
    let mut row_a_no_nfo = row_a.clone();
    row_a_no_nfo.series_editorial = Some(NfoOutcome::Missing);
    let mut row_b_no_nfo = row_b.clone();
    row_b_no_nfo.series_editorial = Some(NfoOutcome::Missing);
    row_b_no_nfo.series_external_ids = Some(ext_ids_a.clone()); // Equal completeness to test relative_path ASC tie-breaker!

    let db_tie_1 = temp.path().join("tie1.sqlite");
    let db_tie_2 = temp.path().join("tie2.sqlite");
    let store_tie_1 = IdentityStore::open(&db_tie_1).await.unwrap();
    let store_tie_2 = IdentityStore::open(&db_tie_2).await.unwrap();
    for s in [&store_tie_1, &store_tie_2] {
        sqlx::query("INSERT INTO libraries VALUES(?, 'Series Lib', 'series', 'local', ?, 1)")
            .bind(&series_lib_id)
            .bind(series_root.to_string_lossy().as_ref())
            .execute(s.pool())
            .await
            .unwrap();
    }
    stage_batch(
        &store_tie_1,
        "tie-1",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "tie-1".into(),
            sequence: 0,
            files: vec![row_a_no_nfo.clone(), row_b_no_nfo.clone()],
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(&store_tie_1, "tie-1", &series_lib_id, &series_root, true)
        .await
        .unwrap();

    stage_batch(
        &store_tie_2,
        "tie-2",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "tie-2".into(),
            sequence: 0,
            files: vec![row_b_no_nfo.clone(), row_a_no_nfo.clone()],
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(&store_tie_2, "tie-2", &series_lib_id, &series_root, true)
        .await
        .unwrap();

    let tie1_row: (String, Option<i64>, String) = sqlx::query_as(
        "SELECT title, year, editorial_json FROM inventory_works WHERE library_id=? AND kind='series'",
    )
    .bind(&series_lib_id)
    .fetch_one(store_tie_1.pool())
    .await
    .unwrap();
    let tie2_row: (String, Option<i64>, String) = sqlx::query_as(
        "SELECT title, year, editorial_json FROM inventory_works WHERE library_id=? AND kind='series'",
    )
    .bind(&series_lib_id)
    .fetch_one(store_tie_2.pool())
    .await
    .unwrap();
    assert_eq!(
        tie1_row,
        ("Alpha Chronicles".to_string(), Some(2021), "{}".to_string())
    );
    assert_eq!(tie1_row, tie2_row);

    // 2. Capture all UUIDs from store_fwd before rescanning
    let initial_seasons: Vec<(String, i64)> = sqlx::query_as(
        "SELECT id, season_number FROM inventory_seasons WHERE series_id=? ORDER BY season_number ASC",
    )
    .bind(&fwd_series_id)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let initial_episodes: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT id, work_id, episode_number FROM inventory_episodes WHERE season_id=? ORDER BY episode_number ASC",
    )
    .bind(&initial_seasons[0].0)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let initial_graph: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT a.id, s.id, v.id, e.id FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         WHERE a.library_id=? ORDER BY a.relative_path ASC",
    )
    .bind(&series_lib_id)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    assert_eq!(initial_graph.len(), 3); // 2 versions of S01E01 + 1 version of S01E02

    // 3. Valid NFO -> Invalid NFO on rescan (with reversed staging order):
    // Simulate tvshow.nfo becoming invalid/corrupted on all rows while local titles/years try to override.
    // Previously validated NFO metadata MUST be preserved, and all UUIDs MUST remain unchanged!
    let mut row_a_invalid = row_a.clone();
    row_a_invalid.series_editorial = Some(NfoOutcome::Invalid {
        path: "Show/tvshow.nfo".into(),
        message: "corrupted on rescan".into(),
    });
    row_a_invalid.series_title = Some("Fallback Title After Corruption".into());
    row_a_invalid.series_year = Some(2026);

    stage_batch(
        &store_fwd,
        "scan-fwd-2",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "scan-fwd-2".into(),
            sequence: 0,
            files: vec![row_c.clone(), row_b.clone(), row_a_invalid],
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(&store_fwd, "scan-fwd-2", &series_lib_id, &series_root, true)
        .await
        .unwrap();
    clean_staging(&store_fwd, "scan-fwd-2").await.unwrap();

    let after_invalid_work =
        sqlx::query("SELECT id, title, year, editorial_json FROM inventory_works WHERE id=?")
            .bind(&fwd_series_id)
            .fetch_one(store_fwd.pool())
            .await
            .unwrap();
    assert_eq!(
        after_invalid_work.get::<String, _>("title"),
        "Alpha Chronicles NFO"
    );
    assert_eq!(after_invalid_work.get::<Option<i64>, _>("year"), Some(2021));
    let after_ed: Value =
        serde_json::from_str(&after_invalid_work.get::<String, _>("editorial_json")).unwrap();
    assert_eq!(after_ed["plot"], "Canonical series plot from valid NFO.");

    // Verify NO UUID was recreated across the rescan
    let after_seasons: Vec<(String, i64)> = sqlx::query_as(
        "SELECT id, season_number FROM inventory_seasons WHERE series_id=? ORDER BY season_number ASC",
    )
    .bind(&fwd_series_id)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let after_episodes: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT id, work_id, episode_number FROM inventory_episodes WHERE season_id=? ORDER BY episode_number ASC",
    )
    .bind(&initial_seasons[0].0)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let after_graph: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT a.id, s.id, v.id, e.id FROM inventory_assets a \
         JOIN inventory_sources s ON s.id=a.source_id \
         JOIN inventory_versions v ON v.id=s.version_id \
         JOIN inventory_editions e ON e.id=v.edition_id \
         WHERE a.library_id=? ORDER BY a.relative_path ASC",
    )
    .bind(&series_lib_id)
    .fetch_all(store_fwd.pool())
    .await
    .unwrap();
    let after_image_id: String =
        sqlx::query_scalar("SELECT id FROM inventory_images WHERE work_id=? AND available=1")
            .bind(&fwd_series_id)
            .fetch_one(store_fwd.pool())
            .await
            .unwrap();
    assert_eq!(initial_seasons, after_seasons);
    assert_eq!(initial_episodes, after_episodes);
    assert_eq!(initial_graph, after_graph);
    assert_eq!(initial_image_id, after_image_id);

    // 4. Verify HTTP catalog detail, multiple episode versions, poster serving, RBAC, and streaming
    let (status, detail, _, _) = request(
        &app_fwd,
        "GET",
        &format!("/api/v1/catalog/{fwd_series_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["title"], "Alpha Chronicles NFO");
    assert_eq!(detail["year"], 2021);
    assert_eq!(detail["episodeCount"], 2);
    let ep1_versions = detail["seasons"][0]["episodes"][0]["editions"][0]["versions"]
        .as_array()
        .unwrap();
    assert_eq!(ep1_versions.len(), 2);

    // Poster URL must serve HTTP 200 with exact coherent bytes (proving no Frankenstein size/mtime mismatch)
    let poster_url = detail["posterUrl"].as_str().unwrap();
    let (status, _, served_poster, _) = request(
        &app_fwd,
        "GET",
        poster_url,
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(served_poster, png_small);

    // Streaming both versions of S01E01 works for admin, forbidden for unauthorized standard user
    for ver in ep1_versions {
        let media_id = ver["sources"][0]["playbackMediaId"].as_str().unwrap();
        let (status, _, bytes, _) = request(
            &app_fwd,
            "GET",
            &format!("/api/v1/assets/{media_id}/content"),
            &admin_cookie,
            None,
            Value::Null,
            Some("bytes=0-255"),
        )
        .await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(bytes.len(), 256);

        let (status_user, _, _, _) = request(
            &app_fwd,
            "GET",
            &format!("/api/v1/assets/{media_id}/content"),
            &user_cookie,
            None,
            Value::Null,
            Some("bytes=0-255"),
        )
        .await;
        assert_eq!(status_user, StatusCode::FORBIDDEN);
    }

    // 5. Verify Films library non-regression
    let film_scan = scan(&app_fwd, &films_lib_id, &admin_cookie, &admin_csrf).await;
    assert_eq!(film_scan["discovered"], 1);
    let (status, films_cat, _, _) = request(
        &app_fwd,
        "GET",
        "/api/v1/catalog?kind=film",
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(films_cat["total"], 1);
    assert_eq!(films_cat["items"][0]["title"], "Blade Horizon");
    assert_eq!(films_cat["items"][0]["year"], 2024);

    // 6. Performance and determinism on 200 episodes across 10 seasons
    let mut ep_200_files = Vec::with_capacity(200);
    for season in (1..=10).rev() {
        for ep in (1..=20).rev() {
            let rel =
                format!("MegaSeries/Season {season:02}/MegaSeries - S{season:02}E{ep:02}.mkv");
            ep_200_files.push(ScannedFile {
                relative_path: rel,
                size: 1024,
                modified_ns: "5000".into(),
                title: format!("Episode S{season:02}E{ep:02}"),
                year: Some(2022),
                identity_key: format!("series:tvdb:9999:s{season:02}e{ep:02}"),
                external_ids: BTreeMap::new(),
                probe: probe_mkv.clone(),
                mime: "video/x-matroska".into(),
                editorial: NfoOutcome::Missing,
                poster: None,
                reused: false,
                series_identity_key: Some("tvdb:9999".into()),
                series_title: Some(if season == 1 && ep == 1 {
                    "MegaSeries Canonical".into()
                } else {
                    format!("MegaSeries Variant S{season:02}E{ep:02}")
                }),
                series_year: Some(if season == 1 && ep == 1 { 2022 } else { 2024 }),
                series_external_ids: None,
                series_editorial: Some(NfoOutcome::Missing),
                series_poster: Some(if season == 1 && ep == 1 {
                    poster_a.clone()
                } else {
                    poster_b.clone()
                }),
                season_number: Some(season),
                episode_number: Some(ep),
            });
        }
    }

    let start_200 = std::time::Instant::now();
    stage_batch(
        &store_fwd,
        "scan-200-rev",
        &series_lib_id,
        &InventoryBatch {
            scan_id: "scan-200-rev".into(),
            sequence: 0,
            files: ep_200_files.clone(),
            errors: vec![],
            warnings: vec![],
        },
    )
    .await
    .unwrap();
    publish_staged_scan(
        &store_fwd,
        "scan-200-rev",
        &series_lib_id,
        &series_root,
        true,
    )
    .await
    .unwrap();
    clean_staging(&store_fwd, "scan-200-rev").await.unwrap();
    let elapsed_200 = start_200.elapsed();
    assert!(
        elapsed_200 < Duration::from_secs(5),
        "publishing 200 episodes took too long: {elapsed_200:?}"
    );

    let mega_id: String = sqlx::query_scalar(
        "SELECT id FROM inventory_works WHERE library_id=? AND kind='series' AND identity_key LIKE '%tvdb:9999'",
    )
    .bind(&series_lib_id)
    .fetch_one(store_fwd.pool())
    .await
    .unwrap();

    let (status, mega_detail, _, _) = request(
        &app_fwd,
        "GET",
        &format!("/api/v1/catalog/{mega_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mega_detail["title"], "MegaSeries Canonical");
    assert_eq!(mega_detail["year"], 2022);
    assert_eq!(mega_detail["seasonCount"], 10);
    assert_eq!(mega_detail["episodeCount"], 200);
}
