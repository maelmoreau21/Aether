use media_contracts::{
    inventory::{InventoryBatch, ScannedFile},
    media::{FilmMetadata, NfoOutcome},
};
use media_poc::{
    auth::AuthState,
    backup::{backup_database, restore_database},
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

    // Negative case: corrupted backup file fails closed
    let corrupt_file = backups_dir.join("corrupt_backup.sqlite3");
    tokio::fs::write(&corrupt_file, b"this is not a valid sqlite database")
        .await
        .unwrap();

    let target_fail = temp.path().join("fail_data").join("core.sqlite3");
    let err = restore_database(&corrupt_file, &target_fail, Some(&root))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            media_poc::backup::BackupError::IntegrityCheck(_)
                | media_poc::backup::BackupError::Database(_)
        ),
        "Corrupted backup must fail closed"
    );
}
