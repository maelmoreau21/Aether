//! The scheduler rework: one trigger engine, maintenance that is never starved, task views that
//! ignore real-time monitoring polls, and the operations queue mounted in the application.
use aether_core::{
    auth::{self, AuthState},
    database::{query, query_as, query_scalar},
    identity::IdentityStore,
    node::Inventory,
    scans::ScanJobs,
    scheduler, task_engine, task_scheduler,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use zeroize::Zeroizing;

const PASSWORD: &str = "synthetic scheduler rework password";

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
) -> (StatusCode, Value, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3000")
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if !cookie.is_empty() {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-csrf-token", csrf);
    }
    let body = if uri.ends_with("/auth/login") {
        Body::from(json!({"login": "admin", "password": PASSWORD}).to_string())
    } else {
        Body::empty()
    };
    let response = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
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
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

async fn login(app: &Router) -> (String, String) {
    let (status, body, cookie) = call(app, "POST", "/api/v1/auth/login", "", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (cookie, body["csrfToken"].as_str().unwrap().to_owned())
}

async fn open(temp: &std::path::Path) -> (IdentityStore, AuthState) {
    let store = IdentityStore::open(&temp.join("scheduler.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    aether_core::libraries::initialize_local(&store)
        .await
        .unwrap();
    (store, auth)
}

async fn add_library(store: &IdentityStore, root: &std::path::Path) {
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)")
        .bind(root.to_string_lossy().as_ref())
        .execute(store.pool())
        .await
        .unwrap();
}

/// Unified task API plus the system-task routes it relies on.
async fn task_app(auth: &AuthState, root: &std::path::Path) -> Router {
    aether_core::task_triggers::router(auth.clone())
        .merge(
            aether_core::system_tasks::router(auth.clone(), root.to_owned())
                .await
                .unwrap(),
        )
        .merge(auth::router(auth.clone()))
}

#[tokio::test]
async fn operations_queue_and_task_api_are_mounted_in_the_application() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    let (_store, auth) = open(temp.path()).await;
    let app = aether_core::secure_app(Inventory::local(media).await.unwrap(), 3000, auth)
        .await
        .unwrap();
    let (cookie, _) = login(&app).await;
    // `/tasks/jobs` used to fall through to `/tasks/{id}` (id "jobs") and answer 404.
    let (status, body, _) = call(
        &app,
        "GET",
        "/api/v1/admin/tasks/jobs?limit=50&offset=0",
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["jobs"].is_array(), "{body}");
    let (status, body, _) = call(&app, "GET", "/api/v1/admin/tasks", &cookie, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == "clean-sessions")
    );
}

#[tokio::test]
async fn a_missed_instant_fires_once_and_a_busy_task_is_skipped() {
    let temp = tempfile::tempdir().unwrap();
    let (store, auth) = open(temp.path()).await;
    let pool = store.pool();
    let now = media_contracts::protocol::unix_now();
    // Three days of downtime: one run, then the next instant after now — never a burst.
    query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,time_of_day_minutes,enabled,next_fire_at,created_at) VALUES('daily','system','clean-cache','daily',180,1,?,0)")
        .bind(now - 3 * 86400)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(task_scheduler::fire_due(&auth, now).await.unwrap(), 1);
    let next: i64 = query_scalar("SELECT next_fire_at FROM task_triggers WHERE id='daily'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(next > now && next <= now + 86400, "{next}");
    assert_eq!(task_scheduler::fire_due(&auth, now).await.unwrap(), 0);
    // The run is still queued when the next instant comes: that firing is skipped and the
    // trigger moves on, without a second queued run.
    query("UPDATE task_triggers SET next_fire_at=? WHERE id='daily'")
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(task_scheduler::fire_due(&auth, now + 1).await.unwrap(), 0);
    let runs: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id='clean-cache'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(runs, 1);
    let next: i64 = query_scalar("SELECT next_fire_at FROM task_triggers WHERE id='daily'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(next > now + 1, "{next}");
    // Triggers of tasks that no longer exist are dropped instead of failing every pass.
    query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES('ghost','scan','deleted-library','interval',60,1,0,0)")
        .execute(pool)
        .await
        .unwrap();
    task_scheduler::fire_due(&auth, now + 2).await.unwrap();
    let ghosts: i64 = query_scalar("SELECT COUNT(*) FROM task_triggers WHERE id='ghost'")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(ghosts, 0);
}

#[tokio::test]
async fn light_maintenance_is_not_starved_by_scans_and_heavy_work_waits_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    let (store, auth) = open(&root).await;
    let pool = store.pool();
    add_library(&store, &root).await;
    let now = media_contracts::protocol::unix_now();
    // A local scan holds the Node and more processing waits behind it.
    task_engine::enqueue(&store, "films", "scheduler", "busy:films", 100, now)
        .await
        .unwrap();
    let running = task_engine::claim(&store, "local", now, 600, 1)
        .await
        .unwrap()
        .expect("scan leased");
    task_engine::enqueue(&store, "films", "scheduler", "waiting:films", 100, now)
        .await
        .unwrap();
    query("UPDATE task_triggers SET next_fire_at=0 WHERE task_kind='system' AND task_id IN ('clean-sessions','optimize-database')")
        .execute(pool)
        .await
        .unwrap();
    let optimize: i64 = query_scalar("SELECT COUNT(*) FROM task_triggers WHERE task_kind='system' AND task_id='optimize-database'")
        .fetch_one(pool)
        .await
        .unwrap();
    if optimize == 0 {
        query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES('opt','system','optimize-database','interval',10080,1,0,0)")
            .execute(pool)
            .await
            .unwrap();
    }
    assert_eq!(task_scheduler::fire_due(&auth, now).await.unwrap(), 2);
    let jobs = Arc::new(ScanJobs::default());
    let attempts = |target: &'static str| async move {
        query_as::<_, (i64, String)>(
            "SELECT attempt,status FROM durable_tasks WHERE category='system' AND target_id=?",
        )
        .bind(target)
        .fetch_one(pool)
        .await
        .unwrap()
    };
    scheduler::dispatch_once(&auth, &root, &jobs, now)
        .await
        .unwrap();
    // The light run starts although the Node is busy; the heavy one keeps waiting.
    assert!(attempts("clean-sessions").await.0 >= 1);
    assert_eq!(attempts("optimize-database").await, (0, "queued".into()));
    for _ in 0..200 {
        if attempts("clean-sessions").await.1 == "completed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(attempts("clean-sessions").await.1, "completed");
    // The light run did not occupy the Node: the scan lease is untouched.
    let lease: Option<i64> = query_scalar("SELECT lease_until FROM durable_tasks WHERE id=?")
        .bind(&running.id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert!(lease.is_some());
    // After waiting its maximum, heavy maintenance runs even on a busy server.
    query(
        "UPDATE durable_tasks SET created_at=created_at-3600 WHERE target_id='optimize-database'",
    )
    .execute(pool)
    .await
    .unwrap();
    scheduler::dispatch_once(&auth, &root, &jobs, now + 1)
        .await
        .unwrap();
    assert!(attempts("optimize-database").await.0 >= 1);
    jobs.shutdown().await;
}

#[tokio::test]
async fn monitoring_polls_and_stale_markers_never_block_a_task() {
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    let (store, auth) = open(&root).await;
    let pool = store.pool();
    add_library(&store, &root).await;
    let app = task_app(&auth, &root).await;
    let (cookie, csrf) = login(&app).await;
    let now = media_contracts::protocol::unix_now();
    // A real-time monitoring poll of the library is running.
    task_engine::enqueue(
        &store,
        "films",
        "library-monitor",
        "monitor:1:films",
        10,
        now,
    )
    .await
    .unwrap();
    let poll = task_engine::claim(&store, "local", now, 600, 1)
        .await
        .unwrap()
        .unwrap();
    let (status, view, _) =
        call(&app, "GET", "/api/v1/admin/tasks/scan-films", &cookie, None).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["state"], "idle", "a monitoring poll is not a scan run");
    task_engine::finish(&store, &poll.id, poll.attempt, "local", now + 1, None)
        .await
        .unwrap();
    let (_, view, _) = call(&app, "GET", "/api/v1/admin/tasks/scan-films", &cookie, None).await;
    assert!(
        view["lastRun"].is_null(),
        "a finished poll is not the last scan: {view}"
    );
    // The administrator can still start a scan.
    let (status, body, _) = call(
        &app,
        "POST",
        "/api/v1/admin/tasks/scan-films/run",
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (_, view, _) = call(&app, "GET", "/api/v1/admin/tasks/scan-films", &cookie, None).await;
    assert_eq!(view["state"], "queued");
    let (status, body, _) = call(
        &app,
        "POST",
        "/api/v1/admin/tasks/scan-films/run",
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "task_already_active");

    // A run that died with the server leaves a "running" marker: it reads as interrupted and
    // the task can be started again instead of looking busy forever.
    query("UPDATE system_task_schedules SET last_outcome='running',last_started_at=? WHERE id='clean-cache'")
        .bind(now - 60)
        .execute(pool)
        .await
        .unwrap();
    let (_, view, _) = call(
        &app,
        "GET",
        "/api/v1/admin/tasks/clean-cache",
        &cookie,
        None,
    )
    .await;
    assert_eq!(view["state"], "idle");
    assert_eq!(view["lastRun"]["outcome"], "interrupted");
    let (status, _, _) = call(
        &app,
        "POST",
        "/api/v1/admin/tasks/clean-cache/run",
        &cookie,
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    // Queueing a run never erases the result shown for the previous one.
    let outcome: String =
        query_scalar("SELECT last_outcome FROM system_task_schedules WHERE id='clean-cache'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(outcome, "running");
    let (status, body, _) = call(
        &app,
        "GET",
        "/api/v1/admin/tasks/does-not-exist",
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "task_not_found");
}
