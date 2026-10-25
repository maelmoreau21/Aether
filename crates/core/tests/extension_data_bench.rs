//! Explicit end-to-end storage-engine benchmark; never run in ordinary suites.
use base64::Engine as _;
use media_extension_sdk::storage::{StorageOperation, StorageRequest, StorageScope};
use media_poc::{
    database::query,
    extension_data::{self, Context},
    identity::IdentityStore,
};
use serde_json::json;
use std::time::Instant;

fn request(operation: StorageOperation) -> StorageRequest {
    StorageRequest {
        api_version: 1,
        scope: StorageScope {
            kind: "global".into(),
            id: String::new(),
        },
        operation,
    }
}

async fn fixture(store: &IdentityStore) -> Context {
    query("INSERT INTO users(id,role) VALUES('storage-bench-actor','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES('storage-bench-publisher','synthetic-key',1)").execute(store.pool()).await.unwrap();
    for id in ["storage.bench.a", "storage.bench.b"] {
        let module = std::env::var("MEDIA_STORAGE_BENCH_WASM")
            .ok()
            .map(|path| std::fs::read(path).unwrap())
            .unwrap_or_else(|| wat::parse_str("(module)").unwrap());
        let package = serde_json::to_string(&media_extension_sdk::Package {
            manifest: media_extension_sdk::Manifest {
                permissions: vec!["storage.private".into()],
                ..Default::default()
            },
            publisher_id: "storage-bench-publisher".into(),
            wasm: base64::engine::general_purpose::STANDARD.encode(&module),
            sha256: String::new(),
            signature: String::new(),
        })
        .unwrap();
        println!(
            "STORAGE_BENCH_PACKAGE {}",
            json!({"extension":id,"wasmBytes":module.len(),"packageBytes":package.len()})
        );
        query("INSERT INTO extensions(id,publisher_id,package_json,generation,enabled) VALUES(?,'storage-bench-publisher',?,'bench-generation',1)")
            .bind(id).bind(package).execute(store.pool()).await.unwrap();
    }
    Context {
        extension_id: "storage.bench.a".into(),
        generation: "bench-generation".into(),
        user_id: "storage-bench-actor".into(),
        ..Default::default()
    }
}

fn stats(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    json!({"samples":n,"p50Ms":samples[n/2],"p95Ms":samples[(n*95/100).min(n-1)],"p99Ms":samples[(n*99/100).min(n-1)],"maxMs":samples[n-1]})
}

