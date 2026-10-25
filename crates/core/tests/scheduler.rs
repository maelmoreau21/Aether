use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::{self, AuthState},
    identity::IdentityStore,
    scans::ScanJobs,
    scheduler,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use zeroize::Zeroizing;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(value) = cookie {
        req = req.header(header::COOKIE, value);
    }
    if let Some(value) = csrf {
        req = req.header("x-csrf-token", value);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn settle(
    store: &IdentityStore,
    auth: &AuthState,
    root: &std::path::Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) {
    for _ in 0..500 {
        scheduler::dispatch_once(auth, root, jobs, timestamp)
            .await
            .unwrap();
        let active: i64 = media_poc::database::query_scalar(
            "SELECT COUNT(*) FROM durable_tasks WHERE lease_until IS NOT NULL",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        if active == 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("durable scan did not settle");
}
#[tokio::test]
async fn metadata_refresh_is_durable_bounded_and_reuses_the_scan_runner() {
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    let store = IdentityStore::open(&root.join("metadata.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "admin",
            Zeroizing::new("synthetic metadata scheduler secret".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json,next_metadata_refresh_at) VALUES('metadata','Films','films','local',?,1,'{\"automaticRefreshIntervalDays\":30}',1000)").bind(root.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    let jobs = Arc::new(ScanJobs::default());
    scheduler::run_due(&auth, &root, &jobs, 999).await.unwrap();
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    scheduler::run_due(&auth, &root, &jobs, 1000).await.unwrap();
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE status='queued'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
    let worker_now = media_contracts::protocol::unix_now();
    scheduler::dispatch_once(&auth, &root, &jobs, worker_now)
        .await
        .unwrap();
    settle(&store, &auth, &root, &jobs, worker_now + 1).await;
    jobs.shutdown().await;
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>(
            "SELECT next_metadata_refresh_at FROM libraries WHERE id='metadata'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1000 + 30 * 86400
    );
    let mode: String = media_poc::database::query_scalar(
        "SELECT refresh_mode FROM scan_jobs WHERE library_id='metadata'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(mode, "incremental");
    store.close().await;
    let reopened = IdentityStore::open(&root.join("metadata.sqlite"))
        .await
        .unwrap();
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>(
            "SELECT next_metadata_refresh_at FROM libraries WHERE id='metadata'"
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap(),
        1000 + 30 * 86400
    );
}
#[tokio::test]
async fn schedules_require_admin_csrf_validate_persist_and_dispatch_real_scans() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    tokio::fs::create_dir(&media).await.unwrap();
    let root = tokio::fs::canonicalize(media).await.unwrap();
    let path = temp.path().join("db.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    store
        .bootstrap(
            "scheduler-admin",
            Zeroizing::new("synthetic scheduler password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)")
        .bind(root.to_str().unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    let app = scheduler::router(auth.clone())
        .await
        .unwrap()
        .merge(auth::router(auth.clone()));
    let response = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        None,
        json!({"login":"scheduler-admin","password":"synthetic scheduler password"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let csrf = body["csrfToken"].as_str().unwrap();
    let endpoint = "/api/v1/admin/tasks/schedules/films";
    let plan = json!({"intervalMinutes":5,"enabled":true});
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks/schedules",
            None,
            None,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "PUT", endpoint, Some(&cookie), None, plan.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            endpoint,
            Some(&cookie),
            Some(csrf),
            json!({"intervalMinutes":4,"enabled":true})
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/tasks/schedules/missing",
            Some(&cookie),
            Some(csrf),
            plan.clone()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "PUT", endpoint, Some(&cookie), Some(csrf), plan)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    media_poc::database::query("UPDATE users SET role='user'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks/schedules",
            Some(&cookie),
            None,
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            endpoint,
            Some(&cookie),
            Some(csrf),
            json!({"intervalMinutes":5,"enabled":true})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    media_poc::database::query("UPDATE users SET role='administrator'")
        .execute(store.pool())
        .await
        .unwrap();
    let saved: Value = serde_json::from_slice(
        &call(
            &app,
            "GET",
            "/api/v1/admin/tasks/schedules",
            Some(&cookie),
            None,
            json!(null),
        )
        .await
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes(),
    )
    .unwrap();
    assert_eq!(saved[0]["intervalMinutes"], 5);
    let due = saved[0]["nextRunAt"].as_i64().unwrap();
    let jobs = Arc::new(ScanJobs::default());
    scheduler::run_due(&auth, &root, &jobs, due - 1)
        .await
        .unwrap();
    let count: i64 = media_poc::database::query_scalar("SELECT COUNT(*) FROM durable_tasks")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    media_poc::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES('films','running',0,0)").execute(store.pool()).await.unwrap();
    scheduler::run_due(&auth, &root, &jobs, due).await.unwrap();
    let unclaimed: Option<i64> = media_poc::database::query_scalar(
        "SELECT last_run_at FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(unclaimed, None);
    media_poc::database::query("DELETE FROM scan_jobs WHERE library_id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    scheduler::run_due(&auth, &root, &jobs, due).await.unwrap();
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','completed','error')"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
    scheduler::dispatch_once(&auth, &root, &jobs, due)
        .await
        .unwrap();
    settle(&store, &auth, &root, &jobs, due + 1).await;
    jobs.shutdown().await;
    scheduler::run_due(&auth, &root, &jobs, due + 1)
        .await
        .unwrap();
    let response = call(
        &app,
        "GET",
        "/api/v1/admin/tasks/schedules",
        Some(&cookie),
        None,
        json!(null),
    )
    .await;
    let saved: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(saved[0]["lastRunAt"], due);
    assert_eq!(saved[0]["nextRunAt"], due + 300);
    assert!(matches!(
        saved[0]["lastOutcome"].as_str(),
        Some("completed" | "error")
    ));
    // Reopening reconstructs configuration from disk, without spawning a worker in a router.
    drop(app);
    drop(auth);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    let interval: i64 = media_poc::database::query_scalar(
        "SELECT interval_minutes FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(interval, 5);
    media_poc::database::query("UPDATE scheduled_scans SET last_outcome='queued'")
        .execute(reopened.pool())
        .await
        .unwrap();
    let recovered_auth = AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let _recovered = scheduler::router(recovered_auth).await.unwrap();
    let recovered_outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert!(matches!(recovered_outcome.as_str(), "completed" | "error"));
    // An old schedule without a durable request key retains the legacy recovery rule.
    media_poc::database::query(
        "UPDATE scheduled_scans SET last_outcome='dispatching',last_request_key=''",
    )
    .execute(reopened.pool())
    .await
    .unwrap();
    let restarted_auth = AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let restarted_app = scheduler::router(restarted_auth).await.unwrap();
    let outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "interrupted");
    assert_eq!(
        call(
            &restarted_app,
            "DELETE",
            endpoint,
            Some(&cookie),
            None,
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &restarted_app,
            "DELETE",
            endpoint,
            Some(&cookie),
            Some(csrf),
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let count: i64 = media_poc::database::query_scalar("SELECT COUNT(*) FROM scheduled_scans")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    reopened.close().await;
}

#[tokio::test]
async fn schedule_reconciliation_tracks_all_durable_scopes_and_pending_retries() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("aggregate.sqlite"))
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    for id in ["one", "two"] {
        media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic-root',1)")
            .bind(id).bind(id).execute(store.pool()).await.unwrap();
        media_poc::task_engine::enqueue(
            &store,
            id,
            "scheduler",
            &format!("schedule:one:1000:{id}"),
            100,
            1000,
        )
        .await
        .unwrap();
    }
    media_poc::database::query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at,last_outcome,last_request_key) VALUES('one',5,1,1300,'dispatching','schedule:one:1000')")
        .execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let _router = scheduler::router(auth.clone()).await.unwrap();
    let outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    let running = media_poc::task_engine::claim(&store, "local", 1000, 30, 1)
        .await
        .unwrap()
        .unwrap();
    media_poc::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES(?,'error',1,0)")
        .bind(&running.library_id).execute(store.pool()).await.unwrap();
    media_poc::task_engine::finish(
        &store,
        &running.id,
        running.attempt,
        "local",
        1001,
        Some("retryable synthetic error"),
    )
    .await
    .unwrap();
    let _router = scheduler::router(auth.clone()).await.unwrap();
    let outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    media_poc::database::query("UPDATE durable_tasks SET status='completed' WHERE id=?")
        .bind(&running.id)
        .execute(store.pool())
        .await
        .unwrap();
    let _router = scheduler::router(auth.clone()).await.unwrap();
    let outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    media_poc::database::query("UPDATE durable_tasks SET status='completed'")
        .execute(store.pool())
        .await
        .unwrap();
    let _router = scheduler::router(auth).await.unwrap();
    let outcome: String = media_poc::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "completed");
}
