use media_contracts::{
    inventory::{InventoryBatch, ScannedFile},
    media::{FilmMetadata, NfoOutcome},
};
use media_poc::{
    auth::AuthState,
    backup::{
        BackupError, BackupOptions, backup_database, backup_database_with_options, restore_database,
    },
    identity::{IdentityStore, Role},
    inventory,
};
use zeroize::Zeroizing;

#[tokio::test]
async fn test_sqlite_wal_backup_and_restore_preserves_users_grants_uuids_and_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films_dir = root.join("Films");
    tokio::fs::create_dir_all(&films_dir).await.unwrap();

    let db_dir = temp.path().join("data");
    tokio::fs::create_dir_all(&db_dir).await.unwrap();
    let db_path = db_dir.join("core.sqlite3");

    // 1. Initialize original database and bootstrap admin
    let store = IdentityStore::open(&db_path).await.unwrap();
    let admin_id = store
        .bootstrap(
            "admin-root",
            Zeroizing::new("super-secret-admin-pass-12345".into()),
        )
        .await
        .unwrap();

    // 2. Create regular user
    let user_id = uuid::Uuid::new_v4().to_string();
    let user_hash = tokio::task::spawn_blocking(|| {
        use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
        let params = Params::new(65536, 3, 1, Some(32)).unwrap();
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(b"super-secret-user-pass-12345")
            .unwrap()
            .to_string()
    })
    .await
    .unwrap();

    sqlx::query("INSERT INTO users(id, role) VALUES (?, ?)")
        .bind(&user_id)
        .bind(Role::User.as_str())
        .execute(store.pool())
        .await
        .unwrap();

    sqlx::query("INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, ?)")
        .bind(&user_id)
        .bind("regular-user")
        .bind(&user_hash)
        .execute(store.pool())
        .await
        .unwrap();

    // 3. Create library and grant access to regular user
    let library_id = "lib-films-test";
    sqlx::query(
        "INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES (?, ?, 'films', 'local', ?, 1)"
    )
    .bind(library_id)
    .bind("Films Originaux")
    .bind(films_dir.to_str().unwrap())
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query("INSERT INTO library_permissions(library_id, user_id) VALUES (?, ?)")
        .bind(library_id)
        .bind(&user_id)
        .execute(store.pool())
        .await
        .unwrap();

    // 4. Stage and publish media entries
    let scan_id = uuid::Uuid::new_v4().to_string();
    let files: Vec<ScannedFile> = (0..5)
        .map(|i| ScannedFile {
            relative_path: format!("movie_{i}/movie.mkv"),
            identity_key: format!("tmdb:{}", 10000 + i),
            title: format!("Film Test {i}"),
            year: Some(2024),
            external_ids: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("tmdb".into(), format!("{}", 10000 + i));
                m
            },
            size: 1048576 * (i + 1),
            modified_ns: "1700000000".into(),
            mime: "video/x-matroska".into(),
            probe: media_poc::media::Probe::default(),
            editorial: NfoOutcome::Valid {
                metadata: FilmMetadata {
                    title: Some(format!("Film Test {i}")),
                    year: Some(2024),
                    plot: Some(format!("Description du film test {i}")),
                    ..Default::default()
                },
                path: format!("movie_{i}/movie.nfo"),
                priority: 1,
            },
            poster: Some(media_contracts::media::LocalImage {
                path: format!("movie_{i}/poster.png"),
                size_bytes: 40960,
                modified_ns: "1700000000".into(),
                mime: "image/png".into(),
                priority: 1,
            }),
            reused: false,
        })
        .collect();

    let batch = InventoryBatch {
        scan_id: scan_id.clone().into(),
        sequence: 1,
        files,
        errors: vec![],
        warnings: vec![],
    };

    inventory::stage_batch(&store, &scan_id, library_id, &batch)
        .await
        .unwrap();

    inventory::publish_staged_scan(&store, &scan_id, library_id, &films_dir, true)
        .await
        .unwrap();

    // Capture UUIDs of published works and assets
    let original_works: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, title, year FROM inventory_works WHERE library_id=? ORDER BY title ASC",
    )
    .bind(library_id)
    .fetch_all(store.pool())
    .await
    .unwrap();

    assert_eq!(original_works.len(), 5);

    let original_assets: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM inventory_assets WHERE library_id=? ORDER BY relative_path ASC"
    )
    .bind(library_id)
    .fetch_all(store.pool())
    .await
    .unwrap();

    assert_eq!(original_assets.len(), 5);

    // 5. Perform hot backup using VACUUM INTO while database is in WAL mode
    let backups_dir = temp.path().join("backups");
    let backup_file = backups_dir.join("core_test_backup.sqlite3");

    let backup_summary = backup_database(&db_path, &backup_file, Some(&root))
        .await
        .unwrap();

    assert_eq!(backup_summary.user_count, 2);
    assert_eq!(backup_summary.library_count, 1);
    assert_eq!(backup_summary.work_count, 5);
    assert!(backup_file.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&backup_file).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    // 6. Simulate a fresh Docker volume (new empty directory)
    let fresh_volume_dir = temp.path().join("fresh_docker_volume");
    let restored_db_path = fresh_volume_dir.join("core.sqlite3");

    // 7. Perform restore into the fresh volume
    let restore_summary = restore_database(&backup_file, &restored_db_path, Some(&root))
        .await
        .unwrap();

    assert_eq!(restore_summary.user_count, 2);
    assert_eq!(restore_summary.library_count, 1);
    assert_eq!(restore_summary.work_count, 5);
    assert!(restored_db_path.exists());

    // 8. Open restored database and verify integrity, users, grants, UUIDs
    let restored_store = IdentityStore::open(&restored_db_path).await.unwrap();

    let admin_user = restored_store.user(&admin_id).await.unwrap().unwrap();
    assert_eq!(admin_user.role, Role::Administrator);

    let reg_user = restored_store.user(&user_id).await.unwrap().unwrap();
    assert_eq!(reg_user.role, Role::User);

    let restored_works: Vec<(String, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, title, year FROM inventory_works WHERE library_id=? ORDER BY title ASC",
    )
    .bind(library_id)
    .fetch_all(restored_store.pool())
    .await
    .unwrap();

    // Verify UUIDs and titles match 100% deterministically
    assert_eq!(restored_works, original_works);

    let restored_assets: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, relative_path FROM inventory_assets WHERE library_id=? ORDER BY relative_path ASC"
    )
    .bind(library_id)
    .fetch_all(restored_store.pool())
    .await
    .unwrap();

    assert_eq!(restored_assets, original_assets);

    // Verify library permissions grant
    let has_grant: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM library_permissions WHERE library_id=? AND user_id=?)",
    )
    .bind(library_id)
    .bind(&user_id)
    .fetch_one(restored_store.pool())
    .await
    .unwrap();

    assert!(has_grant, "User must retain library grant after restore");

    // 9. Validation of reconnection / authentication after restore
    let auth = AuthState::new(restored_store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();

    // Test login for admin
    let admin_login_req = serde_json::json!({
        "login": "admin-root",
        "password": "super-secret-admin-pass-12345"
    });

    use axum::body::Body;
    use tower::ServiceExt;
    let auth_router = media_poc::auth::router(auth.clone());

    let login_response = auth_router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(axum::http::header::ORIGIN, "http://127.0.0.1:3000")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&admin_login_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        login_response.status(),
        axum::http::StatusCode::OK,
        "Admin must successfully log in on restored database"
    );

    let _cookie = login_response.headers()[axum::http::header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();

    // Verify authenticated session can access catalog
    let catalog_res = media_poc::inventory::catalogue(
        &restored_store,
        &[library_id.into()],
        Some("film"),
        "",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();

    assert_eq!(catalog_res["total"], 5);

    // Test regular user login
    let user_login_req = serde_json::json!({
        "login": "regular-user",
        "password": "super-secret-user-pass-12345"
    });

    let user_login_response = auth_router
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(axum::http::header::ORIGIN, "http://127.0.0.1:3000")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&user_login_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        user_login_response.status(),
        axum::http::StatusCode::OK,
        "Regular user must successfully log in on restored database"
    );
}

