//! Publishing a library must not slow down with the number of unavailable works elsewhere.
//!
//! Works whose files are all unavailable (a disconnected disk, an offline node, deleted
//! files kept for history) keep `has_available_asset = 0`. The SQLite asset insert trigger
//! used to let the planner walk every such work through `inventory_works_avail_sort` for
//! each inserted asset. These tests publish a real staged scan next to synthetic offline
//! libraries of several sizes; timings are for comparison on one machine, not NAS figures.

use aether_core::{database::query, identity::IdentityStore, inventory};
use media_contracts::{
    inventory::{InventoryBatch, ScannedFile},
    media::NfoOutcome,
};
use std::time::Instant;

const PUBLISHED: usize = 1_000;

async fn seed_offline_library(store: &IdentityStore, works: usize) {
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('offline','Offline','films','local','/offline',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query(
        "WITH RECURSIVE nums(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM nums WHERE n<?) \
         INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,added_at) \
         SELECT 'off'||n,'offline','off'||n,'Offline '||n,'offline '||n,'film','{}',n FROM nums",
    )
    .bind(works as i64)
    .execute(store.pool())
    .await
    .unwrap();
    for sql in [
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works WHERE library_id='offline'",
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_works WHERE library_id='offline'",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT id,id,'offline','/offline' FROM inventory_works WHERE library_id='offline'",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT id,id,'offline',id||'.mkv',1,'0','video/x-matroska','{}',0 FROM inventory_works WHERE library_id='offline'",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
}

async fn publish_online_library(store: &IdentityStore, root: &std::path::Path) -> f64 {
    let library = "online";
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,'Online','films','local',?,1)")
        .bind(library)
        .bind(root.to_str().unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    let scan_id = uuid::Uuid::new_v4().to_string();
    for chunk in 0..PUBLISHED / 100 {
        let files = (0..100)
            .map(|j| {
                let idx = chunk * 100 + j;
                ScannedFile {
                    relative_path: format!("movie_{idx:05}/movie.mkv"),
                    identity_key: format!("online:{idx:05}"),
                    title: format!("Online {idx:05}"),
                    year: Some(2024),
                    size: 1_048_576,
                    modified_ns: "1700000000".into(),
                    mime: "video/x-matroska".into(),
                    probe: aether_core::media::Probe::default(),
                    editorial: NfoOutcome::Missing,
                    ..Default::default()
                }
            })
            .collect();
        let batch = InventoryBatch {
            scan_id: scan_id.clone().into(),
            sequence: chunk as u64,
            files,
            errors: vec![],
            warnings: vec![],
        };
        inventory::stage_batch(store, &scan_id, library, &batch)
            .await
            .unwrap();
    }
    let start = Instant::now();
    inventory::publish_staged_scan(store, &scan_id, library, root, false)
        .await
        .unwrap();
    start.elapsed().as_secs_f64() * 1000.0
}

#[tokio::test]
async fn publication_time_does_not_follow_unavailable_works() {
    let mut timings = Vec::new();
    for offline in [0usize, 5_000, 20_000] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("films");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
            .await
            .unwrap();
        seed_offline_library(&store, offline).await;
        let milliseconds = publish_online_library(&store, &root).await;
        println!(
            "publication published={PUBLISHED} unavailable_works={offline} milliseconds={milliseconds:.0}"
        );
        let (online, offline_available): (i64, i64) = sqlx::query_as(
            "SELECT \
               (SELECT COUNT(*) FROM inventory_works WHERE library_id='online' AND has_available_asset=1), \
               (SELECT COUNT(*) FROM inventory_works WHERE library_id='offline' AND has_available_asset=1)",
        )
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert_eq!(
            online, PUBLISHED as i64,
            "every published film is available"
        );
        assert_eq!(offline_available, 0, "offline works stay unavailable");
        timings.push(milliseconds);
    }
}

async fn trigger_sql(store: &IdentityStore) -> String {
    sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='inventory_assets_avail_insert'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn the_primary_key_trigger_is_part_of_the_schema_and_survives_a_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("core.sqlite3");
    let store = IdentityStore::open(&path).await.unwrap();
    assert!(
        trigger_sql(&store)
            .await
            .contains("+has_available_asset = 0")
    );
    store.pool().close().await;
    drop(store);

    let store = IdentityStore::open(&path).await.unwrap();
    assert!(
        trigger_sql(&store)
            .await
            .contains("+has_available_asset = 0")
    );
}
