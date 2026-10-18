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
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
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
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            let (status, body, _, _) =
                request(app, "GET", &uri, cookie, None, Value::Null, None).await;
            assert_eq!(status, StatusCode::OK);
            if body["status"] == "completed" {
                return body;
            }
            assert_ne!(body["status"], "error", "{body}");
            tokio::time::sleep(Duration::from_millis(50)).await;
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
async fn test_200_episodes_series_performance_multiversion_and_playback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let series_root = root.join("TV Shows");
    let films_root = root.join("Films");
    tokio::fs::create_dir_all(&series_root).await.unwrap();
    tokio::fs::create_dir_all(&films_root).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mkv = generated.join("representative/representative.mkv");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");

    // 1. Create a 200-episode series across 10 seasons (20 episodes each)
    let series_dir = series_root.join("Centennial (2025)");
    tokio::fs::create_dir_all(&series_dir).await.unwrap();
    tokio::fs::write(series_dir.join("poster.png"), png())
        .await
        .unwrap();
    tokio::fs::write(
        series_dir.join("tvshow.nfo"),
        r#"<tvshow>
            <title>Centennial</title>
            <year>2025</year>
            <plot>An epic spanning 10 seasons and 200 episodes.</plot>
        </tvshow>"#,
    )
    .await
    .unwrap();

    let num_seasons = 10;
    let episodes_per_season = 20;

    for s in 1..=num_seasons {
        let season_dir = series_dir.join(format!("Season {:02}", s));
        tokio::fs::create_dir_all(&season_dir).await.unwrap();
        for e in 1..=episodes_per_season {
            let ep_file = season_dir.join(format!("Centennial - S{:02}E{:02}.mp4", s, e));
            tokio::fs::copy(&sample_mp4, &ep_file).await.unwrap();

            // S01E01 has TWO versions: MP4 and MKV
            if s == 1 && e == 1 {
                let ep_v2_file = season_dir.join("Centennial - S01E01 - Director Cut.mkv");
                tokio::fs::copy(&sample_mkv, &ep_v2_file).await.unwrap();
                tokio::fs::write(
                    season_dir.join("Centennial - S01E01.nfo"),
                    r#"<episodedetails>
                        <title>Pilot Episode</title>
                        <plot>The journey begins.</plot>
                    </episodedetails>"#,
                )
                .await
                .unwrap();
            }
        }
    }

    // Also create 1 film for non-regression verification
    let film_dir = films_root.join("Chronos (2024)");
    tokio::fs::create_dir_all(&film_dir).await.unwrap();
    tokio::fs::copy(&sample_mkv, film_dir.join("movie.mkv"))
        .await
        .unwrap();
    tokio::fs::write(film_dir.join("poster.png"), png())
        .await
        .unwrap();

    // Setup IdentityStore and App
    let db_path = temp.path().join("test.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    let admin_id = store
        .bootstrap("operator", Zeroizing::new("MasterSecret123!".into()))
        .await
        .unwrap();

    let alice_id = "alice-user-id".to_string();
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM local_credentials WHERE user_id=?")
            .bind(&admin_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO users(id, role) VALUES (?, 'user')")
        .bind(&alice_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, 'alice', ?)",
    )
    .bind(&alice_id)
    .bind(&hash)
    .execute(store.pool())
    .await
    .unwrap();

    let jobs = Arc::new(ScanJobs::default());
    let app = router(&root, &store, jobs.clone()).await;

    // Login as admin
    let (status, login, _, admin_cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "operator", "password": "MasterSecret123!"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{login}");
    let csrf = login["csrfToken"].as_str().unwrap().to_string();

    // Create Series library and Films library
    let series_lib_id = create_library(
        &app,
        &series_root,
        "series",
        &admin_cookie,
        &csrf,
        "Séries",
        vec![alice_id.as_str()],
    )
    .await;

    let films_lib_id = create_library(
        &app,
        &films_root,
        "films",
        &admin_cookie,
        &csrf,
        "Films",
        vec![alice_id.as_str()],
    )
    .await;

    // Scan Series library
    let t_scan_start = Instant::now();
    let scan_report = scan(&app, &series_lib_id, &admin_cookie, &csrf).await;
    let scan_duration = t_scan_start.elapsed();
    println!("Scan of 200-episode series took: {:.2?}", scan_duration);
    assert_eq!(scan_report["status"], "completed");
    assert_eq!(scan_report["errors"].as_array().unwrap().len(), 0);

    // Scan Films library
    let film_scan_report = scan(&app, &films_lib_id, &admin_cookie, &csrf).await;
    assert_eq!(film_scan_report["status"], "completed");

    // 2. Measure SQLite detail() performance on 200 episodes
    let (status, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?kind=series",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["total"], 1);
    let series_id = catalog["items"][0]["id"].as_str().unwrap().to_string();

    let t_detail_start = Instant::now();
    let (status, detail_val, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let detail_duration = t_detail_start.elapsed();
    println!(
        "GET /api/v1/catalog/{} (200 episodes) response time: {:.2?}",
        series_id, detail_duration
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail_val["kind"], "series");
    assert_eq!(detail_val["title"], "Centennial");
    assert_eq!(detail_val["seasonCount"], 10);
    assert_eq!(detail_val["episodeCount"], 200);

    let seasons = detail_val["seasons"].as_array().unwrap();
    assert_eq!(seasons.len(), 10);

    // Verify S01E01 has TWO versions
    let s01 = &seasons[0];
    assert_eq!(s01["number"], 1);
    let s01_episodes = s01["episodes"].as_array().unwrap();
    assert_eq!(s01_episodes.len(), 20);

    let ep1 = &s01_episodes[0];
    assert_eq!(ep1["number"], 1);
    assert_eq!(ep1["title"], "Pilot Episode");
    let ep1_editions = ep1["editions"].as_array().unwrap();
    assert!(!ep1_editions.is_empty());
    let ep1_versions = ep1_editions[0]["versions"].as_array().unwrap();
    assert_eq!(ep1_versions.len(), 2, "Expected 2 versions for S01E01");

    // Extract first and second version playback IDs
    let ep1_v1_playback_id = ep1_versions[0]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_string();
    let ep1_v2_playback_id = ep1_versions[1]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(ep1_v1_playback_id, ep1_v2_playback_id);

    // Verify last episode S10E20
    let s10 = &seasons[9];
    assert_eq!(s10["number"], 10);
    let s10_episodes = s10["episodes"].as_array().unwrap();
    assert_eq!(s10_episodes.len(), 20);
    let ep200 = &s10_episodes[19];
    assert_eq!(ep200["number"], 20);
    let ep200_playback_id = ep200["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Verify that unavailable episodes are cleanly excluded
    sqlx::query("UPDATE inventory_assets SET available=0 WHERE relative_path LIKE '%S05E10%'")
        .execute(store.pool())
        .await
        .unwrap();

    let (_, detail_excluded, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(
        detail_excluded["episodeCount"], 199,
        "Unavailable episode should be excluded"
    );
    let s05 = &detail_excluded["seasons"][4];
    assert_eq!(s05["number"], 5);
    let s05_episodes = s05["episodes"].as_array().unwrap();
    assert_eq!(s05_episodes.len(), 19);
    assert!(!s05_episodes.iter().any(|ep| ep["number"] == 10));

    // Restore availability
    sqlx::query("UPDATE inventory_assets SET available=1 WHERE relative_path LIKE '%S05E10%'")
        .execute(store.pool())
        .await
        .unwrap();

    let (_, detail_restored, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(detail_restored["episodeCount"], 200);

    // 4. Playback test: First episode version 1 and version 2, and last episode
    // First episode version 1: descriptor + GET / HEAD / Range / seek
    let (status, desc1, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/media/{}", ep1_v1_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(desc1["id"], ep1_v1_playback_id);

    // HEAD
    let (status, _, _, _) = request(
        &app,
        "HEAD",
        &format!("/api/v1/assets/{}/content", ep1_v1_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // GET Range 0-63
    let (status, _, bytes_range, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{}/content", ep1_v1_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=0-63"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes_range.len(), 64);

    // Seek Range 1000-2000
    let (status, _, bytes_seek, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{}/content", ep1_v1_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=1000-2000"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes_seek.len(), 1001);

    // First episode version 2
    let (status, desc1_v2, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/media/{}", ep1_v2_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(desc1_v2["id"], ep1_v2_playback_id);

    // Last episode playback
    let (status, desc_last, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/media/{}", ep200_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(desc_last["id"], ep200_playback_id);

    let (status, _, bytes_last, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{}/content", ep200_playback_id),
        &admin_cookie,
        None,
        Value::Null,
        Some("bytes=0-63"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes_last.len(), 64);

    // 5. User permissions and revocation before playback
    // Login as alice
    let (status, _, _, alice_cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "alice", "password": "MasterSecret123!"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Alice can access series before revocation
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &alice_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Revoke alice's access to Séries library
    let (status, _, _, _) = request(
        &app,
        "PUT",
        &format!("/api/v1/admin/libraries/{}", series_lib_id),
        &admin_cookie,
        Some(&csrf),
        json!({
            "name": "Séries",
            "kind": "series",
            "nodeId": "local",
            "root": series_root.to_string_lossy(),
            "enabled": true,
            "userIds": [] // alice revoked!
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Alice now receives 403 on catalog detail, media descriptor, and streaming content
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &alice_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/media/{}", ep1_v1_playback_id),
        &alice_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{}/content", ep1_v1_playback_id),
        &alice_cookie,
        None,
        Value::Null,
        Some("bytes=0-63"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 6. Fast incremental rescan: verify zero new probes
    let t_rescan = Instant::now();
    let rescan_report = scan(&app, &series_lib_id, &admin_cookie, &csrf).await;
    let rescan_duration = t_rescan.elapsed();
    println!("Rescan of 200-episode series took: {:.2?}", rescan_duration);
    assert_eq!(rescan_report["status"], "completed");
    assert_eq!(rescan_report["probed"], 0, "All entries must be reused");
    assert_eq!(
        rescan_report["reused"], 201,
        "200 episodes + 1 multi-version = 201 files"
    );

    // 7. SQLite restart and persistence check
    drop(app);
    drop(store);

    let reopened_store = IdentityStore::open(&db_path).await.unwrap();
    let reopened_jobs = Arc::new(ScanJobs::default());
    let reopened_app = router(&root, &reopened_store, reopened_jobs).await;

    // Login after restart
    let (status, _, _, new_admin_cookie) = request(
        &reopened_app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "operator", "password": "MasterSecret123!"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, restarted_detail, _, _) = request(
        &reopened_app,
        "GET",
        &format!("/api/v1/catalog/{}", series_id),
        &new_admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restarted_detail["episodeCount"], 200);
    assert_eq!(restarted_detail["seasonCount"], 10);

    // 8. Non-regression Films
    let (status, films_catalog, _, _) = request(
        &reopened_app,
        "GET",
        "/api/v1/catalog?kind=film",
        &new_admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(films_catalog["total"], 1);
    let film = &films_catalog["items"][0];
    assert_eq!(film["title"], "Chronos");
    let film_id = film["id"].as_str().unwrap();

    let (status, film_detail, _, _) = request(
        &reopened_app,
        "GET",
        &format!("/api/v1/catalog/{}", film_id),
        &new_admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(film_detail["kind"], "film");
    assert!(!film_detail["editions"].as_array().unwrap().is_empty());
    assert_eq!(film_detail["seasons"].as_array().unwrap().len(), 0);

    let film_playback_id =
        film_detail["editions"][0]["versions"][0]["sources"][0]["playbackMediaId"]
            .as_str()
            .unwrap();
    let (status, _, bytes_film, _) = request(
        &reopened_app,
        "GET",
        &format!("/api/v1/assets/{}/content", film_playback_id),
        &new_admin_cookie,
        None,
        Value::Null,
        Some("bytes=0-63"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(bytes_film.len(), 64);
}