#[tokio::test]
async fn test_restore_refused_on_existing_database_and_preserves_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    // 1. Create source database with admin and produce valid backup
    let src_dir = temp.path().join("source_data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let src_store = IdentityStore::open(&src_db).await.unwrap();
    src_store
        .bootstrap(
            "admin-src",
            Zeroizing::new("super-secret-src-pass-12345".into()),
        )
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("backup.sqlite3");
    backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();

    // 2. Create existing target database with different admin
    let target_dir = temp.path().join("target_data");
    tokio::fs::create_dir_all(&target_dir).await.unwrap();
    let target_db = target_dir.join("core.sqlite3");
    let target_store = IdentityStore::open(&target_db).await.unwrap();
    let target_admin_id = target_store
        .bootstrap(
            "admin-target-keep",
            Zeroizing::new("super-secret-target-pass-12345".into()),
        )
        .await
        .unwrap();
    target_store.close().await;

    let target_len_before = std::fs::metadata(&target_db).unwrap().len();

    // 3. Attempt restore onto the existing database: MUST be explicitly refused
    let err = restore_database(&backup_file, &target_db, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(err, BackupError::TargetExists(_)),
        "Restoring onto an existing database must be refused with TargetExists, got: {err}"
    );

    // 4. Verify existing database is strictly unmodified
    assert_eq!(
        std::fs::metadata(&target_db).unwrap().len(),
        target_len_before,
        "Target database size must be unchanged after refused restore"
    );

    let verify_store = IdentityStore::open(&target_db).await.unwrap();
    let user = verify_store
        .user(&target_admin_id)
        .await
        .unwrap()
        .expect("Original admin must still exist");
    assert_eq!(user.role, Role::Administrator);

    // Verify login credentials still match target admin
    let login: String = sqlx::query_scalar("SELECT login FROM local_credentials WHERE user_id=?")
        .bind(&target_admin_id)
        .fetch_one(verify_store.pool())
        .await
        .unwrap();
    assert_eq!(login, "admin-target-keep");
}

