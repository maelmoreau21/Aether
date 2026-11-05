use super::*;
use crate::{
    database::{query_as, query_scalar},
    inventory,
};
use media_contracts::inventory::{InventoryBatch, ScannedFile};

async fn publish(store: &IdentityStore, scan: &str, modified: &str, reused: bool) {
    inventory::stage_batch(store, scan, "previews", &InventoryBatch {
        scan_id: scan.into(), sequence: 0,
        files: vec![ScannedFile {
            relative_path: "Film (2026).mp4".into(), title: "Film".into(),
            identity_key: "film".into(), size: 100, modified_ns: modified.into(),
            mime: "video/mp4".into(), reused,
            probe: serde_json::from_value(json!({"format":{"duration":"120"},"streams":[{"index":0,"codec_type":"video","codec_name":"av1"}]})).unwrap(),
            ..Default::default()
        }], errors: vec![], warnings: vec![],
    }).await.unwrap();
    inventory::publish_staged_scan(store, scan, "previews", Path::new("/read-only"), true)
        .await
        .unwrap();
}

#[tokio::test]
async fn publication_queues_background_previews_durably_once_per_asset_revision() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("previews.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('previews','Previews','films','local','/read-only',1)").execute(store.pool()).await.unwrap();
    publish(&store, "first", "1", false).await;
    let jobs: Vec<(String, String, i64, i64, String)> = query_as("SELECT category,status,priority,width,target_id FROM durable_tasks WHERE owner='library-trickplay'")
        .fetch_all(store.pool()).await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        (&jobs[0].0, &jobs[0].1, jobs[0].2, jobs[0].3),
        (&"trickplay".into(), &"queued".into(), 1, 320)
    );
    let asset = jobs[0].4.clone();
    publish(&store, "replay", "1", false).await;
    publish(&store, "cached-probe", "1", true).await;
    let count: i64 =
        query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
    publish(&store, "changed", "2", false).await;
    let count: i64 =
        query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        query_scalar::<sqlx::Any, String>(
            "SELECT id FROM inventory_assets WHERE relative_path='Film (2026).mp4'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        asset
    );
    query("UPDATE libraries SET options_json='{\"trickplayWidth\":410,\"trickplayIntervalSeconds\":5,\"trickplayQuality\":12}' WHERE id='previews'").execute(store.pool()).await.unwrap();
    publish(&store, "new-profile-with-cached-probe", "2", true).await;
    assert_eq!(
        query_scalar::<sqlx::Any, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay' AND width=410"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
    query("UPDATE libraries SET options_json='{\"trickplayEnabled\":false}' WHERE id='previews'")
        .execute(store.pool())
        .await
        .unwrap();
    publish(&store, "opt-out", "3", false).await;
    let count: i64 =
        query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(count, 3);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        query_scalar::<sqlx::Any, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay' AND status='queued'"
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap(),
        3
    );
}

#[tokio::test]
async fn queued_trickplay_checks_opt_out_again_before_dispatch_and_can_resume() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("disable.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('previews','Previews','films','local','/read-only',1)").execute(store.pool()).await.unwrap();
    publish(&store, "enabled", "1", false).await;
    let now = media_contracts::protocol::unix_now();
    let job = crate::task_engine::claim(&store, "local", now, 60, 1)
        .await
        .unwrap()
        .unwrap();
    query(
        "UPDATE libraries SET options_json='{\"trickplayDuringScan\":false}' WHERE id='previews'",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let auth = crate::auth::AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = std::sync::Arc::new(crate::scans::ScanJobs::default());
    crate::task_execution::dispatch(&auth, temp.path(), &jobs, &job)
        .await
        .unwrap();
    let cancelled = crate::task_engine::get(&store, &job.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(cancelled.error, "library_trickplay_disabled");
    query("UPDATE libraries SET options_json='{}' WHERE id='previews'")
        .execute(store.pool())
        .await
        .unwrap();
    publish(&store, "re-enabled", "1", true).await;
    let resumed = crate::task_engine::get(&store, &job.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.status, "queued");
    assert!(resumed.error.is_empty());
    assert_eq!(
        query_scalar::<sqlx::Any, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE owner='library-trickplay'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn library_switch_alone_controls_manifest_and_tiles() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("policy.sqlite"))
        .await
        .unwrap();
    for statement in [
        "INSERT INTO media_nodes(id,name,kind,status) VALUES('preview-node','Fixture','remote','connected')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('previews','Previews','films','preview-node','/read-only',1)",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('preview-work','previews','preview-work','Preview','{}')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('preview-edition','preview-work')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('preview-version','preview-edition')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('preview-source','preview-version','previews','/read-only')",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('preview-asset','preview-source','previews','film.mp4',100,'1','video/mp4','{\"format\":{\"duration\":\"120\"}}',1)",
    ] {
        query(statement).execute(store.pool()).await.unwrap();
    }
    let legacy = crate::server_settings::Settings {
        trickplay_enabled: false,
        ..Default::default()
    };
    query("INSERT INTO server_settings(singleton,settings_json) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET settings_json=excluded.settings_json")
        .bind(serde_json::to_string(&legacy).unwrap()).execute(store.pool()).await.unwrap();
    let request = || Request::new(Body::empty());
    // No configured transport exists on this fixture: reaching the relay proves
    // the legacy global false no longer vetoes the library's enabled policy.
    assert_eq!(
        manifest(&store, "preview-asset", request()).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    query("UPDATE libraries SET options_json='{\"trickplayEnabled\":false}' WHERE id='previews'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        manifest(&store, "preview-asset", request()).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        tile(&store, "preview-asset", 0, request()).await.status(),
        StatusCode::NOT_FOUND
    );
}
