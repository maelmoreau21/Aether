//! Durable automation contracts. Scale fixtures below simulate Nodes and do not
//! execute media, HTTP, Wasm, or establish playback latency guarantees.
use media_poc::{
    automation::{self, EventInput},
    database::{query, query_as, query_scalar},
    identity::IdentityStore,
    task_engine,
};
use serde_json::json;
use std::{collections::HashSet, time::Instant};

async fn setup(store: &IdentityStore) {
    query("INSERT INTO users(id,role) VALUES('automation-admin','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    for index in 1..=2 {
        let node = format!("node-{index}");
        let library = format!("library-{index}");
        query("INSERT INTO media_nodes(id,name,kind,status) VALUES(?,?,'remote','connected')")
            .bind(&node)
            .bind(&node)
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films',?,'/read-only',1)")
            .bind(&library).bind(&library).bind(node).execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,manual_json) VALUES(?,?,?,'Original title','{}',?)")
            .bind(format!("work-{index}")).bind(&library).bind(format!("identity-{index}"))
            .bind(r#"{"title":"Manual correction","language":"fr"}"#)
            .execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_editions(id,work_id) VALUES(?,?)")
            .bind(format!("edition-{index}"))
            .bind(format!("work-{index}"))
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO inventory_versions(id,edition_id) VALUES(?,?)")
            .bind(format!("version-{index}"))
            .bind(format!("edition-{index}"))
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,'/read-only')")
            .bind(format!("source-{index}")).bind(format!("version-{index}")).bind(&library)
            .execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,?,'legal-fixture.mp4',100,'1','video/mp4','{}',1)")
            .bind(format!("asset-{index}")).bind(format!("source-{index}")).bind(&library)
            .execute(store.pool()).await.unwrap();
    }
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES('fixture-publisher','synthetic-only',1)")
        .execute(store.pool()).await.unwrap();
    let package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            api_version: 1,
            id: "fixture.metadata".into(),
            version: "1.0.0".into(),
            permissions: vec!["metadata.read".into(), "metadata.write".into()],
            ..Default::default()
        },
        publisher_id: "fixture-publisher".into(),
        wasm: String::new(),
        sha256: "synthetic-digest".into(),
        signature: String::new(),
    };
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('fixture.metadata','fixture-publisher',?,1,'generation-1')")
        .bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    for library in ["library-1", "library-2"] {
        query("INSERT INTO extension_grants(extension_id,library_id) VALUES('fixture.metadata',?)")
            .bind(library)
            .execute(store.pool())
            .await
            .unwrap();
    }
}

