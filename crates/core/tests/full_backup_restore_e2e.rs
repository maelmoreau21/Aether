//! End-to-end integration test for full backup and restore: SQL, durable assets,
//! and signed Wasm extensions.
use aether_core::{
    backup::restore_database,
    backup_bundle::{BackupKey, Protection, create_with_assets, extract},
    extensions::storage::{persist, sync_installed, wasm},
    identity::IdentityStore,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

fn backup_key() -> BackupKey {
    BackupKey::parse("3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12").unwrap()
}

fn generate_key() -> Ed25519KeyPair {
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap()
}

fn valid_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "invoke") (param i32 i32) (result i32)
                i32.const 42
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
            name: format!("Full Backup Test Plugin {id}"),
            version: version.into(),
            description: "Durable extension fixture for backup tests".into(),
            author: "Aether Integration".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/repo".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "aether.full_backup".into(),
        sha256: format!("{:x}", Sha256::digest(&wasm_bytes)),
        wasm: STANDARD.encode(&wasm_bytes),
        signature: String::new(),
    };
    p.signature = STANDARD.encode(key.sign(&p.signing_bytes().unwrap()).as_ref());
    p
}

#[tokio::test]
async fn full_backup_and_restore_cycle_restores_sql_assets_and_extensions() {
    let temp = tempfile::tempdir().unwrap();
    let server_a = temp.path().join("server_a");
    let server_b = temp.path().join("server_b");
    tokio::fs::create_dir_all(&server_a).await.unwrap();
    tokio::fs::create_dir_all(&server_b).await.unwrap();

    // === 1. Server A: Setup live database, assets, and extensions ===
    let db_a = server_a.join("core.sqlite3");
    let store_a = IdentityStore::open(&db_a).await.unwrap();

    // Bootstrap Admin
    let admin_id = store_a
        .bootstrap(
            "admin-live",
            Zeroizing::new("master-password-for-aether-1234".into()),
        )
        .await
        .unwrap();
    assert!(!admin_id.is_empty());

    // Create an asset in AssetStore
    let asset_bytes = b"VERIFIED_FULL_BACKUP_AVATAR_IMAGE_DATA_BYTES_PNG_OR_WEBP";
    // The native avatar gallery seeds its own assets on a fresh database.
    let seeded = store_a.assets().export_source().await.unwrap().items.len();
    let meta = store_a
        .assets()
        .put_bytes(asset_bytes, "image/png")
        .await
        .unwrap();
    assert_eq!(meta.size, asset_bytes.len() as i64);

    // Install an extension
    let key = generate_key();
    let wasm_bytes = valid_wasm();
    let package = create_package(&key, "e2e-plugin", "1.0.0", wasm_bytes.clone());

    // Register publisher
    sqlx::query(
        "INSERT INTO extension_publishers(id, public_key, active) VALUES (?, 'fixture', 1)",
    )
    .bind(&package.publisher_id)
    .execute(store_a.pool())
    .await
    .unwrap();

    // Register extension in DB
    let package_json = serde_json::to_string(&package).unwrap();
    sqlx::query("INSERT INTO extensions(id, publisher_id, package_json, enabled, generation) VALUES (?, ?, ?, 1, 'gen-1')")
        .bind(&package.manifest.id)
        .bind(&package.publisher_id)
        .bind(&package_json)
        .execute(store_a.pool())
        .await
        .unwrap();

    // Persist extension to disk
    persist(&store_a, &package).await.unwrap();
    assert_eq!(wasm(&store_a, &package).await.unwrap(), package.wasm);

    // === 2. Take a full encrypted backup of Server A ===
    let backups_dir = temp.path().join("backups");
    tokio::fs::create_dir_all(&backups_dir).await.unwrap();
    let backup_archive = backups_dir.join("live-full-backup.media-backup");

    let snapshot_db = temp.path().join("db-snapshot.tmp");
    aether_core::backup::backup_database(&db_a, &snapshot_db, None)
        .await
        .unwrap();

    let asset_source = store_a.assets().export_source().await.unwrap();
    assert_eq!(asset_source.items.len(), seeded + 1);

    let archive_size = create_with_assets(
        &snapshot_db,
        BTreeMap::from([("MEDIA_MODE".into(), "standalone".into())]),
        &backup_archive,
        "sqlite",
        Some(asset_source),
        Protection::Encrypted(backup_key()),
    )
    .await
    .unwrap();
    assert!(archive_size > 0);
    assert!(
        !std::fs::read(&backup_archive)
            .unwrap()
            .windows(asset_bytes.len())
            .any(|window| window == asset_bytes)
    );

    // Close Server A store
    store_a.close().await;

    // === 3. Disaster recovery onto Server B (clean slate) ===
    let extracted_dir = temp.path().join("extracted-recovery");
    let summary = extract(&backup_archive, &extracted_dir, Some(&backup_key()))
        .await
        .unwrap();
    assert_eq!(summary.engine, "sqlite");
    assert_eq!(summary.assets as usize, seeded + 1);
    assert!(extracted_dir.join("database.sqlite3").exists());
    assert!(extracted_dir.join("assets.json").exists());
    assert!(extracted_dir.join("assets").join(&meta.sha256).exists());

    // Restore Database onto Server B
    let db_b = server_b.join("core.sqlite3");
    restore_database(&extracted_dir.join("database.sqlite3"), &db_b, None)
        .await
        .unwrap();

    // Open restored Server B store
    let store_b = IdentityStore::open(&db_b).await.unwrap();

    // Before asset restore, the asset row exists in SQL but bytes are missing on disk
    let audit_before = store_b.assets().verify_all().await.unwrap();
    assert_eq!(audit_before.missing.len(), seeded + 1);

    // Restore assets using restore_extracted
    let restore_report = store_b
        .assets()
        .restore_extracted(&extracted_dir)
        .await
        .unwrap();
    assert_eq!(restore_report.imported as usize, seeded + 1);
    assert_eq!(restore_report.already_present, 0);
    assert!(restore_report.is_complete());

    // Check asset contents
    let (read_meta, read_bytes) = store_b.assets().read_all(&meta.sha256).await.unwrap();
    assert_eq!(read_meta.sha256, meta.sha256);
    assert_eq!(read_bytes, asset_bytes);

    // Restore extensions onto Server B via sync_installed
    let synced_extensions = sync_installed(&store_b).await.unwrap();
    assert_eq!(synced_extensions, 1);
    assert_eq!(wasm(&store_b, &package).await.unwrap(), package.wasm);

    // Final audit
    let final_audit = store_b.assets().verify_all().await.unwrap();
    assert_eq!(final_audit.checked as usize, seeded + 1);
    assert_eq!(final_audit.missing.len(), 0);
    assert_eq!(final_audit.corrupt.len(), 0);

    // Validate users on Server B
    let admin_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='administrator'")
            .fetch_one(store_b.pool())
            .await
            .unwrap();
    assert_eq!(admin_count, 1);

    store_b.close().await;
}