#[tokio::test]
async fn test_restore_refused_when_sqlite_connection_open_without_active_write() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    // 1. Prepare valid backup
    let src_dir = temp.path().join("source_data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let src_store = IdentityStore::open(&src_db).await.unwrap();
    src_store
        .bootstrap(
            "admin-src",
            Zeroizing::new("super-secret-src-pass-12345".into()),
        )
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("backup.sqlite3");
    backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();

    // 2. Open an idle SQLite connection to target path (no active transaction/write)
    let target_dir = temp.path().join("target_data");
    tokio::fs::create_dir_all(&target_dir).await.unwrap();
    let target_db = target_dir.join("core.sqlite3");

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&target_db)
        .create_if_missing(true);

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    // Connection is alive and idle (no lock held, no write)
    sqlx::query("CREATE TABLE IF NOT EXISTS probe (val INT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO probe VALUES (42)")
        .execute(&pool)
        .await
        .unwrap();

    // 3. Attempt restore: MUST be refused because target database exists
    let err = restore_database(&backup_file, &target_db, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(err, BackupError::TargetExists(_)),
        "Restore must be refused when destination database exists even if idle/open, got: {err}"
    );

    // Verify existing database content remains intact
    let count: i64 = sqlx::query_scalar("SELECT val FROM probe")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 42);

    pool.close().await;
}

