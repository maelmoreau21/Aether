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
