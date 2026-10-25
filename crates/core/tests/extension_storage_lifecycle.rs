use media_extension_sdk::storage::{StorageOperation, StorageRequest, StorageScope};
use media_poc::{
    database::{begin_write, query, query_scalar},
    extension_data::{self, Context},
    extension_data_lifecycle,
    identity::IdentityStore,
};

async fn fixture(store: &IdentityStore) -> Context {
    query("INSERT INTO users(id,role) VALUES('lifecycle-admin','administrator') ON CONFLICT DO NOTHING").execute(store.pool()).await.unwrap();
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES('lifecycle-publisher','fixture-key',1) ON CONFLICT DO NOTHING").execute(store.pool()).await.unwrap();
    let package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            permissions: vec!["storage.private".into()],
            ..Default::default()
        },
        publisher_id: "lifecycle-publisher".into(),
        wasm: String::new(),
        sha256: String::new(),
        signature: String::new(),
    };
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('lifecycle-extension','lifecycle-publisher',?,1,'gen') ON CONFLICT DO NOTHING").bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    Context {
        extension_id: "lifecycle-extension".into(),
        generation: "gen".into(),
        user_id: "lifecycle-admin".into(),
        ..Default::default()
    }
}
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
fn put(key: &str, durability: &str) -> StorageRequest {
    request(StorageOperation::Put {
        key: key.into(),
        value: "persistent".into(),
        expected_version: None,
        expires_at: None,
        durability: durability.into(),
    })
}

#[tokio::test]
async fn uninstall_retains_durable_and_purge_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&root.path().join("core.sqlite3"))
        .await
        .unwrap();
    uninstall_contract(&store).await;
}