#[tokio::test]
async fn test_restore_refused_when_residual_wal_or_shm_journals_present() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    // 1. Prepare valid backup
    let src_dir = temp.path().join("source_data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let src_store = IdentityStore::open(&src_db).await.unwrap();
    src_store
        .bootstrap(
            "admin-src",
            Zeroizing::new("super-secret-src-pass-12345".into()),
        )
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("backup.sqlite3");
    backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();

    // 2. Case A: target database does not exist, but target-wal exists
    let target_dir_a = temp.path().join("target_dir_a");
    tokio::fs::create_dir_all(&target_dir_a).await.unwrap();
    let target_db_a = target_dir_a.join("core.sqlite3");
    let wal_path_a = target_dir_a.join("core.sqlite3-wal");
    tokio::fs::write(&wal_path_a, b"residual wal journal content")
        .await
        .unwrap();

    let err_a = restore_database(&backup_file, &target_db_a, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(err_a, BackupError::ResidualJournals(_)),
        "Restore must refuse when residual WAL is present, got: {err_a}"
    );
    // Crucial: residual journal MUST NOT be deleted automatically
    assert!(
        wal_path_a.exists(),
        "Residual WAL journal must not be automatically deleted"
    );
    assert!(
        !target_db_a.exists(),
        "Target database must not be created on refused restore"
    );

    // 3. Case B: target database does not exist, but target-shm exists
    let target_dir_b = temp.path().join("target_dir_b");
    tokio::fs::create_dir_all(&target_dir_b).await.unwrap();
    let target_db_b = target_dir_b.join("core.sqlite3");
    let shm_path_b = target_dir_b.join("core.sqlite3-shm");
    tokio::fs::write(&shm_path_b, b"residual shm index content")
        .await
        .unwrap();

    let err_b = restore_database(&backup_file, &target_db_b, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(err_b, BackupError::ResidualJournals(_)),
        "Restore must refuse when residual SHM is present, got: {err_b}"
    );
    assert!(
        shm_path_b.exists(),
        "Residual SHM index must not be automatically deleted"
    );
    assert!(
        !target_db_b.exists(),
        "Target database must not be created on refused restore"
    );
}

#[tokio::test]
async fn test_restore_corrupted_backup_fails_closed_without_modifying_destination() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let corrupt_file = temp.path().join("corrupt.sqlite3");
    tokio::fs::write(
        &corrupt_file,
        b"this is not a valid sqlite database header at all",
    )
    .await
    .unwrap();

    let target_dir = temp.path().join("fresh_dest");
    let target_db = target_dir.join("core.sqlite3");

    let err = restore_database(&corrupt_file, &target_db, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            BackupError::IntegrityCheck(_) | BackupError::Database(_)
        ),
        "Corrupted backup must fail integrity/database checks, got: {err}"
    );

    assert!(
        !target_db.exists(),
        "Target database must not exist after corrupt backup failure"
    );

    // Verify target directory has no leftover staging files
    if target_dir.exists() {
        let mut entries = tokio::fs::read_dir(&target_dir).await.unwrap();
        assert!(
            entries.next_entry().await.unwrap().is_none(),
            "Target directory must be completely clean with zero leftover staging files"
        );
    }
}

#[tokio::test]
async fn test_backup_refuses_overwriting_existing_file_by_default() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let src_dir = temp.path().join("data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let store = IdentityStore::open(&src_db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin-pass-12345".into()))
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("core_backup.sqlite3");

    // 1. First backup succeeds
    let summary1 = backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();
    assert_eq!(summary1.user_count, 1);
    let meta_before = std::fs::metadata(&backup_file).unwrap();

    // 2. Second backup to identical path without replace option: MUST be refused
    let err = backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(err, BackupError::TargetExists(_)),
        "Backup must refuse to overwrite existing backup by default, got: {err}"
    );

    // Verify existing backup file was untouched
    let meta_after = std::fs::metadata(&backup_file).unwrap();
    assert_eq!(
        meta_before.len(),
        meta_after.len(),
        "Existing backup must be preserved intact"
    );
}

