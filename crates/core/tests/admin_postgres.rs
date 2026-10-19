//! Opt-in proof with real PostgreSQL 18 dump/restore and encrypted complete Core state.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use media_poc::{
    admin_backups,
    auth::AuthState,
    database::{query, query_scalar},
    identity::IdentityStore,
};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    csrf: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", "http://127.0.0.1:3000")
                .header("cookie", format!("media_session={token}"))
                .header("x-csrf-token", csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL 18 and matching pg_dump/pg_restore; scripts/test-admin-postgres.ps1"]
async fn remote_postgres_full_backup_restores_all_core_tables_and_configuration() {
    let url = zeroize::Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL test URL required"),
    );
    let store = IdentityStore::open_postgres(&url).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let token = "a".repeat(64);
    query("INSERT INTO users(id,role,display_name) VALUES('admin','administrator','Test Admin')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO local_credentials(user_id,login,password_hash) VALUES('admin','admin','synthetic-hash')").execute(store.pool()).await.unwrap();
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'admin',unixepoch(),unixepoch(),unixepoch()+3600)").bind(format!("{:x}",Sha256::digest(token.as_bytes()))).execute(store.pool()).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','Test Films','films','local',?,1)").bind(media.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
    query("INSERT INTO user_groups(id,name) VALUES('g','Family')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO user_group_members(group_id,user_id) VALUES('g','admin')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO user_group_libraries(group_id,library_id) VALUES('g','lib')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at) VALUES('lib',60,1,9999999999)").execute(store.pool()).await.unwrap();
    query("INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,scope) VALUES('k','Integration','admin','synthetic-hashed-key',unixepoch(),9999999999,'catalog:read')").execute(store.pool()).await.unwrap();
    query("INSERT INTO api_key_libraries(key_id,library_id) VALUES('k','lib')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO user_preferences(user_id,preferences_json) VALUES('admin',?)")
        .bind(r#"{"theme":"dark","locale":"fr","audioLanguage":"fr"}"#)
        .execute(store.pool())
        .await
        .unwrap();
    let key = "12".repeat(32);
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = admin_backups::router_with_configuration(
        auth,
        media.clone(),
        Some(temp.path().join("backups")),
        Some(key.clone()),
        std::collections::BTreeMap::from([(
            "MEDIA_OIDC_CLIENT_SECRET".into(),
            "synthetic-oidc-secret".into(),
        )]),
    )
    .await
    .unwrap();
    let csrf = format!("{:x}", Sha256::digest(format!("csrf:{token}").as_bytes()));
    let response = request(&app, "POST", "/api/v1/admin/backups", &token, &csrf).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    let id = value["id"].as_str().unwrap();
    let output = temp.path().join("recovered");
    let summary = media_poc::backup_bundle::extract(
        &temp
            .path()
            .join("backups")
            .join(format!("{id}.media-backup")),
        &output,
        &key,
    )
    .await
    .unwrap();
    assert_eq!(summary.engine, "postgres");
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["MEDIA_OIDC_CLIENT_SECRET"], "synthetic-oidc-secret");
    assert_eq!(config["MEDIA_DB_ENGINE"], "postgres");
    assert_eq!(config["MEDIA_DB_URL"], url.as_str());
    let mut target_url = url::Url::parse(&url).unwrap();
    target_url.set_path("/media_restored");
    query("CREATE DATABASE media_restored")
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::postgres_backup::restore(
        target_url.as_str(),
        &output.join(summary.database_file),
        Some(&media),
    )
    .await
    .unwrap();
    let restored = IdentityStore::open_postgres(target_url.as_str())
        .await
        .unwrap();
    let tables: Vec<String> = query_scalar(
        "SELECT CAST(tablename AS TEXT) FROM pg_tables WHERE schemaname='public' ORDER BY tablename",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert!(tables.len() > 20);
    for table in tables {
        assert!(
            table
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        );
        let sql = format!("SELECT COUNT(*) FROM \"{table}\"");
        let before: i64 = query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM \"{table}\""
        )))
        .fetch_one(store.pool())
        .await
        .unwrap();
        let after: i64 = query_scalar(sqlx::AssertSqlSafe(sql))
            .fetch_one(restored.pool())
            .await
            .unwrap();
        assert_eq!(before, after, "table {table}");
    }
    let group: String = query_scalar("SELECT name FROM user_groups WHERE id='g'")
        .fetch_one(restored.pool())
        .await
        .unwrap();
    assert_eq!(group, "Family");
    let interval: i64 =
        query_scalar("SELECT interval_minutes FROM scheduled_scans WHERE library_id='lib'")
            .fetch_one(restored.pool())
            .await
            .unwrap();
    assert_eq!(interval, 60);
    let hash: String = query_scalar("SELECT token_hash FROM api_keys WHERE id='k'")
        .fetch_one(restored.pool())
        .await
        .unwrap();
    assert_eq!(hash, "synthetic-hashed-key");
    let preferences: String =
        query_scalar("SELECT preferences_json FROM user_preferences WHERE user_id='admin'")
            .fetch_one(restored.pool())
            .await
            .unwrap();
    let preferences: serde_json::Value = serde_json::from_str(&preferences).unwrap();
    assert_eq!(preferences["theme"], "dark");
    assert_eq!(preferences["locale"], "fr");
    drop(app);
    restored.close().await;
    store.close().await;
}
