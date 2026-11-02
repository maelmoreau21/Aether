use media_poc::{
    database::{query, query_scalar},
    identity::IdentityStore,
    task_engine as engine,
};
use std::{collections::HashSet, time::Instant};

#[tokio::test]
async fn repeated_playback_preemption_and_pause_preserve_failure_budget() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("preemption.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let job = engine::enqueue(&store, "one", "administrator", "preempted", 100, 1000)
        .await
        .unwrap();
    let mut now = 1000;
    for _ in 0..5 {
        let running = engine::claim(&store, "local", now, 30, 1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(running.failure_count, 0);
        assert!(
            engine::finish(
                &store,
                &job.id,
                running.attempt,
                "local",
                now + 1,
                Some("playback_preempted")
            )
            .await
            .unwrap()
        );
        now += 2;
        let running = engine::claim(&store, "local", now, 30, 1)
            .await
            .unwrap()
            .unwrap();
        assert!(
            engine::control(&store, &job.id, "pause", now + 1)
                .await
                .unwrap()
        );
        assert!(
            engine::finish(
                &store,
                &job.id,
                running.attempt,
                "local",
                now + 2,
                Some("scan stopped")
            )
            .await
            .unwrap()
        );
        assert!(
            engine::control(&store, &job.id, "resume", now + 3)
                .await
                .unwrap()
        );
        now += 4;
    }
    let after = engine::get(&store, &job.id).await.unwrap().unwrap();
    assert_eq!(after.attempt, 10);
    assert_eq!(after.failure_count, 0);
    for failure in 1..=3 {
        let running = engine::claim(&store, "local", now, 30, 1)
            .await
            .unwrap()
            .unwrap();
        assert!(
            engine::finish(
                &store,
                &job.id,
                running.attempt,
                "local",
                now + 1,
                Some("actual failure")
            )
            .await
            .unwrap()
        );
        let after = engine::get(&store, &job.id).await.unwrap().unwrap();
        assert_eq!(after.failure_count, failure);
        assert_eq!(after.status, if failure < 3 { "queued" } else { "error" });
        now = query_scalar("SELECT available_at FROM durable_tasks WHERE id=?")
            .bind(&job.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    }
}

/// Queue-only simulation: no physical machines, HTTP or media decoding.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit 10,000-job / 100 simulated-node queue benchmark; no media or network"]
async fn ten_thousand_jobs_on_one_hundred_simulated_nodes() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("scale.sqlite"))
        .await
        .unwrap();
    let seeded = Instant::now();
    let mut tx = store.pool().begin().await.unwrap();
    for index in 0..100 {
        let node = format!("sim-node-{index:03}");
        let lib = format!("sim-library-{index:03}");
        query("INSERT INTO media_nodes(id,name,kind,status,last_seen_at) VALUES(?,?,'remote','connected',100)").bind(&node).bind(&node).execute(&mut *tx).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films',?,'/synthetic-read-only',1)").bind(&lib).bind(&lib).bind(&node).execute(&mut *tx).await.unwrap();
        for sequence in 0..100 {
            let id = format!("job-{index:03}-{sequence:03}");
            query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES(?,?,'benchmark',?,?,100,100,100)").bind(&id).bind(&lib).bind(&id).bind(&node).execute(&mut *tx).await.unwrap();
        }
    }
    tx.commit().await.unwrap();
    let seed_elapsed = seeded.elapsed();
    let start = Instant::now();
    let mut durations = Vec::with_capacity(10_000);
    let mut ids = HashSet::with_capacity(10_000);
    // Sequential admission isolates queue latency from mutex wait. The ordinary
    // concurrency test independently exercises racing claimers and capacity.
    for _ in 0..100 {
        for index in 0..100 {
            let node = format!("sim-node-{index:03}");
            let claim_start = Instant::now();
            let job = engine::claim(&store, &node, 100, 30, 1)
                .await
                .unwrap()
                .unwrap();
            durations.push(claim_start.elapsed().as_secs_f64() * 1000.0);
            assert!(ids.insert(job.id.clone()), "duplicate claim");
            assert!(
                engine::finish(&store, &job.id, job.attempt, &node, 101, None)
                    .await
                    .unwrap()
            );
        }
    }
    durations.sort_by(f64::total_cmp);
    let completed: i64 =
        query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE status='completed'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(completed, 10_000);
    println!(
        "SIMULATED_QUEUE nodes=100 jobs=10000 fixture_seed_ms={:.2} total_ms={:.2} throughput_jobs_s={:.2} claim_p50_ms={:.3} claim_p95_ms={:.3} claim_p99_ms={:.3} claim_max_ms={:.3}",
        seed_elapsed.as_secs_f64() * 1000.0,
        start.elapsed().as_secs_f64() * 1000.0,
        10_000.0 / start.elapsed().as_secs_f64(),
        durations[5000],
        durations[9500],
        durations[9900],
        durations[9999]
    );
    query("UPDATE durable_tasks SET status='queued',lease_until=NULL,available_at=100,attempt=0,failure_count=0").execute(store.pool()).await.unwrap();
    let ready: Vec<String> = (0..100).map(|i| format!("sim-node-{i:03}")).collect();
    let mut batches = Vec::with_capacity(100);
    let mut batch_ids = HashSet::with_capacity(10_000);
    for _ in 0..100 {
        let start = Instant::now();
        let claimed = engine::claim_batch(&store, &ready, 100, 30).await.unwrap();
        batches.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(claimed.len(), 100);
        for job in claimed {
            assert!(batch_ids.insert(job.id.clone()));
            assert!(
                engine::finish(&store, &job.id, job.attempt, &job.node_id, 101, None)
                    .await
                    .unwrap()
            );
        }
    }
    batches.sort_by(f64::total_cmp);
    println!(
        "PRODUCTION_BATCH_ADMISSION nodes=100 jobs=10000 transactions=100 batch_p50_ms={:.3} batch_p95_ms={:.3} batch_max_ms={:.3}; each timing covers admission of all 100 Nodes, fixture requeue excluded",
        batches[50], batches[95], batches[99]
    );
    println!(
        "Fixture insertion uses one transaction; execution performs 10,000 real claim transactions plus 10,000 finish statements. CPU/RSS, HTTP latency and video playback are not measured by this queue-only test."
    );
    store.close().await;
}