#[tokio::test]
async fn test_backup_with_replace_produces_and_verifies_before_replacing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let src_dir = temp.path().join("data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let store = IdentityStore::open(&src_db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin-pass-12345".into()))
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("core_backup.sqlite3");

    // 1. Initial backup has 1 user
    let summary1 = backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();
    assert_eq!(summary1.user_count, 1);

    // 2. Add second user to database
    let user_id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO users(id, role) VALUES (?, ?)")
        .bind(&user_id)
        .bind(Role::User.as_str())
        .execute(store.pool())
        .await
        .unwrap();

    // 3. Backup with allow_replace: true succeeds and updates the backup
    let summary2 = backup_database_with_options(
        &src_db,
        &backup_file,
        Some(&root),
        BackupOptions {
            allow_replace: true,
        },
    )
    .await
    .unwrap();

    assert_eq!(summary2.user_count, 2);

    // Verify restored file from replacement backup contains both users
    let restore_dir = temp.path().join("verify_replace");
    let restore_db = restore_dir.join("core.sqlite3");
    let restore_summary = restore_database(&backup_file, &restore_db, Some(&root))
        .await
        .unwrap();
    assert_eq!(restore_summary.user_count, 2);
}

#[tokio::test]
async fn test_backup_failure_preserves_previous_valid_backup_and_cleans_temp_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let src_dir = temp.path().join("data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let store = IdentityStore::open(&src_db).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("admin-pass-12345".into()))
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("core_backup.sqlite3");

    // 1. Create a valid backup
    backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();
    let initial_len = std::fs::metadata(&backup_file).unwrap().len();

    // 2. Simulate a failure during replacement: non-existent source
    let non_existent_source = temp.path().join("non_existent.sqlite3");
    let err = backup_database_with_options(
        &non_existent_source,
        &backup_file,
        Some(&root),
        BackupOptions {
            allow_replace: true,
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, BackupError::InvalidPath(_)));

    // 3. Verify previous valid backup is STILL intact and valid
    assert_eq!(
        std::fs::metadata(&backup_file).unwrap().len(),
        initial_len,
        "Original backup must remain intact after a failed replacement attempt"
    );

    // 4. Verify no temporary .tmp files are left in the backup directory
    let mut entries = tokio::fs::read_dir(&backup_dir).await.unwrap();
    let mut files = vec![];
    while let Some(entry) = entries.next_entry().await.unwrap() {
        files.push(entry.file_name().to_string_lossy().to_string());
    }
    assert_eq!(
        files,
        vec!["core_backup.sqlite3"],
        "Backup directory must contain only the valid backup file, zero leftover tmp files"
    );
}

#[tokio::test]
async fn test_restore_refuses_media_root_and_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let src_dir = temp.path().join("data");
    tokio::fs::create_dir_all(&src_dir).await.unwrap();
    let src_db = src_dir.join("core.sqlite3");
    let store = IdentityStore::open(&src_db).await.unwrap();
    store
        .bootstrap(
            "admin",
            Zeroizing::new("super-secret-admin-pass-12345".into()),
        )
        .await
        .unwrap();

    let backup_dir = temp.path().join("backups");
    let backup_file = backup_dir.join("core_backup.sqlite3");
    backup_database(&src_db, &backup_file, Some(&root))
        .await
        .unwrap();

    // 1. Refuse restore inside media root
    let target_inside_media = root.join("forbidden.sqlite3");
    let err_media = restore_database(&backup_file, &target_inside_media, Some(&root))
        .await
        .unwrap_err();
    assert!(matches!(err_media, BackupError::InvalidPath(_)));

    // 2. Refuse traversal with '..'
    let target_traversal = temp.path().join("sub").join("..").join("evil.sqlite3");
    let err_traversal = restore_database(&backup_file, &target_traversal, Some(&root))
        .await
        .unwrap_err();
    assert!(matches!(err_traversal, BackupError::InvalidPath(_)));
}
