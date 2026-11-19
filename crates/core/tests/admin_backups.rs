use aether_core::{admin_backups, auth::AuthState, database, identity::IdentityStore};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn test_key_configuration() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([(
        "MEDIA_BACKUP_KEY".into(),
        "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12".into(),
    )])
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("origin", "http://127.0.0.1:3000");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        request = request.header("x-csrf-token", csrf);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn backups_authorization_csrf_consistency_download_and_unknown_ids() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("live.sqlite3"))
        .await
        .unwrap();
    let mut cookies = Vec::new();
    for (id, role, token) in [
        ("admin", "administrator", "a".repeat(64)),
        ("reader", "user", "b".repeat(64)),
    ] {
        database::query("INSERT INTO users(id,role) VALUES(?,?)")
            .bind(id)
            .bind(role)
            .execute(store.pool())
            .await
            .unwrap();
        database::query(
            "INSERT INTO local_credentials(user_id,login,password_hash) VALUES(?,?,'test')",
        )
        .bind(id)
        .bind(id)
        .execute(store.pool())
        .await
        .unwrap();
        database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(format!("{:x}", Sha256::digest(token.as_bytes()))).bind(id).execute(store.pool()).await.unwrap();
        cookies.push(format!("aether_session={token}"));
    }
    // Populate persisted administrator settings rather than checking an almost-empty DB.
    database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films privés','films','local',?,1)")
        .bind(media.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    for statement in [
        "INSERT INTO user_groups(id,name) VALUES('family','Famille')",
        "INSERT INTO user_group_members(group_id,user_id) VALUES('family','reader')",
        "INSERT INTO user_group_libraries(group_id,library_id) VALUES('family','films')",
        "INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,revoked_at,scope) VALUES('integration','Catalogue privé','reader','synthetic-hash-not-a-real-key',100,9999999999,NULL,'catalog:read')",
        "INSERT INTO api_key_libraries(key_id,library_id) VALUES('integration','films')",
        "INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at,last_run_at,last_outcome) VALUES('films',120,0,9999999999,123,'succeeded')",
        "UPDATE backup_schedule SET enabled=0,interval_minutes=360,next_due_at=9999999999,last_started_at=100,last_completed_at=110,last_outcome='succeeded' WHERE singleton=1",
        "INSERT INTO media_nodes(id,name,kind,base_url,public_url,status,key_id,shared_secret,roots_json) VALUES('remote-test','Node conservé','remote','https://node.example.test','https://stream.example.test','disconnected','test-key-id','synthetic-node-secret-marker','[\"/media\"]')",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json) VALUES('work','films','fixture-work','Film conservé',2026,'{}')",
        "INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES('reader','work',120.5,600,0,3,234)",
        "INSERT INTO oidc_identities(issuer,subject,user_id) VALUES('https://id.example.test','synthetic-subject','reader')",
        "UPDATE users SET display_name='Lecteur conservé',sso_username='lecteur.sso' WHERE id='reader'",
        "INSERT INTO bootstrap_state(singleton) VALUES(1)",
    ] {
        database::query(statement)
            .execute(store.pool())
            .await
            .unwrap();
    }
    let csrf = format!(
        "{:x}",
        Sha256::digest(format!("csrf:{}", "a".repeat(64)).as_bytes())
    );
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let key = "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12".to_string();
    // Without a (valid) key the endpoint fails closed: no snapshot and no archive is written.
    for configuration in [
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::from([("MEDIA_BACKUP_KEY".into(), "11".repeat(32))]),
    ] {
        let keyless = admin_backups::router_with_configuration(
            AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
                .await
                .unwrap(),
            media.clone(),
            Some(temp.path().join("keyless-backups")),
            configuration,
        )
        .await
        .unwrap();
        let refused = request(
            &keyless,
            "POST",
            "/api/v1/admin/backups",
            Some(&cookies[0]),
            Some(&csrf),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::CONFLICT);
        let body = to_bytes(refused.into_body(), 4096).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("backup_key_required"));
        assert!(!temp.path().join("keyless-backups").exists());
        let listing = request(
            &keyless,
            "GET",
            "/api/v1/admin/backups",
            Some(&cookies[0]),
            None,
        )
        .await;
        let listing: serde_json::Value =
            serde_json::from_slice(&to_bytes(listing.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(listing["available"], false);
    }
    let app = admin_backups::router_with_configuration(
        auth,
        media,
        None,
        std::collections::BTreeMap::from([
            (
                "MEDIA_TEST_SECRET".into(),
                "secret-test-config-marker".into(),
            ),
            ("MEDIA_BACKUP_KEY".into(), key.clone()),
        ]),
    )
    .await
    .unwrap();
    let base = "/api/v1/admin/backups";
    assert_eq!(
        request(&app, "GET", base, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", base, Some(&cookies[1]), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "POST", base, Some(&cookies[0]), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "POST", base, Some(&cookies[1]), Some(&csrf))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let created = request(&app, "POST", base, Some(&cookies[0]), Some(&csrf)).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let archive: serde_json::Value =
        serde_json::from_slice(&to_bytes(created.into_body(), 4096).await.unwrap()).unwrap();
    let id = archive["id"].as_str().unwrap();
    let path = format!("{base}/{id}/download");
    assert_eq!(
        request(&app, "GET", &path, Some(&cookies[1]), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let download = request(&app, "GET", &path, Some(&cookies[0]), None).await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(download.headers()["cache-control"], "no-store");
    assert!(
        download.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    let bytes = to_bytes(download.into_body(), 20_000_000).await.unwrap();
    assert_eq!(&bytes[..8], b"MDBK0003");
    // Secrets, sessions and the key itself never appear in the downloaded bytes.
    for marker in [
        &b"synthetic-node-secret-marker"[..],
        b"secret-test-config-marker",
        key.as_bytes(),
    ] {
        assert!(!bytes.windows(marker.len()).any(|window| window == marker));
    }
    assert!(!bytes.windows(15).any(|window| window == b"SQLite format 3"));
    let package = temp.path().join("download.media-backup");
    std::fs::write(&package, &bytes).unwrap();
    let parsed_key = aether_core::backup_bundle::BackupKey::parse(&key).unwrap();
    assert!(
        aether_core::backup_bundle::extract(&package, &temp.path().join("without-key"), None)
            .await
            .is_err()
    );
    let extracted = temp.path().join("extracted");
    let summary = aether_core::backup_bundle::extract(&package, &extracted, Some(&parsed_key))
        .await
        .unwrap();
    assert_eq!(summary.engine, "sqlite");
    let copy = extracted.join(summary.database_file);
    let configuration: serde_json::Value =
        serde_json::from_slice(&std::fs::read(extracted.join("config.json")).unwrap()).unwrap();
    assert_eq!(
        configuration["MEDIA_TEST_SECRET"],
        "secret-test-config-marker"
    );
    assert_eq!(configuration["MEDIA_DB_ENGINE"], "sqlite");
    assert_eq!(configuration["MEDIA_PUBLIC_URL"], "http://127.0.0.1:3000");
    assert!(configuration.get("MEDIA_BACKUP_KEY").is_none());
    assert!(configuration.get("MEDIA_BACKUP_KEY_FILE").is_none());
    let restored = temp.path().join("restored.sqlite3");
    let restored_summary = aether_core::backup::restore_database(&copy, &restored, None)
        .await
        .unwrap();
    assert_eq!(restored_summary.user_count, 2);
    let conn = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(restored)
                .read_only(true),
        )
        .await
        .unwrap();
    let count: i64 = database::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&conn)
        .await
        .unwrap();
    assert_eq!(count, 2);
    for (statement, expected) in [
        ("SELECT name FROM user_groups WHERE id='family'", "Famille"),
        (
            "SELECT user_id FROM user_group_members WHERE group_id='family'",
            "reader",
        ),
        (
            "SELECT library_id FROM user_group_libraries WHERE group_id='family'",
            "films",
        ),
        (
            "SELECT name FROM api_keys WHERE id='integration'",
            "Catalogue privé",
        ),
        (
            "SELECT token_hash FROM api_keys WHERE id='integration'",
            "synthetic-hash-not-a-real-key",
        ),
        (
            "SELECT scope FROM api_keys WHERE id='integration'",
            "catalog:read",
        ),
        (
            "SELECT library_id FROM api_key_libraries WHERE key_id='integration'",
            "films",
        ),
        (
            "SELECT last_outcome FROM scheduled_scans WHERE library_id='films'",
            "succeeded",
        ),
        (
            "SELECT last_outcome FROM backup_schedule WHERE singleton=1",
            "succeeded",
        ),
        (
            "SELECT shared_secret FROM media_nodes WHERE id='remote-test'",
            "synthetic-node-secret-marker",
        ),
        (
            "SELECT display_name FROM users WHERE id='reader'",
            "Lecteur conservé",
        ),
        (
            "SELECT subject FROM oidc_identities WHERE user_id='reader'",
            "synthetic-subject",
        ),
        (
            "SELECT title FROM inventory_works WHERE id='work'",
            "Film conservé",
        ),
    ] {
        let actual: String = database::query_scalar(statement)
            .fetch_one(&conn)
            .await
            .unwrap();
        assert_eq!(actual, expected, "{statement}");
    }
    // Compare every persisted column in all newly-added administration tables,
    // including disabled flags, expirations, nullable revocations and next-run state.
    for statement in [
        "SELECT json_group_array(json_array(id,name)) FROM (SELECT * FROM user_groups ORDER BY id)",
        "SELECT json_group_array(json_array(group_id,user_id)) FROM (SELECT * FROM user_group_members ORDER BY group_id,user_id)",
        "SELECT json_group_array(json_array(group_id,library_id)) FROM (SELECT * FROM user_group_libraries ORDER BY group_id,library_id)",
        "SELECT json_group_array(json_array(id,name,user_id,token_hash,created_at,expires_at,revoked_at,scope)) FROM (SELECT * FROM api_keys ORDER BY id)",
        "SELECT json_group_array(json_array(key_id,library_id)) FROM (SELECT * FROM api_key_libraries ORDER BY key_id,library_id)",
        "SELECT json_group_array(json_array(library_id,interval_minutes,enabled,next_run_at,last_run_at,last_outcome)) FROM (SELECT * FROM scheduled_scans ORDER BY library_id)",
        "SELECT json_group_array(json_array(singleton,enabled,interval_minutes,next_due_at,last_started_at,last_completed_at,last_outcome)) FROM backup_schedule",
    ] {
        let live: String = database::query_scalar(statement)
            .fetch_one(store.pool())
            .await
            .unwrap();
        let recovered: String = database::query_scalar(statement)
            .fetch_one(&conn)
            .await
            .unwrap();
        assert_eq!(recovered, live, "{statement}");
    }
    for (statement, expected) in [
        (
            "SELECT interval_minutes FROM backup_schedule WHERE singleton=1",
            360_i64,
        ),
        (
            "SELECT next_due_at FROM backup_schedule WHERE singleton=1",
            9999999999,
        ),
        (
            "SELECT last_completed_at FROM backup_schedule WHERE singleton=1",
            110,
        ),
        (
            "SELECT interval_minutes FROM scheduled_scans WHERE library_id='films'",
            120,
        ),
        (
            "SELECT next_run_at FROM scheduled_scans WHERE library_id='films'",
            9999999999,
        ),
        (
            "SELECT revision FROM playback_progress WHERE user_id='reader'",
            3,
        ),
        (
            "SELECT COUNT(*) FROM effective_library_permissions WHERE library_id='films' AND user_id='reader'",
            1,
        ),
        ("SELECT COUNT(*) FROM bootstrap_state", 1),
    ] {
        let actual: i64 = database::query_scalar(statement)
            .fetch_one(&conn)
            .await
            .unwrap();
        assert_eq!(actual, expected, "{statement}");
    }
    conn.close().await;
    let listing = request(&app, "GET", base, Some(&cookies[0]), None).await;
    let listing: serde_json::Value =
        serde_json::from_slice(&to_bytes(listing.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(listing["archives"][0]["id"], id);
    assert_eq!(listing["schedule"]["enabled"], false);
    for (interval, expected) in [
        (59, StatusCode::UNPROCESSABLE_ENTITY),
        (10081, StatusCode::UNPROCESSABLE_ENTITY),
        (60, StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("{base}/schedule"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("cookie", &cookies[0])
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"enabled": true, "intervalMinutes": interval})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    // A second snapshot has its own immutable name; the first archive remains readable.
    let second = request(&app, "POST", base, Some(&cookies[0]), Some(&csrf)).await;
    assert_eq!(second.status(), StatusCode::CREATED);
    let second: serde_json::Value =
        serde_json::from_slice(&to_bytes(second.into_body(), 4096).await.unwrap()).unwrap();
    assert_ne!(second["id"], id);
    assert_eq!(
        request(&app, "HEAD", &path, Some(&cookies[0]), None)
            .await
            .status(),
        StatusCode::OK
    );
    #[cfg(unix)]
    {
        let fake_id = uuid::Uuid::new_v4().to_string();
        std::os::unix::fs::symlink(
            temp.path().join("live.sqlite3"),
            temp.path()
                .join("backups")
                .join(format!("{fake_id}.media-backup")),
        )
        .unwrap();
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("{base}/{fake_id}/download"),
                Some(&cookies[0]),
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/not-a-file/download"),
            Some(&cookies[0]),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/{}/download", uuid::Uuid::new_v4()),
            Some(&cookies[0]),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    store.close().await;
}

#[tokio::test]
async fn backups_list_returns_unavailable_when_directory_refused() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("live.sqlite3"))
        .await
        .unwrap();
    database::query("INSERT INTO users(id,role) VALUES('admin','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    let token = "c".repeat(64);
    database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?, 'admin', unixepoch(), unixepoch(), unixepoch()+3600)")
        .bind(format!("{:x}", Sha256::digest(token.as_bytes())))
        .execute(store.pool())
        .await
        .unwrap();
    let cookie = format!("aether_session={token}");
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    // Directory placed inside media root is refused by policy
    let app = admin_backups::router_with_configuration(
        auth.clone(),
        media.clone(),
        Some(media.join("forbidden-backups")),
        test_key_configuration(),
    )
    .await
    .unwrap();
    let list = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(list.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["available"], false);
    assert!(body["reason"].as_str().is_some());
    // An unreadable backup listing must not masquerade as an available empty folder.
    // A regular file at the configured path deterministically exercises read_dir failure
    // on every OS, including Windows where Unix permission fixtures are ineffective.
    let invalid_directory = temp.path().join("not-a-directory");
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(options.open(&invalid_directory).unwrap());
    let app = admin_backups::router_with_configuration(
        auth.clone(),
        media.clone(),
        Some(invalid_directory),
        test_key_configuration(),
    )
    .await
    .unwrap();
    let list = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(list.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["available"], false);
    assert!(
        body["reason"]
            .as_str()
            .unwrap()
            .contains("ne correspond pas à un dossier")
    );
    assert!(!body.to_string().contains(temp.path().to_str().unwrap()));
    let csrf = format!("{:x}", Sha256::digest(format!("csrf:{token}").as_bytes()));
    let create = request(
        &app,
        "POST",
        "/api/v1/admin/backups",
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(create.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(create.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(body["code"], "backup_directory_unwritable");
    assert!(body["message"].as_str().unwrap().contains("UID/GID"));
    assert!(!body.to_string().contains(temp.path().to_str().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let private = temp.path().join("inaccessible-backups");
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root can bypass Unix mode bits; exercise denial only when it really applies.
        if std::fs::read_dir(&private).is_err() {
            let app = admin_backups::router_with_configuration(
                auth,
                media,
                Some(private.clone()),
                test_key_configuration(),
            )
            .await
            .unwrap();
            let list = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
            assert_eq!(list.status(), StatusCode::OK);
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(list.into_body(), 4096).await.unwrap()).unwrap();
            assert_eq!(body["available"], false);
            assert!(body["reason"].as_str().unwrap().contains("UID/GID"));
            assert!(!body.to_string().contains(temp.path().to_str().unwrap()));
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
            let list = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(list.into_body(), 4096).await.unwrap()).unwrap();
            assert_eq!(body["available"], true);
            assert_eq!(body["reason"], serde_json::Value::Null);
            assert_eq!(body["archives"], serde_json::json!([]));
        } else {
            std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    store.close().await;
}

#[tokio::test]
async fn backup_policy_modes_breakdown_retention_and_selective_restore() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("live.sqlite3"))
        .await
        .unwrap();
    database::query("INSERT INTO users(id,role) VALUES('admin','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    let token = "d".repeat(64);
    database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?, 'admin', unixepoch(), unixepoch(), unixepoch()+3600)")
        .bind(format!("{:x}", Sha256::digest(token.as_bytes())))
        .execute(store.pool())
        .await
        .unwrap();
    let cookie = format!("aether_session={token}");
    let csrf = format!("{:x}", Sha256::digest(format!("csrf:{token}").as_bytes()));

    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();

    // Seed an essential user avatar and a heavy catalogue poster + preview.
    let seeded_avatars = store
        .assets()
        .backup_breakdown()
        .await
        .unwrap()
        .essential_objects;
    let avatar_bytes = b"USER_CUSTOM_AVATAR_BYTES_ESSENTIAL";
    let avatar_meta = store
        .assets()
        .put_bytes(avatar_bytes, "image/png")
        .await
        .unwrap();
    database::query(
        "INSERT INTO user_avatars(user_id,sha256) VALUES('admin',?) ON CONFLICT(user_id) DO UPDATE SET sha256=excluded.sha256",
    )
    .bind(&avatar_meta.sha256)
    .execute(store.pool())
    .await
    .unwrap();

    database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)")
        .bind(media.to_string_lossy().as_ref())
        .execute(store.pool())
        .await
        .unwrap();
    database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,year,external_ids_json) VALUES('work-1','films','w1','Film 1',2026,'{}')")
        .execute(store.pool())
        .await
        .unwrap();
    let poster_bytes = vec![42u8; 120_000];
    let poster_meta = store
        .assets()
        .put_bytes(&poster_bytes, "image/webp")
        .await
        .unwrap();
    database::query("INSERT INTO work_posters(work_id,sha256,revision) VALUES('work-1',?,'rev-1')")
        .bind(&poster_meta.sha256)
        .execute(store.pool())
        .await
        .unwrap();
    database::query("INSERT INTO poster_previews(work_id,width,revision,mime,sha256) VALUES('work-1',320,'rev-1','image/webp',?)")
        .bind(&poster_meta.sha256)
        .execute(store.pool())
        .await
        .unwrap();

    let backups_dir = temp.path().join("backups");
    let app = admin_backups::router_with_configuration(
        auth,
        media.clone(),
        Some(backups_dir.clone()),
        test_key_configuration(),
    )
    .await
    .unwrap();

    // Check breakdown and default policy in GET /api/v1/admin/backups
    let list_res = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
    assert_eq!(list_res.status(), StatusCode::OK);
    let listing: serde_json::Value =
        serde_json::from_slice(&to_bytes(list_res.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(listing["policy"]["assetsMode"], "full");
    assert_eq!(listing["policy"]["retentionCount"], 0);
    assert_eq!(
        listing["breakdown"]["essentialAssetsCount"],
        seeded_avatars + 1
    );
    assert_eq!(listing["breakdown"]["catalogueAssetsCount"], 1);
    assert_eq!(
        listing["breakdown"]["catalogueAssetsBytes"],
        poster_bytes.len() as u64
    );
    assert!(listing["breakdown"]["databaseBytes"].as_u64().unwrap() > 0);

    // Update policy to essential mode and retentionCount = 2
    let policy_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/admin/backups/policy")
                .header("origin", "http://127.0.0.1:3000")
                .header("cookie", &cookie)
                .header("x-csrf-token", &csrf)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"assetsMode": "essential", "retentionCount": 2}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(policy_res.status(), StatusCode::OK);
    let policy_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(policy_res.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(policy_body["assetsMode"], "essential");
    assert_eq!(policy_body["retentionCount"], 2);

    // Create 3 backups (1st default essential, 2nd explicit none, 3rd default essential)
    // Retention count = 2 must keep only 2 archives in backups_dir.
    for mode_override in [None, Some("none"), None] {
        let body = match mode_override {
            Some(m) => Body::from(serde_json::json!({"assetsMode": m}).to_string()),
            None => Body::empty(),
        };
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/backups")
                    .header("origin", "http://127.0.0.1:3000")
                    .header("cookie", &cookie)
                    .header("x-csrf-token", &csrf)
                    .header("content-type", "application/json")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    let list_res = request(&app, "GET", "/api/v1/admin/backups", Some(&cookie), None).await;
    let listing: serde_json::Value =
        serde_json::from_slice(&to_bytes(list_res.into_body(), 65536).await.unwrap()).unwrap();
    let archives = listing["archives"].as_array().unwrap();
    assert_eq!(
        archives.len(),
        2,
        "retentionCount=2 must prune oldest archive"
    );

    // Extract the latest (essential) backup and restore it onto a fresh store:
    // the user avatar must be restored and verified, while the omitted catalogue poster
    // is cleanly pruned from SQL so verify_all() succeeds.
    let latest_id = archives[0]["id"].as_str().unwrap();
    let archive_path = backups_dir.join(format!("{latest_id}.media-backup"));
    let extracted_dir = temp.path().join("extracted-essential");
    let key = aether_core::backup_bundle::BackupKey::parse(
        "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12",
    )
    .unwrap();
    let summary = aether_core::backup_bundle::extract(&archive_path, &extracted_dir, Some(&key))
        .await
        .unwrap();
    assert_eq!(summary.assets, seeded_avatars + 1);

    let server_b = temp.path().join("server_b");
    std::fs::create_dir(&server_b).unwrap();
    let restored_db = server_b.join("core.sqlite3");
    aether_core::backup::restore_database(
        &extracted_dir.join("database.sqlite3"),
        &restored_db,
        None,
    )
    .await
    .unwrap();
    let restored_store = IdentityStore::open(&restored_db).await.unwrap();
    let restore_report = restored_store
        .assets()
        .restore_extracted(&extracted_dir)
        .await
        .unwrap();
    assert!(
        restore_report.is_complete(),
        "essential restore must prune omitted catalogue poster and pass verify_all"
    );
    let (_, read_avatar) = restored_store
        .assets()
        .read_all(&avatar_meta.sha256)
        .await
        .unwrap();
    assert_eq!(read_avatar, avatar_bytes);
    let poster_rows: i64 =
        database::query_scalar("SELECT COUNT(*) FROM work_posters WHERE work_id='work-1'")
            .fetch_one(restored_store.pool())
            .await
            .unwrap();
    assert_eq!(poster_rows, 0);

    restored_store.close().await;
    store.close().await;
}