async fn setup(store: &IdentityStore) {
    media_poc::libraries::initialize_local(store).await.unwrap();
    for id in ["one", "two"] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic-root',1)").bind(id).bind(id).execute(store.pool()).await.unwrap();
    }
}

#[tokio::test]
async fn batch_claims_deduplicate_nodes_and_preserve_live_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("batch.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let a = engine::enqueue(&store, "two", "test", "batch-a", 10, 100)
        .await
        .unwrap();
    let b = engine::enqueue(&store, "one", "test", "batch-b", 20, 100)
        .await
        .unwrap();
    let ready = vec!["local".to_owned(), "local".to_owned()];
    let claimed = engine::claim_batch(&store, &ready, 100, 30).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, b.id);
    assert!(
        engine::claim_batch(&store, &ready, 101, 30)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        engine::claim_batch(&store, &vec!["local".to_owned(); 101], 101, 30)
            .await
            .is_err()
    );
    assert!(
        engine::finish(&store, &b.id, claimed[0].attempt, "local", 101, None)
            .await
            .unwrap()
    );
    assert_eq!(
        engine::claim_batch(&store, &ready, 102, 30).await.unwrap()[0].id,
        a.id
    );
    store.close().await;
}

#[tokio::test]
async fn leases_fence_old_workers_and_survive_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tasks.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    setup(&store).await;
    let job = engine::enqueue(&store, "one", "administrator", "same-key", 100, 1000)
        .await
        .unwrap();
    assert_eq!(
        job.id,
        engine::enqueue(&store, "one", "administrator", "same-key", 100, 1000)
            .await
            .unwrap()
            .id
    );
    assert!(matches!(
        engine::enqueue(&store, "two", "administrator", "same-key", 100, 1000).await,
        Err(engine::EngineError::Conflict)
    ));
    let first = engine::claim(&store, "local", 1000, 10, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.attempt, 1);
    assert!(
        !engine::renew(&store, &first.id, 1, "different-node", 1001, 10, 25)
            .await
            .unwrap()
    );
    assert!(
        engine::renew(&store, &first.id, 1, "local", 1001, 10, 25)
            .await
            .unwrap()
    );
    assert!(
        engine::claim(&store, "local", 1002, 10, 1)
            .await
            .unwrap()
            .is_none()
    );
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        engine::get(&store, &first.id)
            .await
            .unwrap()
            .unwrap()
            .progress,
        25
    );
    assert_eq!(engine::recover_expired(&store, 1010).await.unwrap(), 0);
    assert_eq!(engine::recover_expired(&store, 1011).await.unwrap(), 1);
    assert!(
        !engine::finish(&store, &first.id, 1, "local", 1012, None)
            .await
            .unwrap()
    );
    assert!(
        engine::claim(&store, "local", 1015, 10, 1)
            .await
            .unwrap()
            .is_none()
    );
    let second = engine::claim(&store, "local", 1016, 10, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.attempt, 2);
    assert!(
        !engine::renew(&store, &first.id, 1, "local", 1017, 10, 90)
            .await
            .unwrap()
    );
    assert!(
        engine::finish(&store, &second.id, 2, "local", 1017, None)
            .await
            .unwrap()
    );
    let done = engine::get(&store, &job.id).await.unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.progress, 100);
    assert!(
        !engine::finish(&store, &second.id, 2, "local", 1018, Some("late duplicate"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn controls_hold_capacity_until_stop_ack_and_retries_are_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("controls.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let low = engine::enqueue(&store, "one", "administrator", "low", 1, 1000)
        .await
        .unwrap();
    let high = engine::enqueue(&store, "two", "administrator", "high", 1000, 1000)
        .await
        .unwrap();
    let first = engine::claim(&store, "local", 1000, 30, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, high.id);
    assert!(
        engine::control(&store, &high.id, "pause", 1001)
            .await
            .unwrap()
    );
    assert!(
        !engine::control(&store, &high.id, "resume", 1002)
            .await
            .unwrap()
    );
    assert!(
        engine::claim(&store, "local", 1002, 30, 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        engine::finish(&store, &high.id, 1, "local", 1003, None)
            .await
            .unwrap()
    );
    assert_eq!(
        engine::get(&store, &high.id).await.unwrap().unwrap().status,
        "paused"
    );
    assert!(
        engine::control(&store, &high.id, "resume", 1004)
            .await
            .unwrap()
    );
    let resumed = engine::claim(&store, "local", 1008, 30, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.id, high.id);
    assert!(
        engine::control(&store, &high.id, "cancel", 1009)
            .await
            .unwrap()
    );
    assert!(
        engine::finish(
            &store,
            &high.id,
            resumed.attempt,
            "local",
            1010,
            Some("stopped")
        )
        .await
        .unwrap()
    );
    assert_eq!(
        engine::get(&store, &high.id).await.unwrap().unwrap().status,
        "cancelled"
    );
    let mut now = 1011;
    for attempt in 1..=3 {
        let work = engine::claim(&store, "local", now, 30, 1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(work.id, low.id);
        assert_eq!(work.attempt, attempt);
        assert!(
            engine::finish(
                &store,
                &work.id,
                attempt,
                "local",
                now + 1,
                Some("synthetic failure")
            )
            .await
            .unwrap()
        );
        now = query_scalar("SELECT available_at FROM durable_tasks WHERE id=?")
            .bind(&low.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    }
    assert_eq!(
        engine::get(&store, &low.id).await.unwrap().unwrap().status,
        "error"
    );
    assert!(
        engine::claim(&store, "local", now + 100, 30, 1)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn concurrent_claims_never_overbook_or_run_same_library() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("concurrent.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    for i in 0..10 {
        engine::enqueue(
            &store,
            "one",
            "administrator",
            &format!("task-{i}"),
            100,
            1000,
        )
        .await
        .unwrap();
    }
    let claims = futures_util::future::join_all((0..20).map(|index| {
        let store = &store;
        async move {
            if index % 2 == 0 {
                engine::claim(store, "local", 1000, 30, 10).await
            } else {
                engine::claim_batch(store, &["local".into(), "local".into()], 1000, 30)
                    .await
                    .map(|jobs| jobs.into_iter().next())
            }
        }
    }))
    .await;
    assert_eq!(
        claims
            .into_iter()
            .filter(|claim| claim.as_ref().unwrap().is_some())
            .count(),
        1
    );
    let count: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE status='running'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    query("UPDATE libraries SET enabled=0 WHERE id='one'")
        .execute(store.pool())
        .await
        .unwrap();
    engine::recover_expired(&store, 1031).await.unwrap();
    assert!(
        engine::claim(&store, "local", 1036, 30, 10)
            .await
            .unwrap()
            .is_none()
    );
}
