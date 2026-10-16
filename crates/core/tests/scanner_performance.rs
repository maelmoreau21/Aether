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
            if parts.len() >= 2 {
                if let Ok(pages) = parts[1].parse::<usize>() {
                    return pages * 4096;
                }
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
    sqlx::query("INSERT INTO libraries VALUES('lib-main','Films Populaires','films','local','/media/films',1)")
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
    sqlx::query("INSERT INTO libraries VALUES('orphan-lib', 'Orphan Library', 'films', 'local', '/media/orphan', 1)")
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
    let png = sample_png();

    let t_create = Instant::now();
    for i in 0..file_count {
        let folder = films.join(format!(
            "Synth1000 Movie {:04} (2024) [tmdb-{}]",
            i,
            20000 + i
        ));
        tokio::fs::create_dir_all(&folder).await.unwrap();
        tokio::fs::copy(&sample_mp4, folder.join("movie.mp4"))
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

    // Read metadata of first file to know size
    let meta =
        tokio::fs::metadata(films.join("Synth1000 Movie 0000 (2024) [tmdb-20000]/movie.mp4"))
            .await
            .unwrap();
    let size = meta.len() as i64;
    let modified_ns = match meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
    {
        Some(d) => d.as_nanos().to_string(),
        None => "0".to_string(),
    };

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

        sqlx::query("INSERT INTO inventory_works VALUES(?, ?, ?, ?, 2024, '{}', '{}', ?)")
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
