use aether_core::{
    auth::{self, AuthState},
    identity::IdentityStore,
    scans::ScanJobs,
    scheduler,
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
        let active: i64 = aether_core::database::query_scalar(
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
    aether_core::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json,next_metadata_refresh_at) VALUES('metadata','Films','films','local',?,1,'{\"automaticRefreshIntervalDays\":30}',1000)").bind(root.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    let jobs = Arc::new(ScanJobs::default());
    scheduler::run_due(&auth, &root, &jobs, 999).await.unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    scheduler::run_due(&auth, &root, &jobs, 1000).await.unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>(
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
        aether_core::database::query_scalar::<_, i64>(
            "SELECT next_metadata_refresh_at FROM libraries WHERE id='metadata'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1000 + 30 * 86400
    );
    let mode: String = aether_core::database::query_scalar(
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
        aether_core::database::query_scalar::<_, i64>(
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
    aether_core::libraries::initialize_local(&store)
        .await
        .unwrap();
    aether_core::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)")
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
    aether_core::database::query("UPDATE users SET role='user'")
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
    aether_core::database::query("UPDATE users SET role='administrator'")
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
    let count: i64 = aether_core::database::query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE category='scan'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);
    // A firing that finds the library already being scanned is skipped (the scan is happening
    // anyway) and the trigger moves on: no pile-up of scans behind a long one.
    aether_core::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES('films','running',0,0)").execute(store.pool()).await.unwrap();
    scheduler::run_due(&auth, &root, &jobs, due).await.unwrap();
    let unclaimed: (Option<i64>, i64) = aether_core::database::query_as(
        "SELECT last_run_at,next_run_at FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(unclaimed, (None, due + 300));
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE category='scan'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        0
    );
    aether_core::database::query("DELETE FROM scan_jobs WHERE library_id='films'")
        .execute(store.pool())
        .await
        .unwrap();
    let due = due + 300;
    scheduler::run_due(&auth, &root, &jobs, due).await.unwrap();
    // A second pass at the same instant never fires the same trigger twice.
    scheduler::run_due(&auth, &root, &jobs, due).await.unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE category='scan' AND status IN ('queued','running','completed','error')"
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
    let interval: i64 = aether_core::database::query_scalar(
        "SELECT interval_minutes FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(interval, 5);
    aether_core::database::query("UPDATE scheduled_scans SET last_outcome='queued'")
        .execute(reopened.pool())
        .await
        .unwrap();
    let recovered_auth = AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let _recovered = scheduler::router(recovered_auth).await.unwrap();
    let recovered_outcome: String = aether_core::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='films'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert!(matches!(recovered_outcome.as_str(), "completed" | "error"));
    // An old schedule without a durable request key retains the legacy recovery rule.
    aether_core::database::query(
        "UPDATE scheduled_scans SET last_outcome='dispatching',last_request_key=''",
    )
    .execute(reopened.pool())
    .await
    .unwrap();
    let restarted_auth = AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let restarted_app = scheduler::router(restarted_auth).await.unwrap();
    let outcome: String = aether_core::database::query_scalar(
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
    let count: i64 = aether_core::database::query_scalar("SELECT COUNT(*) FROM scheduled_scans")
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
    aether_core::libraries::initialize_local(&store)
        .await
        .unwrap();
    for id in ["one", "two"] {
        aether_core::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic-root',1)")
            .bind(id).bind(id).execute(store.pool()).await.unwrap();
        aether_core::task_engine::enqueue(
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
    aether_core::database::query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at,last_outcome,last_request_key) VALUES('one',5,1,1300,'dispatching','schedule:one:1000')")
        .execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let _router = scheduler::router(auth.clone()).await.unwrap();
    let outcome: String = aether_core::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    let running = aether_core::task_engine::claim(&store, "local", 1000, 30, 1)
        .await
        .unwrap()
        .unwrap();
    aether_core::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES(?,'error',1,0)")
        .bind(&running.library_id).execute(store.pool()).await.unwrap();
    aether_core::task_engine::finish(
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
    let outcome: String = aether_core::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    aether_core::database::query("UPDATE durable_tasks SET status='completed' WHERE id=?")
        .bind(&running.id)
        .execute(store.pool())
        .await
        .unwrap();
    let _router = scheduler::router(auth.clone()).await.unwrap();
    let outcome: String = aether_core::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "queued");
    aether_core::database::query("UPDATE durable_tasks SET status='completed'")
        .execute(store.pool())
        .await
        .unwrap();
    let _router = scheduler::router(auth).await.unwrap();
    let outcome: String = aether_core::database::query_scalar(
        "SELECT last_outcome FROM scheduled_scans WHERE library_id='one'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(outcome, "completed");
}

#[tokio::test]
async fn missing_credentials_fail_once_while_provider_errors_retry() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("credentials.sqlite"))
        .await
        .unwrap();
    aether_core::libraries::initialize_local(&store)
        .await
        .unwrap();
    aether_core::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','lib','films','local','synthetic-root',1)")
        .execute(store.pool()).await.unwrap();
    for (key, error, expected) in [
        ("k1", "extension_error:missing_api_key", ("error", 1)),
        ("k2", "extension_error:unauthorized", ("error", 1)),
        ("k3", "extension_error:provider_error", ("queued", 1)),
    ] {
        aether_core::task_engine::enqueue(&store, "lib", "scheduler", key, 100, 1000)
            .await
            .unwrap();
        let running = aether_core::task_engine::claim(&store, "local", 1000, 30, 1)
            .await
            .unwrap()
            .unwrap();
        aether_core::task_engine::finish(
            &store,
            &running.id,
            running.attempt,
            "local",
            1001,
            Some(error),
        )
        .await
        .unwrap();
        let (status, failures): (String, i64) = aether_core::database::query_as(
            "SELECT status,failure_count FROM durable_tasks WHERE id=?",
        )
        .bind(&running.id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!((status.as_str(), failures), expected, "{error}");
        aether_core::database::query("UPDATE durable_tasks SET status='completed' WHERE id=?")
            .bind(&running.id)
            .execute(store.pool())
            .await
            .unwrap();
    }
    store.close().await;
}

#[tokio::test]
async fn monitoring_and_online_refresh_are_durable_and_do_not_duplicate_pending_work() {
    let temp = tempfile::tempdir().unwrap();
    let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
    let store = IdentityStore::open(&root.join("automation.sqlite"))
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    aether_core::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,options_json) VALUES('auto','Films','films','local',?,1,?)")
        .bind(root.to_string_lossy().as_ref())
        .bind(json!({"realtimeMonitoring":true,"onlineRefreshIntervalDays":30}).to_string())
        .execute(store.pool()).await.unwrap();
    let jobs = Arc::new(ScanJobs::default());
    scheduler::run_due(&auth, &root, &jobs, 1000).await.unwrap();
    scheduler::run_due(&auth, &root, &jobs, 1000).await.unwrap();
    scheduler::run_due(&auth, &root, &jobs, 1100).await.unwrap();
    let monitored: i64 = aether_core::database::query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE owner='library-monitor'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        monitored, 1,
        "pending monitoring work coalesces even in the next minute"
    );
    let sweeps: i64 = aether_core::database::query_scalar("SELECT COUNT(*) FROM automation_sweeps")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(sweeps, 1);
    aether_core::database::query("UPDATE durable_tasks SET status='completed';")
        .execute(store.pool())
        .await
        .unwrap();
    aether_core::database::query("UPDATE automation_sweeps SET completed_at=1001")
        .execute(store.pool())
        .await
        .unwrap();
    scheduler::run_due(&auth, &root, &jobs, 1101).await.unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE owner='library-monitor'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        2
    );
    scheduler::run_due(&auth, &root, &jobs, 1000 + 30 * 86400)
        .await
        .unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM automation_sweeps")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        2
    );
    aether_core::database::query("UPDATE libraries SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    scheduler::run_due(&auth, &root, &jobs, 1000 + 60 * 86400)
        .await
        .unwrap();
    assert_eq!(
        aether_core::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM automation_sweeps")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        2
    );
}

// --- Jellyfin-style triggers -----------------------------------------------------------------

use aether_core::task_triggers::{TriggerSpec, next_fire};

fn utc(text: &str) -> i64 {
    text.parse::<jiff::Timestamp>().unwrap().as_second()
}

#[test]
fn daily_trigger_fires_at_the_next_local_time_of_day() {
    let zone = jiff::tz::TimeZone::UTC;
    let five = TriggerSpec::daily(5 * 60);
    // Before 05:00 today: today. At or after 05:00: tomorrow.
    assert_eq!(
        next_fire(&five, utc("2026-11-05T04:59:59Z"), &zone),
        Some(utc("2026-11-05T05:00:00Z"))
    );
    assert_eq!(
        next_fire(&five, utc("2026-11-05T05:00:00Z"), &zone),
        Some(utc("2026-11-06T05:00:00Z"))
    );
    // Month and year boundaries.
    assert_eq!(
        next_fire(&TriggerSpec::daily(30), utc("2026-12-31T23:00:00Z"), &zone),
        Some(utc("2027-01-01T00:30:00Z"))
    );
}

#[test]
fn daily_trigger_uses_the_server_time_zone_across_dst() {
    let paris = jiff::tz::TimeZone::get("Europe/Paris").unwrap();
    // Winter (UTC+1): 03:00 local is 02:00 UTC.
    assert_eq!(
        next_fire(
            &TriggerSpec::daily(180),
            utc("2026-01-10T12:00:00Z"),
            &paris
        ),
        Some(utc("2026-01-11T02:00:00Z"))
    );
    // Summer (UTC+2): 03:00 local is 01:00 UTC.
    assert_eq!(
        next_fire(
            &TriggerSpec::daily(180),
            utc("2026-07-10T12:00:00Z"),
            &paris
        ),
        Some(utc("2026-07-11T01:00:00Z"))
    );
    // 02:30 does not exist on 2026-03-29 in Paris: it resolves forward, never skipped.
    let gap = next_fire(
        &TriggerSpec::daily(150),
        utc("2026-03-28T12:00:00Z"),
        &paris,
    )
    .unwrap();
    assert!(
        gap > utc("2026-03-28T12:00:00Z") && gap <= utc("2026-03-29T01:30:00Z"),
        "{gap}"
    );
}

#[test]
fn weekly_trigger_selects_the_weekday_and_wraps_the_week() {
    let zone = jiff::tz::TimeZone::UTC;
    // 2026-11-05 is a Thursday (4). Monday (1) 03:00 is next week's Monday.
    let monday = TriggerSpec::weekly(1, 3 * 60);
    assert_eq!(
        next_fire(&monday, utc("2026-11-05T10:00:00Z"), &zone),
        Some(utc("2026-11-09T03:00:00Z"))
    );
    // Same weekday, later time: today.
    let thursday_late = TriggerSpec::weekly(4, 22 * 60);
    assert_eq!(
        next_fire(&thursday_late, utc("2026-11-05T10:00:00Z"), &zone),
        Some(utc("2026-11-05T22:00:00Z"))
    );
    // Same weekday, time already passed: a full week later.
    let thursday_early = TriggerSpec::weekly(4, 9 * 60);
    assert_eq!(
        next_fire(&thursday_early, utc("2026-11-05T10:00:00Z"), &zone),
        Some(utc("2026-11-12T09:00:00Z"))
    );
    // Saturday -> Sunday (0) wraps across the week boundary.
    assert_eq!(
        next_fire(
            &TriggerSpec::weekly(0, 60),
            utc("2026-11-07T23:59:00Z"),
            &zone
        ),
        Some(utc("2026-11-08T01:00:00Z"))
    );
}

#[test]
fn interval_and_startup_triggers() {
    let zone = jiff::tz::TimeZone::UTC;
    let now = utc("2026-11-05T10:00:00Z");
    assert_eq!(
        next_fire(&TriggerSpec::interval(12 * 60), now, &zone),
        Some(now + 12 * 3600)
    );
    assert_eq!(
        next_fire(&TriggerSpec::interval(15), now, &zone),
        Some(now + 900)
    );
    assert_eq!(next_fire(&TriggerSpec::startup(), now, &zone), None);
}

async fn admin_app(temp: &std::path::Path) -> (IdentityStore, AuthState, Router, String, String) {
    let store = IdentityStore::open(&temp.join("triggers.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "trigger-admin",
            Zeroizing::new("synthetic trigger password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = aether_core::task_triggers::router(auth.clone())
        .merge(scheduler::router(auth.clone()).await.unwrap())
        .merge(auth::router(auth.clone()));
    let response = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        None,
        json!({"login":"trigger-admin","password":"synthetic trigger password"}),
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
    let csrf = body["csrfToken"].as_str().unwrap().to_owned();
    (store, auth, app, cookie, csrf)
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn default_intervals_become_triggers_and_the_unified_api_manages_them() {
    let temp = tempfile::tempdir().unwrap();
    let (store, _auth, app, cookie, csrf) = admin_app(temp.path()).await;
    let pool = store.pool();
    // Migration: every enabled legacy interval is an interval trigger, the statistics cleanup too.
    let migrated: Vec<(String, i64)> = aether_core::database::query_as(
        "SELECT task_id,interval_minutes FROM task_triggers WHERE task_kind='system' AND trigger_type='interval' ORDER BY task_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert!(
        migrated.contains(&("clean-statistics".to_owned(), 5)),
        "{migrated:?}"
    );
    assert!(
        migrated.contains(&("clean-cache".to_owned(), 1440)),
        "{migrated:?}"
    );
    let count =
        |sql: &'static str| aether_core::database::query_scalar::<_, i64>(sql).fetch_one(pool);
    let before = count("SELECT COUNT(*) FROM task_triggers").await.unwrap();
    assert_eq!(
        before as usize,
        migrated.len(),
        "backup is disabled by default"
    );

    // Unified list.
    let list = json_body(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks",
            Some(&cookie),
            None,
            json!(null),
        )
        .await,
    )
    .await;
    let tasks = list.as_array().unwrap();
    let cache = tasks.iter().find(|t| t["id"] == "clean-cache").unwrap();
    assert_eq!(cache["kind"], "system");
    assert_eq!(cache["state"], "idle");
    assert_eq!(cache["triggers"].as_array().unwrap().len(), 1);
    assert!(tasks.iter().any(|t| t["id"] == "backup"));
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/tasks", None, None, json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // Replace the interval with a daily and a weekly trigger.
    let legacy_id = cache["triggers"][0]["id"].as_str().unwrap().to_owned();
    let triggers = "/api/v1/admin/tasks/clean-cache/triggers";
    assert_eq!(
        call(
            &app,
            "POST",
            triggers,
            Some(&cookie),
            None,
            json!({"type":"daily","timeOfDayMinutes":300})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN,
        "mutations require CSRF"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            triggers,
            Some(&cookie),
            Some(&csrf),
            json!({"type":"daily","timeOfDayMinutes":1440})
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let daily = call(
        &app,
        "POST",
        triggers,
        Some(&cookie),
        Some(&csrf),
        json!({"type":"daily","timeOfDayMinutes":300,"maxRuntimeSeconds":7200}),
    )
    .await;
    assert_eq!(daily.status(), StatusCode::CREATED);
    let daily = json_body(daily).await;
    assert_eq!(daily["type"], "daily");
    assert_eq!(daily["maxRuntimeSeconds"], 7200);
    assert!(daily["nextFireAt"].as_i64().unwrap() > media_contracts::protocol::unix_now());
    assert_eq!(
        call(
            &app,
            "POST",
            triggers,
            Some(&cookie),
            Some(&csrf),
            json!({"type":"daily","timeOfDayMinutes":300,"maxRuntimeSeconds":7200})
        )
        .await
        .status(),
        StatusCode::CONFLICT,
        "identical triggers are refused"
    );
    assert_eq!(
        call(
            &app,
            "POST",
            triggers,
            Some(&cookie),
            Some(&csrf),
            json!({"type":"weekly","dayOfWeek":1,"timeOfDayMinutes":180})
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            triggers,
            Some(&cookie),
            Some(&csrf),
            json!({"type":"startup"})
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("{triggers}/{legacy_id}"),
            Some(&cookie),
            Some(&csrf),
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let detail = json_body(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks/clean-cache",
            Some(&cookie),
            None,
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(detail["triggers"].as_array().unwrap().len(), 3);
    // The dispatch cache follows the earliest trigger.
    let (enabled, next): (i64, i64) = aether_core::database::query_as(
        "SELECT enabled,next_due_at FROM system_task_schedules WHERE id='clean-cache'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(enabled, 1);
    assert_eq!(Some(next), detail["nextRunAt"].as_i64());

    // Removing every timed trigger disables periodic runs; the startup trigger remains.
    for trigger in detail["triggers"].as_array().unwrap() {
        if trigger["type"] != "startup" {
            let path = format!("{triggers}/{}", trigger["id"].as_str().unwrap());
            assert_eq!(
                call(
                    &app,
                    "DELETE",
                    &path,
                    Some(&cookie),
                    Some(&csrf),
                    json!(null)
                )
                .await
                .status(),
                StatusCode::NO_CONTENT
            );
        }
    }
    let enabled: i64 = aether_core::database::query_scalar(
        "SELECT enabled FROM system_task_schedules WHERE id='clean-cache'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(enabled, 0);
    let remaining: i64 = aether_core::database::query_scalar(
        "SELECT COUNT(*) FROM task_triggers WHERE task_id='clean-cache'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        remaining, 1,
        "a removed default interval is not resurrected"
    );

    // Run now, then cancel the queued run.
    let run = call(
        &app,
        "POST",
        "/api/v1/admin/tasks/clean-cache/run",
        Some(&cookie),
        Some(&csrf),
        json!(null),
    )
    .await;
    assert_eq!(run.status(), StatusCode::ACCEPTED);
    let detail = json_body(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks/clean-cache",
            Some(&cookie),
            None,
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(detail["state"], "queued");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/tasks/clean-cache/run",
            Some(&cookie),
            Some(&csrf),
            json!(null)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/tasks/clean-cache/cancel",
            Some(&cookie),
            Some(&csrf),
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/tasks/does-not-exist",
            Some(&cookie),
            None,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    store.close().await;
}

#[tokio::test]
async fn startup_triggers_queue_once_and_time_limits_reach_the_queue() {
    let temp = tempfile::tempdir().unwrap();
    let (store, auth, _app, _cookie, _csrf) = admin_app(temp.path()).await;
    let pool = store.pool();
    aether_core::database::query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,max_runtime_seconds,enabled,created_at) VALUES('boot','system','optimize-database','startup',900,1,0)")
        .execute(pool).await.unwrap();
    let fired = aether_core::task_scheduler::fire_startup(&auth, 1_000)
        .await
        .unwrap();
    assert_eq!(fired, 1);
    // A second start in the same instant (or an already active run) does not duplicate work.
    aether_core::task_scheduler::fire_startup(&auth, 1_000)
        .await
        .unwrap();
    let queued: Vec<String> = aether_core::database::query_scalar("SELECT result_json FROM durable_tasks WHERE category='system' AND target_id='optimize-database' AND status='queued'")
        .fetch_all(pool).await.unwrap();
    assert_eq!(queued.len(), 1);
    assert!(
        queued[0].contains("\"maxRuntimeSeconds\":900"),
        "{}",
        queued[0]
    );

    // A due periodic trigger carries its own limit into the queued run.
    aether_core::database::query("DELETE FROM durable_tasks")
        .execute(pool)
        .await
        .unwrap();
    aether_core::database::query("UPDATE task_triggers SET max_runtime_seconds=3600,next_fire_at=0 WHERE task_kind='system' AND task_id='clean-sessions'")
        .execute(pool).await.unwrap();
    let jobs = Arc::new(ScanJobs::default());
    let now = media_contracts::protocol::unix_now();
    // The trigger alone decides: the legacy `next_due_at` column is never read to fire a run.
    assert_eq!(
        aether_core::task_scheduler::fire_due(&auth, now)
            .await
            .unwrap(),
        1
    );
    let (options,): (String,) = aether_core::database::query_as("SELECT result_json FROM durable_tasks WHERE category='system' AND target_id='clean-sessions' ORDER BY created_at DESC LIMIT 1")
        .fetch_one(pool).await.unwrap();
    assert!(options.contains("\"maxRuntimeSeconds\":3600"), "{options}");
    let next: i64 = aether_core::database::query_scalar(
        "SELECT next_due_at FROM system_task_schedules WHERE id='clean-sessions'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        next,
        now + 360 * 60,
        "the interval restarts from the firing"
    );
    jobs.shutdown().await;
    store.close().await;
}