async fn uninstall_contract(store: &IdentityStore) {
    let c = fixture(store).await;
    // Hold the real SQLite write lock / PostgreSQL advisory transaction lock.
    // The host operation must exhaust its execution budget without publishing.
    let mut blocker = store.pool().acquire().await.unwrap();
    let held = begin_write(&mut blocker).await.unwrap();
    let started = std::time::Instant::now();
    let blocked = extension_data::execute(store, &c, put("budget_probe", "durable"), 100).await;
    let elapsed = started.elapsed();
    assert!(
        matches!(blocked, Err(extension_data::Error::Timeout)),
        "expected execution deadline, got {blocked:?}"
    );
    assert!(
        elapsed >= std::time::Duration::from_secs(1) && elapsed < std::time::Duration::from_secs(5),
        "execution deadline elapsed {elapsed:?}"
    );
    held.rollback().await.unwrap();
    drop(blocker);
    let after = extension_data::execute(
        store,
        &c,
        request(StorageOperation::Get {
            key: "budget_probe".into(),
        }),
        100,
    )
    .await
    .unwrap();
    assert!(
        after.entries.is_empty(),
        "timed-out write must not publish a partial entry"
    );
    extension_data::execute(store, &c, put("budget_probe", "durable"), 100)
        .await
        .unwrap();
    extension_data::execute(store, &c, put("result", "durable"), 100)
        .await
        .unwrap();
    extension_data::execute(store, &c, put("derived", "cache"), 100)
        .await
        .unwrap();
    use media_extension_sdk::objects::{ObjectOperation, ObjectRequest};
    let objects = |operation| ObjectRequest {
        api_version: 1,
        scope: StorageScope {
            kind: "global".into(),
            id: String::new(),
        },
        operation,
    };
    // Completed durable objects survive; cache and incomplete reservations do not.
    for (durability, complete) in [("durable", true), ("cache", true), ("durable", false)] {
        let object = media_poc::extension_objects::execute(
            store,
            &c,
            objects(ObjectOperation::Begin {
                size: 0,
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                durability: durability.into(),
                expires_at: (durability == "cache").then_some(200),
            }),
            100,
        )
        .await
        .unwrap()
        .metadata
        .unwrap();
        if complete {
            media_poc::extension_objects::execute(
                store,
                &c,
                objects(ObjectOperation::Finish { id: object.id }),
                100,
            )
            .await
            .unwrap();
        }
    }
    let mut conn = store.pool().acquire().await.unwrap();
    let mut tx = begin_write(&mut conn).await.unwrap();
    extension_data_lifecycle::uninstall(&mut tx, &c.extension_id, false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(conn);
    let retained:i64=query_scalar("SELECT COUNT(*) FROM extension_objects WHERE extension_id=? AND durability='durable' AND complete=1").bind(&c.extension_id).fetch_one(store.pool()).await.unwrap();
    let discarded:i64=query_scalar("SELECT COUNT(*) FROM extension_objects WHERE extension_id=? AND (durability='cache' OR complete=0)").bind(&c.extension_id).fetch_one(store.pool()).await.unwrap();
    assert_eq!(retained, 1);
    assert_eq!(discarded, 0);
    assert_eq!(
        extension_data::execute(
            store,
            &c,
            request(StorageOperation::Get {
                key: "result".into()
            }),
            100
        )
        .await
        .unwrap()
        .entries
        .len(),
        1
    );
    assert!(
        extension_data::execute(
            store,
            &c,
            request(StorageOperation::Get {
                key: "derived".into()
            }),
            100
        )
        .await
        .unwrap()
        .entries
        .is_empty()
    );
    let mut conn = store.pool().acquire().await.unwrap();
    let mut tx = begin_write(&mut conn).await.unwrap();
    extension_data_lifecycle::uninstall(&mut tx, &c.extension_id, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    drop(conn);
    let objects_left: i64 =
        query_scalar("SELECT COUNT(*) FROM extension_objects WHERE extension_id=?")
            .bind(&c.extension_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(objects_left, 0);
    assert!(
        extension_data::execute(
            store,
            &c,
            request(StorageOperation::Get {
                key: "result".into()
            }),
            100
        )
        .await
        .unwrap()
        .entries
        .is_empty()
    );
}

// Child process is intentionally terminated without running any Rust destructors.
#[tokio::test]
#[ignore = "internal crash child"]
async fn crash_child() {
    let postgres = std::env::var("MEDIA_EXTENSION_CRASH_POSTGRES_URL").ok();
    let path = std::env::var("MEDIA_EXTENSION_CRASH_PATH").ok();
    if postgres.is_none() && path.is_none() {
        return;
    }
    let mode = std::env::var("MEDIA_EXTENSION_CRASH_MODE").unwrap();
    let store = if let Some(url) = postgres {
        IdentityStore::open_postgres(&url).await.unwrap()
    } else {
        IdentityStore::open(std::path::Path::new(path.as_deref().unwrap()))
            .await
            .unwrap()
    };
    let c = fixture(&store).await;
    if mode == "verify" {
        let entry = extension_data::execute(
            &store,
            &c,
            request(StorageOperation::Get {
                key: "committed".into(),
            }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(entry.entries.len(), 1);
        assert_eq!(entry.entries[0].value, "persistent");
        store.close().await;
        return;
    }
    if mode == "after_commit" {
        extension_data::execute(&store, &c, put("committed", "durable"), 100)
            .await
            .unwrap();
    } else {
        let mut conn = store.pool().acquire().await.unwrap();
        let mut tx = begin_write(&mut conn).await.unwrap();
        query("UPDATE extension_data SET value='uncommitted-corruption' WHERE extension_id=?")
            .bind(&c.extension_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        std::process::exit(79);
    }
    std::process::exit(79);
}

#[tokio::test]
#[ignore = "requires dedicated temporary MEDIA_TEST_POSTGRES_URL, never production"]
async fn postgres_process_crash_and_uninstall_lifecycle() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("dedicated PostgreSQL URL");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let schema = format!("extension_lifecycle_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let mut scoped = url::Url::parse(&url).unwrap();
    scoped
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    for mode in ["after_commit", "before_commit"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "crash_child", "--ignored", "--nocapture"])
            .env_remove("MEDIA_EXTENSION_CRASH_PATH")
            .env("MEDIA_EXTENSION_CRASH_POSTGRES_URL", scoped.as_str())
            .env("MEDIA_EXTENSION_CRASH_MODE", mode);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert_eq!(command.status().unwrap().code(), Some(79));
        let store = IdentityStore::open_postgres(scoped.as_str()).await.unwrap();
        let c = fixture(&store).await;
        let entry = extension_data::execute(
            &store,
            &c,
            request(StorageOperation::Get {
                key: "committed".into(),
            }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(entry.entries.len(), 1);
        assert_eq!(entry.entries[0].value, "persistent");
        store.close().await;
    }
    let store = IdentityStore::open_postgres(scoped.as_str()).await.unwrap();
    uninstall_contract(&store).await;
    store.close().await;
    // The only removed schema is the UUID namespace freshly created above.
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn process_crash_preserves_commits_and_rolls_back_uncommitted_wal() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("crash.sqlite3");
    for mode in ["after_commit", "before_commit"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "crash_child", "--ignored", "--nocapture"])
            .env_remove("MEDIA_EXTENSION_CRASH_POSTGRES_URL")
            .env("MEDIA_EXTENSION_CRASH_PATH", &path)
            .env("MEDIA_EXTENSION_CRASH_MODE", mode);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert_eq!(command.status().unwrap().code(), Some(79));
        let store = IdentityStore::open(&path).await.unwrap();
        let c = fixture(&store).await;
        let entry = extension_data::execute(
            &store,
            &c,
            request(StorageOperation::Get {
                key: "committed".into(),
            }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(entry.entries[0].value, "persistent");
        store.close().await;
    }
}