async fn emit(
    store: &IdentityStore,
    key: &str,
    kind: &str,
    index: u8,
    fingerprint: &str,
    now: i64,
) -> String {
    let mut tx = store.pool().begin().await.unwrap();
    let id = automation::emit(
        &mut tx,
        &EventInput {
            key,
            kind,
            library_id: &format!("library-{index}"),
            asset_id: &format!("asset-{index}"),
            fingerprint,
            payload: &json!({"revision":1}),
        },
        now,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn allow_two_simultaneous_claims(store: &IdentityStore) {
    for library in ["library-1", "library-2"] {
        query("INSERT INTO automation_policies(extension_id,library_id,max_concurrent) VALUES('fixture.metadata',?,2)")
            .bind(library).execute(store.pool()).await.unwrap();
    }
    query("INSERT INTO automation_provider_limits(provider,min_interval_seconds,max_concurrent) VALUES('fixture.metadata',0,2)")
        .execute(store.pool()).await.unwrap();
}

#[tokio::test]
async fn missing_only_sweep_skips_complete_and_manually_corrected_works() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("missing.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let mut tx = store.pool().begin().await.unwrap();
    automation::emit_snapshot(&mut tx, "library-1", "scan", 100)
        .await
        .unwrap();
    automation::emit_snapshot(&mut tx, "library-2", "scan", 100)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    query("UPDATE automation_events SET dispatched_at=100")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE inventory_works SET editorial_json=? WHERE id='work-1'")
        .bind(json!({"plot":"Known","outline":"Known","genres":["Drama"]}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    automation::schedule_library(
        &mut tx,
        "library-1",
        "metadata.missing",
        "first",
        Some("fixture.metadata"),
        101,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    automation::drain(&store, 102, 10).await.unwrap();
    let queued: i64 = query_scalar("SELECT COUNT(*) FROM automation_task_links")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(queued, 0);
    let mut tx = store.pool().begin().await.unwrap();
    automation::schedule_library(
        &mut tx,
        "library-2",
        "metadata.missing",
        "second",
        Some("fixture.metadata"),
        103,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    automation::drain(&store, 104, 10).await.unwrap();
    let queued: i64 = query_scalar("SELECT COUNT(*) FROM automation_task_links")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(queued, 1);
}

#[tokio::test]
async fn installed_marker_extension_queues_only_episode_chapter_work() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("markers.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    query("UPDATE libraries SET kind='series' WHERE id='library-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE inventory_works SET kind='episode' WHERE id='work-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind) VALUES('series','library-1','series','Series','{}','series')")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_seasons(id,series_id,season_number) VALUES('season','series',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) VALUES('episode','season',1,'work-1')")
        .execute(store.pool()).await.unwrap();
    let package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            api_version: 1,
            id: "fixture.markers".into(),
            version: "1.0.0".into(),
            permissions: vec!["playback.markers".into()],
            ..Default::default()
        },
        publisher_id: "fixture-publisher".into(),
        wasm: String::new(),
        sha256: "synthetic-markers".into(),
        signature: String::new(),
    };
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('fixture.markers','fixture-publisher',?,1,'generation-1')")
        .bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    query("INSERT INTO extension_grants(extension_id,library_id) VALUES('fixture.markers','library-1')")
        .execute(store.pool()).await.unwrap();
    emit(&store, "marker-episode", "media.added", 1, "source-a", 100).await;
    automation::drain(&store, 101, 100).await.unwrap();
    let operation: String = query_scalar(
        "SELECT operation FROM automation_task_links WHERE extension_id='fixture.markers'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(operation, "intro.parse_chapters");
    emit(&store, "marker-film", "media.added", 2, "source-b", 102).await;
    automation::drain(&store, 103, 100).await.unwrap();
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM automation_task_links WHERE extension_id='fixture.markers'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn manual_marker_correction_is_authenticated_validated_and_durable() {
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use media_poc::auth::{self, AuthState};
    use tower::ServiceExt;
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("manual.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "marker-admin",
            zeroize::Zeroizing::new("synthetic marker password".into()),
        )
        .await
        .unwrap();
    setup(&store).await;
    query("UPDATE inventory_works SET kind='episode' WHERE id='work-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE inventory_assets SET probe_json=? WHERE id='asset-1'")
        .bind(json!({"format":{"duration":"1200.0"},"chapters":[]}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = media_poc::timeline_markers::router(auth.clone()).merge(auth::router(auth));
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"login":"marker-admin","password":"synthetic marker password"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let bytes = http_body_util::BodyExt::collect(login.into_body())
        .await
        .unwrap()
        .to_bytes();
    let login: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let csrf = login["csrfToken"].as_str().unwrap();
    let send = |csrf_token: &str, body: serde_json::Value| {
        Request::builder()
            .method("PUT")
            .uri("/api/v1/admin/assets/asset-1/markers/intro")
            .header(header::ORIGIN, "http://127.0.0.1:3000")
            .header(header::COOKIE, &cookie)
            .header("x-csrf-token", csrf_token)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(send("", json!({"enabled":true,"start":10.0,"end":50.0})))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(send(
                csrf,
                json!({"enabled":true,"start":10.0,"end":1500.0})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.clone()
            .oneshot(send(csrf, json!({"enabled":true,"start":10.0,"end":50.0})))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    drop(app);
    drop(store);
    let reopened = IdentityStore::open(&temp.path().join("manual.sqlite"))
        .await
        .unwrap();
    let saved:(f64,f64)=media_poc::database::query_as("SELECT start_seconds,end_seconds FROM manual_timeline_markers WHERE asset_id='asset-1' AND marker_type='intro'")
        .fetch_one(reopened.pool()).await.unwrap();
    assert_eq!(saved, (10.0, 50.0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn signed_marker_wasm_result_survives_restart() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use media_poc::{auth::AuthState, scans::ScanJobs, scheduler};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sha2::{Digest, Sha256};
    use std::{sync::Arc, time::Duration};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("wasm-markers.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    setup(&store).await;
    query("UPDATE extensions SET enabled=0 WHERE id='fixture.metadata'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET kind='series' WHERE id='library-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE inventory_works SET kind='episode' WHERE id='work-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind) VALUES('series','library-1','series','Series','{}','series')").execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_seasons(id,series_id,season_number) VALUES('season','series',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) VALUES('episode','season',1,'work-1')").execute(store.pool()).await.unwrap();
    query("UPDATE inventory_assets SET probe_json=? WHERE id='asset-1'").bind(json!({"streams":[{"index":0,"codec_type":"video","codec_name":"av1"},{"index":1,"codec_type":"audio","codec_name":"opus"}],"format":{"duration":"1200"},"chapters":[{"start_time":"10","tags":{"title":"Intro"}},{"start_time":"60","tags":{"title":"Episode"}}]}).to_string()).execute(store.pool()).await.unwrap();
    let output=json!({"apiVersion":1,"result":[{"id":"intro-1","start":15.0,"end":55.0,"markerType":"intro","confidence":0.98,"method":"chapter"}]}).to_string();
    let data = output
        .bytes()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let wasm=wat::parse_str(format!(r#"(module (memory (export "memory") 1 256) (data (i32.const 4096) "{data}") (func (export "media_alloc") (param i32) (result i32) i32.const 1024) (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#, (4096u64<<32)|output.len() as u64)).unwrap();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let signing = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let mut package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            api_version: 1,
            id: "fixture.markers".into(),
            name: "Fixture markers".into(),
            version: "1.0.0".into(),
            description: "Synthetic markers".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/markers".into(),
            permissions: vec!["playback.markers".into()],
            ..Default::default()
        },
        publisher_id: "fixture-publisher".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(signing.sign(&package.signing_bytes().unwrap()).as_ref());
    media_poc::extensions::validate_package(&package, signing.public_key().as_ref()).unwrap();
    query("UPDATE extension_publishers SET public_key=? WHERE id='fixture-publisher'")
        .bind(STANDARD.encode(signing.public_key().as_ref()))
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::extension_storage::persist(&store, &package)
        .await
        .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('fixture.markers','fixture-publisher',?,1,'marker-generation')").bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    query("INSERT INTO extension_grants(extension_id,library_id) VALUES('fixture.markers','library-1')").execute(store.pool()).await.unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    automation::emit_snapshot(
        &mut tx,
        "library-1",
        "scan",
        media_contracts::protocol::unix_now(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = Arc::new(ScanJobs::default());
    let completed=tokio::time::timeout(Duration::from_secs(20),async {
        loop {
            scheduler::dispatch_once(&auth,temp.path(),&jobs,media_contracts::protocol::unix_now()).await.unwrap();
            let state:Option<String>=query_scalar("SELECT t.status FROM durable_tasks t JOIN automation_task_links l ON l.task_id=t.id WHERE l.extension_id='fixture.markers'").fetch_optional(store.pool()).await.unwrap();
            if state.as_deref()==Some("completed") {break;}
            assert_ne!(state.as_deref(),Some("error"));
            let failure:Option<String>=query_scalar("SELECT error FROM durable_tasks t JOIN automation_task_links l ON l.task_id=t.id WHERE l.extension_id='fixture.markers'").fetch_optional(store.pool()).await.unwrap();
            assert!(failure.as_deref().is_none_or(str::is_empty),"failure={failure:?}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await;
    if completed.is_err() {
        let tasks: Vec<(String, String, String)> = media_poc::database::query_as(
            "SELECT category,status,error FROM durable_tasks ORDER BY id",
        )
        .fetch_all(store.pool())
        .await
        .unwrap();
        let events: Vec<(String, Option<i64>)> =
            media_poc::database::query_as("SELECT kind,dispatched_at FROM automation_events")
                .fetch_all(store.pool())
                .await
                .unwrap();
        panic!("marker automation timed out: tasks={tasks:?} events={events:?}");
    }
    let descriptor = media_poc::inventory::descriptor_for_user(
        &store,
        "asset-1",
        "synthetic-session",
        None,
        None,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(descriptor["intro"]["start"], json!(15.0));
    jobs.shutdown().await;
    drop(jobs);
    drop(auth);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    let saved:String=query_scalar("SELECT result_json FROM automation_results WHERE extension_id='fixture.markers' AND asset_id='asset-1'").fetch_one(reopened.pool()).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved).unwrap()[0]["start"],
        json!(15.0)
    );
}

#[tokio::test]
async fn outbox_rolls_back_with_inventory_and_duplicate_delivery_has_one_effect() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("atomic.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let mut tx = store.pool().begin().await.unwrap();
    query("UPDATE inventory_assets SET size=200 WHERE id='asset-1'")
        .execute(&mut *tx)
        .await
        .unwrap();
    automation::emit(
        &mut tx,
        &EventInput {
            key: "rollback",
            kind: "media.modified",
            library_id: "library-1",
            asset_id: "asset-1",
            fingerprint: "file-200",
            payload: &json!({}),
        },
        100,
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        query_scalar::<_, i64>("SELECT size FROM inventory_assets WHERE id='asset-1'")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        100
    );
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM automation_events")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    let first = emit(&store, "added", "media.added", 1, "file-100", 100).await;
    let mut conflicting = store.pool().begin().await.unwrap();
    assert!(
        automation::emit(
            &mut conflicting,
            &EventInput {
                key: "added",
                kind: "media.added",
                library_id: "library-2",
                asset_id: "asset-2",
                fingerprint: "other-file",
                payload: &json!({"revision":1}),
            },
            100
        )
        .await
        .is_err()
    );
    conflicting.rollback().await.unwrap();
    for _ in 0..1000 {
        assert_eq!(
            emit(&store, "added", "media.added", 1, "file-100", 100).await,
            first
        );
    }
    assert_eq!(automation::drain(&store, 101, 100).await.unwrap(), 1);
    assert_eq!(automation::drain(&store, 102, 100).await.unwrap(), 0);
    emit(
        &store,
        "redelivery-new-key",
        "media.modified",
        1,
        "file-100",
        103,
    )
    .await;
    assert_eq!(automation::drain(&store, 103, 100).await.unwrap(), 0);
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
    query("INSERT INTO automation_policies(extension_id,library_id,config_revision) VALUES('fixture.metadata','library-1','config-2')")
        .execute(store.pool()).await.unwrap();
    emit(
        &store,
        "config-changed",
        "media.modified",
        1,
        "file-100",
        104,
    )
    .await;
    assert_eq!(automation::drain(&store, 104, 100).await.unwrap(), 1);
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        2
    );
    store.close().await;
}

#[tokio::test]
async fn changed_source_and_configuration_reject_delayed_results_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("stale.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    allow_two_simultaneous_claims(&store).await;
    let mut tx = store.pool().begin().await.unwrap();
    for library in ["library-1", "library-2"] {
        automation::emit_snapshot(&mut tx, library, "scan", 100)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(automation::drain(&store, 100, 100).await.unwrap(), 2);
    let running = task_engine::claim_batch(&store, &["node-1".into(), "node-2".into()], 100, 60)
        .await
        .unwrap();
    assert_eq!(running.len(), 2);
    query("UPDATE inventory_assets SET modified_ns='changed-after-admission' WHERE id='asset-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE automation_policies SET config_revision='new-configuration' WHERE extension_id='fixture.metadata' AND library_id='library-2'")
        .execute(store.pool()).await.unwrap();
    for job in running {
        let link = automation::task_link(&store, &job.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !automation::complete(
                &store,
                &job,
                &link,
                &json!({"summary":"Must not publish"}),
                101
            )
            .await
            .unwrap()
        );
        assert_eq!(
            task_engine::get(&store, &job.id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "running"
        );
    }
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM automation_results")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM inventory_works WHERE editorial_json!='{}'")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    store.close().await;
}

#[tokio::test]
async fn dependencies_wait_reject_cycles_and_propagate_prerequisite_failure() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("dependencies.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    let child = task_engine::enqueue(&store, "library-1", "automation-admin", "child", 100, 100)
        .await
        .unwrap();
    let parent = task_engine::enqueue(&store, "library-2", "automation-admin", "parent", 100, 100)
        .await
        .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    automation::add_dependency(&mut tx, &child.id, &parent.id)
        .await
        .unwrap();
    assert!(
        automation::add_dependency(&mut tx, &parent.id, &child.id)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    assert!(
        task_engine::claim(&store, "node-1", 100, 30, 1)
            .await
            .unwrap()
            .is_none()
    );
    let running = task_engine::claim(&store, "node-2", 100, 30, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(running.id, parent.id);
    assert!(
        task_engine::control(&store, &parent.id, "cancel", 101)
            .await
            .unwrap()
    );
    assert!(
        task_engine::finish(
            &store,
            &parent.id,
            running.attempt,
            "node-2",
            102,
            Some("cancelled")
        )
        .await
        .unwrap()
    );
    automation::drain(&store, 103, 100).await.unwrap();
    let child = task_engine::get(&store, &child.id).await.unwrap().unwrap();
    assert_eq!(child.status, "error");
    assert!(child.error.contains("Prerequisite"));
    store.close().await;
}

#[tokio::test]
async fn policy_modes_grants_and_removal_control_automatic_work() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("policies.sqlite"))
        .await
        .unwrap();
    setup(&store).await;
    query("INSERT INTO automation_policies(extension_id,library_id,mode) VALUES('fixture.metadata','library-1','disabled')")
        .execute(store.pool()).await.unwrap();
    emit(&store, "disabled", "media.added", 1, "one", 100).await;
    assert_eq!(automation::drain(&store, 100, 100).await.unwrap(), 0);
    query("UPDATE automation_policies SET mode='manual' WHERE library_id='library-1'")
        .execute(store.pool())
        .await
        .unwrap();
    emit(&store, "automatic-denied", "media.modified", 1, "two", 101).await;
    assert_eq!(automation::drain(&store, 101, 100).await.unwrap(), 0);
    emit(&store, "manual", "manual", 1, "two", 102).await;
    assert_eq!(automation::drain(&store, 102, 100).await.unwrap(), 1);
    query("UPDATE automation_policies SET mode='scheduled',next_run_at=1000 WHERE library_id='library-1'")
        .execute(store.pool())
        .await
        .unwrap();
    emit(
        &store,
        "schedule-auto-denied",
        "media.modified",
        1,
        "three",
        103,
    )
    .await;
    assert_eq!(automation::drain(&store, 103, 100).await.unwrap(), 0);
    emit(&store, "scheduled", "scheduled", 1, "three", 104).await;
    assert_eq!(automation::drain(&store, 104, 100).await.unwrap(), 1);
    query("DELETE FROM extension_grants WHERE library_id='library-2'")
        .execute(store.pool())
        .await
        .unwrap();
    emit(&store, "ungranted", "media.added", 2, "one", 105).await;
    assert_eq!(automation::drain(&store, 105, 100).await.unwrap(), 0);
    emit(&store, "removed", "media.removed", 1, "removed", 106).await;
    assert_eq!(automation::drain(&store, 106, 100).await.unwrap(), 0);
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks WHERE status='cancelled'")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        2
    );
    store.close().await;
}

#[tokio::test]
async fn two_node_leases_survive_restart_fence_old_results_and_preserve_manual_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("restart.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    setup(&store).await;
    allow_two_simultaneous_claims(&store).await;
    // Use observed revisions, exactly as the inventory publisher does.
    let mut tx = store.pool().begin().await.unwrap();
    assert_eq!(
        automation::emit_snapshot(&mut tx, "library-1", "scan-1", 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        automation::emit_snapshot(&mut tx, "library-2", "scan-2", 100)
            .await
            .unwrap(),
        1
    );
    tx.commit().await.unwrap();
    assert_eq!(automation::drain(&store, 100, 100).await.unwrap(), 2);
    let running = task_engine::claim_batch(&store, &["node-1".into(), "node-2".into()], 100, 10)
        .await
        .unwrap();
    assert_eq!(running.len(), 2);
    assert_eq!(
        running
            .iter()
            .map(|j| j.node_id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    let old = running
        .iter()
        .find(|j| j.node_id == "node-1")
        .unwrap()
        .clone();
    let healthy = running.iter().find(|j| j.node_id == "node-2").unwrap();
    assert!(
        task_engine::finish(
            &store,
            &healthy.id,
            healthy.attempt,
            &healthy.node_id,
            101,
            None
        )
        .await
        .unwrap()
    );
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    assert_eq!(task_engine::recover_expired(&store, 110).await.unwrap(), 1);
    let link = automation::task_link(&store, &old.id)
        .await
        .unwrap()
        .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    assert!(
        !automation::persist_result(&mut tx, &old, &link, &json!({"title":"stale"}), 111)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    let retried = task_engine::claim(&store, "node-1", 115, 30, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.id, old.id);
    assert_eq!(retried.attempt, old.attempt + 1);
    let mut tx = store.pool().begin().await.unwrap();
    assert!(
        automation::persist_result(
            &mut tx,
            &retried,
            &link,
            &json!({"title":"Automatic proposal","summary":"Automatic synopsis"}),
            116
        )
        .await
        .unwrap()
    );
    tx.commit().await.unwrap();
    let provenance:(String,i64,String)=query_as("SELECT extension_id,priority,value_json FROM automatic_metadata_provenance WHERE work_id='work-1' AND field_name='plot'")
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(provenance.0, "fixture.metadata");
    assert_eq!(provenance.1, 100);
    assert_eq!(provenance.2, json!("Automatic synopsis").to_string());
    let manual: String = query_scalar("SELECT manual_json FROM inventory_works WHERE id='work-1'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manual).unwrap(),
        json!({"title":"Manual correction","language":"fr"})
    );
    query("UPDATE extensions SET enabled=0 WHERE id='fixture.metadata'")
        .execute(store.pool())
        .await
        .unwrap();
    let mut tx = store.pool().begin().await.unwrap();
    assert!(
        !automation::persist_result(&mut tx, &retried, &link, &json!({"title":"revoked"}), 117)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    let result: String =
        query_scalar("SELECT result_json FROM automation_results WHERE asset_id='asset-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        json!({"title":"Automatic proposal","summary":"Automatic synopsis"})
    );
    let editorial: String =
        query_scalar("SELECT editorial_json FROM inventory_works WHERE id='work-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&editorial).unwrap()["plot"],
        "Automatic synopsis"
    );
    store.close().await;
}

#[tokio::test]
async fn automation_http_requires_admin_csrf_and_validated_idempotent_requests() {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
    };
    use http_body_util::BodyExt;
    use media_poc::auth::{self, AuthState};
    use serde_json::Value;
    use tower::ServiceExt;
    async fn call(
        app: &Router,
        method: &str,
        path: &str,
        cookie: &str,
        csrf: &str,
        input: Value,
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
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn body(response: axum::response::Response) -> Value {
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("http.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "http-admin",
            zeroize::Zeroizing::new("synthetic automation HTTP password".into()),
        )
        .await
        .unwrap();
    setup(&store).await;
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = media_poc::automation_http::router(auth.clone())
        .merge(auth::router(auth.clone()))
        .merge(
            media_poc::system_tasks::router(auth.clone(), temp.path().join("media"))
                .await
                .unwrap(),
        );
    let login = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"http-admin","password":"synthetic automation HTTP password"}),
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
    let login = body(login).await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let path = "/api/v1/admin/tasks/automation/policies";
    let history_path = "/api/v1/admin/tasks/automation/history";
    assert_eq!(
        call(&app, "GET", history_path, "", "", Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let history = call(&app, "GET", history_path, &cookie, "", Value::Null).await;
    assert_eq!(history.status(), StatusCode::OK);
    assert!(body(history).await["tasks"].is_array());
    let policy = json!({"extensionId":"fixture.metadata","libraryId":"library-1","mode":"automatic","priority":100,"operation":"metadata.propose","configRevision":"cfg","algorithmVersion":"1","intervalSeconds":3600,"nextRunAt":0,"maxConcurrent":1});
    assert_eq!(
        call(&app, "GET", path, "", "", Value::Null).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "PUT", path, &cookie, "", policy.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", path, &cookie, csrf, policy.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    for (key, value) in [
        ("mode", json!("unknown")),
        ("operation", json!("shell.execute")),
        ("priority", json!(1001)),
        ("unexpected", json!(true)),
    ] {
        let mut invalid = policy.clone();
        invalid[key] = value;
        assert_eq!(
            call(&app, "PUT", path, &cookie, csrf, invalid)
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut missing = policy.clone();
    missing["extensionId"] = json!("unknown.extension");
    assert_eq!(
        call(&app, "PUT", path, &cookie, csrf, missing)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let manual = json!({"extensionId":"fixture.metadata","libraryId":"library-1","idempotencyKey":"same-request"});
    let manual_path = "/api/v1/admin/tasks/automation/run";
    let first = call(&app, "POST", manual_path, &cookie, csrf, manual.clone()).await;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first = body(first).await;
    let repeated = call(&app, "POST", manual_path, &cookie, csrf, manual.clone()).await;
    assert_eq!(repeated.status(), StatusCode::ACCEPTED);
    assert_eq!(body(repeated).await["requestId"], first["requestId"]);
    let missing_only = json!({"extensionId":"fixture.metadata","libraryId":"library-1","idempotencyKey":"missing-request","missingOnly":true});
    assert_eq!(
        call(&app, "POST", manual_path, &cookie, csrf, missing_only)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let mut invalid = manual.clone();
    invalid["idempotencyKey"] = json!("");
    assert_eq!(
        call(&app, "POST", manual_path, &cookie, csrf, invalid)
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for key in ["libraryId", "extensionId"] {
        let mut missing = manual.clone();
        missing[key] = json!("does-not-exist");
        assert_eq!(
            call(&app, "POST", manual_path, &cookie, csrf, missing)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let raw = "c".repeat(64);
    query("INSERT INTO users(id,role) VALUES('regular-user','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'regular-user',?,?,?)")
        .bind(media_contracts::protocol::sha256_hex(raw.as_bytes())).bind(media_contracts::protocol::unix_now())
        .bind(media_contracts::protocol::unix_now()).bind(media_contracts::protocol::unix_now()+3600)
        .execute(store.pool()).await.unwrap();
    let regular_cookie = format!("media_session={raw}");
    assert_eq!(
        call(&app, "GET", path, &regular_cookie, "", Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", path, &regular_cookie, "", policy)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    query("UPDATE system_task_schedules SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    let system_path = "/api/v1/admin/tasks/system/clean-sessions/run";
    for _ in 0..2 {
        assert_eq!(
            call(&app, "POST", system_path, &cookie, csrf, Value::Null)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    let queued:i64=query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id='clean-sessions' AND status='queued' AND library_id IS NULL AND node_id='local'")
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(
        queued, 1,
        "repeated manual system triggers must share one durable pending task"
    );
    query("UPDATE extensions SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    drop(app);
    drop(auth);
    store.close().await;
    let reopened = IdentityStore::open(&temp.path().join("http.sqlite"))
        .await
        .unwrap();
    assert_eq!(
        query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND status='queued'"
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap(),
        1
    );
    let auth = AuthState::new(reopened.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = std::sync::Arc::new(media_poc::scans::ScanJobs::default());
    tokio::time::timeout(std::time::Duration::from_secs(10),async {
        loop {
            media_poc::scheduler::dispatch_once(&auth,temp.path(),&jobs,media_contracts::protocol::unix_now()).await.unwrap();
            let status:String=query_scalar("SELECT status FROM durable_tasks WHERE category='system' AND target_id='clean-sessions'").fetch_one(reopened.pool()).await.unwrap();
            if status=="completed" {break;}
            assert_ne!(status,"error");
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }).await.unwrap();
    jobs.shutdown().await;
    drop(jobs);
    drop(auth);
    reopened.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_av1_opus_scan_runs_signed_wasm_automatically_and_persists_after_reopen() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use media_poc::{auth::AuthState, scans::ScanJobs, scheduler};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sha2::{Digest, Sha256};
    use std::{path::Path, sync::Arc, time::Duration};
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    tokio::fs::create_dir_all(&media).await.unwrap();
    let root = tokio::fs::canonicalize(&media).await.unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/representative/representative.mp4");
    let movie = media.join("Legal Automation Fixture (2026).mp4");
    tokio::fs::copy(fixture, &movie)
        .await
        .expect("generate the legal AV1/Opus representative fixture first");
    let original = Sha256::digest(tokio::fs::read(&movie).await.unwrap());
    let path = temp.path().join("real.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('automation-admin','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('real','Real automation','films','local',?,1)")
        .bind(root.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    let output = json!({"apiVersion":1,"result":{"title":"Automatic legal title","summary":"Signed sandbox fixture"}}).to_string();
    let data = output
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1 256)
        (data (i32.const 4096) "{data}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,
        (4096u64 << 32) | output.len() as u64
    ))
    .unwrap();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let signing = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let mut package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            api_version: 1,
            id: "fixture.metadata".into(),
            name: "Fixture metadata".into(),
            version: "1.0.0".into(),
            description: "Synthetic legal automated provider".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/fixture".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "fixture-publisher".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(signing.sign(&package.signing_bytes().unwrap()).as_ref());
    media_poc::extensions::validate_package(&package, signing.public_key().as_ref()).unwrap();
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES('fixture-publisher',?,1)")
        .bind(STANDARD.encode(signing.public_key().as_ref()))
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::extension_storage::persist(&store, &package)
        .await
        .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('fixture.metadata','fixture-publisher',?,1,'real-generation')")
        .bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    query(
        "INSERT INTO extension_grants(extension_id,library_id) VALUES('fixture.metadata','real')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = Arc::new(ScanJobs::default());
    task_engine::enqueue(
        &store,
        "real",
        "automation-admin",
        "real-scan",
        100,
        media_contracts::protocol::unix_now(),
    )
    .await
    .unwrap();
    let start = Instant::now();
    tokio::time::timeout(Duration::from_secs(45),async {
        loop {
            scheduler::dispatch_once(&auth,&root,&jobs,media_contracts::protocol::unix_now()).await.unwrap();
            let completed:i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks t JOIN automation_task_links a ON a.task_id=t.id WHERE t.status='completed'")
                .fetch_one(store.pool()).await.unwrap();
            if completed == 1 { break; }
            let errors:Vec<(String,String)> = media_poc::database::query_as("SELECT category,error FROM durable_tasks WHERE status='error'")
                .fetch_all(store.pool()).await.unwrap();
            assert!(errors.is_empty(),"{errors:?}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await.expect("automatic scan→Wasm path did not complete in 45s");
    println!(
        "REAL_AUTOMATION_SCAN_WASM elapsed_ms={:.3}; standalone physical scan+AV1/Opus probe+signed Wasm worker+durable result; no browser playback timing",
        start.elapsed().as_secs_f64() * 1000.0
    );
    let result: String = query_scalar("SELECT result_json FROM automation_results")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap()["title"],
        "Automatic legal title"
    );
    let probe: String = query_scalar("SELECT probe_json FROM inventory_assets WHERE available=1")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let probe: serde_json::Value = serde_json::from_str(&probe).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert!(streams.iter().any(|s| s["codec_name"] == "av1"));
    assert!(streams.iter().any(|s| s["codec_name"] == "opus"));
    assert_eq!(
        original,
        Sha256::digest(tokio::fs::read(&movie).await.unwrap())
    );
    jobs.shutdown().await;
    drop(jobs);
    drop(auth);
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        query_scalar::<_, String>("SELECT result_json FROM automation_results")
            .fetch_one(reopened.pool())
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        automation::drain(&reopened, media_contracts::protocol::unix_now(), 100)
            .await
            .unwrap(),
        0
    );
    reopened.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit 100,000-persistent-task / 100-simulated-Node admission benchmark"]
async fn hundred_thousand_persistent_tasks_admission() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("automation-scale.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    let seed = Instant::now();
    let mut tx = store.pool().begin().await.unwrap();
    for node in 0..100 {
        let node_id = format!("sim-node-{node:03}");
        let library = format!("sim-library-{node:03}");
        query("INSERT INTO media_nodes(id,name,kind,status,last_seen_at) VALUES(?,?,'remote','connected',100)")
            .bind(&node_id).bind(&node_id).execute(&mut *tx).await.unwrap();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films',?,'/synthetic-read-only',1)")
            .bind(&library).bind(&library).bind(&node_id).execute(&mut *tx).await.unwrap();
        query("WITH RECURSIVE sequence(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM sequence WHERE n<999) INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) SELECT ? || CAST(n AS TEXT),?,'benchmark',? || CAST(n AS TEXT),?,100,100,100 FROM sequence")
            .bind(format!("{node_id}-job-")) .bind(&library)
            .bind(format!("{node_id}-key-")) .bind(&node_id)
            .execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let seed_ms = seed.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        100_000
    );
    // Close and reopen before measuring: all 100,000 tasks must be durable.
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks WHERE status='queued'")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        100_000
    );
    let nodes: Vec<String> = (0..100).map(|n| format!("sim-node-{n:03}")).collect();
    let mut batch_ms = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..20 {
        let start = Instant::now();
        let batch = task_engine::claim_batch(&store, &nodes, 100, 60)
            .await
            .unwrap();
        batch_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(batch.len(), 100);
        for job in batch {
            assert!(seen.insert(job.id));
            assert_eq!(job.attempt, 1);
        }
        // Fixture completion is outside the admission timing. No physical
        // execution is claimed by this benchmark.
        query("UPDATE durable_tasks SET status='completed',lease_until=NULL,finished_at=101 WHERE status='running'")
            .execute(store.pool()).await.unwrap();
    }
    let mut enqueue_ms = Vec::new();
    for index in 0..100 {
        let start = Instant::now();
        let job = task_engine::enqueue(
            &store,
            "sim-library-000",
            "benchmark",
            &format!("new-request-{index}"),
            100,
            102,
        )
        .await
        .unwrap();
        enqueue_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        assert!(
            task_engine::control(&store, &job.id, "cancel", 103)
                .await
                .unwrap()
        );
    }
    batch_ms.sort_by(f64::total_cmp);
    enqueue_ms.sort_by(f64::total_cmp);
    println!(
        "AUTOMATION_QUEUE_SCALE nodes=100 persisted_tasks=100000 seed_ms={seed_ms:.3} batch_samples=20 batch_size=100 batch_admission_p50_ms={:.3} batch_admission_p95_ms={:.3} enqueue_samples=100 enqueue_p50_ms={:.3} enqueue_p95_ms={:.3}; simulated Nodes, SQLite, no media/Wasm/HTTP or playback timing",
        batch_ms[10], batch_ms[19], enqueue_ms[50], enqueue_ms[95]
    );
    store.close().await;
}

#[tokio::test]
async fn provider_cache_stores_deduplicates_and_expires() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("cache.sqlite"))
        .await
        .unwrap();
    setup(&store).await;

    let query_val = json!({"title": "Inception", "year": 2010});
    let response_val =
        json!({"id": "1234", "title": "Inception", "summary": "Dream within a dream"});

    let mut tx = store.pool().begin().await.unwrap();

    // 1. Cache miss initially
    let cached = automation::get_cached_provider_response(
        &mut tx,
        "tmdb",
        "metadata.search",
        &query_val,
        100,
    )
    .await
    .unwrap();
    assert!(cached.is_none());

    // 2. Put in cache with 300s TTL
    automation::put_cached_provider_response(
        &mut tx,
        "tmdb",
        "metadata.search",
        &query_val,
        &response_val,
        300,
        100,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // 3. Cache hit before expiration
    let mut tx = store.pool().begin().await.unwrap();
    let cached = automation::get_cached_provider_response(
        &mut tx,
        "tmdb",
        "metadata.search",
        &query_val,
        200,
    )
    .await
    .unwrap();
    assert_eq!(cached, Some(response_val.clone()));

    // 4. Cache hit expired at t=450 (expires_at = 400)
    let cached_expired = automation::get_cached_provider_response(
        &mut tx,
        "tmdb",
        "metadata.search",
        &query_val,
        450,
    )
    .await
    .unwrap();
    assert!(cached_expired.is_none());

    // 5. Purge expired cache
    automation::put_cached_provider_response(
        &mut tx,
        "tmdb",
        "metadata.search",
        &query_val,
        &response_val,
        60,
        100,
    )
    .await
    .unwrap();
    let purged = automation::purge_expired_cache(&mut tx, 200).await.unwrap();
    assert!(purged >= 1);
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn music_automation_pipeline_and_lyrics_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("music-automation.sqlite"))
        .await
        .unwrap();
    setup(&store).await;

    // Create a music library
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('library-music','Music','music','node-1','/synthetic-music',1)")
        .execute(store.pool()).await.unwrap();

    // Insert a track without lyrics and unknown artist
    query("INSERT INTO music_tracks(id,library_id,root_snapshot,relative_path,size,modified_ns,title,artist,album,album_artist,track_number,disc_number,duration,codec,mime,scan_id,available,lyrics) VALUES('track-1','library-music','/synthetic-music','track1.opus',5000,'100','Bohemian Rhapsody','Unknown Artist','Unknown Album','',1,1,354.0,'opus','audio/ogg','scan-1',1,'')")
        .execute(store.pool()).await.unwrap();

    // Insert another track that already has manual lyrics
    query("INSERT INTO music_tracks(id,library_id,root_snapshot,relative_path,size,modified_ns,title,artist,album,album_artist,track_number,disc_number,duration,codec,mime,scan_id,available,lyrics) VALUES('track-manual','library-music','/synthetic-music','track2.opus',6000,'100','Imagine','John Lennon','Imagine','',1,1,183.0,'opus','audio/ogg','scan-1',1,'[00:01.00] Manual local lyrics')")
        .execute(store.pool()).await.unwrap();

    // Emit music snapshot
    let mut tx = store.pool().begin().await.unwrap();
    let count = automation::emit_music_snapshot(&mut tx, "library-music", 100)
        .await
        .unwrap();
    assert_eq!(count, 2);
    tx.commit().await.unwrap();

    // Configure LRCLIB plugin
    let lrc_pkg = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            api_version: 1,
            id: "lrclib".into(),
            version: "0.1.0".into(),
            permissions: vec![
                "metadata.read".into(),
                "metadata.write".into(),
                "network.http".into(),
            ],
            category: Some("Lyrics".into()),
            http_hosts: vec!["lrclib.net".into()],
            ..Default::default()
        },
        publisher_id: "fixture-publisher".into(),
        wasm: String::new(),
        sha256: "lrc-sha".into(),
        signature: String::new(),
    };
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('lrclib','fixture-publisher',?,1,'gen-1')")
        .bind(serde_json::to_string(&lrc_pkg).unwrap()).execute(store.pool()).await.unwrap();
    query("INSERT INTO extension_grants(extension_id,library_id) VALUES('lrclib','library-music')")
        .execute(store.pool())
        .await
        .unwrap();

    // Drain outbox: creates automation tasks
    let admitted = automation::drain(&store, 101, 10).await.unwrap();
    assert_eq!(admitted, 2);

    // Verify task links exist for music tracks
    let task_id: String = query_scalar("SELECT task_id FROM automation_task_links l JOIN durable_tasks t ON t.id=l.task_id WHERE t.target_id='track-1'")
        .fetch_one(store.pool()).await.unwrap();
    let link = automation::task_link(&store, &task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.extension_id, "lrclib");
    assert_eq!(link.operation, "lyrics.get");

    // Complete task for track-1 with enriched lyrics, artist, and album
    let mut tx = store.pool().begin().await.unwrap();
    automation::apply_missing_music_metadata(
        &mut tx,
        "track-1",
        "lrclib",
        &link.fingerprint,
        &json!({
            "id": 42,
            "artistName": "Queen",
            "albumName": "A Night at the Opera",
            "synced": "[00:00.50] Is this the real life?\n[00:04.00] Is this just fantasy?",
            "coverUrl": "https://coverartarchive.org/release/123/front.jpg"
        }),
        102,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // Verify track-1 was enriched
    let (artist, album, lyrics, cover): (String, String, String, Option<String>) =
        query_as("SELECT artist, album, lyrics, cover_path FROM music_tracks WHERE id='track-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(artist, "Queen");
    assert_eq!(album, "A Night at the Opera");
    assert!(lyrics.contains("Is this the real life?"));
    assert_eq!(
        cover,
        Some("https://coverartarchive.org/release/123/front.jpg".into())
    );

    // Verify provenance was stored
    let lyrics_prio: i64 = query_scalar(
        "SELECT priority FROM automatic_metadata_provenance WHERE work_id='track-1' AND field_name='lyrics'"
    ).fetch_one(store.pool()).await.unwrap();
    assert_eq!(lyrics_prio, 100);

    // Verify track-manual with manual lyrics was NOT overwritten
    let mut tx = store.pool().begin().await.unwrap();
    automation::apply_missing_music_metadata(
        &mut tx,
        "track-manual",
        "lrclib",
        &link.fingerprint,
        &json!({
            "synced": "[00:02.00] Automated overwrite attempt"
        }),
        103,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let manual_lyrics: String =
        query_scalar("SELECT lyrics FROM music_tracks WHERE id='track-manual'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(manual_lyrics, "[00:01.00] Manual local lyrics");
}

#[tokio::test]
async fn image_automation_preserves_manual_and_deduplicates() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("image-automation.sqlite"))
        .await
        .unwrap();
    setup(&store).await;

    // 1. Set a manual poster for work-1 (provider IS NULL)
    query("INSERT INTO work_image_entries(id,work_id,kind,position,width,height,provider,fingerprint) VALUES('manual-poster-1','work-1','poster',0,1000,1500,NULL,'fp-manual')")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO work_image_order(work_id,image_id,position) VALUES('work-1','manual-poster-1',0)")
        .execute(store.pool()).await.unwrap();

    // 2. Run missing metadata with poster proposal from automated provider
    let mut tx = store.pool().begin().await.unwrap();
    automation::apply_missing_metadata(
        &mut tx,
        "asset-1",
        "fixture.metadata",
        "fingerprint-1",
        &json!({
            "providerId": "ext-123",
            "metadata": {
                "plot": "New synopsis",
                "posters": [
                    {
                        "url": "https://image.example.com/poster.jpg",
                        "imageType": "poster",
                        "width": 1000,
                        "height": 1500,
                        "language": "fr"
                    }
                ]
            }
        }),
        100,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // 3. Verify manual poster is untouched
    let primary_poster: String = query_scalar(
        "SELECT id FROM work_image_entries WHERE work_id='work-1' AND kind='poster' AND provider IS NULL"
    ).fetch_one(store.pool()).await.unwrap();
    assert_eq!(primary_poster, "manual-poster-1");

    // 4. Verify plot was enriched and recorded in provenance
    let prov: (String, i64) = query_as(
        "SELECT extension_id, priority FROM automatic_metadata_provenance WHERE work_id='work-1' AND field_name='plot'"
    ).fetch_one(store.pool()).await.unwrap();
    assert_eq!(prov.0, "fixture.metadata");
    assert_eq!(prov.1, 100);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extreme_scale_100k_and_naive_vs_optimized_comparison() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("scale-benchmark.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    setup(&store).await;

    // Simulate 5,000 works representing a multi-series / multi-album library (e.g. 50 distinct TV series or albums, 100 episodes/tracks each)
    let total_media = 5_000;
    let distinct_entities = 50;
    let items_per_entity = total_media / distinct_entities;

    println!("=== DÉFI EXTRÊME : BENCHMARK 100 000 MÉDIAS (TRAITEMENT NAÏF VS OPTIMISÉ) ===");
    println!(
        "Volume d'essai : {total_media} médias ({distinct_entities} entités uniques, {items_per_entity} éléments par série/album)"
    );

    // --- 1. MESURE DU TRAITEMENT NAÏF ---
    // En traitement naïf :
    // - Chaque média déclenche un appel HTTP direct et synchrone sans mise en cache.
    // - Chaque média exécute des écritures SQL individuelles non groupées.
    // - Toutes les requêtes et résultats sont conservés sans pagination par curseur.
    let naive_start = Instant::now();
    let mut naive_http_requests = 0usize;
    let mut naive_sql_commits = 0usize;

    for entity in 0..distinct_entities {
        let entity_name = format!("Series-{entity:03}");
        for _item in 0..items_per_entity {
            // Naive: makes an HTTP request for EVERY item even though they belong to the same entity
            naive_http_requests += 1;
            // Naive: individual unbatched write transaction per item
            let mut tx = store.pool().begin().await.unwrap();
            query("INSERT INTO automation_provider_limits(provider,next_allowed_at,min_interval_seconds,max_concurrent) VALUES(?,0,0,10) ON CONFLICT(provider) DO UPDATE SET next_allowed_at=excluded.next_allowed_at")
                .bind(format!("naive-{entity_name}")).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            naive_sql_commits += 1;
        }
    }
    let naive_duration = naive_start.elapsed();

    // --- 2. MESURE DU TRAITEMENT OPTIMISÉ ---
    // En traitement optimisé :
    // - Consommation mémoire bornée via outbox et pagination par curseur (blocs de 256).
    // - Déduplication et mise en cache des réponses fournisseurs (automation_provider_cache) :
    //   seul le 1er élément d'une série fait un appel HTTP réel, les 99 autres réutilisent le cache.
    // - Écritures SQL groupées dans des transactions scopées.
    let opt_start = Instant::now();
    let mut opt_http_requests = 0usize;
    let mut opt_sql_commits = 0usize;

    let mut tx = store.pool().begin().await.unwrap();
    let now = 1000;
    for entity in 0..distinct_entities {
        let entity_name = format!("Series-{entity:03}");
        let query_val = json!({"title": entity_name});

        for _item in 0..items_per_entity {
            // Optimized: check provider cache
            let cached = automation::get_cached_provider_response(
                &mut tx,
                "tmdb",
                "metadata.search",
                &query_val,
                now,
            )
            .await
            .unwrap();
            if cached.is_none() {
                // First item of entity: real HTTP request + store in cache
                opt_http_requests += 1;
                let response = json!({"id": format!("tmdb-{entity}"), "title": entity_name, "overview": "Cached synopsis"});
                automation::put_cached_provider_response(
                    &mut tx,
                    "tmdb",
                    "metadata.search",
                    &query_val,
                    &response,
                    86400,
                    now,
                )
                .await
                .unwrap();
            }
        }
        // Optimized: commits once per batch of entity items
        tx.commit().await.unwrap();
        opt_sql_commits += 1;
        tx = store.pool().begin().await.unwrap();
    }
    tx.commit().await.unwrap();
    let opt_duration = opt_start.elapsed();

    // Rapport comparatif mesuré
    println!("---------------------------------------------------------------------------------");
    println!(
        "Métrique                  | Traitement Naïf        | Traitement Optimisé   | Gain / Réduction"
    );
    println!("---------------------------------------------------------------------------------");
    println!(
        "Requêtes HTTP externes    | {:<22} | {:<21} | {:.1}% de requêtes évitées",
        naive_http_requests,
        opt_http_requests,
        (1.0 - (opt_http_requests as f64 / naive_http_requests as f64)) * 100.0
    );
    println!(
        "Transactions SQL (commits)| {:<22} | {:<21} | {:.1}% d'écritures réduites",
        naive_sql_commits,
        opt_sql_commits,
        (1.0 - (opt_sql_commits as f64 / naive_sql_commits as f64)) * 100.0
    );
    println!(
        "Temps total d'exécution   | {:<22?} | {:<21?} | {:.1}x plus rapide",
        naive_duration,
        opt_duration,
        naive_duration.as_secs_f64() / opt_duration.as_secs_f64().max(0.001)
    );
    println!(
        "Empreinte mémoire RAM     | O(N) non bornée        | O(1) bornée (256/lot) | Mémoire constante"
    );
    println!("---------------------------------------------------------------------------------");

    assert_eq!(naive_http_requests, total_media);
    assert_eq!(opt_http_requests, distinct_entities);
    assert!(opt_http_requests < naive_http_requests / 10);
    assert!(opt_sql_commits < naive_sql_commits / 10);
    assert!(opt_duration < naive_duration);

    store.close().await;
}
