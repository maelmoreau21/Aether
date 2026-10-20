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
use sqlx::{ConnectOptions, sqlite::SqliteConnectOptions};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tower::ServiceExt;
use zeroize::Zeroizing;

const ORIGIN: &str = "http://127.0.0.1:3000";
const ADMIN_PASS: &str = "admin-lock-test-password-123";

async fn send_request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: Value,
    range: Option<&str>,
) -> (StatusCode, Value, Vec<u8>, String, Duration) {
    let start = Instant::now();
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:3000")
        .header(header::ORIGIN, ORIGIN)
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
    let set_cookie = response
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
    let elapsed = start.elapsed();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, bytes, set_cookie, elapsed)
}

fn percentile_us(sorted_us: &[u128], p: f64) -> f64 {
    if sorted_us.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_us.len() as f64 - 1.0) * p).round() as usize;
    sorted_us[idx.min(sorted_us.len() - 1)] as f64 / 1000.0
}

#[tokio::test]
async fn auth_and_catalog_reads_succeed_under_30s_sqlite_begin_immediate_lock() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films_root = root.join("Films");
    let series_root = root.join("Series");
    tokio::fs::create_dir_all(&films_root).await.unwrap();
    tokio::fs::create_dir_all(&series_root).await.unwrap();

    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let sample_mkv = generated.join("representative/representative.mkv");
    let sample_mp4 = generated.join("p1/clip-mp4-en.mp4");

    // 1. Populate film and series fixtures
    let film_dir = films_root.join("Lock Resilience Film (2026)");
    tokio::fs::create_dir_all(&film_dir).await.unwrap();
    tokio::fs::copy(
        &sample_mkv,
        film_dir.join("Lock Resilience Film (2026).mkv"),
    )
    .await
    .unwrap();

    let show_dir = series_root.join("Lock Resilience Show (2026)");
    let season_dir = show_dir.join("Season 01");
    tokio::fs::create_dir_all(&season_dir).await.unwrap();
    tokio::fs::copy(
        &sample_mp4,
        season_dir.join("Lock Resilience Show - S01E01.mp4"),
    )
    .await
    .unwrap();

    // 2. Initialize SQLite with default 15s busy_timeout
    let db_path = temp.path().join("data").join("core.sqlite3");
    tokio::fs::create_dir_all(db_path.parent().unwrap())
        .await
        .unwrap();
    let store = IdentityStore::open(&db_path).await.unwrap();
    let _admin_id = store
        .bootstrap("admin", Zeroizing::new(ADMIN_PASS.into()))
        .await
        .unwrap();

    // Create a second regular user to test user revocation & permission checks
    let reader_id = "user-reader-lock-test";
    sqlx::query("INSERT INTO users(id, role) VALUES(?, 'user')")
        .bind(reader_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO local_credentials(user_id, login, password_hash) VALUES(?, 'reader', 'unused')",
    )
    .bind(reader_id)
    .execute(store.pool())
    .await
    .unwrap();
    let reader_raw = "b".repeat(64);
    let reader_hash = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(reader_raw.as_bytes()))
    };
    sqlx::query(
        "INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at) \
         VALUES(?, ?, unixepoch() - 300, unixepoch() - 120, unixepoch() + 3600)",
    )
    .bind(&reader_hash)
    .bind(reader_id)
    .execute(store.pool())
    .await
    .unwrap();
    let reader_cookie = format!("media_session={reader_raw}");

    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let jobs = Arc::new(ScanJobs::default());
    let app = secure_app_with_jobs(
        Inventory::local(root.clone()).await.unwrap(),
        3000,
        auth.clone(),
        jobs.clone(),
    )
    .await
    .unwrap();

    // 3. Sign in admin via HTTP login
    let (status, login_json, _, admin_cookie, _) = send_request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "admin", "password": ADMIN_PASS}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let admin_csrf = login_json["csrfToken"].as_str().unwrap().to_owned();

    // 4. Create and scan both Films and Series libraries, granting access to reader_id
    let mut lib_ids = Vec::new();
    for (name, kind, path) in [
        ("Films Lock Lib", "films", &films_root),
        ("Series Lock Lib", "series", &series_root),
    ] {
        let (st, body, _, _, _) = send_request(
            &app,
            "POST",
            "/api/v1/admin/libraries",
            &admin_cookie,
            Some(&admin_csrf),
            json!({
                "name": name,
                "kind": kind,
                "nodeId": "local",
                "root": path.to_string_lossy(),
                "enabled": true,
                "userIds": [reader_id]
            }),
            None,
        )
        .await;
        assert_eq!(st, StatusCode::CREATED);
        let lib_id = body["id"].as_str().unwrap().to_owned();

        let scan_uri = format!("/api/v1/admin/libraries/{lib_id}/scan");
        let (st, _, _, _, _) = send_request(
            &app,
            "POST",
            &scan_uri,
            &admin_cookie,
            Some(&admin_csrf),
            Value::Null,
            None,
        )
        .await;
        assert_eq!(st, StatusCode::ACCEPTED);
        loop {
            let (st, poll, _, _, _) = send_request(
                &app,
                "GET",
                &scan_uri,
                &admin_cookie,
                None,
                Value::Null,
                None,
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            if poll["status"] == "completed" {
                break;
            }
            assert_ne!(poll["status"], "error", "Scan failed: {poll}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        lib_ids.push(lib_id);
    }

    // Resolve Series ID, Episode playbackMediaId, and Episode Asset ID before locking
    let (st, series_cat, _, _, _) = send_request(
        &app,
        "GET",
        "/api/v1/catalog?kind=series",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let series_id = series_cat["items"][0]["id"].as_str().unwrap().to_owned();

    let (st, series_detail, _, _, _) = send_request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{series_id}"),
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let episode_media_id = series_detail["seasons"][0]["episodes"][0]["editions"][0]["versions"][0]
        ["sources"][0]["playbackMediaId"]
        .as_str()
        .unwrap()
        .to_owned();
    let episode_asset_id = series_detail["seasons"][0]["episodes"][0]["editions"][0]["versions"][0]
        ["sources"][0]["assetIds"][0]
        .as_str()
        .unwrap()
        .to_owned();

    // Backdate admin session last_seen_at by 120s (> 60s touch interval) so every request
    // during the 30s lock actively triggers an eligible session touch!
    sqlx::query("UPDATE sessions SET last_seen_at = unixepoch() - 120")
        .execute(store.pool())
        .await
        .unwrap();

    let stats_before_lock = auth.touch_stats();
    assert_eq!(stats_before_lock.write_batches_succeeded, 0);

    // 5. Acquire an exclusive SQLite BEGIN IMMEDIATE lock on an independent connection
    // and hold it for a full 30 seconds.
    let mut external_conn = SqliteConnectOptions::new()
        .filename(&db_path)
        .connect()
        .await
        .unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut external_conn)
        .await
        .unwrap();
    let lock_started = Instant::now();
    let lock_duration = Duration::from_secs(30);

    let lock_holder = tokio::spawn(async move {
        tokio::time::sleep(lock_duration).await;
        sqlx::query("ROLLBACK")
            .execute(&mut external_conn)
            .await
            .unwrap();
        lock_started.elapsed()
    });

    // Give the lock holder 100ms to ensure BEGIN IMMEDIATE is active and background retry is exercised
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 6. During the 30-second lock, execute authenticated requests across all mandatory endpoints
    let mut me_latencies_us = Vec::new();
    let mut films_latencies_us = Vec::new();
    let mut series_latencies_us = Vec::new();
    let mut descriptor_latencies_us = Vec::new();
    let mut range_latencies_us = Vec::new();
    let mut all_latencies_us = Vec::new();

    // Run 20 iterations across all 6 read operations (120 authenticated read requests total)
    // spread across the 30s lock window, using both admin and reader sessions.
    for round in 0..20usize {
        let active_cookie = if round % 2 == 0 {
            &admin_cookie
        } else {
            &reader_cookie
        };

        // (a) /api/v1/auth/me
        let (st, me_json, _, _, lat) = send_request(
            &app,
            "GET",
            "/api/v1/auth/me",
            active_cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "Round {round}: /auth/me failed under lock"
        );
        assert!(me_json["user"]["id"].is_string());
        me_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());

        // (b) Catalogue Films
        let (st, films_json, _, _, lat) = send_request(
            &app,
            "GET",
            "/api/v1/catalog?kind=film",
            active_cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "Round {round}: /catalog?kind=film failed"
        );
        assert_eq!(films_json["total"], 1);
        films_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());

        // (c) Catalogue Séries + Fiche Série
        let (st, series_json, _, _, lat) = send_request(
            &app,
            "GET",
            "/api/v1/catalog?kind=series",
            active_cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "Round {round}: /catalog?kind=series failed"
        );
        assert_eq!(series_json["total"], 1);
        series_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());

        let (st, detail_json, _, _, lat) = send_request(
            &app,
            "GET",
            &format!("/api/v1/catalog/{series_id}"),
            active_cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "Round {round}: /catalog/{{series_id}} failed"
        );
        assert_eq!(detail_json["episodeCount"], 1);
        series_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());

        // (d) Descripteur d'un épisode
        let (st, desc_json, _, _, lat) = send_request(
            &app,
            "GET",
            &format!("/api/v1/media/{episode_media_id}"),
            active_cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::OK,
            "Round {round}: /media/{{episode}} failed"
        );
        assert_eq!(desc_json["id"], episode_media_id);
        descriptor_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());

        // (e) Streaming Range authentifié
        let range_header = if round % 2 == 0 {
            "bytes=0-63"
        } else {
            "bytes=1000-2000"
        };
        let expected_len = if round % 2 == 0 { 64 } else { 1001 };
        let (st, _, bytes, _, lat) = send_request(
            &app,
            "GET",
            &format!("/api/v1/assets/{episode_asset_id}/content"),
            active_cookie,
            None,
            Value::Null,
            Some(range_header),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::PARTIAL_CONTENT,
            "Round {round}: Range streaming failed under lock"
        );
        assert_eq!(bytes.len(), expected_len);
        range_latencies_us.push(lat.as_micros());
        all_latencies_us.push(lat.as_micros());
    }

    // Verify all 120 requests finished while the 30-second lock is STILL held!
    assert!(
        !lock_holder.is_finished(),
        "All 120 authenticated read requests must complete without waiting for the 30s lock to release"
    );

    let mid_lock_stats = auth.touch_stats();
    println!("Mid-lock touch stats (while 30s BEGIN IMMEDIATE is active): {mid_lock_stats:?}");
    assert_eq!(
        mid_lock_stats.write_batches_succeeded, 0,
        "Zero SQLite session writes can commit while BEGIN IMMEDIATE is held"
    );
    assert_eq!(
        mid_lock_stats.pending_count, 2,
        "Both active sessions (admin + reader) must be coalesced into 2 pending entries"
    );
    assert!(
        mid_lock_stats.write_conflicts >= 1,
        "Background worker must detect SQLITE_BUSY fast and retry with backoff"
    );

    // Wait for the full 30-second BEGIN IMMEDIATE lock to finish
    let actual_lock_held = lock_holder.await.unwrap();
    assert!(actual_lock_held >= Duration::from_secs(30));

    // Wait for the background worker to flush the coalesced touches after lock release
    auth.flush_session_touches().await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    let post_lock_stats = auth.touch_stats();
    println!("Post-lock touch stats (after 30s lock release): {post_lock_stats:?}");
    assert_eq!(
        post_lock_stats.pending_count, 0,
        "Pending session touches must flush automatically once lock is released"
    );
    assert_eq!(
        post_lock_stats.write_batches_succeeded, 1,
        "120 requests across 2 sessions must coalesce into a single SQLite UPDATE batch after unlock"
    );
    assert_eq!(post_lock_stats.rows_updated, 2);

    // Compute and report p50, p95, p99 latencies
    me_latencies_us.sort_unstable();
    films_latencies_us.sort_unstable();
    series_latencies_us.sort_unstable();
    descriptor_latencies_us.sort_unstable();
    range_latencies_us.sort_unstable();
    all_latencies_us.sort_unstable();

    for (label, samples) in [
        ("GET /api/v1/auth/me", &me_latencies_us),
        ("GET /api/v1/catalog (films)", &films_latencies_us),
        ("GET /api/v1/catalog (series+detail)", &series_latencies_us),
        ("GET /api/v1/media/{episode}", &descriptor_latencies_us),
        (
            "GET /api/v1/assets/{id}/content (Range)",
            &range_latencies_us,
        ),
        ("ALL 120 AUTHENTICATED READS", &all_latencies_us),
    ] {
        let p50 = percentile_us(samples, 0.50);
        let p95 = percentile_us(samples, 0.95);
        let p99 = percentile_us(samples, 0.99);
        let max_ms = *samples.last().unwrap_or(&0) as f64 / 1000.0;
        println!(
            "[METRICS under 30s lock] {label:<40} | count={:<3} | p50={p50:.2} ms | p95={p95:.2} ms | p99={p99:.2} ms | max={max_ms:.2} ms",
            samples.len()
        );
        assert!(
            p99 < 1000.0,
            "{label} p99 ({p99:.2} ms) must remain < 1000 ms under 30s write lock"
        );
    }

    // 7. Test immediate revocation of a user (disabled_at) and permission revocation after lock release
    sqlx::query("UPDATE users SET disabled_at = unixepoch() WHERE id = ?")
        .bind(reader_id)
        .execute(store.pool())
        .await
        .unwrap();

    for uri in [
        "/api/v1/auth/me".to_owned(),
        "/api/v1/catalog?kind=film".to_owned(),
        format!("/api/v1/media/{episode_media_id}"),
        format!("/api/v1/assets/{episode_asset_id}/content"),
    ] {
        let (st, _, _, _, _) =
            send_request(&app, "GET", &uri, &reader_cookie, None, Value::Null, None).await;
        assert_eq!(
            st,
            StatusCode::UNAUTHORIZED,
            "Disabled user must be rejected immediately on {uri} (no positive auth cache)"
        );
    }

    // Re-enable reader, verify 200 OK, then revoke session via DELETE and verify immediate 401
    sqlx::query("UPDATE users SET disabled_at = NULL WHERE id = ?")
        .bind(reader_id)
        .execute(store.pool())
        .await
        .unwrap();
    let (st, _, _, _, _) = send_request(
        &app,
        "GET",
        "/api/v1/auth/me",
        &reader_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(&reader_hash)
        .execute(store.pool())
        .await
        .unwrap();
    let (st, _, _, _, _) = send_request(
        &app,
        "GET",
        "/api/v1/auth/me",
        &reader_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::UNAUTHORIZED,
        "Revoked session must be rejected on the very next request"
    );

    // 8. Test admin session logout and inactivity expiration + server restart
    let (st, _, _, second_admin_cookie, _) = send_request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": "admin", "password": ADMIN_PASS}),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Logout first admin session via HTTP POST /api/v1/auth/logout
    let (st, _, _, _, _) = send_request(
        &app,
        "POST",
        "/api/v1/auth/logout",
        &admin_cookie,
        Some(&admin_csrf),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _, _, _, _) = send_request(
        &app,
        "GET",
        "/api/v1/auth/me",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // Simulate restart: close store & reopen
    drop(app);
    drop(auth);
    store.close().await;

    let reopened_store = IdentityStore::open(&db_path).await.unwrap();
    let reopened_auth = AuthState::new(reopened_store.clone(), ORIGIN.into())
        .await
        .unwrap();
    let reopened_app = secure_app_with_jobs(
        Inventory::local(root.clone()).await.unwrap(),
        3000,
        reopened_auth.clone(),
        Arc::new(ScanJobs::default()),
    )
    .await
    .unwrap();

    // Logged-out session stays revoked after restart; active second session remains valid
    let (st, _, _, _, _) = send_request(
        &reopened_app,
        "GET",
        "/api/v1/auth/me",
        &admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let (st, _, _, _, _) = send_request(
        &reopened_app,
        "GET",
        "/api/v1/auth/me",
        &second_admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Expire second session by inactivity (last_seen_at <= unixepoch() - 1800) and verify 401
    sqlx::query("UPDATE sessions SET last_seen_at = unixepoch() - 1801")
        .execute(reopened_store.pool())
        .await
        .unwrap();
    let (st, _, _, _, _) = send_request(
        &reopened_app,
        "GET",
        "/api/v1/auth/me",
        &second_admin_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::UNAUTHORIZED,
        "Session expired by inactivity must be rejected immediately"
    );
}
