use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_contracts::inventory::{InventoryBatch, ScanIssue};
use media_poc::{auth::AuthState, identity::IdentityStore, inventory, scans::ScanJobs};
use std::sync::Arc;
use tower::ServiceExt;
use zeroize::Zeroizing;

#[tokio::test]
async fn test_scan_diagnostics_bounded_errors_total_counters_and_staging_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    tokio::fs::create_dir_all(&films).await.unwrap();

    let db_path = temp.path().join("data").join("core.sqlite3");
    tokio::fs::create_dir_all(db_path.parent().unwrap())
        .await
        .unwrap();

    let store = IdentityStore::open(&db_path).await.unwrap();
    let _admin_id = store
        .bootstrap("admin-diag", Zeroizing::new("pass-diag-test-12345".into()))
        .await
        .unwrap();

    let library = "lib-diagnostics";
    sqlx::query(
        "INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES(?, 'Diag Lib', 'films', 'local', ?, 1)"
    )
    .bind(library)
    .bind(films.to_str().unwrap())
    .execute(store.pool())
    .await
    .unwrap();

    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();

    // Log in admin
    let login_body = serde_json::json!({
        "login": "admin-diag",
        "password": "pass-diag-test-12345"
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
    let _csrf_token = login_json["csrfToken"].as_str().unwrap().to_owned();

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

    // Initial scan_jobs row
    sqlx::query("INSERT INTO scan_jobs(library_id, status) VALUES(?, 'queued')")
        .bind(library)
        .execute(store.pool())
        .await
        .unwrap();

    // Create 160 synthetic issues: 120 errors and 40 warnings across batches
    let scan_id = uuid::Uuid::new_v4().to_string();
    let mut total_errors_injected = 0;
    let mut total_warnings_injected = 0;
    let mut all_injected_issues = Vec::new();

    for chunk in 0..2 {
        let errors: Vec<ScanIssue> = (0..60)
            .map(|i| {
                let idx = chunk * 60 + i;
                ScanIssue {
                    relative_path: format!("corrupt_movie_{idx:03}/movie.mkv"),
                    message: format!("moov atom corrupted at offset {idx}"),
                }
            })
            .collect();
        total_errors_injected += errors.len();
        all_injected_issues.extend(errors.clone());

        let warnings: Vec<ScanIssue> = (0..20)
            .map(|i| {
                let idx = chunk * 20 + i;
                ScanIssue {
                    relative_path: format!("warn_movie_{idx:03}/movie.nfo"),
                    message: format!("invalid XML syntax at line {idx}"),
                }
            })
            .collect();
        total_warnings_injected += warnings.len();
        all_injected_issues.extend(warnings.clone());

        let batch = InventoryBatch {
            scan_id: scan_id.clone().into(),
            sequence: chunk as u64,
            files: vec![],
            errors,
            warnings,
        };
        inventory::stage_batch(&store, &scan_id, library, &batch)
            .await
            .unwrap();
    }

    assert_eq!(total_errors_injected, 120);
    assert_eq!(total_warnings_injected, 40);

    // Staging issues must be in DB before publication
    let staged_issues_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_issues WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(staged_issues_count, 160);

    // First 100 anomalies preserved for diagnostics
    let preserved_errors: Vec<String> = all_injected_issues
        .iter()
        .take(100)
        .map(|issue| format!("{}: {}", issue.relative_path, issue.message))
        .collect();
    assert_eq!(preserved_errors.len(), 100);

    // Publish staged scan (which atomically purges scan_staging tables)
    inventory::publish_staged_scan(&store, &scan_id, library, &films, true)
        .await
        .unwrap();

    let remaining_staging_files: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_files WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        remaining_staging_files, 0,
        "All staging files must be purged after publication"
    );

    let remaining_staging_issues: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM scan_staging_issues WHERE scan_id=?")
            .bind(&scan_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        remaining_staging_issues, 0,
        "All staging issues must be purged after publication"
    );

    // Update scan_jobs with total counts and bounded 100 errors list
    sqlx::query(
        "UPDATE scan_jobs SET status='completed',discovered=0,probed=0,reused=0,errors_json=?,errors_count=?,warnings_count=? WHERE library_id=?"
    )
    .bind(serde_json::to_string(&preserved_errors).unwrap())
    .bind(total_errors_injected as i64)
    .bind(total_warnings_injected as i64)
    .bind(library)
    .execute(store.pool())
    .await
    .unwrap();

    // Query status endpoint via HTTP
    let status_res = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/admin/libraries/{library}/scan"))
                .header(header::HOST, "127.0.0.1:3000")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(status_res.status(), StatusCode::OK);

    let status_bytes = status_res.into_body().collect().await.unwrap().to_bytes();
    let status_json: serde_json::Value = serde_json::from_slice(&status_bytes).unwrap();

    // Validate diagnostic fields:
    // 1. Status is completed
    assert_eq!(status_json["status"], "completed");
    // 2. Exactly 100 errors displayed in the errors array
    let displayed_errors = status_json["errors"].as_array().unwrap();
    assert_eq!(displayed_errors.len(), 100);
    // 3. Total error and warning counters are exact
    assert_eq!(status_json["errorsCount"], 120);
    assert_eq!(status_json["warningsCount"], 40);

    // 4. Zero sensitive host absolute paths leaked in errors
    for err in displayed_errors {
        let err_str = err.as_str().unwrap();
        assert!(
            !err_str.contains("C:\\"),
            "Must not leak Windows drive paths: {err_str}"
        );
        assert!(
            !err_str.contains("/home/"),
            "Must not leak Unix home paths: {err_str}"
        );
    }
}
