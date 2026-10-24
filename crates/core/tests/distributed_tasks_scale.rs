//! In-process HTTP responsiveness under durable-queue contention. Simulated nodes
//! execute no media: these measurements do not establish video playback stability.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::get,
};
use http_body_util::BodyExt;
use media_poc::{
    auth::{self, AuthState},
    database::{query, query_scalar},
    identity::IdentityStore,
    task_engine as engine,
};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Instant};
use tower::ServiceExt;
use zeroize::Zeroizing;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .header("x-csrf-token", csrf)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn timings(name: &str, values: &mut [f64]) -> Value {
    assert!(!values.is_empty());
    values.sort_by(f64::total_cmp);
    json!({"operation":name,"samples":values.len(),"p50Ms":values[values.len()/2],"p95Ms":values[(values.len()*95/100).min(values.len()-1)],"maxMs":values[values.len()-1]})
}

#[cfg(windows)]
fn process_usage() -> (Option<f64>, Option<usize>) {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    struct Memory {
        size: u32,
        faults: u32,
        peak: usize,
        rss: usize,
        paged_peak: usize,
        paged: usize,
        nonpaged_peak: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            process: *mut std::ffi::c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            memory: *mut Memory,
            size: u32,
        ) -> i32;
    }
    // Windows fills these fixed-size process-counter structs; no pointers escape.
    unsafe {
        let mut created = FileTime::default();
        let mut exited = FileTime::default();
        let mut kernel = FileTime::default();
        let mut user = FileTime::default();
        let ticks = |time: &FileTime| ((u64::from(time.high) << 32) | u64::from(time.low)) as f64;
        let cpu = (GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) != 0)
            .then(|| (ticks(&kernel) + ticks(&user)) / 10_000.0);
        let mut memory: Memory = std::mem::zeroed();
        memory.size = std::mem::size_of::<Memory>() as u32;
        let memory_size = memory.size;
        let rss = (K32GetProcessMemoryInfo(GetCurrentProcess(), &mut memory, memory_size) != 0)
            .then_some(memory.rss);
        (cpu, rss)
    }
}
#[cfg(not(windows))]
fn process_usage() -> (Option<f64>, Option<usize>) {
    (None, None)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit 10k-job backlog, 100 simulated nodes and concurrent in-process admin HTTP benchmark"]
async fn backlog_and_one_hundred_claimers_preserve_admin_http_responsiveness() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("contention.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    store
        .bootstrap(
            "scale-admin",
            Zeroizing::new("synthetic queue load password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = engine::router(auth.clone())
        .merge(auth::router(auth))
        .route("/synthetic-health", get(|| async { StatusCode::OK }));
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"scale-admin","password":"synthetic queue load password"}),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login: Value =
        serde_json::from_slice(&login.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let csrf = login["csrfToken"].as_str().unwrap().to_owned();

    let seed_start = Instant::now();
    let mut tx = store.pool().begin().await.unwrap();
    for node in 0..100 {
        let node_id = format!("sim-node-{node:03}");
        let library = format!("sim-library-{node:03}");
        query("INSERT INTO media_nodes(id,name,kind,status,last_seen_at) VALUES(?,?,'remote','connected',100)")
            .bind(&node_id).bind(&node_id).execute(&mut *tx).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films',?,'/synthetic-read-only',1)")
            .bind(&library).bind(&library).bind(&node_id).execute(&mut *tx).await.unwrap();
        for sequence in 0..100 {
            let id = format!("fixture-{node:03}-{sequence:03}");
            query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES(?,?,'fixture',?,?,100,100,100)")
                .bind(&id).bind(&library).bind(&id).bind(&node_id).execute(&mut *tx).await.unwrap();
        }
    }
    tx.commit().await.unwrap();
    let seed_ms = seed_start.elapsed().as_secs_f64() * 1000.0;

    let (cpu_before, rss_before) = process_usage();
    let wall = Instant::now();
    let mut workers = tokio::task::JoinSet::new();
    for node in 0..100 {
        let store = store.clone();
        workers.spawn(async move {
            let node_id = format!("sim-node-{node:03}");
            let mut claimed = Vec::with_capacity(10);
            for _ in 0..10 {
                let start = Instant::now();
                let job = engine::claim(&store, &node_id, 100, 300, 1)
                    .await
                    .unwrap()
                    .unwrap();
                let latency = start.elapsed().as_secs_f64() * 1000.0;
                assert_eq!(job.node_id, node_id);
                assert!(
                    engine::finish(&store, &job.id, job.attempt, &node_id, 101, None)
                        .await
                        .unwrap()
                );
                claimed.push((job.id, latency));
            }
            claimed
        });
    }
    let writer_app = app.clone();
    let writer_cookie = cookie.clone();
    let writer_csrf = csrf.clone();
    let writer = tokio::spawn(async move {
        let mut samples = Vec::with_capacity(100);
        for index in 0..100 {
            let start = Instant::now();
            let response = call(&writer_app, "POST", "/api/v1/admin/tasks/jobs", &writer_cookie, &writer_csrf,
                json!({"libraryId":format!("sim-library-{index:03}"),"idempotencyKey":format!("http-{index}"),"priority":100})).await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let _: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples
    });
    let reader = tokio::spawn(async move {
        let mut navigation = Vec::with_capacity(100);
        let mut health = Vec::with_capacity(100);
        for _ in 0..100 {
            let start = Instant::now();
            let response = call(
                &app,
                "GET",
                "/api/v1/admin/tasks/jobs?limit=50&offset=0",
                &cookie,
                &csrf,
                Value::Null,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(body["jobs"].as_array().unwrap().len(), 50);
            navigation.push(start.elapsed().as_secs_f64() * 1000.0);
            let start = Instant::now();
            assert_eq!(
                call(&app, "GET", "/synthetic-health", "", "", Value::Null)
                    .await
                    .status(),
                StatusCode::OK
            );
            health.push(start.elapsed().as_secs_f64() * 1000.0);
            tokio::task::yield_now().await;
        }
        (navigation, health)
    });
    let mut ids = HashSet::with_capacity(1000);
    let mut claims = Vec::with_capacity(1000);
    while let Some(worker) = workers.join_next().await {
        for (id, latency) in worker.unwrap() {
            assert!(ids.insert(id), "duplicate claim");
            claims.push(latency);
        }
    }
    let mut enqueues = writer.await.unwrap();
    let (mut navigation, mut health) = reader.await.unwrap();
    let elapsed = wall.elapsed().as_secs_f64();
    let (cpu_after, rss_after) = process_usage();
    let completed: i64 =
        query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE status='completed'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(completed, 1000);
    assert_eq!(ids.len(), 1000);
    let total: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(total, 10_100);
    println!(
        "{}",
        json!({"benchmark":"SIMULATED_QUEUE_HTTP_CONTENTION","simulatedNodes":100,"fixtureJobs":10000,"fixtureSeedMs":seed_ms,"httpEnqueues":100,"completedJobs":1000,"wallMs":elapsed*1000.0,"completedJobsPerSecond":1000.0/elapsed,"processCpuMs":cpu_after.zip(cpu_before).map(|(after,before)|after-before),"rssBeforeBytes":rss_before,"rssAfterBytes":rss_after,"measurements":[timings("claim_under_contention", &mut claims),timings("authenticated_http_enqueue", &mut enqueues),timings("authenticated_admin_page_50", &mut navigation),timings("synthetic_health_no_database", &mut health)]})
    );
    println!(
        "In-process Axum requests, SQLite, 100 simulated node identities; no sockets, scan work, media, browser, NAS or video stability measured. SQL access counts are not instrumented; 1000 actual claim transactions, 1000 finishes and 100 authenticated HTTP enqueues were executed; seed cost is reported separately."
    );
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    let persisted: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(persisted, 10_100);
    reopened.close().await;
}

/// Unlike the fixture-seeded contention test above, every one of these 10,000
/// admissions executes the real validation, idempotency, capacity and commit path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit 10,000 real durable admissions with authenticated HTTP navigation throughout"]
async fn ten_thousand_real_admissions_keep_authenticated_navigation_served() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("admissions.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    store
        .bootstrap(
            "scale-admin",
            Zeroizing::new("synthetic queue load password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = engine::router(auth.clone()).merge(auth::router(auth));
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"scale-admin","password":"synthetic queue load password"}),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login: Value =
        serde_json::from_slice(&login.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let csrf = login["csrfToken"].as_str().unwrap().to_owned();
    let setup = Instant::now();
    let mut tx = store.pool().begin().await.unwrap();
    for node in 0..100 {
        let id = format!("admission-node-{node:03}");
        let library = format!("admission-library-{node:03}");
        query("INSERT INTO media_nodes(id,name,kind,status,last_seen_at) VALUES(?,?,'remote','connected',100)")
            .bind(&id).bind(&id).execute(&mut *tx).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films',?,'/synthetic-read-only',1)")
            .bind(&library).bind(&library).bind(&id).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let setup_ms = setup.elapsed().as_secs_f64() * 1000.0;
    let mut baseline = Vec::with_capacity(20);
    for _ in 0..20 {
        let start = Instant::now();
        let response = call(
            &app,
            "GET",
            "/api/v1/admin/tasks/jobs?limit=50&offset=0",
            &cookie,
            &csrf,
            Value::Null,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let _: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        baseline.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let finished = Arc::new(AtomicBool::new(false));
    let reader_finished = finished.clone();
    let reader = tokio::spawn(async move {
        let mut samples = Vec::new();
        while !reader_finished.load(Ordering::Acquire) {
            let start = Instant::now();
            let response = call(
                &app,
                "GET",
                "/api/v1/admin/tasks/jobs?limit=50&offset=0",
                &cookie,
                &csrf,
                Value::Null,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert!(body["jobs"].as_array().unwrap().len() <= 50);
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        samples
    });
    let (cpu_before, rss_before) = process_usage();
    let wall = Instant::now();
    let mut admission = Vec::with_capacity(10_000);
    for index in 0..10_000 {
        let start = Instant::now();
        let job = engine::enqueue(
            &store,
            &format!("admission-library-{:03}", index % 100),
            "admission-benchmark",
            &format!("request-{index}"),
            100,
            100,
        )
        .await
        .unwrap();
        assert_eq!(job.status, "queued");
        admission.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let duration = wall.elapsed().as_secs_f64();
    finished.store(true, Ordering::Release);
    let mut navigation = reader.await.unwrap();
    let (cpu_after, rss_after) = process_usage();
    let admitted: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE status='queued'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(admitted, 10_000);
    println!(
        "{}",
        json!({"benchmark":"REAL_10000_ADMISSIONS_WITH_ADMIN_HTTP","simulatedNodes":100,"actualDurableAdmissions":admitted,"nodeLibrarySetupMs":setup_ms,"admissionDurationMs":duration*1000.0,"admissionsPerSecond":10_000.0/duration,"processCpuMs":cpu_after.zip(cpu_before).map(|(after,before)|after-before),"rssBeforeBytes":rss_before,"rssAfterBytes":rss_after,"measurements":[timings("real_enqueue", &mut admission),timings("authenticated_admin_empty_baseline", &mut baseline),timings("authenticated_admin_during_all_admissions", &mut navigation)]})
    );
    println!(
        "All 10,000 jobs use engine::enqueue including per-job validation, idempotency, pending-capacity check and durable commit; no direct task seeding. HTTP navigation runs throughout admission in the same Tokio runtime. SQL statement counts are not instrumented. No sockets, real scans, physical nodes or media decoding measured."
    );
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    let persisted: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(persisted, 10_000);
    reopened.close().await;
}
