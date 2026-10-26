//! Durable automation contracts. Scale fixtures below simulate Nodes and do not
//! execute media, HTTP, Wasm, or establish playback latency guarantees.
use media_poc::{
    automation::{self, EventInput},
    database::{query, query_scalar},
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
