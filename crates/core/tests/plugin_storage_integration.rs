//! Integration tests for plugin storage, persistence, diagnostics, and auto-healing.
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use media_poc::{
    auth::AuthState,
    database::query,
    extension_storage::{check_health, persist, sync_installed, wasm},
    identity::IdentityStore,
};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use zeroize::Zeroizing;

fn generate_key() -> Ed25519KeyPair {
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap()
}

fn valid_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "invoke") (param i32 i32) (result i32)
                i32.const 0
            )
        )"#,
    )
    .unwrap()
}

fn create_package(key: &Ed25519KeyPair, id: &str, version: &str, wasm_bytes: Vec<u8>) -> Package {
    let mut p = Package {
        manifest: Manifest {
            api_version: 1,
            id: id.into(),
            name: format!("Integration Plugin {id}"),
            version: version.into(),
            description: "Verified plugin storage fixture".into(),
            author: "Aether Integrations".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/repo".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "aether.test".into(),
        sha256: format!("{:x}", Sha256::digest(&wasm_bytes)),
        wasm: STANDARD.encode(&wasm_bytes),
        signature: String::new(),
    };
    p.signature = STANDARD.encode(key.sign(&p.signing_bytes().unwrap()).as_ref());
    p
}

async fn setup_sqlite_store() -> (tempfile::TempDir, IdentityStore) {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("core.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    (temp, store)
}

#[tokio::test]
async fn test_sqlite_plugin_install_creates_bounded_files() {
    let (_temp, store) = setup_sqlite_store().await;
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "test.plugin", "0.1.0", wasm_bytes.clone());

    // Persist package
    persist(&store, &package).await.unwrap();

    let plugin_dir = store.plugins_path().join("test.plugin");
    assert!(plugin_dir.exists(), "Plugin directory must exist");

    // Verify exactly package.json, manifest.json, module.wasm exist
    let pkg_file = plugin_dir.join("package.json");
    let manifest_file = plugin_dir.join("manifest.json");
    let module_file = plugin_dir.join("module.wasm");

    assert!(pkg_file.exists(), "package.json must exist");
    assert!(manifest_file.exists(), "manifest.json must exist");
    assert!(module_file.exists(), "module.wasm must exist");

    // Verify module.wasm matches SHA-256
    let on_disk_wasm = tokio::fs::read(&module_file).await.unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&on_disk_wasm)),
        package.sha256
    );

    // Verify wasm() helper returns correct base64
    let loaded_wasm = wasm(&store, &package).await.unwrap();
    assert_eq!(loaded_wasm, package.wasm);
}

#[tokio::test]
async fn test_plugin_storage_health_diagnostics() {
    let (_temp, store) = setup_sqlite_store().await;
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "test.health", "1.0.0", wasm_bytes);

    // Register in DB and persist
    query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('aether.test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'gen1')")
        .bind(&package.manifest.id)
        .bind("aether.test")
        .bind(serde_json::to_string(&package).unwrap())
        .execute(store.pool())
        .await
        .unwrap();

    persist(&store, &package).await.unwrap();

    let health = check_health(&store).await;
    assert!(health.exists);
    assert!(health.writable);
    assert!(health.healthy);
    assert_eq!(health.installed_count, 1);
    assert_eq!(health.on_disk_count, 1);
    assert_eq!(health.missing_count, 0);
    assert_eq!(health.corrupted_count, 0);
    assert!(health.error.is_none());
}

#[tokio::test]
async fn test_sync_installed_restores_missing_files_after_restart() {
    let (_temp, store) = setup_sqlite_store().await;
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "test.missing", "1.0.0", wasm_bytes);

    // Register in DB and initial persist
    query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('aether.test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'gen1')")
        .bind(&package.manifest.id)
        .bind("aether.test")
        .bind(serde_json::to_string(&package).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    persist(&store, &package).await.unwrap();

    let plugin_dir = store.plugins_path().join("test.missing");
    // Simulate accidental deletion of module.wasm and manifest.json
    tokio::fs::remove_file(plugin_dir.join("module.wasm"))
        .await
        .unwrap();
    tokio::fs::remove_file(plugin_dir.join("manifest.json"))
        .await
        .unwrap();

    let health_before = check_health(&store).await;
    assert!(!health_before.healthy);
    assert_eq!(health_before.corrupted_count, 1);

    // Run sync_installed (simulates container restart)
    let synced = sync_installed(&store).await.unwrap();
    assert_eq!(synced, 1);

    // Verify files are restored
    assert!(plugin_dir.join("module.wasm").exists());
    assert!(plugin_dir.join("manifest.json").exists());
    let restored_module = tokio::fs::read(plugin_dir.join("module.wasm"))
        .await
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&restored_module)),
        package.sha256
    );

    let health_after = check_health(&store).await;
    assert!(health_after.healthy);
    assert_eq!(health_after.on_disk_count, 1);
    assert_eq!(health_after.missing_count, 0);
    assert_eq!(health_after.corrupted_count, 0);
}

