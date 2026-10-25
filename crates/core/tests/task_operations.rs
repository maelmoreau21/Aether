//! Real standalone scanner and confined executors; no fake Node or synthetic HTTP result.
use media_poc::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::IdentityStore,
    scans::ScanJobs,
    scheduler,
    task_engine::{self, Job},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc, time::Duration};

async fn complete(
    store: &IdentityStore,
    auth: &AuthState,
    root: &Path,
    jobs: &Arc<ScanJobs>,
    id: &str,
) -> Job {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            scheduler::dispatch_once(auth, root, jobs, media_contracts::protocol::unix_now())
                .await
                .unwrap();
            let job = task_engine::get(store, id).await.unwrap().unwrap();
            match job.status.as_str() {
                "completed" => return job,
                "error" | "cancelled" => panic!("{} failed: {}", job.category, job.error),
                _ => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        }
    })
    .await
    .expect("real standalone task did not complete within 30 seconds")
}

#[tokio::test]
async fn standalone_scans_then_executes_real_analysis_preparation_and_image_jobs() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    let film = media.join("Task Fixture (2026)");
    tokio::fs::create_dir_all(&film).await.unwrap();
    let root = tokio::fs::canonicalize(&media).await.unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/representative/representative.mp4");
    let movie = film.join("Task Fixture (2026).mp4");
    tokio::fs::copy(&fixture, &movie)
        .await
        .expect("generate the legal representative fixture first");
    let poster = film.join("poster.png");
    image::RgbImage::from_pixel(640, 960, image::Rgb([28, 72, 48]))
        .save(&poster)
        .unwrap();
    let before_movie = Sha256::digest(tokio::fs::read(&movie).await.unwrap());
    let before_poster = Sha256::digest(tokio::fs::read(&poster).await.unwrap());
    let path = temp.path().join("tasks.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('tasks','Task fixtures','films','local',?,1)")
        .bind(root.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = Arc::new(ScanJobs::default());
    let scan = task_engine::enqueue(
        &store,
        "tasks",
        "test",
        "real-scan",
        100,
        media_contracts::protocol::unix_now(),
    )
    .await
    .unwrap();
    let scanned = complete(&store, &auth, &root, &jobs, &scan.id).await;
    assert_eq!(scanned.failure_count, 0, "{}", scanned.error);
    assert!(scanned.processed_items > 0);
    let asset: String =
        query_scalar("SELECT id FROM inventory_assets WHERE library_id='tasks' AND available=1")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let image: String =
        query_scalar("SELECT id FROM inventory_images WHERE library_id='tasks' AND available=1")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let analyze = task_engine::enqueue_task(
        &store,
        "tasks",
        "test",
        "real-analysis",
        100,
        media_contracts::protocol::unix_now(),
        "analyze",
        &asset,
        "",
        640,
    )
    .await
    .unwrap();
    let analyzed = complete(&store, &auth, &root, &jobs, &analyze.id).await;
    let probe: Value = serde_json::from_str(&analyzed.result_json).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert!(
        streams
            .iter()
            .any(|track| track["codec_type"] == "video" && track["codec_name"] == "av1"),
        "{probe}"
    );
    let audio = streams
        .iter()
        .find(|track| track["codec_type"] == "audio" && track["codec_name"] == "opus")
        .expect("fixture must contain actual Opus audio");
    let component = format!("audio-{}", audio["index"].as_u64().unwrap());
    let prepare = task_engine::enqueue_task(
        &store,
        "tasks",
        "test",
        "real-preparation",
        100,
        media_contracts::protocol::unix_now(),
        "prepare",
        &asset,
        &component,
        640,
    )
    .await
    .unwrap();
    let prepared = complete(&store, &auth, &root, &jobs, &prepare.id).await;
    let prepared_result: Value = serde_json::from_str(&prepared.result_json).unwrap();
    assert_eq!(prepared_result["cacheWarm"], true);
    assert_eq!(prepared_result["component"], component);
    assert_eq!(prepared_result["mime"], "audio/ogg");
    assert!(prepared_result["size"].as_u64().unwrap() > 0);
    let image_job = task_engine::enqueue_task(
        &store,
        "tasks",
        "test",
        "real-image",
        100,
        media_contracts::protocol::unix_now(),
        "image",
        &image,
        "",
        320,
    )
    .await
    .unwrap();
    let rendered = complete(&store, &auth, &root, &jobs, &image_job.id).await;
    let image_result: Value = serde_json::from_str(&rendered.result_json).unwrap();
    assert_eq!(image_result["width"], 320);
    assert_eq!(image_result["cacheWarm"], true);
    assert!(image_result["size"].as_u64().unwrap() > 0);
    assert_eq!(
        before_movie,
        Sha256::digest(tokio::fs::read(&movie).await.unwrap())
    );
    assert_eq!(
        before_poster,
        Sha256::digest(tokio::fs::read(&poster).await.unwrap())
    );
    jobs.shutdown().await;
    drop(jobs);
    drop(auth);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    for completed in [analyzed, prepared, rendered] {
        let durable = task_engine::get(&reopened, &completed.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(durable.status, "completed");
        assert_eq!(durable.result_json, completed.result_json);
        assert_eq!(durable.failure_count, 0);
    }
    reopened.close().await;
}