#[tokio::test]
async fn restore_assets_cli_rejects_partial_extension_sync() {
    let root = tempfile::tempdir().unwrap();
    let extracted = root.path().join("extracted");
    std::fs::create_dir(&extracted).unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    for (name, invalid) in [("healthy", false), ("partial", true)] {
        let target = root.path().join(name);
        std::fs::create_dir(&target).unwrap();
        let db = target.join("core.sqlite3");
        let store = IdentityStore::open(&db).await.unwrap();
        let key = generate_key();
        let package = create_package(&key, "fixture.extension", "1.0.0", valid_wasm());
        aether_core::database::query(
            "INSERT INTO extension_publishers(id,public_key,active) VALUES(?,?,1)",
        )
        .bind(&package.publisher_id)
        .bind("fixture-key")
        .execute(store.pool())
        .await
        .unwrap();
        aether_core::database::query(
            "INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,?)",
        )
        .bind(&package.manifest.id)
        .bind(&package.publisher_id)
        .bind(serde_json::to_string(&package).unwrap())
        .bind("fixture-generation")
        .execute(store.pool())
        .await
        .unwrap();
        if invalid {
            aether_core::database::query(
                "INSERT INTO extensions(id,publisher_id,package_json,generation) VALUES(?,?,?,?)",
            )
            .bind("broken.extension")
            .bind(&package.publisher_id)
            .bind("{}")
            .bind("broken-generation")
            .execute(store.pool())
            .await
            .unwrap();
        }
        store.close().await;
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-restore-assets"))
            .arg(&extracted)
            .current_dir(&target)
            .env("MEDIA_DB_ENGINE", "sqlite")
            .env("MEDIA_DB_PATH", &db)
            .env_remove("MEDIA_DB_URL")
            .env("MEDIA_LIBRARY_ROOT", &media)
            .env("MEDIA_ASSETS_PATH", target.join("assets"))
            .env("MEDIA_PLUGINS_PATH", target.join("plugins"))
            .output()
            .await
            .unwrap();
        if invalid {
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("restore is INCOMPLETE"));
            assert!(
                !String::from_utf8_lossy(&output.stdout).contains("extension storage is healthy")
            );
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("extension storage is healthy")
            );
        }
    }
}

