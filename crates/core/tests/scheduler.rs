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
    let count: i64 = media_poc::database::query_scalar("SELECT COUNT(*) FROM scan_jobs")
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
    media_poc::database::query("UPDATE scheduled_scans SET last_outcome='dispatching'")
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
