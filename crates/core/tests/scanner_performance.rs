use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::AuthState,
    identity::IdentityStore,
    inventory::{InventoryBatch, NfoOutcome, ScannedFile},
    node::Inventory,
    scans::{ScanJobs, router as scans_router},
    secure_app, secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceExt;
use zeroize::Zeroizing;

fn current_rss_bytes() -> usize {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn K32GetProcessMemoryInfo(
                h_process: *mut std::ffi::c_void,
                ppsmem_counters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }
        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0 {
                counters.working_set_size
            } else {
                0
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let parts: Vec<&str> = statm.split_whitespace().collect();
            if let Some(pages) = parts.get(1).and_then(|p| p.parse::<usize>().ok()) {
                return pages * 4096;
            }
        }
        0
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}

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

async fn create_library(app: &Router, root: &Path, cookie: &str, csrf: &str, name: &str) -> String {
    let (status, body, _, _) = request(
        app,
        "POST",
        "/api/v1/admin/libraries",
        cookie,
        Some(csrf),
        json!({
            "name": name,
            "kind": "films",
            "nodeId": "local",
            "root": root.to_string_lossy(),
            "enabled": true,
            "userIds": []
        }),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().into()
}

async fn trigger_scan(app: &Router, library: &str, cookie: &str, csrf: &str) {
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    let (status, body, _, _) =
        request(app, "POST", &uri, cookie, Some(csrf), Value::Null, None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
}

async fn wait_scan(app: &Router, library: &str, cookie: &str, timeout_secs: u64) -> Value {
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        loop {
            let (status, body, _, _) =
                request(app, "GET", &uri, cookie, None, Value::Null, None).await;
            assert_eq!(status, StatusCode::OK);
            let state = body["status"].as_str().unwrap_or("");
            if state == "completed" || state == "error" {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scan timed out")
}

async fn setup_app(root: &Path, store: &IdentityStore) -> (Router, String, String) {
    let app = secure_app(
        Inventory::local(root.to_path_buf()).await.unwrap(),
        3000,
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
    )
    .await
    .unwrap();

    let (status, login, _, cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login":"scan-admin","password":"synthetic scanner password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap().to_owned();
    (app, cookie, csrf)
}

fn sample_png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 3, 8, 2,
        0, 0, 0, 54, 136, 73, 214, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 136, 48, 234, 7, 34,
        6, 20, 10, 0, 67, 242, 6, 151, 143, 92, 131, 231, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

#[tokio::test]
async fn benchmark_scanner_flow_and_metrics() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");
    assert!(sample_mp4.exists(), "Sample mp4 fixture must exist");

    let initial_rss = current_rss_bytes();
    println!(
        "Baseline RSS before test: {:.2} MiB",
        initial_rss as f64 / (1024.0 * 1024.0)
    );

    // Create 100 synthetic movie directories with video, nfo, poster
    let file_count = 100;
    let png = sample_png();
    let t_create = Instant::now();
    for i in 0..file_count {
        let folder = films.join(format!(
            "Synthetic Movie {:04} (2024) [tmdb-{}]",
            i,
            10000 + i
        ));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::copy(&sample_mp4, folder.join("movie.mp4"))
            .await
            .unwrap();
        tokio::fs::write(
            folder.join("movie.nfo"),
            format!(
                r#"<movie><title>Synthetic Movie {:04}</title><year>2024</year><plot>Synopsis {:04}</plot></movie>"#,
                i, i
            ),
        )
        .await
        .unwrap();
        tokio::fs::write(folder.join("poster.png"), &png)
            .await
            .unwrap();
    }
    println!(
        "Created {} synthetic files on disk in {:.2} ms",
        file_count,
        t_create.elapsed().as_millis()
    );

    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    let (app, cookie, csrf) = setup_app(&root, &store).await;
    let library = create_library(&app, &films, &cookie, &csrf, "Films").await;

    // 1. Initial Scan
    let t_scan1 = Instant::now();
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res1 = wait_scan(&app, &library, &cookie, 120).await;
    let dur_scan1 = t_scan1.elapsed();
    let rss_after_scan1 = current_rss_bytes();

    println!(
        "Scan 1 (Initial): status={}, discovered={}, probed={}, reused={}, errors={} in {:.2} s (RSS: {:.2} MiB)",
        res1["status"],
        res1["discovered"],
        res1["probed"],
        res1["reused"],
        res1["errors"].as_array().unwrap().len(),
        dur_scan1.as_secs_f64(),
        rss_after_scan1 as f64 / (1024.0 * 1024.0)
    );
    assert_eq!(res1["status"], "completed");
    assert_eq!(res1["discovered"], file_count);
    assert_eq!(res1["probed"], file_count);
    assert_eq!(res1["reused"], 0);

    // 2. Second Scan (unchanged): MUST have zero new probes!
    let t_scan2 = Instant::now();
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res2 = wait_scan(&app, &library, &cookie, 30).await;
    let dur_scan2 = t_scan2.elapsed();
    let rss_after_scan2 = current_rss_bytes();

    println!(
        "Scan 2 (Unchanged): status={}, discovered={}, probed={}, reused={} in {:.2} ms (RSS: {:.2} MiB)",
        res2["status"],
        res2["discovered"],
        res2["probed"],
        res2["reused"],
        dur_scan2.as_millis(),
        rss_after_scan2 as f64 / (1024.0 * 1024.0)
    );
    assert_eq!(res2["status"], "completed");
    assert_eq!(res2["discovered"], file_count);
    assert_eq!(
        res2["probed"], 0,
        "Second scan must have zero new video probes"
    );
    assert_eq!(res2["reused"], file_count);

    // 3. Modification of an NFO
    let folder_0 = films.join("Synthetic Movie 0000 (2024) [tmdb-10000]");
    tokio::fs::write(
        folder_0.join("movie.nfo"),
        r#"<movie><title>Updated Title 0000</title><year>2025</year><plot>Updated Plot</plot></movie>"#,
    )
    .await
    .unwrap();

    let t_scan3 = Instant::now();
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res3 = wait_scan(&app, &library, &cookie, 30).await;
    let dur_scan3 = t_scan3.elapsed();
    println!(
        "Scan 3 (NFO modified): probed={}, reused={} in {:.2} ms",
        res3["probed"],
        res3["reused"],
        dur_scan3.as_millis()
    );
    assert_eq!(res3["probed"], 0, "NFO change must NOT trigger video probe");
    assert_eq!(res3["reused"], file_count);

    // Verify catalog reflects modified NFO
    let (_, cat, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?q=Updated",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(cat["total"], 1);
    assert_eq!(cat["items"][0]["title"], "Updated Title 0000");

    // 4. Deletion of a poster
    tokio::fs::remove_file(folder_0.join("poster.png"))
        .await
        .unwrap();
    let t_scan4 = Instant::now();
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res4 = wait_scan(&app, &library, &cookie, 30).await;
    let dur_scan4 = t_scan4.elapsed();
    println!(
        "Scan 4 (Poster deleted): probed={}, reused={} in {:.2} ms",
        res4["probed"],
        res4["reused"],
        dur_scan4.as_millis()
    );
    assert_eq!(res4["probed"], 0);

    // Verify catalog reflects removed poster (posterUrl is null for that item)
    let (_, cat4, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?q=Updated",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(cat4["items"][0]["posterUrl"], Value::Null);
}

#[tokio::test]
async fn benchmark_10000_entries_cache_and_catalog_latency() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin password 12345".into()))
        .await
        .unwrap();

    let count = 10000;
    let mut tx = store.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib-main','Films Populaires','films','local','/media/films',1)")
        .execute(&mut *tx).await.unwrap();

    let sample_titles = [
        "Été à Paris",
        "Éléphant Blanc",
        "À la dérive",
        "Ça chapitre un",
        "L'Odyssée de l'espace",
        "Cinéma Paradiso",
        "The Matrix Resurrections",
        "Interstellar Voyage",
        "Blade Runner 2049",
        "Le Fabuleux Destin d'Amélie",
    ];

    for i in 0..count {
        let work_id = format!("work-{:06}", i);
        let title_base = sample_titles[i % sample_titles.len()];
        let title = format!("{} #{:06}", title_base, i);
        let title_sort = title.to_lowercase();
        let year = 1990 + (i % 35) as i64;
        let identity_key = format!("synthetic:{:06}", i);

        sqlx::query(
            "INSERT INTO inventory_works(id, library_id, identity_key, title, year, external_ids_json, editorial_json, title_sort) \
             VALUES(?, 'lib-main', ?, ?, ?, '{}', '{\"plot\": \"Synopsis synthétique\"}', ?)"
        )
        .bind(&work_id).bind(&identity_key).bind(&title).bind(year).bind(&title_sort)
        .execute(&mut *tx).await.unwrap();

        let edition_id = format!("edition-{:06}", i);
        sqlx::query("INSERT INTO inventory_editions VALUES(?, ?)")
            .bind(&edition_id)
            .bind(&work_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let version_id = format!("version-{:06}", i);
        sqlx::query("INSERT INTO inventory_versions VALUES(?, ?)")
            .bind(&version_id)
            .bind(&edition_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let source_id = format!("source-{:06}", i);
        sqlx::query("INSERT INTO inventory_sources VALUES('/media/films', ?, ?, 'lib-main')")
            .bind(&source_id)
            .bind(&version_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let asset_id = format!("asset-{:06}", i);
        let rel_path = format!("film_{:06}/movie.mkv", i);
        // Realistic probe JSON with streams
        let probe_json = r#"{"streams":[{"index":0,"codec_name":"av1","codec_type":"video","pix_fmt":"yuv420p10le"},{"index":1,"codec_name":"opus","codec_type":"audio","channels":6,"tags":{"language":"fra"}}],"format":{"format_name":"matroska,webm","duration":"120.000"}}"#;
        sqlx::query(
            "INSERT INTO inventory_assets VALUES(?, ?, 'lib-main', ?, 1048576, '1700000000', 'video/x-matroska', ?, 1)"
        )
        .bind(&asset_id).bind(&source_id).bind(&rel_path).bind(probe_json)
        .execute(&mut *tx).await.unwrap();

        if i % 2 == 0 {
            let img_id = format!("img-{:06}", i);
            let img_rel = format!("film_{:06}/poster.png", i);
            sqlx::query(
                "INSERT INTO inventory_images VALUES(?, ?, 'lib-main', '/media/films', ?, 51200, '1700000000', 'image/png', 1, 1)"
            )
            .bind(&img_id).bind(&work_id).bind(&img_rel)
            .execute(&mut *tx).await.unwrap();
        }
    }
    tx.commit().await.unwrap();
    println!("Inserted 10,000 synthetic database entries");

    let rss_before_scan_cache = current_rss_bytes();
    let t_scan_cache = Instant::now();
    let scan_cache = media_poc::inventory::load_scan_cache(&store, "lib-main")
        .await
        .unwrap();
    let dur_scan_cache = t_scan_cache.elapsed();
    let rss_after_scan_cache = current_rss_bytes();

    println!(
        "Optimized load_scan_cache on 10,000 entries: loaded {} items in {:.2} ms (RSS: {:.2} MiB -> {:.2} MiB, diff: +{:.2} MiB)",
        scan_cache.len(),
        dur_scan_cache.as_millis(),
        rss_before_scan_cache as f64 / (1024.0 * 1024.0),
        rss_after_scan_cache as f64 / (1024.0 * 1024.0),
        (rss_after_scan_cache.saturating_sub(rss_before_scan_cache)) as f64 / (1024.0 * 1024.0)
    );
    assert_eq!(scan_cache.len(), count);

    let rss_before_cache = current_rss_bytes();
    let t_cache = Instant::now();
    let cache = media_poc::inventory::load_cache(&store, "lib-main")
        .await
        .unwrap();
    let dur_cache = t_cache.elapsed();
    let rss_after_cache = current_rss_bytes();

    println!(
        "Legacy load_cache on 10,000 entries: loaded {} items in {:.2} ms (RSS: {:.2} MiB -> {:.2} MiB, diff: +{:.2} MiB)",
        cache.len(),
        dur_cache.as_millis(),
        rss_before_cache as f64 / (1024.0 * 1024.0),
        rss_after_cache as f64 / (1024.0 * 1024.0),
        (rss_after_cache.saturating_sub(rss_before_cache)) as f64 / (1024.0 * 1024.0)
    );
    assert_eq!(cache.len(), count);

    // Test staging 10,000 entries in bounded batches of 100
    let scan_id = uuid::Uuid::new_v4().to_string();
    let mut total_stage_duration = Duration::ZERO;
    let mut max_batch_duration = Duration::ZERO;
    let batches_count = 100;
    for chunk in 0..batches_count {
        let files: Vec<ScannedFile> = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                ScannedFile {
                    audios: Vec::new(),
                    subtitles: Vec::new(),
                    relative_path: format!("film_{:06}/movie.mkv", idx),
                    identity_key: format!("synth:{:06}", idx),
                    title: format!("Synthetic #{:06}", idx),
                    year: Some(2020),
                    external_ids: std::collections::BTreeMap::new(),
                    size: 1048576,
                    modified_ns: "1700000000".into(),
                    mime: "video/x-matroska".into(),
                    probe: media_poc::media::Probe::default(),
                    editorial: NfoOutcome::Missing,
                    poster: None,
                    reused: true,
                    ..Default::default()
                }
            })
            .collect();
        let batch = InventoryBatch {
            scan_id: scan_id.clone().into(),
            sequence: chunk as u64,
            files,
            errors: vec![],
            warnings: vec![],
        };
        let t_stage = Instant::now();
        media_poc::inventory::stage_batch(&store, &scan_id, "lib-main", &batch)
            .await
            .unwrap();
        let dur = t_stage.elapsed();
        total_stage_duration += dur;
        if dur > max_batch_duration {
            max_batch_duration = dur;
        }
    }
    println!(
        "Staged 10,000 entries in {} batches: total {:.2} ms, avg {:.2} ms/batch, max {:.2} ms/batch",
        batches_count,
        total_stage_duration.as_millis(),
        total_stage_duration.as_millis() as f64 / batches_count as f64,
        max_batch_duration.as_millis()
    );

    // Test atomic publish of 10,000 entries
    let t_publish = Instant::now();
    media_poc::inventory::publish_staged_scan(
        &store,
        &scan_id,
        "lib-main",
        Path::new("/media/films"),
        true,
    )
    .await
    .unwrap();
    let dur_publish = t_publish.elapsed();
    println!(
        "Published 10,000 staged entries in {:.2} ms",
        dur_publish.as_millis()
    );

    // Test rescan publish of 10,000 unchanged staged entries (reused = true with already indexed editorial)
    let scan_id_rescan = uuid::Uuid::new_v4().to_string();
    for chunk in 0..batches_count {
        let files: Vec<ScannedFile> = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                ScannedFile {
                    audios: Vec::new(),
                    subtitles: Vec::new(),
                    relative_path: format!("film_{:06}/movie.mkv", idx),
                    identity_key: format!("synth:{:06}", idx),
                    title: format!("Synthetic #{:06}", idx),
                    year: Some(2020),
                    external_ids: std::collections::BTreeMap::new(),
                    size: 1048576,
                    modified_ns: "1700000000".into(),
                    mime: "video/x-matroska".into(),
                    probe: media_poc::media::Probe::default(),
                    editorial: NfoOutcome::Missing,
                    poster: None,
                    reused: true,
                    ..Default::default()
                }
            })
            .collect();
        let batch = InventoryBatch {
            scan_id: scan_id_rescan.clone().into(),
            sequence: chunk as u64,
            files,
            errors: vec![],
            warnings: vec![],
        };
        media_poc::inventory::stage_batch(&store, &scan_id_rescan, "lib-main", &batch)
            .await
            .unwrap();
    }
    let t_publish_rescan = Instant::now();
    media_poc::inventory::publish_staged_scan(
        &store,
        &scan_id_rescan,
        "lib-main",
        Path::new("/media/films"),
        true,
    )
    .await
    .unwrap();
    let dur_publish_rescan = t_publish_rescan.elapsed();
    println!(
        "Rescan publish of 10,000 unchanged entries: published in {:.2} ms (speedup: {:.1}x)",
        dur_publish_rescan.as_millis(),
        dur_publish.as_millis() as f64 / dur_publish_rescan.as_millis().max(1) as f64
    );

    // Test catalogue latency with 10,000 items
    let t_cat = Instant::now();
    let cat_res = media_poc::inventory::catalogue(
        &store,
        &["lib-main".into()],
        Some("film"),
        "",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    let dur_cat = t_cat.elapsed();
    println!(
        "Catalogue query on 10,000 entries: total={} returned={} in {:.2} ms",
        cat_res["total"],
        cat_res["items"].as_array().unwrap().len(),
        dur_cat.as_millis()
    );
}

#[tokio::test]
async fn test_scan_cancellation_and_restart_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    let jobs = Arc::new(ScanJobs::default());
    let (app, cookie, csrf) = {
        let app = secure_app_with_jobs(
            Inventory::local(root.clone()).await.unwrap(),
            3000,
            AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
                .await
                .unwrap(),
            jobs.clone(),
        )
        .await
        .unwrap();
        let (status, login, _, cookie) = request(
            &app,
            "POST",
            "/api/v1/auth/login",
            "",
            None,
            json!({"login":"scan-admin","password":"synthetic scanner password"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let csrf = login["csrfToken"].as_str().unwrap().to_owned();
        (app, cookie, csrf)
    };

    let library = create_library(&app, &films, &cookie, &csrf, "CancelLib").await;

    // 1. Test Cancellation during scan
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");
    for i in 0..5 {
        let folder = films.join(format!("Cancel Movie {:02} (2024)", i));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::copy(&sample_mp4, folder.join("movie.mp4"))
            .await
            .unwrap();
    }

    // Set cancel flag BEFORE scan start to test early cancellation handling
    jobs.cancel_all();
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    let (status, _, _, _) =
        request(&app, "POST", &uri, &cookie, Some(&csrf), Value::Null, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // Reset cancel flag
    jobs.reset();

    // 2. Test Restart Recovery
    // Simulate process termination leaving orphan running job and staging data
    let scan_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('orphan-lib', 'Orphan Library', 'films', 'local', '/media/orphan', 1)")
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO scan_jobs(library_id, status) VALUES('orphan-lib', 'running')")
        .execute(store.pool())
        .await
        .unwrap();
    let orphan_batch = InventoryBatch {
        scan_id: scan_id.clone().into(),
        sequence: 0,
        files: vec![ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: "orphan.mkv".into(),
            identity_key: "orphan".into(),
            title: "Orphan Movie".into(),
            year: Some(2024),
            external_ids: std::collections::BTreeMap::new(),
            size: 1048576,
            modified_ns: "1700".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Missing,
            poster: None,
            reused: false,
            ..Default::default()
        }],
        errors: vec![],
        warnings: vec![media_contracts::inventory::ScanIssue {
            relative_path: "orphan.mkv".into(),
            message: "synthetic warning".into(),
        }],
    };
    media_poc::inventory::stage_batch(&store, &scan_id, "orphan-lib", &orphan_batch)
        .await
        .unwrap();

    let orphan_files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(orphan_files, 1);

    // Re-initialize scans router (as happens on Core startup)
    let _new_router = scans_router(
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
        root.clone(),
        Arc::new(ScanJobs::default()),
    )
    .await
    .unwrap();

    // Verify orphan job was marked as error
    let (status, errors_json): (String, String) =
        sqlx::query_as("SELECT status, errors_json FROM scan_jobs WHERE library_id = 'orphan-lib'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(status, "error");
    assert!(errors_json.contains("Scan interrompu par un redémarrage"));

    // Verify staging tables were completely cleaned up
    let remaining_staging: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        remaining_staging, 0,
        "Staging files must be 0 after restart"
    );

    let remaining_issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_issues")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        remaining_issues, 0,
        "Staging issues must be 0 after restart"
    );
}

#[tokio::test]
async fn test_concurrent_playback_and_catalog_navigation_during_scan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");

    // Populate 20 movies
    for i in 0..20 {
        let folder = films.join(format!("Concurrent Movie {:02} (2024)", i));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::copy(&sample_mp4, folder.join("movie.mp4"))
            .await
            .unwrap();
        tokio::fs::write(
            folder.join("movie.nfo"),
            format!(
                "<movie><title>Concurrent Movie {:02}</title><year>2024</year></movie>",
                i
            ),
        )
        .await
        .unwrap();
    }

    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    let (app, cookie, csrf) = setup_app(&root, &store).await;
    let library = create_library(&app, &films, &cookie, &csrf, "ConcurrentFilms").await;

    // Initial scan to establish catalogue & assets
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res1 = wait_scan(&app, &library, &cookie, 60).await;
    assert_eq!(res1["status"], "completed");

    // Get an asset ID for stream test
    let (_, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?limit=5",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let work_id = catalog["items"][0]["id"].as_str().unwrap();
    let (_, detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{work_id}"),
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let asset_id = detail["editions"][0]["versions"][0]["sources"][0]["assetIds"][0]
        .as_str()
        .unwrap()
        .to_owned();

    // Trigger a second scan (runs in background)
    trigger_scan(&app, &library, &cookie, &csrf).await;

    // Concurrently send catalog requests and stream range requests while scan is running
    let mut catalog_latencies = Vec::new();
    let mut stream_successes = 0;

    for _ in 0..25 {
        let t_start = Instant::now();
        let (status, cat, _, _) = request(
            &app,
            "GET",
            "/api/v1/catalog?q=Concurrent",
            &cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        let latency = t_start.elapsed();
        assert_eq!(status, StatusCode::OK, "Catalog query failed during scan");
        assert!(cat["total"].as_i64().unwrap() >= 20);
        catalog_latencies.push(latency);

        let (stream_status, _, stream_bytes, _) = request(
            &app,
            "GET",
            &format!("/api/v1/assets/{asset_id}/content"),
            &cookie,
            None,
            Value::Null,
            Some("bytes=0-100"),
        )
        .await;
        assert_eq!(
            stream_status,
            StatusCode::PARTIAL_CONTENT,
            "Stream Range request failed during scan"
        );
        assert_eq!(stream_bytes.len(), 101);
        stream_successes += 1;
    }

    let scan_res = wait_scan(&app, &library, &cookie, 30).await;
    assert_eq!(scan_res["status"], "completed");

    let avg_cat_latency =
        catalog_latencies.iter().sum::<Duration>() / catalog_latencies.len() as u32;
    let max_cat_latency = catalog_latencies.iter().max().unwrap();

    println!(
        "Concurrent tests during scan: 25 catalog requests (avg {:.2} ms, max {:.2} ms), 25 stream requests ({} ok, 0 failed)",
        avg_cat_latency.as_millis(),
        max_cat_latency.as_millis(),
        stream_successes
    );
}

#[tokio::test]
async fn test_1000_synthetic_files_scan_and_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let file_count = 1000;
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");
    let sample_mp4_stub = {
        let full = tokio::fs::read(&sample_mp4).await.unwrap();
        full[..full.len().min(512)].to_vec()
    };
    let png = sample_png();

    let t_create = Instant::now();
    for i in 0..file_count {
        let folder = films.join(format!(
            "Synth1000 Movie {:04} (2024) [tmdb-{}]",
            i,
            20000 + i
        ));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::write(folder.join("movie.mp4"), &sample_mp4_stub)
            .await
            .unwrap();
        tokio::fs::write(
            folder.join("movie.nfo"),
            format!(
                r#"<movie><title>Synth1000 Movie {:04}</title><year>2024</year><plot>Synopsis {:04}</plot></movie>"#,
                i, i
            ),
        )
        .await
        .unwrap();
        tokio::fs::write(folder.join("poster.png"), &png)
            .await
            .unwrap();
    }
    println!(
        "Created 1,000 synthetic movies on disk in {:.2} ms",
        t_create.elapsed().as_millis()
    );

    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    let (app, cookie, csrf) = setup_app(&root, &store).await;
    let library = create_library(&app, &films, &cookie, &csrf, "Films 1000").await;

    let lib_root: String = sqlx::query_scalar("SELECT root FROM libraries WHERE id=?")
        .bind(&library)
        .fetch_one(store.pool())
        .await
        .unwrap();

    // Pre-populate probe cache in database so this simulates an existing 1,000-movie library rescan
    // (Testing the full streaming scanner, folder discovery, batch staging, and publication without 1,000 ffprobe child processes)
    let probe_json = r#"{"streams":[{"index":0,"codec_name":"h264","codec_type":"video","pix_fmt":"yuv420p"},{"index":1,"codec_name":"aac","codec_type":"audio","channels":2,"tags":{"language":"eng"}}],"format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"5.000"}}"#;

    let mut tx = store.pool().begin().await.unwrap();
    for i in 0..file_count {
        let work_id = format!("work-1000-{:04}", i);
        let edition_id = format!("edition-1000-{:04}", i);
        let version_id = format!("version-1000-{:04}", i);
        let source_id = format!("source-1000-{:04}", i);
        let asset_id = format!("asset-1000-{:04}", i);
        let rel_path = format!(
            "Synth1000 Movie {:04} (2024) [tmdb-{}]/movie.mp4",
            i,
            20000 + i
        );

        let file_meta = tokio::fs::metadata(films.join(&rel_path)).await.unwrap();
        let size = file_meta.len() as i64;
        let modified_ns = match file_meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        {
            Some(d) => d.as_nanos().to_string(),
            None => "0".to_string(),
        };

        sqlx::query("INSERT INTO inventory_works(id, library_id, identity_key, title, year, external_ids_json, editorial_json, title_sort, kind) VALUES(?, ?, ?, ?, 2024, '{}', '{}', ?, 'film')")
            .bind(&work_id)
            .bind(&library)
            .bind(&rel_path)
            .bind(format!("Synth1000 Movie {:04}", i))
            .bind(format!("synth1000 movie {:04}", i))
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inventory_editions VALUES(?, ?)")
            .bind(&edition_id)
            .bind(&work_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inventory_versions VALUES(?, ?)")
            .bind(&version_id)
            .bind(&edition_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO inventory_sources(id, version_id, library_id, root_snapshot) VALUES(?, ?, ?, ?)")
            .bind(&source_id).bind(&version_id).bind(&library).bind(&lib_root).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO inventory_assets VALUES(?, ?, ?, ?, ?, ?, 'video/mp4', ?, 1)")
            .bind(&asset_id)
            .bind(&source_id)
            .bind(&library)
            .bind(&rel_path)
            .bind(size)
            .bind(&modified_ns)
            .bind(probe_json)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    println!("Pre-seeded 1,000 cache entries in SQLite");

    let rss_before = current_rss_bytes();
    let t_scan = Instant::now();
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res = wait_scan(&app, &library, &cookie, 60).await;
    let dur_scan = t_scan.elapsed();
    let rss_after = current_rss_bytes();

    println!(
        "Scan of 1,000 files on disk: status={}, discovered={}, probed={}, reused={} in {:.2} s (RSS: {:.2} MiB -> {:.2} MiB, diff: +{:.2} MiB)",
        res["status"],
        res["discovered"],
        res["probed"],
        res["reused"],
        dur_scan.as_secs_f64(),
        rss_before as f64 / (1024.0 * 1024.0),
        rss_after as f64 / (1024.0 * 1024.0),
        (rss_after.saturating_sub(rss_before)) as f64 / (1024.0 * 1024.0)
    );

    assert_eq!(res["status"], "completed");
    assert_eq!(res["discovered"], file_count);
    assert_eq!(res["probed"], 0, "All 1,000 files must reuse cached probe");
    assert_eq!(res["reused"], file_count);

    // Verify catalog has 1,000 items
    let (_, cat, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?limit=1",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(cat["total"], file_count);
}

#[tokio::test]
async fn test_10000_new_synthetic_media_cold_ingestion_benchmark() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("fresh_10k.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin password 12345".into()))
        .await
        .unwrap();

    let root_path = Path::new("/media/cold_ingest");
    sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib-cold','Cold Ingest Library','films','local','/media/cold_ingest',1)")
        .execute(store.pool())
        .await
        .unwrap();

    let count = 10000;
    let batches_count = 100;
    let scan_id = uuid::Uuid::new_v4().to_string();

    let rss_baseline = current_rss_bytes();
    println!(
        "Cold Ingest 10,000 New Media: Baseline RSS: {:.2} MiB",
        rss_baseline as f64 / (1024.0 * 1024.0)
    );

    // Realistic technical probe (AV1 10-bit Main10 + Opus 6ch + Opus 2ch + 3 Subtitles)
    let probe = media_contracts::media::Probe {
        format: {
            let mut m = std::collections::BTreeMap::new();
            m.insert("format_name".into(), json!("matroska,webm"));
            m.insert("duration".into(), json!("7200.000"));
            m.insert("size".into(), json!("4294967296"));
            m.insert("probe_score".into(), json!(100));
            m
        },
        streams: vec![
            media_contracts::media::Track {
                index: 0,
                codec_name: Some("av1".into()),
                codec_type: Some("video".into()),
                time_base: Some("1/1000".into()),
                tags: std::collections::BTreeMap::new(),
                details: {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("profile".into(), json!("Main 10"));
                    d.insert("width".into(), json!(3840));
                    d.insert("height".into(), json!(2160));
                    d.insert("pix_fmt".into(), json!("yuv420p10le"));
                    d
                },
            },
            media_contracts::media::Track {
                index: 1,
                codec_name: Some("opus".into()),
                codec_type: Some("audio".into()),
                time_base: Some("1/48000".into()),
                tags: {
                    let mut t = std::collections::BTreeMap::new();
                    t.insert("language".into(), json!("fra"));
                    t
                },
                details: {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("channels".into(), json!(6));
                    d.insert("channel_layout".into(), json!("5.1"));
                    d.insert("sample_rate".into(), json!("48000"));
                    d
                },
            },
            media_contracts::media::Track {
                index: 2,
                codec_name: Some("opus".into()),
                codec_type: Some("audio".into()),
                time_base: Some("1/48000".into()),
                tags: {
                    let mut t = std::collections::BTreeMap::new();
                    t.insert("language".into(), json!("eng"));
                    t
                },
                details: {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("channels".into(), json!(2));
                    d.insert("channel_layout".into(), json!("stereo"));
                    d.insert("sample_rate".into(), json!("48000"));
                    d
                },
            },
            media_contracts::media::Track {
                index: 3,
                codec_name: Some("subrip".into()),
                codec_type: Some("subtitle".into()),
                time_base: Some("1/1000".into()),
                tags: {
                    let mut t = std::collections::BTreeMap::new();
                    t.insert("language".into(), json!("fra"));
                    t
                },
                details: std::collections::BTreeMap::new(),
            },
        ],
    };

    // Stage 10,000 brand new media files with probe, NFO editorial, and posters
    let t_stage = Instant::now();
    for chunk in 0..batches_count {
        let files: Vec<ScannedFile> = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                let rel = format!("film_{:06}/movie.mkv", idx);
                let title = format!("Cold Movie #{:06}", idx);
                let mut ext = std::collections::BTreeMap::new();
                ext.insert("tmdb".into(), format!("{}", 100000 + idx));

                let editorial = NfoOutcome::Valid {
                    metadata: media_contracts::media::FilmMetadata {
                        title: Some(title.clone()),
                        year: Some(2024),
                        plot: Some(format!("Synopsis for cold movie #{:06}", idx)),
                        external_ids: ext.clone(),
                        ..Default::default()
                    },
                    path: format!("film_{:06}/movie.nfo", idx),
                    priority: 1,
                };

                let poster = if idx % 2 == 0 {
                    Some(media_contracts::media::LocalImage {
                        path: format!("film_{:06}/poster.png", idx),
                        size_bytes: 65536,
                        modified_ns: "1710000000".into(),
                        mime: "image/png".into(),
                        priority: 1,
                    })
                } else {
                    None
                };

                ScannedFile {
                    audios: Vec::new(),
                    subtitles: Vec::new(),
                    relative_path: rel,
                    identity_key: format!("tmdb:{}", 100000 + idx),
                    title,
                    year: Some(2024),
                    external_ids: ext,
                    size: 4294967296,
                    modified_ns: "1710000000".into(),
                    mime: "video/x-matroska".into(),
                    probe: probe.clone(),
                    editorial,
                    poster,
                    reused: false,
                    ..Default::default()
                }
            })
            .collect();

        let batch = InventoryBatch {
            scan_id: scan_id.clone().into(),
            sequence: chunk as u64,
            files,
            errors: vec![],
            warnings: vec![],
        };
        media_poc::inventory::stage_batch(&store, &scan_id, "lib-cold", &batch)
            .await
            .unwrap();
    }
    let dur_stage = t_stage.elapsed();
    let rss_after_stage = current_rss_bytes();
    println!(
        "Cold Ingestion 10,000 files staged in {:.2} ms (avg {:.2} ms/batch) (RSS: {:.2} MiB, diff: +{:.2} MiB)",
        dur_stage.as_millis(),
        dur_stage.as_millis() as f64 / batches_count as f64,
        rss_after_stage as f64 / (1024.0 * 1024.0),
        (rss_after_stage.saturating_sub(rss_baseline)) as f64 / (1024.0 * 1024.0)
    );

    // Verify 10,000 staged in DB
    let staged_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(staged_count, count as i64);

    // Publish all 10,000 brand new media atomically
    let t_publish = Instant::now();
    media_poc::inventory::publish_staged_scan(&store, &scan_id, "lib-cold", root_path, true)
        .await
        .unwrap();
    let dur_publish = t_publish.elapsed();
    let rss_after_publish = current_rss_bytes();
    println!(
        "Cold Ingestion 10,000 NEW media PUBLISHED in {:.2} ms (RSS: {:.2} MiB, diff from base: +{:.2} MiB)",
        dur_publish.as_millis(),
        rss_after_publish as f64 / (1024.0 * 1024.0),
        (rss_after_publish.saturating_sub(rss_baseline)) as f64 / (1024.0 * 1024.0)
    );

    // Verify integrity of database records:
    let works_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM inventory_works WHERE library_id='lib-cold'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(works_count, count as i64);

    let assets_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventory_assets WHERE library_id='lib-cold' AND available=1",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(assets_count, count as i64);

    let tracks_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventory_tracks t JOIN inventory_assets a ON a.id=t.asset_id WHERE a.library_id='lib-cold'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        tracks_count,
        count as i64 * 4,
        "Each file must have 4 tracks persisted"
    );

    let images_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventory_images WHERE library_id='lib-cold' AND available=1",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        images_count,
        count as i64 / 2,
        "5,000 posters must be indexed and available"
    );

    // Staging table must be completely empty after atomic publish
    let staging_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(staging_remaining, 0);

    // Verify catalogue query on 10,000 items
    let cat_res = media_poc::inventory::catalogue(
        &store,
        &["lib-cold".into()],
        Some("film"),
        "",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    assert_eq!(cat_res["total"], count as i64);
    assert_eq!(cat_res["items"].as_array().unwrap().len(), 24);

    // Verify search
    let cat_search = media_poc::inventory::catalogue(
        &store,
        &["lib-cold".into()],
        Some("film"),
        "cold movie #000042",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    assert_eq!(cat_search["total"], 1);
    assert_eq!(cat_search["items"][0]["title"], "Cold Movie #000042");
    assert_eq!(cat_search["items"][0]["year"], 2024);
}

#[tokio::test]
async fn test_mixed_diff_difficult_cases_and_atomic_publication() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("mixed_diff.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin password 12345".into()))
        .await
        .unwrap();

    let root_path = Path::new("/media/mixed");
    sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib-mixed','Mixed Library','films','local','/media/mixed',1)")
        .execute(store.pool())
        .await
        .unwrap();

    let make_meta = |title: &str, year: u16| media_contracts::media::FilmMetadata {
        title: Some(title.to_string()),
        year: Some(year),
        ..Default::default()
    };

    // 1. Initial seed of 100 movies
    let initial_count = 100;
    let scan_id1 = uuid::Uuid::new_v4().to_string();
    let files1: Vec<ScannedFile> = (0..initial_count)
        .map(|i| ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Original Movie {:03}", i),
            year: Some(2020),
            external_ids: std::collections::BTreeMap::new(),
            size: 1000000 + i as u64,
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Original Movie {:03}", i), 2020),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: false,
            ..Default::default()
        })
        .collect();

    let batch1 = InventoryBatch {
        scan_id: scan_id1.clone().into(),
        sequence: 1,
        files: files1,
        errors: vec![],
        warnings: vec![],
    };
    media_poc::inventory::stage_batch(&store, &scan_id1, "lib-mixed", &batch1)
        .await
        .unwrap();
    media_poc::inventory::publish_staged_scan(&store, &scan_id1, "lib-mixed", root_path, true)
        .await
        .unwrap();

    // Record initial asset and work IDs to verify deterministic persistence
    let asset_0_before: String = sqlx::query_scalar(
        "SELECT id FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_000/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();

    let work_0_before: String = sqlx::query_scalar(
        "SELECT id FROM inventory_works WHERE library_id='lib-mixed' AND identity_key='/media/mixed\ntmdb:20000'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();

    // 2. Second scan with difficult mix:
    // - Movies 0..20: unchanged (reused = true)
    // - Movies 20..40: modified size/mtime (reused = false)
    // - Movies 40..60: modified NFO (reused = true, but editorial title updated)
    // - Movies 60..70: poster deleted (reused = true, poster = None)
    // - Movies 70..80: disappeared (omitted from staging)
    // - Movies 80..90: errored (staged as error in scan_staging_issues)
    // - Movies 90..100: unchanged (reused = true)
    // - Movies 100..120: 20 brand new movies (reused = false)

    let scan_id2 = uuid::Uuid::new_v4().to_string();
    let mut files2 = Vec::new();

    // 0..20 unchanged
    for i in 0..20 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Original Movie {:03}", i),
            year: Some(2020),
            external_ids: std::collections::BTreeMap::new(),
            size: 1000000 + i as u64,
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Original Movie {:03}", i), 2020),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: true,
            ..Default::default()
        });
    }

    // 20..40 modified
    for i in 20..40 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Original Movie {:03}", i),
            year: Some(2020),
            external_ids: std::collections::BTreeMap::new(),
            size: 2000000 + i as u64,         // changed
            modified_ns: "1720000000".into(), // changed
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Original Movie {:03}", i), 2020),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: false, // re-probed
            ..Default::default()
        });
    }

    // 40..60 modified NFO
    for i in 40..60 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Updated Title {:03}", i),
            year: Some(2025),
            external_ids: std::collections::BTreeMap::new(),
            size: 1000000 + i as u64,
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Updated Title {:03}", i), 2025),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: true,
            ..Default::default()
        });
    }

    // 60..70 deleted poster
    for i in 60..70 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Original Movie {:03}", i),
            year: Some(2020),
            external_ids: std::collections::BTreeMap::new(),
            size: 1000000 + i as u64,
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Original Movie {:03}", i), 2020),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: None, // poster deleted
            reused: true,
            ..Default::default()
        });
    }

    // 70..80 omitted (disappeared)

    // 80..90 errored
    let mut errors2 = Vec::new();
    for i in 80..90 {
        errors2.push(media_contracts::inventory::ScanIssue {
            relative_path: format!("movie_{:03}/movie.mkv", i),
            message: "file corrupt or unreadable".into(),
        });
    }

    // 90..100 unchanged
    for i in 90..100 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Original Movie {:03}", i),
            year: Some(2020),
            external_ids: std::collections::BTreeMap::new(),
            size: 1000000 + i as u64,
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Original Movie {:03}", i), 2020),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: true,
            ..Default::default()
        });
    }

    // 100..120 new
    for i in 100..120 {
        files2.push(ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: format!("movie_{:03}/movie.mkv", i),
            identity_key: format!("tmdb:{}", 20000 + i),
            title: format!("Brand New Movie {:03}", i),
            year: Some(2024),
            external_ids: std::collections::BTreeMap::new(),
            size: 3000000 + i as u64,
            modified_ns: "1730000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: make_meta(&format!("Brand New Movie {:03}", i), 2024),
                path: format!("movie_{:03}/movie.nfo", i),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{:03}/poster.png", i),
                size_bytes: 50000,
                modified_ns: "1730000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: false,
            ..Default::default()
        });
    }

    let batch2 = InventoryBatch {
        scan_id: scan_id2.clone().into(),
        sequence: 1,
        files: files2,
        errors: errors2,
        warnings: vec![],
    };
    media_poc::inventory::stage_batch(&store, &scan_id2, "lib-mixed", &batch2)
        .await
        .unwrap();

    media_poc::inventory::publish_staged_scan(&store, &scan_id2, "lib-mixed", root_path, true)
        .await
        .unwrap();

    // Assertions:
    // Deterministic UUIDs: movie 0 must retain same asset ID and work ID
    let asset_0_after: String = sqlx::query_scalar(
        "SELECT id FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_000/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        asset_0_before, asset_0_after,
        "Asset UUID must remain deterministic and stable"
    );

    let work_0_after: String = sqlx::query_scalar(
        "SELECT id FROM inventory_works WHERE library_id='lib-mixed' AND identity_key='/media/mixed\ntmdb:20000'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        work_0_before, work_0_after,
        "Work UUID must remain deterministic and stable"
    );

    // Modified file (movie_020) must have updated size and modified_ns
    let (mod_size, mod_time): (i64, String) = sqlx::query_as(
        "SELECT size, modified_ns FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_020/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(mod_size, 2000020);
    assert_eq!(mod_time, "1720000000");

    // Modified NFO (movie_045) must have updated title and year in inventory_works
    let (upd_title, upd_year): (String, Option<i64>) = sqlx::query_as(
        "SELECT title, year FROM inventory_works WHERE library_id='lib-mixed' AND identity_key='/media/mixed\ntmdb:20045'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(upd_title, "Updated Title 045");
    assert_eq!(upd_year, Some(2025));

    // Deleted poster (movie_065) must have available=0 in inventory_images
    let poster_avail: i64 = sqlx::query_scalar(
        "SELECT available FROM inventory_images WHERE library_id='lib-mixed' AND relative_path='movie_065/poster.png'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(poster_avail, 0, "Deleted poster must be marked available=0");

    // Disappeared movie (movie_075) must have available=0
    let dis_avail: i64 = sqlx::query_scalar(
        "SELECT available FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_075/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(dis_avail, 0, "Disappeared movie must be marked available=0");

    // Errored movie (movie_085) must have available=0
    let err_avail: i64 = sqlx::query_scalar(
        "SELECT available FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_085/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(err_avail, 0, "Errored movie must be marked available=0");

    // New movie (movie_110) must have available=1
    let new_avail: i64 = sqlx::query_scalar(
        "SELECT available FROM inventory_assets WHERE library_id='lib-mixed' AND relative_path='movie_110/movie.mkv'"
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(new_avail, 1, "New movie must be indexed and available=1");

    // Total available assets:
    // Initial 100 - 10 disappeared (70..80) - 10 errored (80..90) + 20 new (100..120) = 100
    let total_active_assets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inventory_assets WHERE library_id='lib-mixed' AND available=1",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(total_active_assets, 100);

    // Catalogue must also return total = 100
    let cat_res = media_poc::inventory::catalogue(
        &store,
        &["lib-mixed".into()],
        Some("film"),
        "",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    assert_eq!(cat_res["total"], 100);
}

#[tokio::test]
async fn test_sqlite_pool_audit_and_concurrent_reads_during_publication() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");
    assert!(sample_mp4.exists());

    // Populate baseline files on disk for live streaming
    for i in 0..10 {
        let folder = films.join(format!("Audit Movie {:02} (2024)", i));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::copy(&sample_mp4, folder.join("movie.mp4"))
            .await
            .unwrap();
        tokio::fs::write(
            folder.join("movie.nfo"),
            format!(
                "<movie><title>Audit Movie {:02}</title><year>2024</year></movie>",
                i
            ),
        )
        .await
        .unwrap();
    }

    let db = temp.path().join("pool_audit.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    // Audit pool configuration: assert pool max_connections == 4
    let pool_options = store.pool().options();
    assert_eq!(
        pool_options.get_max_connections(),
        4,
        "IdentityStore pool must be configured with 4 connections for concurrent readers in WAL mode"
    );

    let (app, cookie, csrf) = setup_app(&root, &store).await;
    let library = create_library(&app, &films, &cookie, &csrf, "AuditFilms").await;

    // Initial scan to establish catalogue & live asset IDs for streaming
    trigger_scan(&app, &library, &cookie, &csrf).await;
    let res = wait_scan(&app, &library, &cookie, 60).await;
    assert_eq!(res["status"], "completed");

    let (_, cat, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?limit=5",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let work_id = cat["items"][0]["id"].as_str().unwrap();
    let (_, detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{work_id}"),
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let asset_id = detail["editions"][0]["versions"][0]["sources"][0]["assetIds"][0]
        .as_str()
        .unwrap()
        .to_owned();

    // Now stage 1,000 synthetic entries to publish
    let scan_id = uuid::Uuid::new_v4().to_string();
    let stage_count = 1000;
    for chunk in 0..10 {
        let files: Vec<ScannedFile> = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                ScannedFile {
                    audios: Vec::new(),
                    subtitles: Vec::new(),
                    relative_path: format!("audit_synth_{:04}/movie.mkv", idx),
                    identity_key: format!("synth:audit:{:04}", idx),
                    title: format!("Audit Synth #{:04}", idx),
                    year: Some(2024),
                    external_ids: std::collections::BTreeMap::new(),
                    size: 1048576,
                    modified_ns: "1700000000".into(),
                    mime: "video/x-matroska".into(),
                    probe: media_poc::media::Probe::default(),
                    editorial: NfoOutcome::Missing,
                    poster: None,
                    reused: false,
                    ..Default::default()
                }
            })
            .collect();
        let batch = InventoryBatch {
            scan_id: scan_id.clone().into(),
            sequence: chunk as u64,
            files,
            errors: vec![],
            warnings: vec![],
        };
        media_poc::inventory::stage_batch(&store, &scan_id, &library, &batch)
            .await
            .unwrap();
    }

    // Spawn publish_staged_scan in background to hold active writer transaction
    let store_pub = store.clone();
    let scan_id_pub = scan_id.clone();
    let library_pub = library.clone();
    let films_pub = films.clone();
    let publish_task = tokio::spawn(async move {
        let t0 = Instant::now();
        media_poc::inventory::publish_staged_scan(
            &store_pub,
            &scan_id_pub,
            &library_pub,
            &films_pub,
            false,
        )
        .await
        .map(|_| t0.elapsed())
    });

    // Concurrently send 50 queries (25 catalog + 25 range streaming) WHILE publish_staged_scan is running
    let app_clone = app.clone();
    let cookie_clone = cookie.clone();
    let asset_id_clone = asset_id.clone();

    let concurrent_task = tokio::spawn(async move {
        let mut latencies: Vec<Duration> = Vec::with_capacity(50);
        let mut sqlite_busy_errors = 0;
        let mut success_count = 0;

        for step in 0..50 {
            let t0 = Instant::now();
            let (status, _body, _bytes, _) = if step % 2 == 0 {
                request(
                    &app_clone,
                    "GET",
                    "/api/v1/catalog?limit=10",
                    &cookie_clone,
                    None,
                    Value::Null,
                    None,
                )
                .await
            } else {
                request(
                    &app_clone,
                    "GET",
                    &format!("/api/v1/assets/{asset_id_clone}/content"),
                    &cookie_clone,
                    None,
                    Value::Null,
                    Some("bytes=0-63"),
                )
                .await
            };
            let lat = t0.elapsed();
            latencies.push(lat);

            if status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT {
                success_count += 1;
            } else {
                eprintln!("STEP {} STATUS: {}", step, status);
                if status == StatusCode::INTERNAL_SERVER_ERROR {
                    sqlite_busy_errors += 1;
                }
            }
        }
        (latencies, sqlite_busy_errors, success_count)
    });

    let (pub_join, concurrent_join) = tokio::join!(publish_task, concurrent_task);
    let pub_dur = pub_join.unwrap().unwrap();
    let (mut latencies, sqlite_busy, success_count) = concurrent_join.unwrap();
    println!(
        "publish_staged_scan duration: {:.2} ms",
        pub_dur.as_millis()
    );

    assert_eq!(
        sqlite_busy, 0,
        "Zero SQLITE_BUSY errors must occur during atomic publication with 4-connection pool"
    );
    assert_eq!(
        success_count, 50,
        "All 50 concurrent requests during publication must succeed"
    );

    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
    let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

    println!(
        "Concurrent Latencies DURING write transaction: p50={:.2} ms, p95={:.2} ms, p99={:.2} ms (50 requests, 0 SQLITE_BUSY)",
        p50.as_secs_f64() * 1000.0,
        p95.as_secs_f64() * 1000.0,
        p99.as_secs_f64() * 1000.0
    );

    // Verify catalog total now includes both baseline and newly published
    let (_, final_cat, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?limit=1",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(final_cat["total"], 10 + stage_count);
}

#[tokio::test]
async fn test_crash_cancellation_permissions_and_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let db = temp.path().join("resilience.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
        )
        .await
        .unwrap();

    let (app, cookie, csrf) = setup_app(&root, &store).await;
    let library = create_library(&app, &films, &cookie, &csrf, "ResilienceLib").await;

    // 1. Rollback on library configuration change during publication
    let scan_id = uuid::Uuid::new_v4().to_string();
    let batch = InventoryBatch {
        scan_id: scan_id.clone().into(),
        sequence: 1,
        files: vec![ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: "uncommitted.mkv".into(),
            identity_key: "uncommitted".into(),
            title: "Uncommitted Movie".into(),
            year: Some(2024),
            external_ids: std::collections::BTreeMap::new(),
            size: 1048576,
            modified_ns: "1700".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Missing,
            poster: None,
            reused: false,
            ..Default::default()
        }],
        errors: vec![],
        warnings: vec![],
    };
    media_poc::inventory::stage_batch(&store, &scan_id, &library, &batch)
        .await
        .unwrap();

    // Mutate library root path in DB to simulate mismatch / concurrent config change
    sqlx::query("UPDATE libraries SET root='/changed/path' WHERE id=?")
        .bind(&library)
        .execute(store.pool())
        .await
        .unwrap();

    // publish_staged_scan MUST fail and rollback
    let pub_res =
        media_poc::inventory::publish_staged_scan(&store, &scan_id, &library, &films, true).await;
    assert!(
        pub_res.is_err(),
        "Must reject publication when root does not match"
    );

    // Clean staging
    media_poc::inventory::clean_staging(&store, &scan_id)
        .await
        .unwrap();
    let remaining_staging: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(remaining_staging, 0);

    // Verify nothing leaked into inventory_assets or works
    let asset_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM inventory_assets WHERE library_id=?")
            .bind(&library)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(asset_count, 0, "Failed publication must leave 0 assets");

    // 2. Permission enforcement: non-admin gets 403 Forbidden on scan
    sqlx::query("UPDATE users SET role='user' WHERE role='administrator'")
        .execute(store.pool())
        .await
        .unwrap();

    let (scan_status, _, _, _) = request(
        &app,
        "POST",
        &format!("/api/v1/admin/libraries/{library}/scan"),
        &cookie,
        Some(&csrf),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(
        scan_status,
        StatusCode::FORBIDDEN,
        "Non-admin role must get 403 on admin scan"
    );

    let (cat_status, _, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(
        cat_status,
        StatusCode::FORBIDDEN,
        "User without library grant must get 403 Forbidden on catalog"
    );

    // Restore administrator role
    sqlx::query("UPDATE users SET role='administrator' WHERE role='user'")
        .execute(store.pool())
        .await
        .unwrap();

    // 3. Incomplete scan (complete = false): missing files are NOT marked unavailable
    // Restore library root
    sqlx::query("UPDATE libraries SET root=? WHERE id=?")
        .bind(films.to_string_lossy().as_ref())
        .bind(&library)
        .execute(store.pool())
        .await
        .unwrap();

    // Stage an asset
    let scan_id_ok = uuid::Uuid::new_v4().to_string();
    let batch_ok = InventoryBatch {
        scan_id: scan_id_ok.clone().into(),
        sequence: 1,
        files: vec![ScannedFile {
            audios: Vec::new(),
            subtitles: Vec::new(),
            relative_path: "file1.mkv".into(),
            identity_key: "file1".into(),
            title: "File 1".into(),
            year: Some(2024),
            external_ids: std::collections::BTreeMap::new(),
            size: 1048576,
            modified_ns: "1700".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Missing,
            poster: None,
            reused: false,
            ..Default::default()
        }],
        errors: vec![],
        warnings: vec![],
    };
    media_poc::inventory::stage_batch(&store, &scan_id_ok, &library, &batch_ok)
        .await
        .unwrap();
    media_poc::inventory::publish_staged_scan(&store, &scan_id_ok, &library, &films, true)
        .await
        .unwrap();

    // Now run an incomplete scan with an empty batch (complete = false)
    let scan_id_incomplete = uuid::Uuid::new_v4().to_string();
    let batch_empty = InventoryBatch {
        scan_id: scan_id_incomplete.clone().into(),
        sequence: 1,
        files: vec![],
        errors: vec![],
        warnings: vec![],
    };
    media_poc::inventory::stage_batch(&store, &scan_id_incomplete, &library, &batch_empty)
        .await
        .unwrap();
    media_poc::inventory::publish_staged_scan(&store, &scan_id_incomplete, &library, &films, false)
        .await
        .unwrap();

    // Verify file1 is STILL available!
    let file1_avail: i64 = sqlx::query_scalar(
        "SELECT available FROM inventory_assets WHERE library_id=? AND relative_path='file1.mkv'",
    )
    .bind(&library)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        file1_avail, 1,
        "Incomplete scan must NOT mark missing files unavailable"
    );
}
