use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use media_poc::{admin_backups, auth::AuthState, database, identity::IdentityStore};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

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
        cookies.push(format!("media_session={token}"));
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
    let key = "11".repeat(32);
    let app = admin_backups::router_with_configuration(
        auth,
        media,
        None,
        Some(key.clone()),
        std::collections::BTreeMap::from([
            (
                "MEDIA_TEST_SECRET".into(),
                "secret-test-config-marker".into(),
            ),
            ("MEDIA_BACKUP_KEY".into(), key.clone()),
            (
                "MEDIA_BACKUP_KEY_FILE".into(),
                "/private/recovery-key-source".into(),
            ),
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
    assert!(
        !bytes
            .windows(28)
            .any(|window| window == b"synthetic-node-secret-marker")
    );
    assert!(
        !bytes
            .windows(25)
            .any(|window| window == b"secret-test-config-marker")
    );
    let package = temp.path().join("download.media-backup");
    std::fs::write(&package, &bytes).unwrap();
    let extracted = temp.path().join("extracted");
    let summary = media_poc::backup_bundle::extract(&package, &extracted, &key)
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
    let restored_summary = media_poc::backup::restore_database(&copy, &restored, None)
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