#[tokio::test]
async fn test_sync_installed_repairs_corrupted_wasm_file() {
    let (_temp, store) = setup_sqlite_store().await;
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "test.corrupt", "1.0.0", wasm_bytes);

    // Register in DB and persist
    query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('aether.test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'gen1')")
        .bind(&package.manifest.id)
        .bind("aether.test")
        .bind(serde_json::to_string(&package).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    persist(&store, &package).await.unwrap();

    let plugin_dir = store.plugins_path().join("test.corrupt");

    // Tamper module.wasm with corrupted bytes
    tokio::fs::write(plugin_dir.join("module.wasm"), b"tampered corrupt bytes")
        .await
        .unwrap();

    // Verify health detects corruption on disk
    let health_before = check_health(&store).await;
    assert!(!health_before.healthy);
    assert_eq!(health_before.corrupted_count, 1);

    // Run sync_installed
    let synced = sync_installed(&store).await.unwrap();
    assert_eq!(synced, 1);

    // Verify on-disk module was restored and matches package sha256
    let restored_module = tokio::fs::read(plugin_dir.join("module.wasm"))
        .await
        .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&restored_module)),
        package.sha256
    );

    let health_after = check_health(&store).await;
    assert!(health_after.healthy);
    assert_eq!(health_after.corrupted_count, 0);
}

#[tokio::test]
async fn test_sync_installed_recovers_empty_plugin_directory() {
    let (_temp, store) = setup_sqlite_store().await;
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "test.emptyfolder", "1.0.0", wasm_bytes);

    query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('aether.test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'gen1')")
        .bind(&package.manifest.id)
        .bind("aether.test")
        .bind(serde_json::to_string(&package).unwrap())
        .execute(store.pool())
        .await
        .unwrap();

    // Create an empty folder directly
    let plugin_dir = store.plugins_path().join("test.emptyfolder");
    tokio::fs::create_dir_all(&plugin_dir).await.unwrap();

    let synced = sync_installed(&store).await.unwrap();
    assert_eq!(synced, 1);

    assert!(plugin_dir.join("package.json").exists());
    assert!(plugin_dir.join("manifest.json").exists());
    assert!(plugin_dir.join("module.wasm").exists());
}

#[tokio::test]
async fn test_clean_staging_removes_dangling_install_folders() {
    let (_temp, store) = setup_sqlite_store().await;

    // Create plugin storage root
    tokio::fs::create_dir_all(store.plugins_path())
        .await
        .unwrap();

    // Create simulated interrupted staging folder
    let staging = store.plugins_path().join(".install-interrupted-uuid");
    tokio::fs::create_dir_all(&staging).await.unwrap();
    tokio::fs::write(staging.join("package.json"), b"partial")
        .await
        .unwrap();

    assert!(staging.exists());

    // Running sync_installed cleans staging
    sync_installed(&store).await.unwrap();

    assert!(
        !staging.exists(),
        "Interrupted staging folder must be cleaned up on restart"
    );
}

#[tokio::test]
async fn test_storage_health_endpoint_http() {
    let (_temp, store) = setup_sqlite_store().await;
    let admin_id = store
        .bootstrap("admin", Zeroizing::new("AdministratorPassword123#".into()))
        .await
        .unwrap();
    assert!(!admin_id.is_empty());

    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = media_poc::extensions::router(auth.clone());

    // Log in admin to get session cookie
    let auth_app = media_poc::auth::router(auth.clone());
    let login_body = serde_json::to_vec(&serde_json::json!({
        "login": "admin",
        "password": "AdministratorPassword123#"
    }))
    .unwrap();

    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .body(Body::from(login_body))
        .unwrap();

    let login_res = auth_app.oneshot(login_req).await.unwrap();
    assert_eq!(login_res.status(), StatusCode::OK);
    let cookie_header = login_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Query GET /api/v1/admin/extensions/storage/health
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/extensions/storage/health")
        .header(header::COOKIE, &cookie_header)
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(res.into_body(), 100000).await.unwrap();
    let health_json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(health_json["healthy"].as_bool().unwrap());
    assert!(health_json["writable"].as_bool().unwrap());
    assert!(health_json["path"].as_str().is_some());

    // Query GET /api/v1/admin/extensions and verify "storage" is present
    let req2 = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/extensions")
        .header(header::COOKIE, &cookie_header)
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();

    let res2 = app.oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body2 = axum::body::to_bytes(res2.into_body(), 100000)
        .await
        .unwrap();
    let list_json: Value = serde_json::from_slice(&body2).unwrap();

    assert!(list_json.get("storage").is_some());
    assert!(list_json["storage"]["healthy"].as_bool().unwrap());
}

#[tokio::test]
#[ignore = "requires dedicated temporary MEDIA_TEST_POSTGRES_URL, never production"]
async fn test_postgres_plugin_storage_lifecycle() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("dedicated PostgreSQL URL");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let schema = format!("plugin_storage_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();

    let mut scoped = url::Url::parse(&url).unwrap();
    scoped
        .query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));

    let store = IdentityStore::open_postgres(scoped.as_str()).await.unwrap();
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "pg.plugin", "1.0.0", wasm_bytes);

    query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('aether.test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,'gen1')")
        .bind(&package.manifest.id)
        .bind("aether.test")
        .bind(serde_json::to_string(&package).unwrap())
        .execute(store.pool())
        .await
        .unwrap();

    // Persist and verify
    persist(&store, &package).await.unwrap();
    let health = check_health(&store).await;
    assert!(health.healthy);
    assert_eq!(health.on_disk_count, 1);

    // Delete files and test sync_installed
    let plugin_dir = store.plugins_path().join("pg.plugin");
    tokio::fs::remove_file(plugin_dir.join("module.wasm"))
        .await
        .unwrap();

    let synced = sync_installed(&store).await.unwrap();
    assert_eq!(synced, 1);
    assert!(plugin_dir.join("module.wasm").exists());
}