async fn exercise(store: IdentityStore, engine: &str) {
    let context = fixture(&store).await;
    let count: usize = std::env::var("MEDIA_STORAGE_BENCH_RECORDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    assert!((2..=100_000).contains(&count));
    let started = Instant::now();
    for i in 0..count {
        let mut owner = context.clone();
        if i % 2 == 1 {
            owner.extension_id = "storage.bench.b".into();
        }
        extension_data::execute(
            &store,
            &owner,
            request(StorageOperation::Put {
                key: format!("k{i:08}"),
                value: "small synthetic value".into(),
                expected_version: None,
                expires_at: None,
                durability: "durable".into(),
            }),
            1_000,
        )
        .await
        .unwrap();
        if (i + 1) % 10_000 == 0 {
            println!(
                "STORAGE_BENCH_SEED {}",
                json!({"records":i+1,"elapsedSeconds":started.elapsed().as_secs_f64()})
            );
        }
    }
    let seed_seconds = started.elapsed().as_secs_f64();
    #[cfg(feature = "query-metrics")]
    if engine.starts_with("postgres") {
        for concurrency in [1, 4] {
            let mut execution = Vec::new();
            let mut planning = Vec::new();
            let mut last_plan = Vec::new();
            let mut tasks = Vec::new();
            for worker in 0..concurrency {
                let store = store.clone();
                let context = context.clone();
                tasks.push(tokio::spawn(async move {
                    let mut plans = Vec::new();
                    for i in 0..100 {
                        let index = ((i * 7919 + worker * 13) % (count / 2)) * 2;
                        plans.push(
                            extension_data::postgres_get_plan(
                                &store,
                                &context,
                                request(StorageOperation::Get {
                                    key: format!("k{index:08}"),
                                }),
                                1_000,
                            )
                            .await
                            .unwrap(),
                        );
                    }
                    plans
                }));
            }
            for task in tasks {
                for plan in task.await.unwrap() {
                    for line in &plan {
                        if let Some(value) = line
                            .strip_prefix("Execution Time: ")
                            .and_then(|v| v.strip_suffix(" ms"))
                        {
                            execution.push(value.parse::<f64>().unwrap());
                        }
                        if let Some(value) = line
                            .strip_prefix("Planning Time: ")
                            .and_then(|v| v.strip_suffix(" ms"))
                        {
                            planning.push(value.parse::<f64>().unwrap());
                        }
                    }
                    last_plan = plan;
                }
            }
            println!(
                "STORAGE_BENCH_PG_SERVER {}",
                json!({"concurrency":concurrency,"execution":stats(execution),"planning":stats(planning),"lastPlan":last_plan,"includes":"EXPLAIN ANALYZE server SQL only on same authenticated get query; excludes network, SQLx, Rust manifest/hash validation; instrumentation adds overhead"})
            );
        }
    }
    let mut cells = Vec::new();
    for concurrency in [1, 4] {
        let statements_before = media_poc::database::statement_calls();
        let mut tasks = Vec::new();
        let started = Instant::now();
        for worker in 0..concurrency {
            let store = store.clone();
            let owner = context.clone();
            tasks.push(tokio::spawn(async move {
                let mut samples = Vec::new();
                for i in 0..2_000 {
                    let index = ((i * 7919 + worker * 13) % (count / 2)) * 2;
                    let started = Instant::now();
                    let response = extension_data::execute(
                        &store,
                        &owner,
                        request(StorageOperation::Get {
                            key: format!("k{index:08}"),
                        }),
                        1_000,
                    )
                    .await
                    .unwrap();
                    samples.push(started.elapsed().as_secs_f64() * 1000.0);
                    assert_eq!(response.entries.len(), 1);
                    assert_eq!(response.entries[0].value, "small synthetic value");
                }
                samples
            }));
        }
        let mut samples = Vec::new();
        for task in tasks {
            samples.extend(task.await.unwrap());
        }
        let statement_constructions = media_poc::database::statement_calls()
            .zip(statements_before)
            .map(|(after, before)| after - before);
        cells.push(json!({"workload":"read","concurrency":concurrency,"wallSeconds":started.elapsed().as_secs_f64(),"latency":stats(samples),"statementConstructionCalls":statement_constructions}));
    }
    for concurrency in [1, 4] {
        let mut tasks = Vec::new();
        let started = Instant::now();
        for worker in 0..concurrency {
            let store = store.clone();
            let owner = context.clone();
            tasks.push(tokio::spawn(async move {
                let key = format!("mixed-{concurrency}-{worker}");
                let mut version = None;
                let mut reads = Vec::new();
                let mut writes = Vec::new();
                for i in 0..1_000 {
                    let started = Instant::now();
                    if i % 10 == 0 {
                        let result = extension_data::execute(
                            &store,
                            &owner,
                            request(StorageOperation::Put {
                                key: key.clone(),
                                value: i.to_string(),
                                expected_version: version.clone(),
                                expires_at: None,
                                durability: "durable".into(),
                            }),
                            1_000,
                        )
                        .await
                        .unwrap();
                        writes.push(started.elapsed().as_secs_f64() * 1000.0);
                        version = Some(result.entries[0].version.clone());
                    } else {
                        let index = ((i * 7919 + worker * 13) % (count / 2)) * 2;
                        let response = extension_data::execute(
                            &store,
                            &owner,
                            request(StorageOperation::Get {
                                key: format!("k{index:08}"),
                            }),
                            1_000,
                        )
                        .await
                        .unwrap();
                        reads.push(started.elapsed().as_secs_f64() * 1000.0);
                        assert_eq!(response.entries[0].value, "small synthetic value");
                    }
                }
                (reads, writes)
            }));
        }
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        for task in tasks {
            let (r, w) = task.await.unwrap();
            reads.extend(r);
            writes.extend(w);
        }
        cells.push(json!({"workload":"mixed-90read-10cas-write","concurrency":concurrency,"wallSeconds":started.elapsed().as_secs_f64(),"reads":stats(reads),"writes":stats(writes)}));
    }
    println!(
        "STORAGE_BENCH {}",
        json!({"engine":engine,"profile":if cfg!(debug_assertions){"debug"}else{"release"},"records":count,"extensions":2,"valueBytes":21,"seedSeconds":seed_seconds,"cells":cells,"includes":"SQLx pool, live authorization, value SHA-256, decoding; excludes Wasm/worker IPC","cache":"OS/database warmed naturally; no in-memory value cache"})
    );
    store.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit 100k real-engine benchmark"]
async fn sqlite_storage_benchmark() {
    let root = tempfile::tempdir().unwrap();
    exercise(
        IdentityStore::open(&root.path().join("bench.sqlite3"))
            .await
            .unwrap(),
        "sqlite-wal-local",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires disposable MEDIA_TEST_POSTGRES_URL"]
async fn postgres_storage_benchmark() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL required");
    sqlx::any::install_default_drivers();
    let pool = sqlx::AnyPool::connect(&url).await.unwrap();
    let schema = format!("storage_bench_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut isolated = url::Url::parse(&url).unwrap();
    isolated
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    exercise(
        IdentityStore::open_postgres(isolated.as_str())
            .await
            .unwrap(),
        "postgres-loopback-docker",
    )
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}