#[tokio::test]
async fn cli_recover_restore_and_restore_assets_e2e() {
    let temp = tempfile::tempdir().unwrap();
    let server_a = temp.path().join("server_a");
    let server_b = temp.path().join("server_b");
    let media_root = temp.path().join("media");
    tokio::fs::create_dir_all(&server_a).await.unwrap();
    tokio::fs::create_dir_all(&server_b).await.unwrap();
    tokio::fs::create_dir_all(&media_root).await.unwrap();

    let db_a = server_a.join("core.sqlite3");
    let store_a = IdentityStore::open(&db_a).await.unwrap();
    store_a
        .bootstrap(
            "admin-cli",
            Zeroizing::new("master-password-for-aether-1234".into()),
        )
        .await
        .unwrap();
    let asset_bytes = b"CLI_E2E_VERIFIED_ASSET_BYTES";
    let meta = store_a
        .assets()
        .put_bytes(asset_bytes, "image/png")
        .await
        .unwrap();

    let key = generate_key();
    let package = create_package(&key, "cli-e2e-plugin", "1.0.0", valid_wasm());
    sqlx::query(
        "INSERT INTO extension_publishers(id, public_key, active) VALUES (?, 'fixture', 1)",
    )
    .bind(&package.publisher_id)
    .execute(store_a.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO extensions(id, publisher_id, package_json, enabled, generation) VALUES (?, ?, ?, 1, 'gen-1')",
    )
    .bind(&package.manifest.id)
    .bind(&package.publisher_id)
    .bind(serde_json::to_string(&package).unwrap())
    .execute(store_a.pool())
    .await
    .unwrap();

    let snapshot_db = temp.path().join("db-snapshot.tmp");
    aether_core::backup::backup_database(&db_a, &snapshot_db, None)
        .await
        .unwrap();
    let asset_source = store_a.assets().export_source().await.unwrap();
    let backup_archive = temp.path().join("cli-e2e.media-backup");
    create_with_assets(
        &snapshot_db,
        BTreeMap::from([("MEDIA_MODE".into(), "standalone".into())]),
        &backup_archive,
        "sqlite",
        Some(asset_source),
        Protection::Encrypted(backup_key()),
    )
    .await
    .unwrap();
    store_a.close().await;

    let extracted_dir = temp.path().join("extracted-cli");
    let recover = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-recover"))
        .arg(&backup_archive)
        .arg(&extracted_dir)
        .env(
            "MEDIA_BACKUP_KEY",
            "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12",
        )
        .output()
        .await
        .unwrap();
    assert!(
        recover.status.success(),
        "media-recover failed: {}",
        String::from_utf8_lossy(&recover.stderr)
    );

    let db_b = server_b.join("core.sqlite3");
    let restore = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-restore"))
        .arg(extracted_dir.join("database.sqlite3"))
        .arg(&db_b)
        .env("MEDIA_DB_ENGINE", "sqlite")
        .env("MEDIA_LIBRARY_ROOT", &media_root)
        .output()
        .await
        .unwrap();
    assert!(
        restore.status.success(),
        "media-restore failed: {}",
        String::from_utf8_lossy(&restore.stderr)
    );

    let assets_b = server_b.join("assets");
    let plugins_b = server_b.join("plugins");
    let restore_assets = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-restore-assets"))
        .arg(&extracted_dir)
        .env("MEDIA_DB_ENGINE", "sqlite")
        .env("MEDIA_DB_PATH", &db_b)
        .env_remove("MEDIA_DB_URL")
        .env("MEDIA_LIBRARY_ROOT", &media_root)
        .env("MEDIA_ASSETS_PATH", &assets_b)
        .env("MEDIA_PLUGINS_PATH", &plugins_b)
        .output()
        .await
        .unwrap();
    assert!(
        restore_assets.status.success(),
        "media-restore-assets failed: {}",
        String::from_utf8_lossy(&restore_assets.stderr)
    );
    assert!(
        String::from_utf8_lossy(&restore_assets.stdout).contains("extension storage is healthy")
    );

    let store_b = IdentityStore::open(&db_b).await.unwrap();
    let (_, read_bytes) = store_b.assets().read_all(&meta.sha256).await.unwrap();
    assert_eq!(read_bytes, asset_bytes);
    assert_eq!(wasm(&store_b, &package).await.unwrap(), package.wasm);
    store_b.close().await;
}
