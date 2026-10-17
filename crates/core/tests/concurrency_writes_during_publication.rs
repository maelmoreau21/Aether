use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_contracts::{
    inventory::{InventoryBatch, ScannedFile},
    media::NfoOutcome,
};
use media_poc::{auth::AuthState, identity::IdentityStore, inventory, scans::ScanJobs};
use sqlx::Connection;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceExt;
use zeroize::Zeroizing;

#[tokio::test]
async fn test_concurrent_writes_during_sqlite_publication_and_timeout_behavior() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let db_path = temp.path().join("data").join("core.sqlite3");
    tokio::fs::create_dir_all(db_path.parent().unwrap())
        .await
        .unwrap();

    // Use open with 15s busy timeout
    let store = IdentityStore::open(&db_path).await.unwrap();
    let _admin_id = store
        .bootstrap(
            "admin-concurrency",
            Zeroizing::new("pass-concurrency-write-12345".into()),
        )
        .await
        .unwrap();

    let library = "lib-concurrent-writes";
    sqlx::query(
        "INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES(?, 'Concurrent Lib', 'films', 'local', ?, 1)"
    )
    .bind(library)
    .bind(films.to_str().unwrap())
    .execute(store.pool())
    .await
    .unwrap();

    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();

    // Log in admin to acquire cookie and csrf token
    let login_body = serde_json::json!({
        "login": "admin-concurrency",
        "password": "pass-concurrency-write-12345"
    });

    let auth_router = media_poc::auth::router(auth.clone());
    let login_res = auth_router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&login_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(login_res.status(), StatusCode::OK);
    let cookie = login_res.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let body_bytes = login_res.into_body().collect().await.unwrap().to_bytes();
    let login_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let csrf_token = login_json["csrfToken"].as_str().unwrap().to_owned();

    let jobs = Arc::new(ScanJobs::default());
    let app = media_poc::secure_app_with_jobs(
        media_poc::node::Inventory::local(root.clone())
            .await
            .unwrap(),
        3000,
        auth.clone(),
        jobs.clone(),
    )
    .await
    .unwrap();

    // Stage 1,000 synthetic entries to publish
    let scan_id = uuid::Uuid::new_v4().to_string();
    for chunk in 0..10 {
        let files: Vec<ScannedFile> = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                ScannedFile {
                    relative_path: format!("movie_{:04}/movie.mkv", idx),
                    identity_key: format!("synth:write:{:04}", idx),
                    title: format!("Synth Write #{:04}", idx),
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
        inventory::stage_batch(&store, &scan_id, library, &batch)
            .await
            .unwrap();
    }

    // 1. Concurrency under publication:
    // Launch publish_staged_scan holding write transaction
    let store_pub = store.clone();
    let scan_id_pub = scan_id.clone();
    let library_pub = library.to_owned();
    let films_pub = films.clone();

    let publish_task = tokio::spawn(async move {
        let t0 = Instant::now();
        let res = inventory::publish_staged_scan(
            &store_pub,
            &scan_id_pub,
            &library_pub,
            &films_pub,
            false,
        )
        .await;
        (res, t0.elapsed())
    });

    // 2. Concurrently execute writes while publication is running:
    let app_clone1 = app.clone();
    let cookie_clone1 = cookie.clone();
    let csrf_clone1 = csrf_token.clone();
    let films_clone1 = films.clone();

    let write_tasks = tokio::spawn(async move {
        let mut results = Vec::new();

        // Write A: Authenticated request updating session last_seen_at
        let t_a = Instant::now();
        let auth_res = app_clone1
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/catalog?limit=5")
                    .header(header::HOST, "127.0.0.1:3000")
                    .header(header::COOKIE, &cookie_clone1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        results.push(("session_update_read", auth_res.status(), t_a.elapsed()));

        // Write B: Library administration create (INSERT into libraries & permissions)
        let t_b = Instant::now();
        let lib_input = serde_json::json!({
            "name": "Concurrent Dynamic Lib",
            "kind": "films",
            "nodeId": "local",
            "root": films_clone1.to_str().unwrap(),
            "enabled": true,
            "userIds": []
        });

        let lib_res = app_clone1
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/libraries")
                    .header(header::HOST, "127.0.0.1:3000")
                    .header(header::ORIGIN, "http://127.0.0.1:3000")
                    .header(header::COOKIE, &cookie_clone1)
                    .header("x-csrf-token", &csrf_clone1)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_string(&lib_input).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        results.push(("library_create", lib_res.status(), t_b.elapsed()));

        // Write C: Scan job creation (INSERT/UPDATE scan_jobs)
        let t_c = Instant::now();
        let scan_job_res = app_clone1
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/libraries/lib-concurrent-writes/scan")
                    .header(header::HOST, "127.0.0.1:3000")
                    .header(header::ORIGIN, "http://127.0.0.1:3000")
                    .header(header::COOKIE, &cookie_clone1)
                    .header("x-csrf-token", &csrf_clone1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        results.push(("scan_job_start", scan_job_res.status(), t_c.elapsed()));

        results
    });

    let (pub_res, pub_duration) = publish_task.await.unwrap();
    assert!(pub_res.is_ok(), "Publication must succeed");
    println!(
        "Publication of 1,000 items completed in {:.2} ms",
        pub_duration.as_millis()
    );

    let write_results = write_tasks.await.unwrap();
    for (name, status, lat) in &write_results {
        println!(
            "Concurrent write '{name}': status={status}, latency={:.2} ms",
            lat.as_millis()
        );
        // All writes must succeed (or scan returns ACCEPTED 202)
        assert!(
            *status == StatusCode::OK
                || *status == StatusCode::CREATED
                || *status == StatusCode::ACCEPTED,
            "Write '{name}' should succeed, got {status}"
        );
    }

    // 3. Test timeout behavior when write lock wait exceeds busy_timeout:
    // Open a second store with a very short 100ms busy timeout
    let short_store = IdentityStore::open_with_timeout(&db_path, Duration::from_millis(100))
        .await
        .unwrap();

    // Acquire exclusive write lock and hold it for 400ms
    let lock_holder = tokio::spawn({
        let store_holder = store.clone();
        async move {
            let mut conn = store_holder.pool().acquire().await.unwrap();
            let tx = conn.begin_with("BEGIN IMMEDIATE").await.unwrap();
            tokio::time::sleep(Duration::from_millis(400)).await;
            tx.rollback().await.unwrap();
        }
    });

    // Give lock_holder 50ms to acquire the write lock
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Try to acquire write lock with the short_store (100ms timeout < 400ms hold)
    let timed_out_write = async {
        let mut conn = short_store.pool().acquire().await?;
        let tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        tx.rollback().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;

    assert!(
        timed_out_write.is_err(),
        "Write waiting longer than busy_timeout must return error"
    );

    let err_str = timed_out_write.unwrap_err().to_string();
    assert!(
        err_str.to_lowercase().contains("busy") || err_str.to_lowercase().contains("locked"),
        "Error must be SQLite busy/locked: {err_str}"
    );
    println!("Expected busy timeout verified: {err_str}");

    lock_holder.await.unwrap();

    // 4. Verify absence of permanent deadlock (absence de blocage permanent):
    // After lock is released, writes on short_store immediately succeed
    let immediate_write = async {
        let mut conn = short_store.pool().acquire().await?;
        let tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        tx.rollback().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;

    assert!(
        immediate_write.is_ok(),
        "Database must be completely unblocked after lock holder finishes"
    );
    println!("Absence of permanent deadlock verified: immediate write succeeded.");
}
